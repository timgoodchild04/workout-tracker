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

/// Every 📅 Plan slot uses the same exercise in the app as in the sheet's own
/// "Use exercise" column (H), including stand-ins when kit is switched off.
#[test]
fn plan_slots_resolve_like_the_sheet() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let path = std::path::PathBuf::from(path);
    let data = snapshot::load(&path).unwrap();
    let engine = Engine::new(&data.settings);
    let plan = tab(&path, workout_tracker_lib::model::TAB_PLAN);
    let mut bad = 0;
    for slot in &data.plan {
        let ours = coach::resolve(&data, &engine, slot).map(|l| l.name.clone()).unwrap_or_default();
        let sheet = cell(&plan, slot.row as u32 - 1, 7);
        if ours != sheet {
            bad += 1;
            eprintln!("plan row {} {} / {}: sheet uses {sheet:?}, app uses {ours:?}", slot.row, slot.day, slot.exercise);
        }
    }
    let stand_ins = data.plan.iter().filter(|s| coach::resolve(&data, &engine, s).is_some_and(|l| l.name != s.exercise)).count();
    eprintln!("{} plan slots checked, {stand_ins} stand-ins, {bad} mismatches", data.plan.len());
    assert_eq!(bad, 0);
}

/// Print the coach's suggestions for the snapshot (no assertions beyond "it runs").
#[test]
fn suggestions_run() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let data = snapshot::load(std::path::Path::new(&path)).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    let vol = workout_tracker_lib::suggest::planned_volume(&data, &engine);
    let mut v: Vec<_> = vol.iter().collect();
    v.sort_by(|x, y| x.0.cmp(y.0));
    eprintln!("planned sets/week: {v:?}");
    for s in workout_tracker_lib::suggest::suggestions(&data, &engine, &a, chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()) {
        eprintln!("[{}] {} — {} {:?}", s.kind, s.title, s.detail, s.action);
    }
}

/// Opt-in: write the UI's view for the snapshot to WT_VIEW_OUT, for headless screenshots.
#[test]
#[ignore]
fn write_preview_view() {
    let (Ok(path), Ok(out)) = (std::env::var("WT_SNAPSHOT"), std::env::var("WT_VIEW_OUT")) else { return };
    let data = snapshot::load(std::path::Path::new(&path)).unwrap();
    std::fs::write(out, workout_tracker_lib::preview_view_json(data)).unwrap();
}

/// Ease-back, warm-ups and personal bests on the real log.
#[test]
fn ease_back_warmups_and_pbs() {
    use workout_tracker_lib::coach::EASE_BACK_SHARE;
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let data = snapshot::load(std::path::Path::new(&path)).unwrap();
    let engine = Engine::new(&data.settings);
    let a = coach::analyse(&data, &engine);
    let d = |m, day| chrono::NaiveDate::from_ymd_opt(2026, m, day).unwrap();

    // Push viewed on 12 Oct: Overhead Press was last done on 31 Aug at 11kg (42 days).
    let (cards, _) = coach::session_cards(&data, &engine, &a, "🦾 Day 1 — Push", d(10, 12));
    let ohp = cards.iter().find(|c| c.exercise == "Standing Overhead Press").unwrap();
    let eb = ohp.ease_back.as_ref().expect("OHP after a 6-week break should ease back");
    assert_eq!((eb.days, eb.last_kg), (42, 11.0));
    assert!(eb.kg <= 11.0 * EASE_BACK_SHARE + 1e-9 && eb.kg < ohp.recommended_kg, "{eb:?}");
    eprintln!("OHP 12 Oct: usual {} -> ease back {} ({} days off)", ohp.recommended, eb.text, eb.days);
    // Viewed on 15 Oct nothing is 10+ days stale for Pull.
    let (pull, _) = coach::session_cards(&data, &engine, &a, "🐺 Day 2 — Pull", d(10, 16));
    assert!(pull.iter().all(|c| c.ease_back.is_none()));

    // Warm-ups: rising, lighter than the work, only on compound plate-loaded lifts.
    for (day, date) in [("🦾 Day 1 — Push", d(10, 16)), ("🐺 Day 2 — Pull", d(10, 16)), ("🦵 Day 3 — Legs", d(10, 16))] {
        let (cards, _) = coach::session_cards(&data, &engine, &a, day, date);
        for c in &cards {
            let work = c.ease_back.as_ref().map_or(c.recommended_kg, |e| e.kg);
            let kgs: Vec<f64> = c.warmups.iter().map(|w| w.kg).collect();
            assert!(kgs.windows(2).all(|w| w[0] < w[1]) && kgs.iter().all(|&k| k < work), "{} {kgs:?} vs {work}", c.exercise);
            if !c.warmups.is_empty() {
                assert!(coach::COMPOUND_PATTERNS.contains(&c.pattern.as_str()), "{} ({}) isn't a compound lift", c.exercise, c.pattern);
                eprintln!("{:<26} work {:>5}kg  warm-up {}", c.exercise, work, c.warmups.iter().map(|w| format!("{} {}kg×{}", w.label, w.kg, w.reps)).collect::<Vec<_>>().join(", "));
            }
        }
    }

    // Personal bests.
    let pbs = coach::personal_bests(&data, &a);
    let find = |ex: &str, date| (0..data.log.len()).find(|&i| data.log[i].exercise == ex && data.log[i].date == date).unwrap();
    assert!(pbs[find("Barbell Row", d(9, 1))], "first time at 23.5kg");
    assert!(!pbs[find("Barbell Row", d(10, 15))], "same 23.5kg x 48 again");
    assert!(!pbs[find("Barbell Row", d(8, 5))], "first ever entry");
    eprintln!("{} PBs in {} entries", pbs.iter().filter(|&&p| p).count(), pbs.len());
}

/// Rotation: rounds of all four days; a skipped day stays owed and comes next.
#[test]
fn rotation_keeps_skipped_days_owed() {
    let Ok(path) = std::env::var("WT_SNAPSHOT") else { return };
    let mut data = snapshot::load(std::path::Path::new(&path)).unwrap();
    let d = |m, day| chrono::NaiveDate::from_ymd_opt(2026, m, day).unwrap();
    assert!(!coach::today_for(&data, d(10, 16)).rotation, "weekday schedule by default");
    data.session_order = workout_tracker_lib::model::ROTATION.into();
    let real = coach::today_for(&data, d(10, 16));
    eprintln!("real log, 16 Oct: {} ({})", real.session, real.notes);
    assert_eq!(real.morning, "🌅 Morning", "Morning keeps its weekday slot");

    let (push, pull, legs, upper) = ("🦾 Day 1 — Push", "🐺 Day 2 — Pull", "🦵 Day 3 — Legs", "💪 Day 4 — Upper");
    let entry = |day: u32, w: &str| workout_tracker_lib::model::LogEntry {
        date: d(11, day), workout: w.into(), exercise: "x".into(), weight: 0.0, sets: [Some(1.0), None, None, None], row: None, queued: false,
    };
    let next_after = |data: &mut workout_tracker_lib::model::Data, done: &[(u32, &str)]| {
        data.log = done.iter().map(|(day, w)| entry(*day, w)).collect();
        coach::today_for(data, d(11, 30))
    };
    for (done, want, owed) in [
        (vec![], push, false),
        (vec![(1, push), (2, pull)], legs, false),
        (vec![(1, push), (2, pull), (3, upper)], legs, true),               // skipped Legs: still owed
        (vec![(1, push), (2, pull), (3, upper), (4, legs)], push, false),   // round complete
        (vec![(1, push), (3, upper)], pull, true),                          // two owed: first in order
        (vec![(1, push), (3, upper), (4, pull)], legs, true),
        (vec![(1, push), (2, push), (3, pull)], legs, false),               // a repeat doesn't count twice
    ] {
        let t = next_after(&mut data, &done);
        assert_eq!((t.session.as_str(), t.owed), (want, owed), "after {done:?}: {}", t.notes);
        eprintln!("after {:?}: {} — {}", done.iter().map(|(_, w)| w.rsplit(" — ").next().unwrap()).collect::<Vec<_>>(), t.session, t.notes);
    }
}
