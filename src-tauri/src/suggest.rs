//! Coach suggestions from the plan and the log: weekly volume per muscle against
//! the targets on 📅 Plan, exercises that have stalled, and bodyweight moves that
//! are ready for their harder (or need their easier) version. Each suggestion can
//! carry one action the app applies to 📅 Plan (add a slot, or swap a slot's exercise).

use crate::coach::{resolve, Analysis};
use crate::engine::{Engine, BODYWEIGHT};
use crate::model::{Data, LibEx};
use chrono::NaiveDate;
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

pub fn suggestions(data: &Data, engine: &Engine, analysis: &[Analysis], _today: NaiveDate) -> Vec<Suggestion> {
    let mut out = Vec::new();

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
        let hist: Vec<usize> = (0..data.log.len()).filter(|&i| data.log[i].exercise == lib.name).collect();
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
