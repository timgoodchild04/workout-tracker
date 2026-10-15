//! Port of the hidden PlateEngine / DumbbellEngine tabs: every weight you can
//! physically build from the shared plate pool, and the plate text for it.

use crate::model::Settings;

/// Pairs-per-side caps hard-coded in the engines' K1:K4 (0.5, 1.25, 2.5, 5kg).
const ENGINE_CAPS: [u32; 4] = [4, 15, 6, 4];
/// The engines only know these four plate sizes, in this order.
const SIZES: [f64; 4] = [0.5, 1.25, 2.5, 5.0];

pub const BARBELL: &str = "Barbell plates";
pub const DUMBBELL: &str = "Dumbbell plates";
pub const BODYWEIGHT: &str = "Bodyweight";
pub const FIXED: &str = "Fixed steps";

/// Format a number the way Sheets does when it is concatenated into text.
pub fn fmt_num(n: f64) -> String {
    let r = (n * 100.0).round() / 100.0;
    let s = format!("{:.2}", r);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

pub fn round2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

#[derive(Debug, Clone)]
pub struct Combo {
    pub total: f64,
    pub plates: u32,
    pub text: String,
}
impl Combo {
    fn rank(&self) -> f64 {
        self.total * 1000.0 + self.plates as f64
    }
}

fn pairs_for(settings: &Settings, size: f64) -> u32 {
    settings
        .plates
        .iter()
        .filter(|p| (p.weight - size).abs() < 1e-9)
        .map(|p| p.usable_pairs())
        .sum()
}

fn plate_list(n: &[u32; 4]) -> String {
    // Heaviest first: 5, 2.5, 1.25, 0.5
    (0..4)
        .rev()
        .filter(|&i| n[i] > 0)
        .map(|i| format!("{}x{}kg", n[i], fmt_num(SIZES[i])))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn combos(settings: &Settings, dumbbell: bool) -> Vec<Combo> {
    let mut max = [0u32; 4];
    for i in 0..4 {
        let m = pairs_for(settings, SIZES[i]).min(ENGINE_CAPS[i]);
        // A dumbbell set is two handles, so each "per end" plate needs two pairs.
        max[i] = if dumbbell { m / 2 } else { m };
    }
    let mut out = Vec::new();
    for a in 0..=max[0] {
        for b in 0..=max[1] {
            for c in 0..=max[2] {
                for d in 0..=max[3] {
                    let n = [a, b, c, d];
                    let per: f64 = (0..4).map(|i| n[i] as f64 * SIZES[i]).sum();
                    let count: u32 = n.iter().sum();
                    out.push(if dumbbell {
                        let total = round2(settings.handle_weight + 2.0 * per);
                        let plates = 2 * count;
                        let text = if plates == 0 {
                            format!("{}kg — handle only, no plates", fmt_num(total))
                        } else {
                            format!("{}kg per dumbbell — {} each end ({} plates)", fmt_num(total), plate_list(&n), plates)
                        };
                        Combo { total, plates, text }
                    } else {
                        let total = round2(settings.bar_weight + 2.0 * per);
                        let text = if count == 0 {
                            format!("{}kg — bar only, no plates", fmt_num(total))
                        } else {
                            format!(
                                "{}kg total — {} each side ({} plates, {} a side)",
                                fmt_num(total),
                                plate_list(&n),
                                count * 2,
                                count
                            )
                        };
                        Combo { total, plates: count, text }
                    });
                }
            }
        }
    }
    out
}

/// Pre-computed lookup tables for one set of settings.
pub struct Engine {
    pub settings: Settings,
    barbell: Vec<Combo>,
    dumbbell: Vec<Combo>,
}

/// A load the coach can recommend: the kg figure plus how to build it.
#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub kg: f64,
    pub text: String,
}

impl Engine {
    pub fn new(settings: &Settings) -> Engine {
        Engine { settings: settings.clone(), barbell: combos(settings, false), dumbbell: combos(settings, true) }
    }

    pub fn load_type(&self, equipment: &str) -> String {
        self.settings
            .equipment
            .iter()
            .find(|e| e.name == equipment)
            .map(|e| e.load_type.clone())
            .unwrap_or_default()
    }

    pub fn equipment_on(&self, equipment: &str) -> bool {
        !equipment.is_empty() && self.settings.equipment.iter().any(|e| e.name == equipment && e.enabled)
    }

    fn table(&self, load_type: &str) -> Option<&[Combo]> {
        match load_type {
            BARBELL => Some(&self.barbell),
            DUMBBELL => Some(&self.dumbbell),
            _ => None,
        }
    }

    /// Closest buildable load to `target` (fewest plates on a tie), like the
    /// sheet's LET(... SORT(FILTER({text,rank}, d=min(d)), 2) ...) lookups.
    pub fn closest(&self, load_type: &str, equipment: &str, target: f64) -> Option<Load> {
        let t = round2(target);
        if load_type == BODYWEIGHT {
            return Some(Load { kg: 0.0, text: "BW".into() });
        }
        if let Some(tab) = self.table(load_type) {
            return tab
                .iter()
                .min_by(|a, b| {
                    let da = (a.total - t).abs();
                    let db = (b.total - t).abs();
                    da.partial_cmp(&db).unwrap().then(a.rank().partial_cmp(&b.rank()).unwrap())
                })
                .map(|c| Load { kg: c.total, text: c.text.clone() });
        }
        if load_type == FIXED {
            let steps: Vec<_> = self.settings.ladder.iter().filter(|s| s.equipment == equipment).collect();
            let best = steps.iter().map(|s| (s.kg - t).abs()).fold(f64::INFINITY, f64::min);
            return steps.iter().find(|s| (s.kg - t).abs() == best).map(|s| Load {
                kg: s.kg,
                text: format!("{} — ~{}kg equivalent", s.label, fmt_num(s.kg)),
            });
        }
        None
    }

    /// The next load strictly above `current`. `None` when the load type has no
    /// notion of weight (bodyweight / unknown); `Some(0.0)` when nothing heavier exists.
    pub fn next_up(&self, load_type: &str, equipment: &str, current: f64) -> Option<f64> {
        let c = round2(current);
        let pick = |it: &mut dyn Iterator<Item = f64>| it.filter(|&w| w > c).fold(None, |m: Option<f64>, w| Some(m.map_or(w, |m| m.min(w)))).unwrap_or(0.0);
        if let Some(tab) = self.table(load_type) {
            return Some(pick(&mut tab.iter().map(|x| x.total)));
        }
        if load_type == FIXED {
            return Some(pick(&mut self.settings.ladder.iter().filter(|s| s.equipment == equipment).map(|s| s.kg)));
        }
        None
    }
}
