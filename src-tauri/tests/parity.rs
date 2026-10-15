//! Checks the Rust coach against the values the Google Sheet itself computed.
//! Point WT_SNAPSHOT at an .xlsx export of the tracker:
//!   WT_SNAPSHOT=~/Downloads/tracker.xlsx cargo test -- --nocapture
use workout_tracker_lib::{coach, engine::Engine, snapshot};

#[test]
fn log_columns_match_sheet() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else {
        eprintln!("WT_SNAPSHOT not set, skipping");
        return;
    };
    let path = std::path::PathBuf::from(path);
    let data = snapshot::load(&path).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    // K Previous, L Recommended, M Decision
    let k = snapshot::log_column(&path, 10).unwrap();
    let l = snapshot::log_column(&path, 11).unwrap();
    let m = snapshot::log_column(&path, 12).unwrap();
    let mut bad = 0;
    for (i, an) in a.iter().enumerate() {
        for (name, ours, theirs) in [("K", &an.previous_text, &k[i]), ("L", &an.recommended, &l[i]), ("M", &an.decision, &m[i])] {
            if ours != theirs {
                bad += 1;
                eprintln!("row {} {} {}:\n  sheet: {}\n  ours:  {}", i + 2, data.log[i].exercise, name, theirs, ours);
            }
        }
    }
    eprintln!("{} log rows checked, {} mismatches", a.len(), bad);
    assert_eq!(bad, 0);
}

fn tab(path: &std::path::Path, name: &str) -> calamine::Range<calamine::Data> {
    use calamine::Reader;
    calamine::open_workbook_auto(path).unwrap().worksheet_range(name).unwrap()
}
fn cell(r: &calamine::Range<calamine::Data>, row: u32, col: u32) -> String {
    r.get_value((row, col)).map(|v| v.to_string()).unwrap_or_default()
}

#[test]
fn dashboard_and_progress_match_sheet() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let path = std::path::PathBuf::from(path);
    let data = snapshot::load(&path).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    let mut bad = 0;

    let dash = tab(&path, "🏠 Dashboard");
    let session = cell(&dash, 3, 2);
    let (cards, _) = coach::session_cards(&data, &engine, &a, &session);
    for (i, c) in cards.iter().enumerate() {
        let r = 6 + i as u32;
        for (name, ours, theirs) in [
            ("name", c.exercise.clone(), cell(&dash, r, 0)),
            ("rec", c.recommended.clone(), cell(&dash, r, 4)),
            ("reps", c.previous_reps.clone(), cell(&dash, r, 6)),
            ("trend", c.trend.clone(), cell(&dash, r, 7)),
            ("todo", c.instruction.clone(), cell(&dash, r, 9)),
        ] {
            if ours != theirs {
                bad += 1;
                eprintln!("dashboard {session} {} {name}:\n  sheet: {theirs}\n  ours:  {ours}", c.exercise);
            }
        }
    }
    eprintln!("dashboard: {} cards for {session}", cards.len());

    let prog = tab(&path, "📈 Progress");
    let rows = coach::progress(&data, &engine, &a);
    for (i, p) in rows.iter().enumerate() {
        let r = 3 + i as u32;
        for (name, ours, theirs) in [
            ("name", p.exercise.clone(), cell(&prog, r, 0)),
            // the sheet shows "kg" through a number format, so compare the bare number
            ("current", p.current.trim_end_matches("kg").to_string(), cell(&prog, r, 1)),
            ("best", p.best.trim_end_matches("kg").to_string(), cell(&prog, r, 2)),
            ("sessions", p.sessions.to_string(), cell(&prog, r, 3)),
            ("change", p.change.clone(), cell(&prog, r, 4)),
            ("next", p.next.clone(), cell(&prog, r, 5)),
        ] {
            if ours != theirs {
                bad += 1;
                eprintln!("progress {} {name}:\n  sheet: {theirs}\n  ours:  {ours}", p.exercise);
            }
        }
    }
    eprintln!("progress: {} rows", rows.len());
    assert_eq!(bad, 0);
}
