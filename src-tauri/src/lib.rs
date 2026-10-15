pub mod coach;
pub mod engine;
pub mod model;
pub mod plan;
pub mod sheets;
pub mod snapshot;

use chrono::{Local, NaiveDate};
use coach::{Analysis, Card, ProgressRow, Today, Verdict};
use engine::Engine;
use model::{Data, LogEntry, ProgRow, Settings};
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
    config: Config,
    data: Option<Data>,
    pending: Vec<LogEntry>,
    online: bool,
    message: String,
    last_sync: Option<String>,
    account: String,
}

pub struct AppState(Mutex<Inner>);

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
    sessions: Vec<SessionView>,
    history: Vec<HistRow>,
    progress: Vec<ProgressRow>,
    programme: Vec<ProgRow>,
    settings: Option<Settings>,
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
        sessions: vec![],
        history: vec![],
        progress: vec![],
        programme: vec![],
        settings: None,
    };
    let Some(base) = &s.data else { return view };
    let mut data = base.clone();
    data.log.extend(s.pending.iter().cloned());
    let engine = Engine::new(&data.settings);
    let analysis: Vec<Analysis> = coach::analyse(&data, &engine);

    let last_row = base.log.iter().filter_map(|e| e.row).max().unwrap_or(model::LOG_FIRST_ROW - 1);
    view.status.rows_left = Some(model::LOG_LAST_FORMULA_ROW as i64 - last_row as i64 - s.pending.len() as i64);
    view.today = Some(coach::today(&data.schedule, Local::now().date_naive()));
    view.sessions = coach::sessions(&data)
        .into_iter()
        .map(|name| {
            let (cards, skipped) = coach::session_cards(&data, &engine, &analysis, &name);
            let (programme_plan, best_plan) = plan::plans(&cards, &engine);
            SessionView { name, cards, skipped, programme_plan, best_plan }
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
            pending: e.row.is_none(),
        })
        .collect();
    view.progress = coach::progress(&data, &engine, &analysis);
    view.programme = data.programme.clone();
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
            sheets::Client::new(std::path::Path::new(&c.key_path), &c.sheet_id).map(Source::Sheets).map_err(|e| e.to_string())
        }
    }
}

async fn sync(state: &AppState) {
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
            let pending = state.0.lock().unwrap().pending.clone();
            let mut msg = String::new();
            if !pending.is_empty() {
                match client.append(&pending).await {
                    Ok(row) => {
                        let mut s = state.0.lock().unwrap();
                        s.pending.clear();
                        s.save_pending();
                        msg = format!("Saved {} exercise(s) to the sheet from row {row}.", pending.len());
                    }
                    Err(e) => msg = format!("Couldn't save queued entries: {e}"),
                }
            }
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
async fn load(state: State<'_, AppState>, refresh: bool) -> Result<View, String> {
    let needs = refresh || state.0.lock().unwrap().last_sync.is_none();
    if needs {
        sync(&state).await;
    }
    Ok(build_view(&state.0.lock().unwrap()))
}

#[derive(Deserialize)]
pub struct NewEntry {
    date: String,
    workout: String,
    exercise: String,
    weight: f64,
    sets: Vec<Option<f64>>,
}

#[tauri::command]
async fn save_entries(state: State<'_, AppState>, entries: Vec<NewEntry>) -> Result<View, String> {
    {
        let mut s = state.0.lock().unwrap();
        for e in entries {
            let date = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").map_err(|_| format!("bad date {}", e.date))?;
            let mut sets = [None; 4];
            for (i, v) in e.sets.into_iter().take(4).enumerate() {
                sets[i] = v;
            }
            if sets.iter().all(|v| v.is_none()) {
                continue;
            }
            s.pending.push(LogEntry { date, workout: e.workout, exercise: e.exercise, weight: e.weight, sets, row: None });
        }
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

#[tauri::command]
async fn save_config(state: State<'_, AppState>, config: Config) -> Result<View, String> {
    {
        let mut s = state.0.lock().unwrap();
        s.config = config;
        write_json(&s.path("config.json"), &s.config);
        s.last_sync = None;
    }
    sync(&state).await;
    Ok(build_view(&state.0.lock().unwrap()))
}

#[tauri::command]
fn data_dir(state: State<'_, AppState>) -> String {
    state.0.lock().unwrap().dir.display().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let config: Config = read_json(&dir.join("config.json")).unwrap_or_default();
            let data: Option<Data> = if config.mode == "sheets" { read_json(&dir.join("cache.json")) } else { None };
            let pending: Vec<LogEntry> = read_json(&dir.join("pending.json")).unwrap_or_default();
            app.manage(AppState(Mutex::new(Inner {
                dir,
                config,
                data,
                pending,
                online: false,
                message: String::new(),
                last_sync: None,
                account: String::new(),
            })));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load, save_entries, discard_pending, save_config, data_dir])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
