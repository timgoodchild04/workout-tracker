//! Exercise order and plate stacking for a session with the fewest plate moves.
//!
//! You have one bar and one pair of dumbbells sharing the same plates. Plates
//! sit on a sleeve as a stack: to reach a plate you first take off everything
//! outside it. So the order plates go on matters as much as which plates.
//! Every plate on or off counts as one move: x2 per change on the bar (both
//! sides) and x4 on the dumbbells (both ends of both handles).
//!
//! Plates stay on between sessions. So a session starts from whatever the
//! previous one left on the bar and dumbbells, worked out by replaying the log
//! (`kit_before`). The log is shared, so every computer gets the same answer.
//!
//! For each exercise the planner picks how to stack the plates (any order is
//! allowed), searching every arrangement across the whole session, so plates
//! that are needed for longest go innermost. Ties prefer the order closest
//! to the programme, then heavier plates inside.
//!
//! Rules, so the exercise order still makes sense as training:
//! - only plate-loaded exercises move; bodyweight / band work keeps its slot
//!   (pull-ups first, abs last);
//! - the first plate-loaded exercise is the session's main lift and stays first;
//! - compound slots stay compound and isolation slots stay isolation.

use crate::coach::{Analysis, Card};
use crate::model::Data;
use chrono::NaiveDate;
use crate::engine::{fmt_num, Engine, BARBELL, DUMBBELL, SIZES};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    /// Card indices in the order to do them.
    pub order: Vec<usize>,
    /// Total plates on + off for the whole session.
    pub moves: u32,
    /// The same exercise order loaded the usual way (heaviest first, keep what
    /// fits), for comparison.
    pub naive_moves: u32,
    /// What to change before each card (indexed by card, empty for non-plate work).
    pub changes: Vec<Vec<String>>,
}

/// Plate size indexes (into SIZES), innermost first.
type Stack = Vec<u8>;

#[derive(Clone, Copy)]
struct Need {
    imp: usize, // 0 = bar, 1 = dumbbells
    n: [u32; 4],
}

const NAME: [&str; 2] = ["Bar (each side)", "Dumbbells (each end)"];
const PER: [u32; 2] = [2, 4]; // physical plates moved per change

fn needs(cards: &[Card], engine: &Engine) -> Vec<Option<Need>> {
    cards
        .iter()
        .map(|c| {
            let imp = match c.load_type.as_str() {
                BARBELL => 0,
                DUMBBELL => 1,
                _ => return None,
            };
            engine.plates_for(&c.load_type, c.recommended_kg).map(|n| Need { imp, n })
        })
        .collect()
}

/// Every distinct stacking of a set of plates.
fn arrangements(n: &[u32; 4]) -> Vec<Stack> {
    fn go(left: &mut [u32; 4], cur: &mut Stack, out: &mut Vec<Stack>) {
        if left.iter().all(|&x| x == 0) {
            out.push(cur.clone());
            return;
        }
        for i in (0..4).rev() {
            if left[i] > 0 {
                left[i] -= 1;
                cur.push(i as u8);
                go(left, cur, out);
                cur.pop();
                left[i] += 1;
            }
        }
    }
    let mut out = Vec::new();
    go(&mut n.clone(), &mut Vec::new(), &mut out);
    out
}

fn lcp(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// Lighter plates sitting inside heavier ones (the tie-break).
fn inversions(s: &[u8]) -> u32 {
    let mut n = 0;
    for i in 0..s.len() {
        for j in i + 1..s.len() {
            if s[i] < s[j] {
                n += 1;
            }
        }
    }
    n
}

fn count(s: &[u8], i: usize) -> u32 {
    s.iter().filter(|&&x| x as usize == i).count() as u32
}

/// Take plates off the other implement, outermost first, until the pool has
/// enough for `stack` on `me`. Returns the plates removed.
fn strip(other: &mut Stack, stack: &[u8], me: usize, pool: [u32; 4]) -> Stack {
    let fits = |o: &Stack| (0..4).all(|i| PER[me] * count(stack, i) + PER[1 - me] * count(o, i) <= pool[i]);
    let mut removed = Vec::new();
    while !fits(other) {
        match other.pop() {
            Some(p) => removed.push(p),
            None => break,
        }
    }
    removed
}

fn plates_text(s: &[u8]) -> String {
    s.iter().map(|&i| format!("{}kg", fmt_num(SIZES[i as usize]))).collect::<Vec<_>>().join(", ")
}

/// Cost so far: (moves, lighter-inside-heavier count).
type Cost = (u32, u32);

fn step_cost(old: &[u8], new: &[u8], me: usize) -> u32 {
    let k = lcp(old, new);
    PER[me] * ((old.len() - k) + (new.len() - k)) as u32
}

#[derive(Clone)]
struct Step {
    ci: usize,
    me: usize,
    new: Stack,
    off: Stack,      // taken off `me`, outermost first
    on: Stack,       // put on `me`, in order
    stripped: Stack, // taken off the other implement, outermost first
}

pub type State = [Stack; 2];

/// Best stacking for a fixed exercise order: dynamic programming over what's
/// on the bar and the dumbbells after each exercise.
fn best_stacking(order: &[usize], needs: &[Option<Need>], pool: [u32; 4], arr: &HashMap<usize, Vec<Stack>>, start: &State) -> (Cost, Vec<Step>, State) {
    // Each layer: (state, cost, index of parent in previous layer, step taken)
    let mut layers: Vec<Vec<(State, Cost, usize, Option<Step>)>> = vec![vec![(start.clone(), (0, 0), 0, None)]];
    for &ci in order {
        let Some(need) = needs[ci] else { continue };
        let me = need.imp;
        let prev = layers.last().unwrap();
        let mut next: HashMap<State, (Cost, usize, Step)> = HashMap::new();
        for (pi, (state, cost, _, _)) in prev.iter().enumerate() {
            for new in &arr[&ci] {
                let mut other = state[1 - me].clone();
                let stripped = strip(&mut other, new, me, pool);
                let moves = step_cost(&state[me], new, me) + PER[1 - me] * stripped.len() as u32;
                let c = (cost.0 + moves, cost.1 + inversions(new));
                let mut ns: State = [vec![], vec![]];
                ns[me] = new.clone();
                ns[1 - me] = other;
                if next.get(&ns).is_none_or(|(old, _, _)| c < *old) {
                    let k = lcp(&state[me], new);
                    let step = Step {
                        ci,
                        me,
                        new: new.clone(),
                        off: state[me][k..].iter().rev().copied().collect(),
                        on: new[k..].to_vec(),
                        stripped,
                    };
                    next.insert(ns, (c, pi, step));
                }
            }
        }
        // Sorted so ties always resolve the same way: the log replay has to
        // reproduce exactly the plan that was shown on the day.
        let mut layer: Vec<_> = next.into_iter().map(|(s, (c, p, st))| (s, c, p, Some(st))).collect();
        layer.sort_by(|a, b| a.0.cmp(&b.0));
        layers.push(layer);
    }
    let last = layers.last().unwrap();
    let (mut idx, cost) = last.iter().enumerate().min_by(|a, b| a.1 .1.cmp(&b.1 .1).then(a.1 .0.cmp(&b.1 .0))).map(|(i, l)| (i, l.1)).unwrap();
    let end = last[idx].0.clone();
    let mut steps = Vec::new();
    for li in (1..layers.len()).rev() {
        let (_, _, parent, step) = &layers[li][idx];
        steps.push(step.clone().unwrap());
        idx = *parent;
    }
    steps.reverse();
    (cost, steps, end)
}

/// The usual way: keep whatever inner plates still fit, add the rest heaviest first.
fn naive_moves(order: &[usize], needs: &[Option<Need>], pool: [u32; 4], start: &State) -> u32 {
    let mut state: State = start.clone();
    let mut moves = 0;
    for &ci in order {
        let Some(need) = needs[ci] else { continue };
        let me = need.imp;
        let mut left = need.n;
        let mut keep = 0;
        for &p in &state[me] {
            if left[p as usize] == 0 {
                break;
            }
            left[p as usize] -= 1;
            keep += 1;
        }
        let mut new: Stack = state[me][..keep].to_vec();
        for i in (0..4).rev() {
            new.extend(std::iter::repeat_n(i as u8, left[i] as usize));
        }
        moves += step_cost(&state[me], &new, me);
        let mut other = state[1 - me].clone();
        moves += PER[1 - me] * strip(&mut other, &new, me, pool).len() as u32;
        state[me] = new;
        state[1 - me] = other;
    }
    moves
}

fn describe(steps: &[Step], n: usize) -> Vec<Vec<String>> {
    let mut out = vec![Vec::new(); n];
    for s in steps {
        let lines = &mut out[s.ci];
        if !s.stripped.is_empty() {
            lines.push(format!(
                "{}: take off {} (outermost first, {})",
                NAME[1 - s.me],
                plates_text(&s.stripped),
                if s.me == 0 { "the bar needs them" } else { "the dumbbells need them" }
            ));
        }
        let line = match (s.off.is_empty(), s.on.is_empty()) {
            (true, true) => "no change".to_string(),
            (false, true) => format!("take off {} (outermost first)", plates_text(&s.off)),
            (true, false) => format!("put on {}, in that order", plates_text(&s.on)),
            (false, false) => format!("take off {} (outermost first), then put on {}, in that order", plates_text(&s.off), plates_text(&s.on)),
        };
        lines.push(format!("{}: {}", NAME[s.me], line));
        if !s.new.is_empty() {
            lines.push(format!("Sleeve now, inside → out: {}", plates_text(&s.new)));
        }
    }
    out
}

fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, first);
            out.push(p);
        }
    }
    out
}

/// (as programmed, fewest plate moves). Both use the best stacking for their order.
pub fn plans(cards: &[Card], engine: &Engine, start: &State) -> (Plan, Plan) {
    let needs = needs(cards, engine);
    let pool = engine.pool();
    let arr: HashMap<usize, Vec<Stack>> = needs.iter().enumerate().filter_map(|(i, n)| n.map(|n| (i, arrangements(&n.n)))).collect();
    let make = |order: Vec<usize>| {
        let (cost, steps, _) = best_stacking(&order, &needs, pool, &arr, start);
        Plan { naive_moves: naive_moves(&order, &needs, pool, start), moves: cost.0, changes: describe(&steps, cards.len()), order }
    };
    let programme: Vec<usize> = (0..cards.len()).collect();

    // Skip the first plate-loaded exercise: the main lift keeps its place.
    let slots: Vec<usize> = (0..cards.len()).filter(|&i| needs[i].is_some()).skip(1).collect();
    let iso_low = engine.settings.isolation_rep_low;
    let is_iso = |i: usize| cards[i].rep_low >= iso_low;
    let comp: Vec<usize> = slots.iter().copied().filter(|&i| !is_iso(i)).collect();
    let iso: Vec<usize> = slots.iter().copied().filter(|&i| is_iso(i)).collect();
    let fact = |n: usize| (1..=n).product::<usize>();
    if fact(comp.len()) * fact(iso.len()) > 720 {
        return (make(programme.clone()), make(programme));
    }

    let mut best: Option<((u32, usize, u32), Vec<usize>)> = None;
    let iso_perms = permutations(&iso);
    for cp in permutations(&comp) {
        for ip in &iso_perms {
            let (mut c, mut s) = (cp.iter(), ip.iter());
            let order: Vec<usize> = (0..cards.len())
                .map(|i| {
                    if !slots.contains(&i) {
                        i
                    } else if is_iso(i) {
                        *s.next().unwrap()
                    } else {
                        *c.next().unwrap()
                    }
                })
                .collect();
            let (cost, _, _) = best_stacking(&order, &needs, pool, &arr, start);
            let shift: usize = order.iter().enumerate().map(|(pos, &ci)| pos.abs_diff(ci)).sum();
            let key = (cost.0, shift, cost.1);
            if best.as_ref().is_none_or(|(k, _)| key < *k) {
                best = Some((key, order));
            }
        }
    }
    let best = best.map(|b| b.1).unwrap_or_else(|| programme.clone());
    (make(programme), make(best))
}

/// What's on the kit, for display: plate weights inside → out.
#[derive(Debug, Clone, Serialize)]
pub struct Kit {
    /// Date of the session that left the plates on (None = nothing logged yet).
    pub from: Option<NaiveDate>,
    pub bar: Vec<f64>,
    pub dumbbells: Vec<f64>,
}

impl Kit {
    pub fn new(state: &State, from: Option<NaiveDate>) -> Kit {
        let kg = |s: &Stack| s.iter().map(|&i| SIZES[i as usize]).collect();
        Kit { from, bar: kg(&state[0]), dumbbells: kg(&state[1]) }
    }
}

/// What's on the bar and dumbbells before `date`: replay every logged session
/// before it, in the order the exercises were logged, at the weights actually
/// used, with the same planner the Today screen uses.
pub fn kit_before(data: &Data, analysis: &[Analysis], engine: &Engine, date: NaiveDate) -> (State, Option<NaiveDate>) {
    let pool = engine.pool();
    let mut days: Vec<NaiveDate> = data.log.iter().map(|e| e.date).filter(|d| *d < date).collect();
    days.sort();
    days.dedup();
    let mut state: State = [vec![], vec![]];
    let mut from = None;
    for day in days {
        let idx: Vec<usize> = (0..data.log.len()).filter(|&i| data.log[i].date == day).collect();
        let needs: Vec<Option<Need>> = idx
            .iter()
            .map(|&i| {
                let lt = analysis[i].load_type.as_str();
                let imp = match lt {
                    BARBELL => 0,
                    DUMBBELL => 1,
                    _ => return None,
                };
                engine.plates_for(lt, data.log[i].weight).map(|n| Need { imp, n })
            })
            .collect();
        if needs.iter().all(|n| n.is_none()) {
            continue;
        }
        let arr: HashMap<usize, Vec<Stack>> = needs.iter().enumerate().filter_map(|(i, n)| n.map(|n| (i, arrangements(&n.n)))).collect();
        let order: Vec<usize> = (0..idx.len()).collect();
        state = best_stacking(&order, &needs, pool, &arr, &state).2;
        from = Some(day);
    }
    (state, from)
}
