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
        let p = data.programme.iter().find(|p| p.enabled).expect("an exercise that is on");
        let value = if p.enabled { "Yes" } else { "No" };
        // Same value back into the same cell: succeeds, changes nothing.
        client
            .set_cell(model::TAB_WORKOUTS, &format!("B{}", p.row), &p.exercise, &format!("{}{}", model::COL_EXERCISE_ON, p.row), value)
            .await
            .unwrap();
        // Wrong name for that row: refused before writing.
        let err = client
            .set_cell(model::TAB_WORKOUTS, &format!("B{}", p.row), "Not An Exercise", &format!("{}{}", model::COL_EXERCISE_ON, p.row), "No")
            .await
            .unwrap_err();
        eprintln!("row {} {}: write ok; mismatch refused: {err}", p.row, p.exercise);
        let after = client.fetch().await.unwrap();
        assert!(after.programme.iter().find(|q| q.row == p.row).unwrap().enabled, "exercise must still be on");
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
