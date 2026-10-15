pub mod coach;
pub mod engine;
#[cfg(target_os = "linux")]
mod linux;
pub mod model;
pub mod paths;
pub mod plan;
pub mod sheets;
pub mod snapshot;
pub mod suggest;

use chrono::{Local, NaiveDate};
use coach::{Analysis, Card, ProgressRow, Today, Verdict};
use engine::Engine;
use model::{Data, DayInfo, LibEx, LogEntry, Settings};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

pub const DEFAULT_SHEET_ID: &str = "1Pw_IPfqvv23Mko4Y71secAUq8-bzZN0HC8M_XKghldY";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// "sheets" (live Google Sheet) or "snapshot" (local .xlsx, read-only)
    pub mode: String,
    pub sheet_id: String,
    pub key_path: String,
    pub snapshot_path: String,
}
impl Default for Config {
    fn default() -> Self {
        Config { mode: "sheets".into(), sheet_id: DEFAULT_SHEET_ID.into(), key_path: String::new(), snapshot_path: String::new() }
    }
}

struct Inner {
    dir: PathBuf,
    /// Date picked on the Today screen (None = today).
    view_date: Option<NaiveDate>,
    config: Config,
    data: Option<Data>,
    pending: Vec<LogEntry>,
    online: bool,
    message: String,
    last_sync: Option<String>,
    account: String,
}

/// App state, plus a lock held for a whole sync so two syncs (an auto-save and a
/// window-focus refresh, say) can never send the same queued entry twice.
pub struct AppState(Mutex<Inner>, tauri::async_runtime::Mutex<()>);

fn read_json<T: for<'a> Deserialize<'a>>(p: &PathBuf) -> Option<T> {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())
}
fn write_json<T: Serialize>(p: &PathBuf, v: &T) {
    if let Ok(s) = serde_json::to_string_pretty(v) {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
}

impl Inner {
    fn path(&self, f: &str) -> PathBuf {
        self.dir.join(f)
    }
    fn save_pending(&self) {
        write_json(&self.path("pending.json"), &self.pending);
    }
}

// ------------------------------------------------------------------ view model

#[derive(Serialize)]
pub struct SessionView {
    name: String,
    cards: Vec<Card>,
    skipped: Vec<String>,
    /// Exercises switched off in Settings (💪 Workouts column U).
    switched_off: Vec<String>,
    programme_plan: plan::Plan,
    best_plan: plan::Plan,
}


#[derive(Serialize)]
pub struct HistRow {
    date: NaiveDate,
    workout: String,
    exercise: String,
    weight: f64,
    sets: String,
    total_reps: f64,
    volume: f64,
    decision: String,
    verdict: Verdict,
    bodyweight: bool,
    pending: bool,
}

#[derive(Serialize)]
pub struct Status {
    mode: String,
    online: bool,
    message: String,
    last_sync: Option<String>,
    pending: usize,
    account: String,
    rows_left: Option<i64>,
    configured: bool,
}

#[derive(Serialize)]
pub struct View {
    status: Status,
    config: Config,
    today: Option<Today>,
    /// What the last session left on the bar and dumbbells.
    kit: Option<plan::Kit>,
    sessions: Vec<SessionView>,
    history: Vec<HistRow>,
    progress: Vec<ProgressRow>,
    /// 📚 Exercises, in sheet order.
    library: Vec<LibEx>,
    /// 📅 Plan slots with what each one uses today.
    plan: Vec<SlotView>,
    days: Vec<DayInfo>,
    suggestions: Vec<suggest::Suggestion>,
    volume: Vec<suggest::VolumeRow>,
    settings: Option<Settings>,
}

#[derive(Serialize)]
pub struct SlotView {
    #[serde(flatten)]
    slot: model::Slot,
    /// What it uses today ("" = switched off or nothing available).
    uses: String,
    /// Uses a stand-in because the slot's own kit is off.
    stand_in: bool,
}

/// The view the UI would get for `data`, connected and with nothing queued, as JSON.
/// Only for rendering the UI headlessly in tests (no window, no sheet).
#[doc(hidden)]
pub fn preview_view_json(data: Data) -> String {
    let inner = Inner {
        dir: std::env::temp_dir(),
        view_date: None,
        config: Config::default(),
        data: Some(data),
        pending: vec![],
        online: true,
        message: String::new(),
        last_sync: Some("12:00".into()),
        account: String::new(),
    };
    serde_json::to_string(&build_view(&inner)).unwrap()
}

fn build_view(s: &Inner) -> View {
    let configured = match s.config.mode.as_str() {
        "snapshot" => !s.config.snapshot_path.is_empty(),
        _ => !s.config.key_path.is_empty() && !s.config.sheet_id.is_empty(),
    };
    let mut view = View {
        status: Status {
            mode: s.config.mode.clone(),
            online: s.online,
            message: s.message.clone(),
            last_sync: s.last_sync.clone(),
            pending: s.pending.len(),
            account: s.account.clone(),
            rows_left: None,
            configured,
        },
        config: s.config.clone(),
        today: None,
        kit: None,
        sessions: vec![],
        history: vec![],
        progress: vec![],
        library: vec![],
        plan: vec![],
        days: vec![],
        suggestions: vec![],
        volume: vec![],
        settings: None,
    };
    let Some(base) = &s.data else { return view };
    let mut data = base.clone();
    // Queued entries: edits replace their row's entry, new ones go on the end.
    for p in &s.pending {
        let q = LogEntry { queued: true, ..p.clone() };
        match p.row.and_then(|r| data.log.iter().position(|e| e.row == Some(r))) {
            Some(i) => data.log[i] = q,
            None => data.log.push(q),
        }
    }
    let engine = Engine::new(&data.settings);
    let analysis: Vec<Analysis> = coach::analyse(&data, &engine);

    let last_row = base.log.iter().filter_map(|e| e.row).max().unwrap_or(model::LOG_FIRST_ROW - 1);
    view.status.rows_left = Some(model::LOG_LAST_FORMULA_ROW as i64 - last_row as i64 - s.pending.len() as i64);
    let today = Local::now().date_naive();
    let date = s.view_date.unwrap_or(today);
    view.today = Some(coach::today(&data.schedule, today));
    // Plates stay on between sessions: start from what's on the kit now, including
    // anything already logged on this date.
    let (start, from) = plan::kit_through(&data, &analysis, &engine, date);
    view.kit = Some(plan::Kit::new(&start, from));
    view.sessions = coach::sessions(&data)
        .into_iter()
        .map(|name| {
            let (cards, skipped) = coach::session_cards(&data, &engine, &analysis, &name, date);
            let (programme_plan, best_plan) = plan::plans_remaining(&cards, &engine, &start);
            let switched_off = data.plan.iter().filter(|p| p.day == name && !p.enabled).map(|p| p.exercise.clone()).collect();
            SessionView { name, cards, skipped, switched_off, programme_plan, best_plan }
        })
        .collect();
    view.history = data
        .log
        .iter()
        .zip(&analysis)
        .rev()
        .map(|(e, a)| HistRow {
            date: e.date,
            workout: e.workout.clone(),
            exercise: e.exercise.clone(),
            weight: e.weight,
            sets: coach::sets_text(e),
            total_reps: a.total_reps,
            volume: a.volume,
            decision: a.decision.clone(),
            verdict: a.verdict,
            bodyweight: a.load_type == engine::BODYWEIGHT,
            pending: e.queued || e.row.is_none(),
        })
        .collect();
    view.progress = coach::progress(&data, &engine, &analysis);
    view.library = data.library.clone();
    view.plan = data
        .plan
        .iter()
        .map(|slot| {
            let uses = coach::resolve(&data, &engine, slot).map(|l| l.name.clone()).unwrap_or_default();
            SlotView { stand_in: !uses.is_empty() && uses != slot.exercise, uses, slot: slot.clone() }
        })
        .collect();
    view.days = data.days.clone();
    view.suggestions = suggest::suggestions(&data, &engine, &analysis, today);
    view.volume = suggest::volume(&data, &engine, today);
    view.settings = Some(data.settings.clone());
    view
}

// ------------------------------------------------------------------ syncing

enum Source {
    Sheets(sheets::Client),
    Snapshot(PathBuf),
}

fn source(c: &Config) -> Result<Source, String> {
    match c.mode.as_str() {
        "snapshot" => {
            if c.snapshot_path.is_empty() {
                return Err("Pick an .xlsx snapshot in Settings.".into());
            }
            Ok(Source::Snapshot(PathBuf::from(&c.snapshot_path)))
        }
        _ => {
            if c.key_path.is_empty() {
                return Err("Add your service-account key file in Settings to connect to the sheet.".into());
            }
            // {:#} keeps the cause ("No such file or directory", "Permission denied"), not just the summary.
            sheets::Client::new(std::path::Path::new(&c.key_path), &c.sheet_id).map(Source::Sheets).map_err(|e| {
                let mut msg = format!("{e:#}");
                if !std::path::Path::new(&c.key_path).exists() {
                    let found = paths::key_candidates(paths::home_dir().as_deref());
                    msg += &match found.as_slice() {
                        [] => ". Click Browse next to the key box to pick the file.".to_string(),
                        [one] => format!(". Found a key at {}: click Browse and pick it, or paste that path.", one.display()),
                        many => format!(". Keys found: {}. Click Browse and pick yours.", many.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")),
                    };
                }
                msg
            })
        }
    }
}

async fn sync(state: &AppState) {
    let _one_at_a_time = state.1.lock().await;
    let config = state.0.lock().unwrap().config.clone();
    let src = match source(&config) {
        Ok(s) => s,
        Err(e) => {
            let mut s = state.0.lock().unwrap();
            s.online = false;
            s.message = e;
            return;
        }
    };
    match src {
        Source::Snapshot(p) => {
            let mut s = state.0.lock().unwrap();
            match snapshot::load(&p) {
                Ok(d) => {
                    s.data = Some(d);
                    s.online = false;
                    s.account.clear();
                    s.message = "Snapshot mode: reading a local .xlsx copy. Nothing is sent to the sheet; new entries stay queued on this computer.".into();
                    s.last_sync = Some(Local::now().format("%H:%M").to_string());
                }
                Err(e) => s.message = format!("Couldn't open snapshot: {e}"),
            }
        }
        Source::Sheets(client) => {
            let msg = send_pending(state, &client).await;
            let fetched = client.fetch().await;
            let mut s = state.0.lock().unwrap();
            s.account = client.account_email().to_string();
            match fetched {
                Ok(d) => {
                    write_json(&s.path("cache.json"), &d);
                    s.data = Some(d);
                    s.online = true;
                    s.last_sync = Some(Local::now().format("%H:%M").to_string());
                    s.message = msg;
                }
                Err(e) => {
                    s.online = false;
                    s.message = format!("Offline, showing the last copy saved on this computer. ({e})");
                }
            }
        }
    }
}

// ------------------------------------------------------------------ commands

#[tauri::command]
async fn load(state: State<'_, AppState>, refresh: bool, date: Option<String>) -> Result<View, String> {
    state.0.lock().unwrap().view_date = date.and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    let needs = refresh || state.0.lock().unwrap().last_sync.is_none();
    if needs {
        sync(&state).await;
    }
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Queue one exercise. Already queued for that date? Replace it, keeping the row
/// it targets. Otherwise add it (row Some = edit that row, None = new row).
fn enqueue(pending: &mut Vec<LogEntry>, e: LogEntry) {
    match pending.iter_mut().find(|p| p.date == e.date && p.exercise == e.exercise) {
        Some(p) => {
            let row = p.row.or(e.row);
            *p = LogEntry { row, ..e };
        }
        None => pending.push(e),
    }
}

/// After a send: drop exactly what was sent. Anything changed meanwhile stays,
/// and a new entry that has just been appended turns into an edit of its new row.
fn settle(pending: &mut Vec<LogEntry>, sent: &[LogEntry], new_rows: &[(NaiveDate, String, usize)]) {
    for done in sent {
        if let Some(i) = pending.iter().position(|p| p == done) {
            pending.remove(i);
        }
    }
    for p in pending.iter_mut().filter(|p| p.row.is_none()) {
        if let Some((_, _, row)) = new_rows.iter().find(|(d, ex, _)| *d == p.date && *ex == p.exercise) {
            p.row = Some(*row);
        }
    }
}

/// Send the local queue: edits update their own rows, new entries are appended.
/// Only what was actually sent leaves the queue, so an edit made while this was
/// in flight survives, retargeted at the row its entry just got. Returns a
/// message for the status bar ("" when everything went).
async fn send_pending(state: &AppState, client: &sheets::Client) -> String {
    let pending = state.0.lock().unwrap().pending.clone();
    if pending.is_empty() {
        return String::new();
    }
    let mut sent: Vec<LogEntry> = Vec::new();
    let mut new_rows: Vec<(NaiveDate, String, usize)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for e in pending.iter().filter(|e| e.row.is_some()) {
        match client.update_row(e).await {
            Ok(()) => sent.push(e.clone()),
            Err(err) => errors.push(format!("{}: {err:#}", e.exercise)),
        }
    }
    let appends: Vec<LogEntry> = pending.iter().filter(|e| e.row.is_none()).cloned().collect();
    if !appends.is_empty() {
        match client.append(&appends).await {
            Ok(first) => {
                for (k, e) in appends.into_iter().enumerate() {
                    new_rows.push((e.date, e.exercise.clone(), first + k));
                    sent.push(e);
                }
            }
            Err(err) => errors.push(format!("{err:#}")),
        }
    }
    let mut s = state.0.lock().unwrap();
    settle(&mut s.pending, &sent, &new_rows);
    s.save_pending();
    if errors.is_empty() { String::new() } else { format!("Couldn't save to the sheet: {}", errors.join("; ")) }
}

#[derive(Deserialize)]
pub struct NewEntry {
    date: String,
    workout: String,
    exercise: String,
    weight: f64,
    sets: Vec<Option<f64>>,
    /// The entry's sheet row when it's already logged (an edit), else None.
    #[serde(default)]
    row: Option<usize>,
}

/// Save one exercise as you go (the Today screen calls this per card). A new
/// exercise is appended; one already logged has its row updated in place; one
/// still waiting in the local queue is just replaced there.
#[tauri::command]
async fn save_entry(state: State<'_, AppState>, entry: NewEntry) -> Result<View, String> {
    {
        let date = NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d").map_err(|_| format!("bad date {}", entry.date))?;
        let mut sets = [None; 4];
        for (i, v) in entry.sets.into_iter().take(4).enumerate() {
            sets[i] = v;
        }
        if sets.iter().all(|v| v.is_none()) {
            return Err("Enter at least one set.".into());
        }
        let e = LogEntry { date, workout: entry.workout, exercise: entry.exercise, weight: entry.weight, sets, row: entry.row, queued: false };
        let mut s = state.0.lock().unwrap();
        enqueue(&mut s.pending, e);
        s.save_pending();
    }
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

#[tauri::command]
fn discard_pending(state: State<'_, AppState>) -> View {
    let mut s = state.0.lock().unwrap();
    s.pending.clear();
    s.save_pending();
    build_view(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ex: &str, reps: f64, row: Option<usize>) -> LogEntry {
        LogEntry {
            date: NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
            workout: "Pull".into(),
            exercise: ex.into(),
            weight: 10.0,
            sets: [Some(reps), Some(reps), None, None],
            row,
            queued: false,
        }
    }

    #[test]
    fn editing_a_queued_entry_replaces_it() {
        let mut q = vec![];
        enqueue(&mut q, entry("Row", 8.0, None));
        enqueue(&mut q, entry("Row", 10.0, None));
        assert_eq!(q, vec![entry("Row", 10.0, None)]);
        // An edit of a logged row keeps targeting that row.
        enqueue(&mut q, entry("Curl", 8.0, Some(80)));
        enqueue(&mut q, entry("Curl", 9.0, None));
        assert_eq!(q[1], entry("Curl", 9.0, Some(80)));
    }

    #[test]
    fn edit_made_during_a_send_survives_as_an_edit_of_the_new_row() {
        // Row 8 reps was being appended when it was edited to 10.
        let sent = vec![entry("Row", 8.0, None)];
        let mut q = vec![entry("Row", 10.0, None), entry("Curl", 9.0, Some(80))];
        settle(&mut q, &sent, &[(sent[0].date, "Row".into(), 81)]);
        assert_eq!(q, vec![entry("Row", 10.0, Some(81)), entry("Curl", 9.0, Some(80))]);
        // Sending those clears the queue.
        let sent2 = q.clone();
        settle(&mut q, &sent2, &[]);
        assert!(q.is_empty());
    }

}

#[tauri::command]
async fn save_config(state: State<'_, AppState>, mut config: Config) -> Result<View, String> {
    let home = paths::home_dir();
    // Whatever was typed for the key: find the file it means and keep the app's own copy.
    // If it can't be found, keep what was typed so the error (with suggestions) says so.
    let dir = state.0.lock().unwrap().dir.clone();
    config.key_path = match paths::find_file(&config.key_path, home.as_deref()) {
        Some(found) if found != dir.join("service-account.json") => {
            let bytes = std::fs::read(&found).map_err(|e| format!("can't read {}: {e}", found.display()))?;
            store_key(&bytes, &dir)?.display().to_string()
        }
        Some(found) => found.display().to_string(),
        None => paths::clean_path(&config.key_path),
    };
    config.snapshot_path = paths::find_file(&config.snapshot_path, home.as_deref())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| paths::clean_path(&config.snapshot_path));
    {
        let mut s = state.0.lock().unwrap();
        s.config = config;
        write_json(&s.path("config.json"), &s.config);
        s.last_sync = None;
    }
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Write one Yes/No switch to the live sheet, then re-sync so the view reflects it.
async fn set_switch(state: &AppState, tab: &str, row: usize, name_col: &str, expect: &str, col: &str, on: bool) -> Result<View, String> {
    let config = state.0.lock().unwrap().config.clone();
    let Source::Sheets(client) = source(&config)? else {
        return Err("Snapshot mode is read-only. Connect to the Google Sheet to switch things on or off.".into());
    };
    client
        .set_cell(tab, &format!("{name_col}{row}"), expect, &format!("{col}{row}"), if on { "Yes" } else { "No" })
        .await
        .map_err(|e| e.to_string())?;
    sync(state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Add a slot for `exercise` to `day`, using its library pattern, in the first empty
/// 📅 Plan row.
#[tauri::command]
async fn add_slot(state: State<'_, AppState>, day: String, exercise: String) -> Result<View, String> {
    let (config, data) = {
        let s = state.0.lock().unwrap();
        (s.config.clone(), s.data.clone().ok_or("Not synced yet.")?)
    };
    let lib = data.lib(&exercise).ok_or_else(|| format!("{exercise} isn't in 📚 Exercises."))?;
    if data.plan.iter().any(|s| s.day == day && s.exercise == exercise) {
        return Err(format!("{exercise} is already on {day}."));
    }
    let used: std::collections::HashSet<usize> = data.plan.iter().map(|p| p.row).collect();
    let row = (model::PLAN_FIRST_ROW..=model::PLAN_LAST_ROW)
        .find(|r| !used.contains(r))
        .ok_or("📅 Plan is full (rows 5-104). Remove an unused slot first.")?;
    let Source::Sheets(client) = source(&config)? else {
        return Err("Snapshot mode is read-only. Connect to the Google Sheet to change the plan.".into());
    };
    let values = vec![serde_json::json!(day), serde_json::json!(lib.pattern), serde_json::json!(exercise), serde_json::json!(""), serde_json::json!("Yes")];
    client.write_plan_row(row, values).await.map_err(|e| format!("{e:#}"))?;
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Put a different exercise in a slot (📅 Plan column C).
#[tauri::command]
async fn swap_slot(state: State<'_, AppState>, row: usize, from: String, to: String) -> Result<View, String> {
    let (config, known) = {
        let s = state.0.lock().unwrap();
        (s.config.clone(), s.data.as_ref().is_some_and(|d| d.lib(&to).is_some()))
    };
    if !known {
        return Err(format!("{to} isn't in 📚 Exercises."));
    }
    let Source::Sheets(client) = source(&config)? else {
        return Err("Snapshot mode is read-only. Connect to the Google Sheet to change the plan.".into());
    };
    client.set_cell(model::TAB_PLAN, &format!("C{row}"), &from, &format!("C{row}"), &to).await.map_err(|e| format!("{e:#}"))?;
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Set a slot's sets (📅 Plan column D; None = back to the library's).
#[tauri::command]
async fn set_slot_sets(state: State<'_, AppState>, row: usize, exercise: String, sets: Option<u32>) -> Result<View, String> {
    let config = state.0.lock().unwrap().config.clone();
    let Source::Sheets(client) = source(&config)? else {
        return Err("Snapshot mode is read-only. Connect to the Google Sheet to change the plan.".into());
    };
    let value = sets.map(|n| n.to_string()).unwrap_or_default();
    client.set_cell(model::TAB_PLAN, &format!("C{row}"), &exercise, &format!("D{row}"), &value).await.map_err(|e| format!("{e:#}"))?;
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Remove a slot: clear its 📅 Plan row (A:E).
#[tauri::command]
async fn remove_slot(state: State<'_, AppState>, row: usize, exercise: String) -> Result<View, String> {
    let config = state.0.lock().unwrap().config.clone();
    let Source::Sheets(client) = source(&config)? else {
        return Err("Snapshot mode is read-only. Connect to the Google Sheet to change the plan.".into());
    };
    let blank = vec![serde_json::json!(""); 5];
    client.set_cells(model::TAB_PLAN, &format!("C{row}"), &exercise, &format!("A{row}:E{row}"), blank).await.map_err(|e| format!("{e:#}"))?;
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

/// Switch a piece of kit on or off (⚙️ Settings, Enabled? column).
#[tauri::command]
async fn set_equipment(state: State<'_, AppState>, row: usize, name: String, enabled: bool) -> Result<View, String> {
    set_switch(&state, model::TAB_SETTINGS, row, "A", &name, model::COL_EQUIPMENT_ON, enabled).await
}

/// Switch a plan slot on or off (📅 Plan, On? column).
#[tauri::command]
async fn set_slot(state: State<'_, AppState>, row: usize, exercise: String, enabled: bool) -> Result<View, String> {
    set_switch(&state, model::TAB_PLAN, row, "C", &exercise, "E", enabled).await
}

#[derive(Serialize)]
pub struct UpdateInfo {
    current: String,
    /// Newer version on GitHub, or None when this is the latest.
    latest: Option<String>,
    notes: String,
    /// False where the app can't replace itself (a .deb install on Linux; only the AppImage can).
    can_install: bool,
}

/// Ask GitHub Releases (the updater endpoint in tauri.conf.json) for a newer version.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;
    let current = app.package_info().version.to_string();
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(UpdateInfo {
        current,
        latest: update.as_ref().map(|u| u.version.clone()),
        notes: update.and_then(|u| u.body).unwrap_or_default(),
        can_install: !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some(),
    })
}

/// Download the newer version, check its signature, replace the app and restart.
/// Queued entries and typed-in reps are already saved to disk, so nothing is lost.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let Some(update) = update else { return Err("You're already on the latest version.".into()) };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}

/// Store a file picked in Settings inside the app's own folder and return its path.
/// The browser-side picker hands over the file's contents rather than a path, which works
/// the same on every platform (the native dialog's path came back empty on Ubuntu).
#[tauri::command]
fn import_file(state: State<'_, AppState>, kind: String, data: String) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| format!("couldn't read the file: {e}"))?;
    let dir = state.0.lock().unwrap().dir.clone();
    let path = match kind.as_str() {
        "key" => store_key(&bytes, &dir)?,
        "snapshot" => {
            let p = dir.join("snapshot.xlsx");
            std::fs::write(&p, &bytes).map_err(|e| format!("couldn't save {}: {e}", p.display()))?;
            p
        }
        _ => return Err(format!("unknown file kind {kind}")),
    };
    Ok(path.display().to_string())
}

/// Check `bytes` is a service-account key and keep a private copy in the app's folder,
/// so the original can be moved or deleted.
fn store_key(bytes: &[u8], dir: &std::path::Path) -> Result<PathBuf, String> {
    let sa: sheets::ServiceAccount = serde_json::from_slice(bytes).map_err(|_| "That file isn't a Google service-account JSON key.".to_string())?;
    if sa.private_key.is_empty() {
        return Err("That key file has no private key in it.".into());
    }
    let path = dir.join("service-account.json");
    std::fs::write(&path, bytes).map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

/// Linux desktop details for Settings ("" elsewhere).
#[tauri::command]
fn platform_info() -> String {
    #[cfg(target_os = "linux")]
    return linux::diagnostics();
    #[cfg(not(target_os = "linux"))]
    String::new()
}

#[tauri::command]
fn data_dir(state: State<'_, AppState>) -> String {
    state.0.lock().unwrap().dir.display().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    linux::pin_app_id();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            linux::integrate_appimage();
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let config: Config = read_json(&dir.join("config.json")).unwrap_or_default();
            let data: Option<Data> = if config.mode == "sheets" { read_json(&dir.join("cache.json")) } else { None };
            let pending: Vec<LogEntry> = read_json(&dir.join("pending.json")).unwrap_or_default();
            app.manage(AppState(Mutex::new(Inner {
                dir,
                view_date: None,
                config,
                data,
                pending,
                online: false,
                message: String::new(),
                last_sync: None,
                account: String::new(),
            }), tauri::async_runtime::Mutex::new(())));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load, save_entry, discard_pending, save_config, data_dir, check_update, install_update, set_equipment, set_slot, add_slot, swap_slot, set_slot_sets, remove_slot, import_file, platform_info])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
