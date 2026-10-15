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
    let (cards, _) = coach::session_cards(&data, &engine, &a, &session, chrono::NaiveDate::MAX);
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

#[test]
fn plate_plans() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let data = snapshot::load(std::path::Path::new(&path)).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    for s in coach::sessions(&data) {
        let (cards, _) = coach::session_cards(&data, &engine, &a, &s, chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        let (start, from) = workout_tracker_lib::plan::kit_through(&data, &a, &engine, today);
        eprintln!("\nkit through {today} (from {from:?}): {:?}", workout_tracker_lib::plan::Kit::new(&start, from));
        let (p, b) = workout_tracker_lib::plan::plans_remaining(&cards, &engine, &start);
        // Logged exercises come first with nothing to change.
        let done = cards.iter().filter(|c| c.logged.is_some()).count();
        for &i in &b.order[..done] {
            assert!(cards[i].logged.is_some() && b.changes[i].is_empty(), "{s}: logged {} should be first, unchanged", cards[i].exercise);
        }
        assert!(b.moves <= p.moves);
        eprintln!("\n{s}: programme {} moves (heaviest-first loading {}), best {} moves (heaviest-first {})", p.moves, p.naive_moves, b.moves, b.naive_moves);
        assert!(p.moves <= p.naive_moves);
        for &i in &b.order {
            eprintln!("  {:<28} {:<16} {}", cards[i].exercise, cards[i].recommended.split(" — ").next().unwrap(), b.changes[i].join("\n      "));
        }
    }
}

#[test]
fn schedule_morning_sessions_exist() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let data = snapshot::load(std::path::Path::new(&path)).unwrap();
    let sessions = coach::sessions(&data);
    for d in &data.schedule {
        assert!(d.morning.is_empty() || sessions.contains(&d.morning), "{}: {} isn't a session on 💪 Workouts", d.day, d.morning);
    }
    eprintln!("morning: {:?}", data.schedule.iter().map(|d| d.morning.as_str()).collect::<Vec<_>>());
}

/// After every logged day, the bar and the dumbbells hold exactly the plates of the most
/// recent exercise that used them (up to and including that day), and the kit says which
/// day left them on.
#[test]
fn kit_matches_latest_logged_load() {
    use workout_tracker_lib::engine::{BARBELL, DUMBBELL};
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let data = snapshot::load(std::path::Path::new(&path)).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    let mut days: Vec<_> = data.log.iter().map(|e| e.date).collect();
    days.sort();
    days.dedup();
    for &day in &days {
        let (state, from) = workout_tracker_lib::plan::kit_through(&data, &a, &engine, day);
        let plate_day = (0..data.log.len())
            .filter(|&i| data.log[i].date <= day && [BARBELL, DUMBBELL].contains(&a[i].load_type.as_str()))
            .map(|i| data.log[i].date)
            .max();
        assert_eq!(from, plate_day, "{day}: kit should be left from the latest plate-loaded day");
        for (imp, lt) in [(0, BARBELL), (1, DUMBBELL)] {
            // Latest entry for this implement: last day first, then the last one logged that day.
            let last = (0..data.log.len())
                .filter(|&i| data.log[i].date <= day && a[i].load_type == lt)
                .max_by_key(|&i| (data.log[i].date, i));
            let want = last.and_then(|i| engine.plates_for(lt, data.log[i].weight)).unwrap_or([0; 4]);
            let mut got = [0u32; 4];
            for &p in &state[imp] {
                got[p as usize] += 1;
            }
            // Plates are shared: a later exercise on the other implement may have taken some
            // off. So this implement holds its latest load, minus only what was taken.
            let taken_later = last.is_some_and(|li| {
                (0..data.log.len()).any(|j| data.log[j].date <= day && (data.log[j].date, j) > (data.log[li].date, li) && a[j].load_type != lt && [BARBELL, DUMBBELL].contains(&a[j].load_type.as_str()))
            });
            let ok = if taken_later { (0..4).all(|k| got[k] <= want[k]) } else { got == want };
            assert!(ok, "{day} {lt}: kit {got:?} should be {want:?}{} for {:?}", if taken_later { " or fewer" } else { "" }, last.map(|i| (&data.log[i].exercise, data.log[i].weight)));
        }
    }
    let (state, from) = workout_tracker_lib::plan::kit_through(&data, &a, &engine, *days.last().unwrap());
    eprintln!("{} days checked; kit now (from {from:?}): {:?}", days.len(), workout_tracker_lib::plan::Kit::new(&state, from));
}
