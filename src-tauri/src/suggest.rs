//! Coach suggestions from the plan and the log: weekly volume per muscle against
//! the targets on 📅 Plan, exercises that have stalled, and bodyweight moves that
//! are ready for their harder (or need their easier) version. Each suggestion can
//! carry one action the app applies to 📅 Plan (add a slot, or swap a slot's exercise).

use crate::coach::{resolve, Analysis};
use crate::engine::{Engine, BODYWEIGHT};
use crate::model::{Data, LibEx};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Action {
    /// Add a slot for `exercise` to `day`.
    Add { day: String, exercise: String },
    /// Put `to` in the slot on 📅 Plan row `row` (currently `from`).
    Swap { row: usize, from: String, to: String },
    /// Set the sets on the slot on 📅 Plan row `row` (holding `exercise`).
    Sets { row: usize, exercise: String, sets: u32 },
    /// Start a deload week on `start`.
    Deload { start: NaiveDate },
}

/// Monday of `date`'s week.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// A gap this long without a main session counts as rest, like a deload.
pub const BREAK_RESETS_AFTER_DAYS: i64 = 14;

/// Weeks (Mon–Sun) with at least one main (non-Morning) session since the last rest,
/// up to and including `today`'s week. Rest = a deload week, or a break of 14+ days.
pub fn training_weeks_since_deload(data: &Data, today: NaiveDate) -> usize {
    let since = data.deloads.iter().filter(|&&s| s <= today).max().map(|&s| s + chrono::Duration::days(7));
    let mut days: Vec<NaiveDate> = data
        .log
        .iter()
        .filter(|e| e.date <= today && since.is_none_or(|s| e.date >= s) && data.focus(&e.workout) != "Morning")
        .map(|e| e.date)
        .collect();
    days.sort();
    days.dedup();
    // Only count from the last long break onwards.
    let from = days.windows(2).filter(|w| (w[1] - w[0]).num_days() >= BREAK_RESETS_AFTER_DAYS).map(|w| w[1]).last();
    let mut weeks: Vec<NaiveDate> = days.into_iter().filter(|d| from.is_none_or(|f| *d >= f)).map(week_start).collect();
    weeks.dedup();
    weeks.len()
}

/// Most sets a suggestion will put on one exercise.
const MAX_SETS: u32 = 5;
/// Three sessions only count as a stall if they fall within this many days
/// (otherwise it's a break, and a dip after a break isn't a plateau).
const STALL_WINDOW_DAYS: i64 = 21;

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    /// "volume", "stall" or "progress", for grouping.
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    pub action: Option<Action>,
}

/// Working sets per week each muscle gets from the plan as scheduled: every slot's sets
/// times how many days a week its day comes round (Schedule sessions and the Morning
/// column), primary muscle in full and secondary muscles at half.
pub fn planned_volume(data: &Data, engine: &Engine) -> HashMap<String, f64> {
    let per_week = times_per_week(data);
    let mut vol: HashMap<String, f64> = HashMap::new();
    for slot in &data.plan {
        let times = per_week.get(slot.day.as_str()).copied().unwrap_or(0.0);
        let Some(lib) = resolve(data, engine, slot) else { continue };
        let sets = slot.sets.unwrap_or(lib.sets) as f64 * times;
        *vol.entry(lib.primary.clone()).or_default() += sets;
        for m in &lib.secondary {
            *vol.entry(m.clone()).or_default() += sets * 0.5;
        }
    }
    vol
}

/// Best place to add an exercise for `muscle`: one with kit on, on a day it fits,
/// preferring a day that doesn't train that pattern yet, then the day with the fewest
/// slots. Never an exercise the day already has.
fn where_to_add<'a>(data: &'a Data, engine: &Engine, muscle: &str) -> Option<(&'a LibEx, String)> {
    let days = crate::coach::sessions(data);
    let slots = |day: &str| data.plan.iter().filter(|s| s.day == day && s.enabled).count();
    let pattern_on = |day: &str, pattern: &str| data.plan.iter().any(|s| s.day == day && s.enabled && s.pattern == pattern);
    let has = |day: &str, name: &str| data.plan.iter().any(|s| s.day == day && (s.exercise == name || resolve(data, engine, s).is_some_and(|r| r.name == name)));
    for new_pattern_only in [true, false] {
        let mut best: Option<(&LibEx, String, usize)> = None;
        for lib in data.library.iter().filter(|l| l.primary == muscle && engine.equipment_on(&l.equipment)) {
            for day in &days {
                let fits = lib.fits.iter().any(|f| f == data.focus(day));
                let ok = fits && !has(day, &lib.name) && !(new_pattern_only && pattern_on(day, &lib.pattern));
                if ok && best.as_ref().is_none_or(|(_, _, n)| slots(day) < *n) {
                    best = Some((lib, day.clone(), slots(day)));
                }
            }
            if best.is_some() {
                break; // library order = preference, so the first exercise with a home wins
            }
        }
        if let Some((l, d, _)) = best {
            return Some((l, d));
        }
    }
    None
}

/// How many times a week each day comes round (Schedule sessions and Morning column).
fn times_per_week(data: &Data) -> HashMap<&str, f64> {
    let mut per_week: HashMap<&str, f64> = HashMap::new();
    for d in &data.schedule {
        for s in [&d.session, &d.morning] {
            if !s.is_empty() && s != "Rest" {
                *per_week.entry(s.as_str()).or_default() += 1.0;
            }
        }
    }
    per_week
}

pub fn suggestions(data: &Data, engine: &Engine, analysis: &[Analysis], today: NaiveDate) -> Vec<Suggestion> {
    let mut out = Vec::new();
    let in_deload = data.deload_on(today);

    // 1. Weekly volume against the targets.
    let vol = planned_volume(data, engine);
    for t in &data.targets {
        let have = vol.get(&t.muscle).copied().unwrap_or(0.0);
        if t.sets <= 0.0 || have >= t.sets * 0.8 {
            continue;
        }
        let gap = t.sets - have;
        // First choice: more sets on an exercise that already trains it (the most frequent
        // day first). Otherwise a new exercise on a day it fits.
        let per_week = times_per_week(data);
        let grow = data
            .plan
            .iter()
            .filter_map(|s| {
                let lib = resolve(data, engine, s)?;
                let now = s.sets.unwrap_or(lib.sets);
                let times = per_week.get(s.day.as_str()).copied().unwrap_or(0.0);
                (lib.primary == t.muscle && now < MAX_SETS && times > 0.0).then_some((s, lib, now, times))
            })
            .max_by(|a, b| a.3.partial_cmp(&b.3).unwrap());
        let (detail, action) = match (grow, where_to_add(data, engine, &t.muscle)) {
            (Some((s, lib, now, times)), _) => {
                let add = ((gap / times).ceil() as u32).clamp(1, MAX_SETS - now);
                (
                    format!("Do {} sets of {} on {} instead of {}.", now + add, lib.name, s.day, now),
                    Some(Action::Sets { row: s.row, exercise: s.exercise.clone(), sets: now + add }),
                )
            }
            (None, Some((lib, day))) => (
                format!("Add {} to {} ({} sets), or another {} exercise.", lib.name, day, lib.sets, t.muscle.to_lowercase()),
                Some(Action::Add { day, exercise: lib.name.clone() }),
            ),
            (None, None) => (format!("No {} exercise with your kit fits a day that hasn't got one. Add one in Settings.", t.muscle.to_lowercase()), None),
        };
        out.push(Suggestion {
            kind: "volume",
            title: format!("{}: {} of {} sets a week (about {} short)", t.muscle, fmt(have), fmt(t.sets), fmt(gap.ceil())),
            detail,
            action,
        });
    }

    // 2 & 3. Per slot in use: stalls and bodyweight progressions.
    let mut seen = std::collections::HashSet::new();
    for slot in &data.plan {
        let Some(lib) = resolve(data, engine, slot) else { continue };
        if !seen.insert(lib.name.clone()) {
            continue; // one suggestion per exercise, even if it's on several days
        }
        let hist: Vec<usize> = (0..data.log.len()).filter(|&i| data.log[i].exercise == lib.name && data.deload_on(data.log[i].date).is_none()).collect();
        let bodyweight = engine.load_type(&lib.equipment) == BODYWEIGHT;
        let last = hist.last().copied();

        // Bodyweight: top of the range → harder; two sessions below it → easier.
        if bodyweight {
            if let Some(i) = last {
                let sets: Vec<f64> = data.log[i].sets.iter().flatten().copied().collect();
                let topped = !sets.is_empty() && sets.iter().all(|&r| r >= lib.rep_high);
                if topped && !lib.harder.is_empty() {
                    out.push(Suggestion {
                        kind: "progress",
                        title: format!("{} is ready for {}", lib.name, lib.harder),
                        detail: format!("Every set hit the top of {}–{} last time ({}).", fmt(lib.rep_low), fmt(lib.rep_high), crate::coach::sets_text(&data.log[i])),
                        action: Some(Action::Swap { row: slot.row, from: slot.exercise.clone(), to: lib.harder.clone() }),
                    });
                    continue;
                }
            }
            let below = |i: usize| {
                let sets: Vec<f64> = data.log[i].sets.iter().flatten().copied().collect();
                !sets.is_empty() && sets.iter().any(|&r| r < lib.rep_low)
            };
            if hist.len() >= 2 && below(hist[hist.len() - 1]) && below(hist[hist.len() - 2]) && !lib.easier.is_empty() {
                out.push(Suggestion {
                    kind: "progress",
                    title: format!("Try {} instead of {} for a while", lib.easier, lib.name),
                    detail: format!("The last two sessions had sets under {} reps. Build up on the easier version, then come back.", fmt(lib.rep_low)),
                    action: Some(Action::Swap { row: slot.row, from: slot.exercise.clone(), to: lib.easier.clone() }),
                });
                continue;
            }
        }

        // Stall: three sessions in a row with no gain in weight or total reps.
        if hist.len() >= 3 {
            let w = &hist[hist.len() - 3..];
            let (a, b, c) = (w[0], w[1], w[2]);
            if (data.log[c].date - data.log[a].date).num_days() > STALL_WINDOW_DAYS {
                continue; // spans a break, not a plateau
            }
            let weight_up = data.log[c].weight > data.log[a].weight || data.log[b].weight > data.log[a].weight;
            let reps_up = analysis[c].total_reps > analysis[a].total_reps || analysis[b].total_reps > analysis[a].total_reps;
            if !weight_up && !reps_up {
                let alt = data
                    .library
                    .iter()
                    .find(|l| l.pattern == lib.pattern && l.name != lib.name && engine.equipment_on(&l.equipment));
                out.push(Suggestion {
                    kind: "stall",
                    title: format!("{} hasn't moved in 3 sessions", lib.name),
                    detail: match alt {
                        Some(l) => format!("Same weight and no more reps since {}. A few weeks of {} works the same pattern from a new angle.", data.log[a].date.format("%-d %b"), l.name),
                        None => format!("Same weight and no more reps since {}. Check sleep and recovery, or drop the weight 10% and build back up.", data.log[a].date.format("%-d %b")),
                    },
                    action: alt.map(|l| Action::Swap { row: slot.row, from: slot.exercise.clone(), to: l.name.clone() }),
                });
            }
        }
    }
    // Deloads: due after N training weeks, or early when several lifts have stalled.
    if in_deload.is_none() {
        let weeks = training_weeks_since_deload(data, today);
        let stalls = out.iter().filter(|s| s.kind == "stall").count();
        let every = data.deload_every;
        let start = today;
        if every > 0.0 && weeks as f64 >= every {
            out.insert(0, Suggestion {
                kind: "deload",
                title: format!("Deload week due: {weeks} training weeks since the last one"),
                detail: "For one week every exercise gets about 60% of its usual sets at your normal weights. Recovery catches up and progress usually picks up after it.".into(),
                action: Some(Action::Deload { start }),
            });
        } else if stalls >= 3 {
            out.insert(0, Suggestion {
                kind: "deload",
                title: format!("{stalls} lifts have stalled: a deload week could unstick them"),
                detail: "One week at about 60% of the usual sets, normal weights, then back to normal progression.".into(),
                action: Some(Action::Deload { start }),
            });
        }
    }
    out
}

fn fmt(n: f64) -> String {
    crate::engine::fmt_num((n * 2.0).round() / 2.0)
}

/// One row of the Coach tab's volume table.
#[derive(Debug, Clone, Serialize)]
pub struct VolumeRow {
    pub muscle: String,
    /// Working sets per week the plan gives it, as scheduled.
    pub planned: f64,
    pub target: f64,
    /// Sets actually logged in the 7 days up to `today` (sets of 0 don't count).
    pub last_7_days: f64,
}

pub fn volume(data: &Data, engine: &Engine, today: NaiveDate) -> Vec<VolumeRow> {
    let planned = planned_volume(data, engine);
    let mut done: HashMap<String, f64> = HashMap::new();
    let from = today - chrono::Duration::days(6);
    for e in data.log.iter().filter(|e| e.date >= from && e.date <= today) {
        let Some(lib) = data.lib(&e.exercise) else { continue };
        let sets = e.sets.iter().flatten().filter(|&&r| r > 0.0).count() as f64;
        *done.entry(lib.primary.clone()).or_default() += sets;
        for m in &lib.secondary {
            *done.entry(m.clone()).or_default() += sets * 0.5;
        }
    }
    let mut muscles: Vec<String> = data.targets.iter().map(|t| t.muscle.clone()).collect();
    for m in planned.keys().chain(done.keys()) {
        if !muscles.contains(m) {
            muscles.push(m.clone());
        }
    }
    muscles
        .into_iter()
        .map(|m| VolumeRow {
            planned: planned.get(&m).copied().unwrap_or(0.0),
            target: data.targets.iter().find(|t| t.muscle == m).map_or(0.0, |t| t.sets),
            last_7_days: done.get(&m).copied().unwrap_or(0.0),
            muscle: m,
        })
        .collect()
}

/// The Coach tab's weekly review (Monday to Sunday of `today`'s week).
#[derive(Debug, Clone, Serialize)]
pub struct WeekReview {
    pub week_start: NaiveDate,
    /// (session, date) for each session logged this week, in order.
    pub sessions: Vec<(String, NaiveDate)>,
    /// Working sets logged (sets of 0 don't count), this week and last.
    pub sets: f64,
    pub sets_last_week: f64,
    /// "Barbell Row 25kg × 12/12/12/12" for each personal best this week.
    pub pbs: Vec<String>,
    /// Rotation mode: rounds completed this week, and what's owed now.
    pub rounds_completed: usize,
    pub owed: Option<String>,
    /// The deload week `today` is in (its last day).
    pub deload_until: Option<NaiveDate>,
    /// Training weeks since the last deload, and the setting.
    pub weeks_since_deload: usize,
    pub deload_every: f64,
}

pub fn review(data: &Data, analysis: &[Analysis], today: NaiveDate) -> WeekReview {
    let start = week_start(today);
    let last_start = start - chrono::Duration::days(7);
    let sets_in = |from: NaiveDate, to: NaiveDate| {
        data.log.iter().filter(|e| e.date >= from && e.date < to).map(|e| e.sets.iter().flatten().filter(|&&r| r > 0.0).count() as f64).sum::<f64>()
    };
    let mut sessions: Vec<(String, NaiveDate)> = Vec::new();
    for (date, workout) in data.sessions_done().filter(|&(d, _)| d >= start && d <= today) {
        if !sessions.iter().any(|(w, d)| w == workout && *d == date) {
            sessions.push((workout.to_string(), date));
        }
    }
    sessions.sort_by_key(|(_, d)| *d);
    let pb = crate::coach::personal_bests(data, analysis);
    let pbs = (0..data.log.len())
        .filter(|&i| pb[i] && data.log[i].date >= start && data.log[i].date <= today)
        .map(|i| {
            let e = &data.log[i];
            if analysis[i].load_type == BODYWEIGHT { format!("{} × {}", e.exercise, crate::coach::sets_text(e)) } else { format!("{} {}kg × {}", e.exercise, crate::engine::fmt_num(e.weight), crate::coach::sets_text(e)) }
        })
        .collect();
    let rot = (data.session_order == crate::model::ROTATION).then(|| crate::coach::rotation(data, today + chrono::Duration::days(1))).flatten();
    WeekReview {
        week_start: start,
        sessions,
        sets: sets_in(start, today + chrono::Duration::days(1)),
        sets_last_week: sets_in(last_start, start),
        pbs,
        rounds_completed: rot.as_ref().map_or(0, |r| r.completed.iter().filter(|&&d| d >= start && d <= today).count()),
        owed: rot.filter(|r| !r.ahead.is_empty()).map(|r| r.next),
        deload_until: data.deload_on(today).map(|s| s + chrono::Duration::days(6)),
        weeks_since_deload: training_weeks_since_deload(data, today),
        deload_every: data.deload_every,
    }
}
