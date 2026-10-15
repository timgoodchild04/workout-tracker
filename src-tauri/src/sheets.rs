//! Google Sheets backend, authenticated as a service account (a JSON key file
//! copied to each computer; the sheet is shared with the account's email).
//!
//! The app only ever WRITES columns A:H of the next empty row(s) on the
//! 📝 Workout Log tab. Everything else is read-only; the sheet's own formulas
//! keep working on the rows the app writes.

use crate::model::*;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SCOPE: &str = "https://www.googleapis.com/auth/spreadsheets";
const API: &str = "https://sheets.googleapis.com/v4/spreadsheets";

#[derive(Deserialize, Clone)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key: String,
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
}
fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".into()
}

pub struct Client {
    sa: ServiceAccount,
    sheet_id: String,
    http: reqwest::Client,
    token: Mutex<Option<(String, Instant)>>,
}

pub fn read_key(path: &Path) -> Result<ServiceAccount> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("can't read key file {}", path.display()))?;
    serde_json::from_str(&raw).context("key file isn't a Google service-account JSON key")
}

fn q(tab: &str, a1: &str) -> String {
    format!("'{}'!{}", tab.replace('\'', "''"), a1)
}

fn to_cell(v: &Value) -> Cell {
    match v {
        Value::Number(n) => Cell::Num(n.as_f64().unwrap_or(0.0)),
        Value::String(s) if s.trim().is_empty() => Cell::Empty,
        Value::String(s) => Cell::Str(s.clone()),
        Value::Bool(b) => Cell::Bool(*b),
        _ => Cell::Empty,
    }
}

impl Client {
    pub fn new(key_path: &Path, sheet_id: &str) -> Result<Client> {
        Ok(Client {
            sa: read_key(key_path)?,
            sheet_id: sheet_id.trim().to_string(),
            http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?,
            token: Mutex::new(None),
        })
    }

    pub fn account_email(&self) -> &str {
        &self.sa.client_email
    }

    async fn token(&self) -> Result<String> {
        if let Some((t, exp)) = self.token.lock().unwrap().as_ref() {
            if Instant::now() < *exp {
                return Ok(t.clone());
            }
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let claims = json!({ "iss": self.sa.client_email, "scope": SCOPE, "aud": self.sa.token_uri, "iat": now, "exp": now + 3600 });
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(self.sa.private_key.as_bytes())?;
        let jwt = jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256), &claims, &key)?;
        let res: Value = self
            .http
            .post(&self.sa.token_uri)
            .form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", jwt.as_str())])
            .send()
            .await?
            .json()
            .await?;
        let t = res["access_token"].as_str().ok_or_else(|| anyhow!("Google sign-in failed: {}", res))?.to_string();
        *self.token.lock().unwrap() = Some((t.clone(), Instant::now() + Duration::from_secs(3000)));
        Ok(t)
    }

    async fn check(res: reqwest::Response) -> Result<Value> {
        let status = res.status();
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
            if status.as_u16() == 403 || status.as_u16() == 404 {
                bail!("Google Sheets said: {msg}. Is the sheet shared (Editor) with the service account email?");
            }
            bail!("Google Sheets error {status}: {msg}");
        }
        Ok(body)
    }

    async fn batch_get(&self, ranges: &[String], render: &str) -> Result<Vec<Grid>> {
        let mut params: Vec<(&str, &str)> = ranges.iter().map(|r| ("ranges", r.as_str())).collect();
        params.push(("valueRenderOption", render));
        params.push(("dateTimeRenderOption", "SERIAL_NUMBER"));
        let res = self
            .http
            .get(format!("{API}/{}/values:batchGet", self.sheet_id))
            .bearer_auth(self.token().await?)
            .query(&params)
            .send()
            .await?;
        let body = Self::check(res).await?;
        Ok(body["valueRanges"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|vr| {
                        vr["values"]
                            .as_array()
                            .map(|rows| rows.iter().map(|r| r.as_array().map(|c| c.iter().map(to_cell).collect()).unwrap_or_default()).collect())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn fetch(&self) -> Result<Data> {
        let g = self
            .batch_get(
                &[q(TAB_LIBRARY, RANGE_LIBRARY), q(TAB_PLAN, RANGE_PLAN), q(TAB_SETTINGS, RANGE_SETTINGS), q(TAB_SCHEDULE, RANGE_SCHEDULE), q(TAB_LOG, RANGE_LOG)],
                "UNFORMATTED_VALUE",
            )
            .await?;
        if g.len() < 5 {
            bail!("the sheet didn't return the expected tabs");
        }
        Ok(Data::from_grids(&g[0], &g[1], &g[2], &g[3], &g[4]))
    }

    /// Set one cell to `value`, but only if `check_a1` (same tab) still reads `expect`.
    /// Rows are addressed by number from the last sync, so this stops a write landing
    /// on the wrong row if the sheet was rearranged since.
    pub async fn set_cell(&self, tab: &str, check_a1: &str, expect: &str, a1: &str, value: &str) -> Result<()> {
        self.set_cells(tab, check_a1, expect, a1, vec![json!(value)]).await
    }

    /// `set_cell` for a run of cells in one row (`a1` like "A7:E7").
    pub async fn set_cells(&self, tab: &str, check_a1: &str, expect: &str, a1: &str, values: Vec<Value>) -> Result<()> {
        let got = self.batch_get(&[q(tab, check_a1)], "UNFORMATTED_VALUE").await?;
        let got = got.first().and_then(|g| g.first()).and_then(|r| r.first()).map(|c| c.text()).unwrap_or_default();
        if got != expect {
            bail!("{tab} has changed since the last sync ({check_a1} is now \"{got}\", expected \"{expect}\"). Sync and try again.");
        }
        let range = q(tab, a1);
        let res = self
            .http
            .put(format!("{API}/{}/values/{}", self.sheet_id, urlencoding::encode(&range)))
            .bearer_auth(self.token().await?)
            .query(&[("valueInputOption", "USER_ENTERED")])
            .json(&json!({ "range": range, "majorDimension": "ROWS", "values": [values] }))
            .send()
            .await?;
        Self::check(res).await?;
        Ok(())
    }

    /// Fill an empty 📅 Plan row (A:E). The row is re-read first and must still be
    /// empty, so nothing typed into the sheet since the last sync gets overwritten.
    pub async fn write_plan_row(&self, row: usize, values: Vec<Value>) -> Result<()> {
        let got = self.batch_get(&[q(TAB_PLAN, &format!("A{row}:C{row}"))], "UNFORMATTED_VALUE").await?;
        if got.first().and_then(|g| g.first()).is_some_and(|r| r.iter().any(|c| !c.is_empty())) {
            bail!("row {row} of 📅 Plan isn't empty any more. Sync and try again.");
        }
        let range = q(TAB_PLAN, &format!("A{row}:E{row}"));
        let res = self
            .http
            .put(format!("{API}/{}/values/{}", self.sheet_id, urlencoding::encode(&range)))
            .bearer_auth(self.token().await?)
            .query(&[("valueInputOption", "USER_ENTERED")])
            .json(&json!({ "range": range, "majorDimension": "ROWS", "values": [values] }))
            .send()
            .await?;
        Self::check(res).await?;
        Ok(())
    }

    /// Overwrite A:H of an already-logged row with an edited entry. The row is
    /// re-read first and must still hold the same date and exercise, so an edit
    /// never lands on a row that moved or was replaced since the last sync.
    pub async fn update_row(&self, e: &LogEntry) -> Result<()> {
        let row = e.row.ok_or_else(|| anyhow!("no row to update"))?;
        let got = self.batch_get(&[q(TAB_LOG, &format!("A{row}:C{row}"))], "UNFORMATTED_VALUE").await?;
        let cells = got.first().and_then(|g| g.first()).cloned().unwrap_or_default();
        let (date, exercise) = (cells.first().and_then(parse_date), cells.get(2).map(|c| c.text()).unwrap_or_default());
        if date != Some(e.date) || exercise != e.exercise {
            bail!("row {row} of the Workout Log no longer holds {} on {} (it has {exercise:?}). Sync and re-enter it.", e.exercise, e.date);
        }
        let s = |i: usize| e.sets[i].map(|v| json!(v)).unwrap_or(json!(""));
        let range = q(TAB_LOG, &format!("A{row}:H{row}"));
        let values = json!([[e.date.format("%Y-%m-%d").to_string(), e.workout, e.exercise, e.weight, s(0), s(1), s(2), s(3)]]);
        let res = self
            .http
            .put(format!("{API}/{}/values/{}", self.sheet_id, urlencoding::encode(&range)))
            .bearer_auth(self.token().await?)
            .query(&[("valueInputOption", "USER_ENTERED")])
            .json(&json!({ "range": range, "majorDimension": "ROWS", "values": values }))
            .send()
            .await?;
        Self::check(res).await?;
        Ok(())
    }

    /// Write entries into the first empty rows after the last logged exercise.
    /// Re-reads column C first so two computers can't overwrite each other.
    pub async fn append(&self, entries: &[LogEntry]) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let col = self.batch_get(&[q(TAB_LOG, "C2:C2000")], "UNFORMATTED_VALUE").await?;
        let col = col.first().cloned().unwrap_or_default();
        let last = col.iter().rposition(|r| r.first().is_some_and(|c| !c.is_empty()));
        let first_row = last.map_or(LOG_FIRST_ROW, |i| i + LOG_FIRST_ROW + 1);
        let last_row = first_row + entries.len() - 1;
        if last_row > LOG_LAST_FORMULA_ROW {
            bail!(
                "the Workout Log's formulas stop at row {LOG_LAST_FORMULA_ROW} and these entries would reach row {last_row}. \
                 Extend the log's formulas (and the Dashboard ranges) in the sheet first."
            );
        }
        let values: Vec<Value> = entries
            .iter()
            .map(|e| {
                let s = |i: usize| e.sets[i].map(|v| json!(v)).unwrap_or(json!(""));
                json!([e.date.format("%Y-%m-%d").to_string(), e.workout, e.exercise, e.weight, s(0), s(1), s(2), s(3)])
            })
            .collect();
        let range = q(TAB_LOG, &format!("A{first_row}:H{last_row}"));
        let res = self
            .http
            .put(format!("{API}/{}/values/{}", self.sheet_id, urlencoding::encode(&range)))
            .bearer_auth(self.token().await?)
            .query(&[("valueInputOption", "USER_ENTERED")])
            .json(&json!({ "range": range, "majorDimension": "ROWS", "values": values }))
            .send()
            .await?;
        Self::check(res).await?;
        Ok(first_row)
    }
}
