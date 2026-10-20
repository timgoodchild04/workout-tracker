//! Port of the 📝 Workout Log helper columns (I:AI), the 🏠 Dashboard and the
//! 📈 Progress tab. Each function names the sheet column it reproduces.

use crate::engine::{fmt_num, round2, Engine, Load, BARBELL, BODYWEIGHT, DUMBBELL, FIXED};
use crate::model::{Data, LibEx, LogEntry, SchedDay, Slot};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Increase,
    Repeat,
    Reduce,
    New,
}

/// Everything the sheet derives for one log row.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub index: usize,
    pub prev: Option<usize>,          // N  PrevRow (index into the log)
    pub total_reps: f64,              // I
    pub volume: f64,                  // J
    pub previous_text: String,        // K
    pub recommended: String,          // L
    pub recommended_kg: Option<f64>,  //    (numeric twin of L, for pre-filling)
    pub decision: String,             // M
    pub verdict: Verdict,
    pub hit_ceiling: bool,            // S
    pub big_drop: bool,               // V
    pub overreach: bool,              // W
    pub ceiling_ok: bool,             // X
    pub jump_ok: bool,                // AB
    pub equipment: String,            // AC
    pub load_type: String,            // AI
}

/// What a plan slot actually uses today, like 📅 Plan's "Use exercise" column: the
/// slot's exercise if its kit is on, otherwise the first library exercise with the
/// same pattern whose kit is on. None = switched off, or nothing available.
pub fn resolve<'a>(data: &'a Data, engine: &Engine, slot: &Slot) -> Option<&'a LibEx> {
    if !slot.enabled || slot.exercise.is_empty() {
        return None;
    }
    match data.lib(&slot.exercise) {
        Some(l) if engine.equipment_on(&l.equipment) => Some(l),
        _ => data.library.iter().find(|l| l.pattern == slot.pattern && engine.equipment_on(&l.equipment)),
    }
}

pub fn sets_text(e: &LogEntry) -> String {
    let f = |o: Option<f64>| o.map(fmt_num).unwrap_or_default();
    let mut s = format!("{}/{}/{}", f(e.sets[0]), f(e.sets[1]), f(e.sets[2]));
    if e.sets[3].is_some() {
        s.push('/');
        s.push_str(&f(e.sets[3]));
    }
    s
}

fn text_or_empty(l: Option<Load>) -> (String, Option<f64>) {
    l.map(|l| (l.text, Some(l.kg))).unwrap_or((String::new(), None))
}

pub fn analyse(data: &Data, engine: &Engine) -> Vec<Analysis> {
    let st = &engine.settings;
    let log = &data.log;
    let mut out: Vec<Analysis> = Vec::with_capacity(log.len());
    for (i, e) in log.iter().enumerate() {
        let prev = (0..i).rev().find(|&j| log[j].exercise == e.exercise);
        let p = prev.map(|j| &log[j]);
        let pa = prev.map(|j| &out[j]);

        let set_vals: Vec<f64> = e.sets.iter().flatten().copied().collect();
        let total: f64 = set_vals.iter().sum();
        let actual_sets = set_vals.len() as f64;
        let min_set = set_vals.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))).unwrap_or(0.0);

        // Log P/Q/AC: rep range and kit from 📚 Exercises.
        let (rep_low, rep_high, equipment) = match data.lib(&e.exercise) {
            Some(l) => (Some(l.rep_low), Some(l.rep_high), l.equipment.clone()),
            None => (None, None, String::new()),
        };
        let load_type = engine.load_type(&equipment);

        let hit_ceiling = rep_high.is_some_and(|h| min_set >= h);
        let wt_up = p.is_some_and(|p| round2(e.weight) > round2(p.weight));
        let wt_same = p.is_some_and(|p| round2(e.weight) == round2(p.weight));
        let big_drop = p.is_some() && wt_same && total <= pa.unwrap().total_reps * (1.0 - st.drop_pct);
        let overreach = p.is_some() && actual_sets > 0.0 && rep_low.is_some() && wt_up && (total / actual_sets) < rep_low.unwrap();
        let ceiling_ok = if st.ceiling_streak <= 1.0 {
            hit_ceiling
        } else {
            hit_ceiling && p.is_some() && wt_same && pa.unwrap().hit_ceiling
        };

        let next_up = if load_type == BODYWEIGHT { None } else { engine.next_up(&load_type, &equipment, e.weight) };
        let (jump_pct, jump_abs) = match next_up {
            Some(n) if n != 0.0 => (Some(if e.weight == 0.0 { 0.0 } else { (n - e.weight) / e.weight }), Some(n - e.weight)),
            _ => (None, None),
        };
        let jump_ok = match jump_pct {
            None => false,
            Some(_) if load_type == FIXED => true,
            Some(pct) => pct <= st.jump_pct || jump_abs.unwrap() <= st.jump_abs,
        };

        let (current, current_kg) = text_or_empty(engine.closest(&load_type, &equipment, e.weight));
        let (reduce, reduce_kg) = text_or_empty(engine.closest(&load_type, &equipment, p.map_or(e.weight, |p| p.weight)));
        let (increase, increase_kg) = match next_up {
            Some(n) => text_or_empty(engine.closest(&load_type, &equipment, n)),
            None => text_or_empty(engine.closest(&load_type, &equipment, 0.0)),
        };

        let previous_text = match p {
            None => "— first session —".to_string(),
            Some(p) => format!("Prev: {:.1}kg  {}", p.weight, sets_text(p)),
        };

        let (recommended, recommended_kg) = if load_type == BODYWEIGHT {
            ("BW".to_string(), Some(0.0))
        } else if big_drop || overreach {
            (reduce.clone(), reduce_kg)
        } else if ceiling_ok && jump_ok {
            (increase.clone(), increase_kg)
        } else {
            (current.clone(), current_kg)
        };

        let reps0 = format!("{}", total.round());
        let (decision, verdict) = if big_drop || overreach {
            let why = if overreach {
                format!("reps landed below range after a weight increase. Drop back to {}.", reduce)
            } else {
                "total reps (or seconds) dropped sharply since last time — check recovery, sleep or form before continuing.".to_string()
            };
            (format!("🔴 Reduce/Review — {}", why), Verdict::Reduce)
        } else if load_type == BODYWEIGHT {
            if ceiling_ok {
                ("🟢 Increase — add a few more reps/seconds, or move to a harder variation.".to_string(), Verdict::Increase)
            } else {
                (format!("🟡 Repeat — aim to beat {} total reps/seconds next time.", reps0), Verdict::Repeat)
            }
        } else if ceiling_ok {
            match next_up {
                None | Some(0.0) => (
                    format!("🟡 Repeat {} — that is the strongest setting you own for this exercise, so keep adding reps rather than load.", current),
                    Verdict::Repeat,
                ),
                _ if jump_ok => (format!("🟢 Increase — next time load {}.", increase), Verdict::Increase),
                _ => (
                    format!(
                        "🟡 Repeat {} — next available jump is {}%, too large to auto-approve. Build more reps/quality first.",
                        current,
                        (jump_pct.unwrap_or(0.0) * 100.0).round()
                    ),
                    Verdict::Repeat,
                ),
            }
        } else {
            (format!("🟡 Repeat {} — aim to beat {} total reps next time.", current, reps0), Verdict::Repeat)
        };

        out.push(Analysis {
            index: i,
            prev,
            total_reps: total,
            volume: e.weight * total,
            previous_text,
            recommended,
            recommended_kg,
            decision,
            verdict,
            hit_ceiling,
            big_drop,
            overreach,
            ceiling_ok,
            jump_ok,
            equipment,
            load_type,
        });
    }
    out
}

// ---------------------------------------------------------------- Dashboard

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub exercise: String,
    pub original: String,     // programme exercise (differs when substituted)
    pub substituted: bool,
    pub equipment: String,
    pub load_type: String,
    pub video: String,
    pub sets: u32,
    pub rep_low: f64,
    pub rep_high: f64,
    pub recommended: String,
    pub recommended_kg: f64,
    pub previous_weight: Option<f64>,
    pub previous_reps: String,
    pub previous_date: Option<NaiveDate>,
    pub trend: String,
    pub verdict: Verdict,
    pub instruction: String,
    /// What was logged for this exercise on the card's date, if anything.
    pub logged: Option<Logged>,
    /// First session back after a break: a lighter load to use instead of `recommended`
    /// (which stays exactly what the sheet says).
    pub ease_back: Option<EaseBack>,
    /// Warm-up sets before the working sets (plate-loaded compound lifts only).
    pub warmups: Vec<WarmUp>,
    /// Rest between sets for the Today screen's timer.
    pub rest_secs: u32,
    /// The card's date is in a deload week (its sets are already cut).
    pub deload: bool,
    /// A main (compound) lift: short-on-time mode never trims these.
    pub compound: bool,
    /// Cues from the library's Notes column.
    pub notes: String,
    /// The sets of the last normal session, for "Same as last time".
    pub previous_sets: [Option<f64>; 4],
    /// The slot's movement pattern (what can be swapped in).
    pub pattern: String,
    /// 📅 Plan row of the slot this card comes from.
    pub slot_row: usize,
}

/// Movement patterns that load the whole body enough to need warm-up sets.
pub const COMPOUND_PATTERNS: [&str; 7] = ["Horizontal push", "Vertical push", "Vertical pull", "Horizontal pull", "Squat", "Lunge", "Hinge"];

/// Sets in a deload week: about 60% of the usual, at least one.
pub fn deload_sets(sets: u32) -> u32 {
    ((sets as f64 * 0.6).round() as u32).max(1)
}

/// After this many days without an exercise, its first session back is eased.
pub const EASE_BACK_AFTER_DAYS: i64 = 10;
/// Share of the last load used to restart at after a break.
pub const EASE_BACK_SHARE: f64 = 0.85;

#[derive(Debug, Clone, Serialize)]
pub struct EaseBack {
    pub kg: f64,
    pub text: String,
    /// Days since the exercise was last done.
    pub days: i64,
    /// The load used last time.
    pub last_kg: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarmUp {
    pub label: String,
    pub reps: u32,
    pub kg: f64,
    pub text: String,
}

/// Warm-ups for a working load: empty bar (barbell only), then about 50% and 75%,
/// each at or below the target, each heavier than the last and lighter than the work.
/// None for light work (under 10kg), where warming up with weight adds nothing.
pub fn warmups(engine: &Engine, load_type: &str, equipment: &str, working: f64) -> Vec<WarmUp> {
    if !(load_type == BARBELL || load_type == DUMBBELL) || working < 10.0 {
        return vec![];
    }
    let mut out: Vec<WarmUp> = Vec::new();
    if load_type == BARBELL {
        out.push(WarmUp { label: "Empty bar".into(), reps: 10, kg: engine.settings.bar_weight, text: "no plates".into() });
    }
    for (share, reps) in [(0.5, 8), (0.75, 5)] {
        let Some(l) = engine.at_most(load_type, equipment, working * share) else { continue };
        let floor = out.last().map_or(0.0, |w| w.kg);
        if l.kg > floor + 1e-9 && l.kg < working - 1e-9 {
            out.push(WarmUp { label: format!("{}%", (share * 100.0) as u32), reps, kg: l.kg, text: l.text });
        }
    }
    out
}

/// For each log entry: does it beat every earlier entry of the same exercise?
/// Weighted: a heavier load, or more total reps at the heaviest load so far.
/// Bodyweight: more total reps (or seconds). An exercise's first entry isn't a PB.
pub fn personal_bests(data: &Data, analysis: &[Analysis]) -> Vec<bool> {
    let mut best: std::collections::HashMap<&str, (f64, f64)> = std::collections::HashMap::new(); // (weight, reps at it)
    let mut out = Vec::with_capacity(data.log.len());
    for (i, e) in data.log.iter().enumerate() {
        let total = analysis[i].total_reps;
        let bw = analysis[i].load_type == BODYWEIGHT;
        let w = if bw { 0.0 } else { round2(e.weight) };
        let pb = match best.get(e.exercise.as_str()) {
            None => false,
            Some(&(bw_, br)) => w > bw_ || (w == bw_ && total > br),
        };
        out.push(pb);
        let entry = best.entry(e.exercise.as_str()).or_insert((w, total));
        if w > entry.0 || (w == entry.0 && total > entry.1) {
            *entry = (w, total);
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct Logged {
    pub weight: f64,
    pub sets: [Option<f64>; 4],
    pub decision: String,
    pub verdict: Verdict,
    /// Still queued on this computer, not yet in the sheet.
    pub pending: bool,
    /// Beat every earlier entry of this exercise.
    pub pb: bool,
    /// Its row in the sheet, so an edit updates that row instead of adding another.
    pub row: Option<usize>,
}

/// Cards for one session on `date`. The targets come from sessions before
/// `date`, so they stay put while you log the day's exercises one by one;
/// anything already logged on `date` is attached as `logged`. Pass a date
/// after the last entry to use the whole log (the Dashboard's behaviour).
pub fn session_cards(data: &Data, engine: &Engine, analysis: &[Analysis], session: &str, date: NaiveDate) -> (Vec<Card>, Vec<String>) {
    let mut cards = Vec::new();
    let mut skipped = Vec::new();
    let pbs = personal_bests(data, analysis);
    for slot in data.plan.iter().filter(|s| s.day == session && s.enabled) {
        let Some(lib) = resolve(data, engine, slot) else {
            skipped.push(slot.exercise.clone());
            continue;
        };
        let (exercise, equipment, start, video) = (&lib.name, &lib.equipment, lib.start_weight, &lib.video);
        let load_type = engine.load_type(equipment);
        // Progression comes from the last normal session: deload-week entries are lighter
        // on purpose, so they mustn't read as a drop. (Ease-back counts days from any.)
        let last = data.log.iter().rposition(|e| &e.exercise == exercise && e.date < date && data.deload_on(e.date).is_none());
        let last_any = data.log.iter().rposition(|e| &e.exercise == exercise && e.date < date);
        let logged = data.log.iter().rposition(|e| &e.exercise == exercise && e.date == date).map(|i| Logged {
            pb: pbs[i],
            weight: data.log[i].weight,
            sets: data.log[i].sets,
            decision: analysis[i].decision.clone(),
            verdict: analysis[i].verdict,
            pending: data.log[i].queued || data.log[i].row.is_none(),
            row: data.log[i].row,
        });
        let (recommended, recommended_kg, previous_weight, previous_reps, previous_date, trend, verdict, instruction) = match last {
            None => {
                let range = format!("{}–{}", fmt_num(lib.rep_low), fmt_num(lib.rep_high));
                let start_load = engine.closest(&load_type, equipment, start);
                let (text, kg) = if load_type == BODYWEIGHT {
                    ("BW".to_string(), 0.0)
                } else {
                    start_load.clone().map(|l| (l.text, l.kg)).unwrap_or((String::new(), start))
                };
                let instr = if load_type == BODYWEIGHT {
                    format!("Bodyweight — aim for {} reps/seconds per set.", range)
                } else {
                    format!("Load {} — aim for {} reps per set.", text, range)
                };
                (text, kg, None, "—".to_string(), None, "🆕 New".to_string(), Verdict::New, instr)
            }
            Some(n) => {
                let a = &analysis[n];
                let e = &data.log[n];
                let trend = match a.prev {
                    None => "🆕 New",
                    Some(o) if a.total_reps > analysis[o].total_reps => "📈 Improving",
                    Some(o) if a.total_reps == analysis[o].total_reps => "➡️ Steady",
                    Some(_) => "📉 Down",
                };
                (
                    a.recommended.clone(),
                    a.recommended_kg.unwrap_or(e.weight),
                    if load_type == BODYWEIGHT { None } else { Some(e.weight) },
                    sets_text(e),
                    Some(e.date),
                    trend.to_string(),
                    a.verdict,
                    a.decision.clone(),
                )
            }
        };
        // First session back after a break: restart lighter than last time.
        let ease_back = last_any.and_then(|n| {
            let days = (date - data.log[n].date).num_days();
            let weighted = [BARBELL, DUMBBELL, FIXED].contains(&load_type.as_str());
            // Light loads (under 10kg) aren't worth easing: a step down is most of the weight.
            if days <= EASE_BACK_AFTER_DAYS || !weighted || data.log[n].weight < 10.0 {
                return None;
            }
            let l = engine.at_most(&load_type, equipment, data.log[n].weight * EASE_BACK_SHARE)?;
            (l.kg < recommended_kg - 1e-9).then(|| EaseBack { kg: l.kg, text: l.text, days, last_kg: data.log[n].weight })
        });
        let compound = COMPOUND_PATTERNS.contains(&lib.pattern.as_str());
        let working = ease_back.as_ref().map_or(recommended_kg, |e| e.kg);
        let warmups = if compound { warmups(engine, &load_type, equipment, working) } else { vec![] };
        let deload = data.deload_on(date).is_some();
        let rest_secs = if compound { 150 } else if lib.pattern.starts_with("Core") { 60 } else { 90 };
        cards.push(Card {
            ease_back,
            warmups,
            rest_secs,
            exercise: exercise.clone(),
            original: slot.exercise.clone(),
            substituted: lib.name != slot.exercise,
            equipment: equipment.clone(),
            load_type,
            video: video.clone(),
            sets: if deload { deload_sets(slot.sets.unwrap_or(lib.sets)) } else { slot.sets.unwrap_or(lib.sets) },
            deload,
            compound,
            notes: lib.notes.clone(),
            previous_sets: last.map(|n| data.log[n].sets).unwrap_or([None; 4]),
            rep_low: lib.rep_low,
            rep_high: lib.rep_high,
            pattern: lib.pattern.clone(),
            slot_row: slot.row,
            recommended,
            recommended_kg,
            previous_weight,
            previous_reps,
            previous_date,
            trend,
            verdict,
            instruction,
            logged,
        });
    }
    (cards, skipped)
}

/// Days in plan order, then any day from the Days table that has no slots yet.
pub fn sessions(data: &Data) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for d in data.plan.iter().map(|s| &s.day).chain(data.days.iter().map(|d| &d.name)) {
        if !v.contains(d) {
            v.push(d.clone());
        }
    }
    v
}

#[derive(Debug, Clone, Serialize)]
pub struct Today {
    pub date: NaiveDate,
    pub weekday: String,
    pub session: String,
    pub notes: String,
    pub is_rest: bool,
    pub next_training_day: String,
    /// Short session also due today, e.g. "🌅 Morning" ("" = none).
    pub morning: String,
    /// Following the rotation rather than the weekday schedule.
    pub rotation: bool,
    /// Rotation only: today's session was skipped earlier this round and is still owed.
    pub owed: bool,
}

/// Where the rotation stands before `date`. The days (Days table order, Morning-focus
/// days left out; they keep their weekday slot) are done in rounds: a round ends once
/// every day has been done at least once, in any order, and a break of 14+ days
/// starts a fresh one. Next up is the first day, in
/// order, not yet done this round, so a day you skip stays owed until you do it and
/// everything after it shifts back.
#[derive(Debug, Clone, Serialize)]
pub struct Rotation {
    pub next: String,
    /// Each day in order and whether it's been done this round.
    pub round: Vec<(String, bool)>,
    /// Days done this round that come after `next` in the order (done ahead of it).
    pub ahead: Vec<String>,
    /// The last main session logged before `date`.
    pub last: Option<(String, NaiveDate)>,
    /// Dates rounds were completed on, oldest first.
    pub completed: Vec<NaiveDate>,
}

pub fn rotation(data: &Data, date: NaiveDate) -> Option<Rotation> {
    let mut order: Vec<String> = data.days.iter().filter(|d| d.focus != "Morning").map(|d| d.name.clone()).collect();
    if order.is_empty() {
        order = sessions(data).into_iter().filter(|s| data.focus(s) != "Morning").collect();
    }
    if order.is_empty() {
        return None;
    }
    // Sessions in the order they were logged (sheet row order within a day, queued
    // entries after the sheet's), a session marked complete counting like a logged one.
    let mut done: Vec<String> = Vec::new();
    let mut last = None;
    let mut completed = Vec::new();
    let mut entries: Vec<(NaiveDate, usize, usize, &str)> = data.log.iter().chain(&data.marks).enumerate()
        .filter(|(_, e)| e.date < date && order.iter().any(|o| *o == e.workout))
        .map(|(i, e)| (e.date, e.row.unwrap_or(usize::MAX), i, e.workout.as_str()))
        .collect();
    entries.sort();
    let mut prev: Option<NaiveDate> = None;
    for (d, _, _, w) in entries {
        // After a long break the round starts afresh: sessions from before it don't count.
        if prev.is_some_and(|p| (d - p).num_days() >= crate::suggest::BREAK_RESETS_AFTER_DAYS) {
            done.clear();
        }
        prev = Some(d);
        let w = &w.to_string();
        if !done.contains(w) {
            done.push(w.clone());
        }
        last = Some((w.clone(), d));
        if order.iter().all(|o| done.contains(o)) {
            done.clear();
            completed.push(d);
        }
    }
    let next = order.iter().find(|o| !done.contains(o)).unwrap().clone();
    let pos = order.iter().position(|o| *o == next).unwrap();
    let ahead = order[pos + 1..].iter().filter(|o| done.contains(o)).cloned().collect();
    let round = order.iter().map(|o| (o.clone(), done.contains(o))).collect();
    Some(Rotation { next, round, ahead, last, completed })
}

/// Today's session, by weekday or by rotation as 📅 Plan's Session order says.
pub fn today_for(data: &Data, date: NaiveDate) -> Today {
    let mut t = today(&data.schedule, date);
    if data.session_order == crate::model::ROTATION {
        if let Some(r) = rotation(data, date) {
            let short = |d: &str| d.rsplit(" — ").next().unwrap_or(d).to_string();
            let progress = r.round.iter().map(|(d, done)| format!("{}{}", short(d), if *done { " ✓" } else if *d == r.next && !r.ahead.is_empty() { " (owed)" } else { "" })).collect::<Vec<_>>().join(" · ");
            t.notes = if !r.ahead.is_empty() {
                format!("{} is owed: you did {} ahead of it this round. This round: {}", short(&r.next), r.ahead.iter().map(|d| short(d)).collect::<Vec<_>>().join(" and "), progress)
            } else {
                match &r.last {
                    Some((s, d)) => format!("Next in your rotation (last: {} on {}). This round: {}", short(s), d.format("%a %-d %b"), progress),
                    None => format!("First in your rotation. This round: {progress}"),
                }
            };
            t.owed = !r.ahead.is_empty();
            t.session = r.next;
            t.is_rest = false;
            t.rotation = true;
        }
    }
    t
}

pub fn today(schedule: &[SchedDay], date: NaiveDate) -> Today {
    let wd = date.weekday().num_days_from_monday() as usize; // WEEKDAY(,2)-1
    let day = schedule.get(wd).cloned().unwrap_or(SchedDay { day: String::new(), session: "Rest".into(), notes: String::new(), morning: String::new() });
    // Schedule!C9: smallest positive days-ahead among non-Rest days.
    let next = (0..7)
        .filter(|&i| schedule.get(i).is_some_and(|d| d.session != "Rest"))
        .map(|i| ((i + 7 - wd) % 7, i))
        .filter(|(d, _)| *d > 0)
        .min()
        .map(|(_, i)| schedule[i].day.clone())
        .unwrap_or_else(|| "-".into());
    Today {
        date,
        weekday: date.format("%A").to_string(),
        is_rest: day.session == "Rest" || day.session.is_empty(),
        session: day.session,
        notes: day.notes,
        next_training_day: next,
        morning: day.morning,
        rotation: false,
        owed: false,
    }
}

// ---------------------------------------------------------------- Progress

#[derive(Debug, Clone, Serialize)]
pub struct Point {
    pub date: NaiveDate,
    pub weight: f64,
    pub total_reps: f64,
    pub sets: String,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressRow {
    pub exercise: String,
    pub equipment: String,
    pub bodyweight: bool,
    pub current: String,
    pub best: String,
    pub sessions: usize,
    pub change: String,
    pub next: String,
    pub points: Vec<Point>,
}

pub fn progress(data: &Data, engine: &Engine, analysis: &[Analysis]) -> Vec<ProgressRow> {
    // 📈 Progress A: every exercise in the plan or the log.
    let mut names: Vec<String> = data.plan.iter().map(|p| p.exercise.clone()).chain(data.log.iter().map(|e| e.exercise.clone())).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let equipment = data.lib(&name).map(|l| l.equipment.clone()).unwrap_or_default();
            let bw = engine.load_type(&equipment) == BODYWEIGHT;
            let idx: Vec<usize> = data.log.iter().enumerate().filter(|(_, e)| e.exercise == name).map(|(i, _)| i).collect();
            let points: Vec<Point> = idx
                .iter()
                .map(|&i| Point {
                    date: data.log[i].date,
                    weight: data.log[i].weight,
                    total_reps: analysis[i].total_reps,
                    sets: sets_text(&data.log[i]),
                    verdict: analysis[i].verdict,
                })
                .collect();
            let last = idx.last().copied();
            let kg = |w: f64| format!("{}kg", fmt_num(w));
            let current = match last {
                None => "—".into(),
                Some(_) if bw => "BW".into(),
                // Not in the library and logged without a weight: the sheet shows a blank cell.
                Some(i) if equipment.is_empty() && data.log[i].weight == 0.0 => String::new(),
                Some(i) => kg(data.log[i].weight),
            };
            let best = if idx.is_empty() {
                "—".into()
            } else if bw {
                "BW".into()
            } else {
                kg(idx.iter().map(|&i| data.log[i].weight).fold(f64::MIN, f64::max))
            };
            let change = match last {
                None => "—".to_string(),
                Some(i) => match analysis[i].prev {
                    None => "first session".to_string(),
                    Some(j) => {
                        let (cur, prv, u) = if bw {
                            (analysis[i].total_reps, analysis[j].total_reps, " reps")
                        } else {
                            (data.log[i].weight, data.log[j].weight, "kg")
                        };
                        let d = round2(cur - prv);
                        if d > 0.0 {
                            format!("↑ +{}{}", fmt_num(d), u)
                        } else if d < 0.0 {
                            format!("↓ {}{}", fmt_num(d), u)
                        } else {
                            "same as last time".to_string()
                        }
                    }
                },
            };
            let next = match last {
                None => "—".into(),
                Some(i) => analysis[i].recommended.split('—').next().unwrap_or("").trim().to_string(),
            };
            ProgressRow { exercise: name, equipment, bodyweight: bw, current, best, sessions: idx.len(), change, next, points }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DayInfo, LogEntry, SESSION_COMPLETE};

    #[test]
    fn a_session_marked_complete_counts_in_the_rotation() {
        let mut data = Data::from_grids(&vec![], &vec![], &vec![], &vec![], &vec![], &vec![]);
        data.days = ["Push", "Pull", "Legs"].map(|n| DayInfo { name: n.into(), focus: n.into() }).to_vec();
        let d = |day| NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        let entry = |day, w: &str, ex: &str| LogEntry { date: d(day), workout: w.into(), exercise: ex.into(), weight: 0.0, sets: [None; 4], row: None, queued: false };
        data.log = vec![entry(1, "Push", "Press")];
        assert_eq!(rotation(&data, d(10)).unwrap().next, "Pull");
        // Pull marked complete with nothing logged: Legs is next, nothing owed.
        data.marks = vec![entry(2, "Pull", SESSION_COMPLETE)];
        let r = rotation(&data, d(10)).unwrap();
        assert_eq!((r.next.as_str(), r.ahead.len()), ("Legs", 0));
        data.marks.push(entry(3, "Legs", SESSION_COMPLETE));
        assert_eq!(rotation(&data, d(10)).unwrap().completed, vec![d(3)], "the round completes");
    }

    #[test]
    fn a_mark_counts_in_sheet_row_order_within_a_day() {
        let mut data = Data::from_grids(&vec![], &vec![], &vec![], &vec![], &vec![], &vec![]);
        data.days = ["Push", "Pull", "Legs"].map(|n| DayInfo { name: n.into(), focus: n.into() }).to_vec();
        let d = |day| NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        let entry = |day, row, w: &str, ex: &str| LogEntry { date: d(day), workout: w.into(), exercise: ex.into(), weight: 0.0, sets: [None; 4], row: Some(row), queued: false };
        data.log = vec![entry(1, 2, "Push", "Press"), entry(2, 3, "Pull", "Row"), entry(4, 5, "Push", "Press")];
        // Legs marked complete on the 4th in the row above that day's Push: it closes
        // the round and Push starts the next one, so Pull is next (not Push again).
        data.marks = vec![entry(4, 4, "Legs", SESSION_COMPLETE)];
        let r = rotation(&data, d(10)).unwrap();
        assert_eq!((r.next.as_str(), r.completed), ("Pull", vec![d(4)]));
    }
}
