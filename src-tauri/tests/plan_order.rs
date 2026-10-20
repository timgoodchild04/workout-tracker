//! The fewest-plate-changes order for a session that's partly logged.
use chrono::NaiveDate;
use serde_json::json;
use workout_tracker_lib::{coach, engine::Engine, model::Data, plan};

fn lib(row: usize, name: &str, equipment: &str, rep_low: f64, start: f64) -> serde_json::Value {
    json!({"row": row, "name": name, "pattern": name, "primary": "Back", "secondary": [], "equipment": equipment, "sets": 3,
           "rep_low": rep_low, "rep_high": rep_low + 4.0, "start_weight": start, "video": "", "easier": "", "harder": "", "fits": ["Pull"], "notes": ""})
}

#[test]
fn once_the_main_lift_is_done_the_rest_reorder_freely() {
    let pull = "Day 2 — Pull";
    let slot = |row: usize, ex: &str| json!({"row": row, "day": pull, "pattern": ex, "exercise": ex, "sets": null, "enabled": true});
    let log = |row: usize, date: &str, w: &str, ex: &str, kg: f64| json!({"date": date, "workout": w, "exercise": ex, "weight": kg, "sets": [12.0, 12.0, 12.0, null], "row": row});
    let data: Data = serde_json::from_value(json!({
        "settings": {"bar_weight": 0.0, "handle_weight": 0.0, "jump_pct": 0.15, "jump_abs": 2.5, "ceiling_streak": 1.0, "drop_pct": 0.25, "isolation_rep_low": 10.0,
            "plates": [{"weight": 0.5, "owned": 4.0, "enabled": true}, {"weight": 1.25, "owned": 12.0, "enabled": true}, {"weight": 2.5, "owned": 8.0, "enabled": true}, {"weight": 5.0, "owned": 4.0, "enabled": true}],
            "equipment": [{"row": 33, "name": "EZ Barbell", "enabled": true, "load_type": "Barbell plates", "notes": ""}, {"row": 34, "name": "Dumbbell", "enabled": true, "load_type": "Dumbbell plates", "notes": ""}],
            "ladder": []},
        "library": [lib(30, "Barbell Row", "EZ Barbell", 8.0, 25.0), lib(31, "Barbell Curl", "EZ Barbell", 8.0, 12.5),
                    lib(32, "Hammer Curl", "Dumbbell", 10.0, 5.0), lib(33, "Rear Delt Raise", "Dumbbell", 10.0, 2.5), lib(34, "Lateral Raise", "Dumbbell", 10.0, 2.5)],
        "plan": [slot(9, "Barbell Row"), slot(10, "Barbell Curl"), slot(11, "Hammer Curl"), slot(12, "Rear Delt Raise")],
        "days": [{"name": pull, "focus": "Pull"}], "targets": [], "session_order": "Rotation", "deload_every": 6.0, "deloads": [], "schedule": [],
        // Yesterday left 2.5kg dumbbells (1.25kg each end); today the two barbell lifts are done.
        "log": [log(2, "2026-10-19", "Day 1 — Push", "Lateral Raise", 2.5),
                log(3, "2026-10-20", pull, "Barbell Row", 25.0), log(4, "2026-10-20", pull, "Barbell Curl", 12.5)],
        "marks": []
    })).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    let (start, _) = plan::kit_through(&data, &a, &engine, today);
    let (cards, _) = coach::session_cards(&data, &engine, &a, pull, today);
    let names = |order: &[usize]| order.iter().map(|&i| cards[i].exercise.as_str()).collect::<Vec<_>>();
    let (programme, best) = plan::plans_remaining(&cards, &engine, &start);
    // Rear Delt Raise uses the 2.5kg dumbbells as they are, so it goes before Hammer Curl.
    assert_eq!(names(&best.order), ["Barbell Row", "Barbell Curl", "Rear Delt Raise", "Hammer Curl"]);
    assert!(best.moves < programme.moves, "best {} vs programme {}", best.moves, programme.moves);
}
