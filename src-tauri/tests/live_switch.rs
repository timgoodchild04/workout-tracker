//! Opt-in check of the Settings switch write path against the live sheet. It writes the value a
//! cell already holds, so nothing changes. Run with the service-account key:
//!   WT_KEY=/path/to/key.json cargo test --test live_switch -- --ignored --nocapture
use workout_tracker_lib::{model, sheets, DEFAULT_SHEET_ID};

#[test]
#[ignore]
fn switch_write_is_guarded() {
    let key = std::env::var("WT_KEY").expect("set WT_KEY to the service-account key file");
    let client = sheets::Client::new(std::path::Path::new(&key), DEFAULT_SHEET_ID).unwrap();
    tauri::async_runtime::block_on(async {
        let data = client.fetch().await.unwrap();
        let p = data.plan.iter().find(|p| p.enabled).expect("a slot that is on");
        // Same value back into the same cell: succeeds, changes nothing.
        client.set_cell(model::TAB_PLAN, &format!("C{}", p.row), &p.exercise, &format!("E{}", p.row), "Yes").await.unwrap();
        // Wrong name for that row: refused before writing.
        let err = client
            .set_cell(model::TAB_PLAN, &format!("C{}", p.row), "Not An Exercise", &format!("E{}", p.row), "No")
            .await
            .unwrap_err();
        eprintln!("plan row {} {}: write ok; mismatch refused: {err}", p.row, p.exercise);
        let after = client.fetch().await.unwrap();
        assert!(after.plan.iter().find(|q| q.row == p.row).unwrap().enabled, "slot must still be on");
        let e = &after.settings.equipment[0];
        eprintln!("equipment row {} = {} ({})", e.row, e.name, if e.enabled { "on" } else { "off" });
    });
}

/// Opt-in: the Today screen's edit path. Rewrites the newest log row with the values it
/// already holds (so nothing changes), and checks a row/exercise mismatch is refused.
#[test]
#[ignore]
fn log_row_update_is_guarded() {
    let key = std::env::var("WT_KEY").expect("set WT_KEY to the service-account key file");
    let client = sheets::Client::new(std::path::Path::new(&key), DEFAULT_SHEET_ID).unwrap();
    tauri::async_runtime::block_on(async {
        let data = client.fetch().await.unwrap();
        let last = data.log.iter().max_by_key(|e| e.row).unwrap().clone();
        client.update_row(&last).await.unwrap();
        let wrong = model::LogEntry { exercise: "Not An Exercise".into(), ..last.clone() };
        let err = client.update_row(&wrong).await.unwrap_err();
        let after = client.fetch().await.unwrap();
        let again = after.log.iter().find(|e| e.row == last.row).unwrap();
        assert_eq!((again.date, &again.exercise, again.weight, again.sets), (last.date, &last.exercise, last.weight, last.sets));
        eprintln!("row {:?} {} rewritten unchanged; mismatch refused: {err}", last.row, last.exercise);
    });
}

/// Opt-in: adding an exercise never overwrites a row that's in use. Writes nothing.
#[test]
#[ignore]
fn plan_row_write_needs_an_empty_row() {
    let key = std::env::var("WT_KEY").expect("set WT_KEY to the service-account key file");
    let client = sheets::Client::new(std::path::Path::new(&key), DEFAULT_SHEET_ID).unwrap();
    tauri::async_runtime::block_on(async {
        let data = client.fetch().await.unwrap();
        let used = data.plan[0].row;
        let err = client.write_plan_row(used, vec![serde_json::json!("x"); 5]).await.unwrap_err();
        let after = client.fetch().await.unwrap();
        assert_eq!(after.plan[0].exercise, data.plan[0].exercise);
        eprintln!("plan row {used} ({}) in use: refused: {err}", data.plan[0].exercise);
    });
}

/// Opt-in: deleting from History refuses a row that doesn't match. Writes nothing.
#[test]
#[ignore]
fn log_row_delete_is_guarded() {
    let key = std::env::var("WT_KEY").expect("set WT_KEY to the service-account key file");
    let client = sheets::Client::new(std::path::Path::new(&key), DEFAULT_SHEET_ID).unwrap();
    tauri::async_runtime::block_on(async {
        let data = client.fetch().await.unwrap();
        let last = data.log.iter().max_by_key(|e| e.row).unwrap();
        let err = client.clear_log_row(last.row.unwrap(), last.date, "Not An Exercise").await.unwrap_err();
        let after = client.fetch().await.unwrap();
        assert!(after.log.iter().any(|e| e.row == last.row && e.exercise == last.exercise), "row must be untouched");
        eprintln!("delete of row {:?} with the wrong exercise refused: {err}", last.row);
    });
}
