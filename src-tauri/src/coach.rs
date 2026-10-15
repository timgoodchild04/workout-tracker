//! Port of the 📝 Workout Log helper columns (I:AI), the 🏠 Dashboard and the
//! 📈 Progress tab. Each function names the sheet column it reproduces.

use crate::engine::{fmt_num, round2, Engine, Load, BODYWEIGHT, FIXED};
use crate::model::{Data, LogEntry, ProgRow, SchedDay};
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

/// Find an exercise in the stacked {main; alt} programme columns, first match
/// wins, exactly like the sheet's MATCH over {B5:B61; I5:I61}.
/// Returns (rep_low, rep_high, equipment).
fn programme_lookup<'a>(prog: &'a [ProgRow], exercise: &str) -> Option<(f64, f64, &'a str)> {
    prog.iter()
        .find(|p| p.exercise == exercise)
        .map(|p| (p.rep_low, p.rep_high, p.equipment.as_str()))
        .or_else(|| prog.iter().find(|p| p.alt_exercise == exercise).map(|p| (p.rep_low, p.rep_high, p.alt_equipment.as_str())))
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

        let (rep_low, rep_high, equipment) = match programme_lookup(&data.programme, &e.exercise) {
            Some((l, h, eq)) => (Some(l), Some(h), eq.to_string()),
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
}

#[derive(Debug, Clone, Serialize)]
pub struct Logged {
    pub weight: f64,
    pub sets: [Option<f64>; 4],
    pub decision: String,
    pub verdict: Verdict,
    /// Still queued on this computer, not yet in the sheet.
    pub pending: bool,
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
    for p in data.programme.iter().filter(|p| p.workout == session && p.enabled) {
        let main_on = engine.equipment_on(&p.equipment);
        let alt_on = !p.alt_exercise.is_empty() && engine.equipment_on(&p.alt_equipment);
        let (exercise, equipment, start, video) = if main_on {
            (&p.exercise, &p.equipment, p.start_weight, &p.video)
        } else if alt_on {
            (&p.alt_exercise, &p.alt_equipment, p.alt_start_weight, &p.alt_video)
        } else {
            skipped.push(p.exercise.clone());
            continue;
        };
        let load_type = engine.load_type(equipment);
        let last = data.log.iter().rposition(|e| &e.exercise == exercise && e.date < date);
        let logged = data.log.iter().rposition(|e| &e.exercise == exercise && e.date == date).map(|i| Logged {
            weight: data.log[i].weight,
            sets: data.log[i].sets,
            decision: analysis[i].decision.clone(),
            verdict: analysis[i].verdict,
            pending: data.log[i].queued || data.log[i].row.is_none(),
            row: data.log[i].row,
        });
        let (recommended, recommended_kg, previous_weight, previous_reps, previous_date, trend, verdict, instruction) = match last {
            None => {
                let range = format!("{}–{}", fmt_num(p.rep_low), fmt_num(p.rep_high));
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
        cards.push(Card {
            exercise: exercise.clone(),
            original: p.exercise.clone(),
            substituted: !main_on,
            equipment: equipment.clone(),
            load_type,
            video: video.clone(),
            sets: p.sets,
            rep_low: p.rep_low,
            rep_high: p.rep_high,
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

/// Sessions in programme order, de-duplicated.
pub fn sessions(data: &Data) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for p in &data.programme {
        if !v.contains(&p.workout) {
            v.push(p.workout.clone());
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
    let mut names: Vec<String> = data.programme.iter().map(|p| p.exercise.clone()).chain(data.log.iter().map(|e| e.exercise.clone())).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let equipment = programme_lookup(&data.programme, &name).map(|x| x.2.to_string()).unwrap_or_default();
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
                // No longer in the programme and logged without a weight: the sheet shows a blank cell.
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
