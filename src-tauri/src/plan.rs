//! Exercise order for a session that needs the fewest plate moves.
//!
//! You have one bar and one pair of dumbbells sharing the same plates. Each
//! plate taken off or put on counts as one move. That means x2 per change on the
//! bar (both sides) and x4 on the dumbbells (both ends of both handles). When the
//! next exercise needs plates that are sitting on the other implement, they come
//! off it too. You start with everything on the rack.
//!
//! Rules, so the order still makes sense as training:
//! - only plate-loaded exercises move; bodyweight / band work keeps its slot
//!   (pull-ups first, abs last);
//! - the first plate-loaded exercise is the session's main lift and stays first;
//! - compound slots stay compound and isolation slots stay isolation, so an
//!   isolation exercise never jumps ahead of a main lift;
//! - on a tie, the order closest to the programme wins.

use crate::coach::Card;
use crate::engine::{plate_list, Engine, BARBELL, DUMBBELL};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    /// Card indices in the order to do them.
    pub order: Vec<usize>,
    /// Total plates on + off for the whole session.
    pub moves: u32,
    /// What to change before each card (indexed by card, empty for non-plate work).
    pub changes: Vec<Vec<String>>,
}

#[derive(Clone, Copy)]
struct Need {
    imp: usize, // 0 = bar, 1 = dumbbells
    n: [u32; 4],
}

const NAME: [&str; 2] = ["Bar (each side)", "Dumbbells (each end)"];
const PER: [u32; 2] = [2, 4]; // physical plates per count

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

fn list_or_none(n: &[u32; 4]) -> Option<String> {
    if n.iter().all(|&x| x == 0) { None } else { Some(plate_list(n)) }
}

fn simulate(order: &[usize], needs: &[Option<Need>], pool: [u32; 4], want_text: bool) -> (u32, Vec<Vec<String>>) {
    let mut state = [[0u32; 4]; 2];
    let mut moves = 0;
    let mut changes = vec![Vec::new(); needs.len()];
    for &ci in order {
        let Some(need) = needs[ci] else { continue };
        let (me, other) = (need.imp, 1 - need.imp);
        let mut off = [0u32; 4];
        let mut on = [0u32; 4];
        for i in 0..4 {
            off[i] = state[me][i].saturating_sub(need.n[i]);
            on[i] = need.n[i].saturating_sub(state[me][i]);
        }
        moves += PER[me] * (off.iter().sum::<u32>() + on.iter().sum::<u32>());
        state[me] = need.n;

        // Shared pool: borrow plates from the other implement if the rack is short.
        let mut strip = [0u32; 4];
        for i in 0..4 {
            let used = PER[me] * need.n[i];
            let keep = pool[i].saturating_sub(used) / PER[other];
            if state[other][i] > keep {
                strip[i] = state[other][i] - keep;
                state[other][i] = keep;
            }
        }
        moves += PER[other] * strip.iter().sum::<u32>();

        if want_text {
            let lines = &mut changes[ci];
            if let Some(s) = list_or_none(&strip) {
                lines.push(format!("{}: take off {} ({})", NAME[other], s, if me == 0 { "the bar needs them" } else { "the dumbbells need them" }));
            }
            match (list_or_none(&off), list_or_none(&on)) {
                (None, None) => lines.push(format!("{}: no change", NAME[me])),
                (Some(o), None) => lines.push(format!("{}: take off {}", NAME[me], o)),
                (None, Some(p)) => lines.push(format!("{}: put on {}", NAME[me], p)),
                (Some(o), Some(p)) => lines.push(format!("{}: take off {}, put on {}", NAME[me], o, p)),
            }
        }
    }
    (moves, changes)
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

fn factorial(n: usize) -> usize {
    (1..=n).product()
}

/// (as programmed, fewest plate moves)
pub fn plans(cards: &[Card], engine: &Engine) -> (Plan, Plan) {
    let needs = needs(cards, engine);
    let pool = engine.pool();
    let make = |order: Vec<usize>| {
        let (moves, changes) = simulate(&order, &needs, pool, true);
        Plan { order, moves, changes }
    };
    let programme: Vec<usize> = (0..cards.len()).collect();

    let iso_low = engine.settings.isolation_rep_low;
    // Skip the first plate-loaded exercise: the main lift keeps its place.
    let slots: Vec<usize> = (0..cards.len()).filter(|&i| needs[i].is_some()).skip(1).collect();
    let is_iso = |i: usize| cards[i].rep_low >= iso_low;
    let comp: Vec<usize> = slots.iter().copied().filter(|&i| !is_iso(i)).collect();
    let iso: Vec<usize> = slots.iter().copied().filter(|&i| is_iso(i)).collect();
    if factorial(comp.len()) * factorial(iso.len()) > 200_000 {
        return (make(programme.clone()), make(programme));
    }

    let mut best: Option<(u32, usize, Vec<usize>)> = None;
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
            let (moves, _) = simulate(&order, &needs, pool, false);
            let shift: usize = order.iter().enumerate().map(|(pos, &ci)| pos.abs_diff(ci)).sum();
            if best.as_ref().is_none_or(|(m, d, _)| (moves, shift) < (*m, *d)) {
                best = Some((moves, shift, order));
            }
        }
    }
    let best = best.map(|b| b.2).unwrap_or_else(|| programme.clone());
    (make(programme), make(best))
}
