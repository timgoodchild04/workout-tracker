//! Plain data read from the workbook. Both sources (Google Sheets and a local
//! .xlsx snapshot) produce the same `Grid`s, and `Data::from_grids` turns them
//! into typed rows using the workbook's fixed layout.

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

pub const TAB_LIBRARY: &str = "📚 Exercises";
pub const TAB_PLAN: &str = "📅 Plan";
pub const TAB_SETTINGS: &str = "⚙️ Settings";
pub const TAB_SCHEDULE: &str = "Schedule";
pub const TAB_LOG: &str = "📝 Workout Log";

/// Ranges the app reads. Rows/columns match the sheet's own named ranges.
/// 📚 Exercises: one exercise per row (A:N), from row 5.
pub const RANGE_LIBRARY: &str = "A5:N204";
pub const LIBRARY_FIRST_ROW: usize = 5;
/// 📅 Plan: slots in A:E (rows 5-104), the Days table in R:S and weekly set
/// targets in U:V, all from row 5. Read as one block.
pub const RANGE_PLAN: &str = "A5:V104";
pub const PLAN_FIRST_ROW: usize = 5;
/// 📅 Plan's settings row: "Session order" (R2 label, S2 value) and
/// "Deload every (weeks)" (U2 label, V2 value).
pub const RANGE_PLAN_SETTINGS: &str = "R2:V2";
/// 📅 Plan column listing the start date of each deload week (from row 5).
pub const COL_DELOADS: &str = "T";
pub const ROTATION: &str = "Rotation";
/// Last 📅 Plan row the sheet's helper formulas and the Dashboard cover.
pub const PLAN_LAST_ROW: usize = 104;
/// ⚙️ Settings column holding each piece of equipment's Enabled? Yes/No.
pub const COL_EQUIPMENT_ON: &str = "B";
pub const RANGE_SETTINGS: &str = "A1:D100";
pub const RANGE_SCHEDULE: &str = "B13:E19";
pub const LOG_FIRST_ROW: usize = 2;
/// The log's helper formulas, and the Dashboard and Progress lookups, stop at
/// row 2000, so the sheet only "sees" entries up to here.
pub const LOG_LAST_FORMULA_ROW: usize = 2000;
pub const RANGE_LOG: &str = "A2:H2000";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Cell {
    Empty,
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Cell {
    pub fn num(&self) -> Option<f64> {
        match self {
            Cell::Num(n) => Some(*n),
            Cell::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn text(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Num(n) => crate::engine::fmt_num(*n),
            Cell::Str(s) => s.trim().to_string(),
            Cell::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        }
    }
    pub fn is_empty(&self) -> bool {
        matches!(self, Cell::Empty) || matches!(self, Cell::Str(s) if s.trim().is_empty())
    }
}

pub type Grid = Vec<Vec<Cell>>;

pub fn at(g: &Grid, r: usize, c: usize) -> &Cell {
    static EMPTY: Cell = Cell::Empty;
    g.get(r).and_then(|row| row.get(c)).unwrap_or(&EMPTY)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plate {
    pub weight: f64,
    pub owned: f64,
    pub enabled: bool,
}
impl Plate {
    pub fn usable_pairs(&self) -> u32 {
        if self.enabled { (self.owned / 2.0).floor().max(0.0) as u32 } else { 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Equipment {
    /// Sheet row on ⚙️ Settings.
    #[serde(default)]
    pub row: usize,
    pub name: String,
    pub enabled: bool,
    pub load_type: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LadderStep {
    pub equipment: String,
    pub label: String,
    pub kg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub bar_weight: f64,
    pub handle_weight: f64,
    pub plates: Vec<Plate>,
    pub jump_pct: f64,
    pub jump_abs: f64,
    pub ceiling_streak: f64,
    pub drop_pct: f64,
    /// ⚙️ Settings B18: lowest rep target of an isolation exercise (compounds sit below it)
    #[serde(default = "default_isolation_low")]
    pub isolation_rep_low: f64,
    pub equipment: Vec<Equipment>,
    pub ladder: Vec<LadderStep>,
}

fn default_isolation_low() -> f64 {
    10.0
}

/// One exercise in 📚 Exercises.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibEx {
    pub row: usize,
    pub name: String,
    /// Movement pattern; exercises with the same pattern can stand in for each other.
    pub pattern: String,
    pub primary: String,
    /// Secondary muscles (count half towards weekly volume).
    pub secondary: Vec<String>,
    pub equipment: String,
    pub sets: u32,
    pub rep_low: f64,
    pub rep_high: f64,
    pub start_weight: f64,
    pub video: String,
    pub easier: String,
    pub harder: String,
    /// Day focuses it suits (Push, Pull, Legs, Upper, Morning): the hint when picking.
    pub fits: Vec<String>,
    pub notes: String,
}

/// One slot of a day in 📅 Plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    pub row: usize,
    pub day: String,
    pub pattern: String,
    /// The exercise you'd like for this slot.
    pub exercise: String,
    /// Sets override; None = the library's.
    pub sets: Option<u32>,
    /// "On?" (blank counts as on).
    pub enabled: bool,
}

/// A day and its focus (📅 Plan's Days table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayInfo {
    pub name: String,
    pub focus: String,
}

/// Weekly working-set target for a muscle (📅 Plan's targets table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub muscle: String,
    pub sets: f64,
}

fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedDay {
    pub day: String,
    pub session: String,
    pub notes: String,
    /// Schedule column E: a second, short session due the same day ("" = none).
    #[serde(default)]
    pub morning: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub date: NaiveDate,
    pub workout: String,
    pub exercise: String,
    pub weight: f64,
    pub sets: [Option<f64>; 4],
    /// Sheet row number. In the local queue: None = append as a new row,
    /// Some(row) = an edit to that existing row.
    pub row: Option<usize>,
    /// Set on entries merged in from the local queue (not yet in the sheet).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data {
    pub settings: Settings,
    pub library: Vec<LibEx>,
    pub plan: Vec<Slot>,
    pub days: Vec<DayInfo>,
    pub targets: Vec<Target>,
    /// "Weekday schedule" (default) or "Rotation" (📅 Plan S2).
    #[serde(default)]
    pub session_order: String,
    /// Suggest a deload after this many training weeks (📅 Plan V2; 0 = never).
    #[serde(default)]
    pub deload_every: f64,
    /// Start date of each deload week (📅 Plan column T), oldest first.
    #[serde(default)]
    pub deloads: Vec<NaiveDate>,
    pub schedule: Vec<SchedDay>,
    pub log: Vec<LogEntry>,
}

impl Data {
    /// The library entry for an exercise (first match, like the sheet's MATCH).
    pub fn lib(&self, name: &str) -> Option<&LibEx> {
        self.library.iter().find(|l| l.name == name)
    }

    /// The deload week `date` falls in (its start), if any. A deload week is the start
    /// date and the six days after it.
    pub fn deload_on(&self, date: NaiveDate) -> Option<NaiveDate> {
        self.deloads.iter().rev().find(|&&s| s <= date && (date - s).num_days() < 7).copied()
    }

    /// A day's focus from the Days table ("" if it isn't listed).
    pub fn focus(&self, day: &str) -> &str {
        self.days.iter().find(|d| d.name == day).map(|d| d.focus.as_str()).unwrap_or("")
    }
}

/// Google/Excel serial date (days since 1899-12-30).
pub fn serial_to_date(n: f64) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(1899, 12, 30)?.checked_add_signed(Duration::days(n.floor() as i64))
}

pub fn parse_date(c: &Cell) -> Option<NaiveDate> {
    match c {
        Cell::Num(n) => serial_to_date(*n),
        Cell::Str(s) => {
            let s = s.trim();
            NaiveDate::parse_from_str(s, "%d/%m/%Y")
                .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                .or_else(|_| NaiveDate::parse_from_str(&s[..s.len().min(10)], "%Y-%m-%d"))
                .ok()
        }
        _ => None,
    }
}

/// Pull a URL out of a cell that is either a plain link or =HYPERLINK("url","label").
pub fn extract_url(c: &Cell) -> String {
    let s = c.text();
    if let Some(i) = s.find("http") {
        let rest = &s[i..];
        let end = rest.find(['"', ' ']).unwrap_or(rest.len());
        return rest[..end].to_string();
    }
    String::new()
}

fn yes(c: &Cell) -> bool {
    match c {
        Cell::Bool(b) => *b,
        _ => c.text().eq_ignore_ascii_case("yes"),
    }
}

impl Data {
    /// `library` is 📚 Exercises A5:N204, `plan` 📅 Plan A5:V104, `settings` A1:D100,
    /// `schedule` B13:E19, `log` A2:H…
    pub fn from_grids(library: &Grid, plan: &Grid, plan_settings: &Grid, settings: &Grid, schedule: &Grid, log: &Grid) -> Data {
        let s = |r: usize, c: usize| at(settings, r - 1, c); // 1-based sheet rows
        let num = |r: usize, c: usize, d: f64| s(r, c).num().unwrap_or(d);

        let plates = (9..=12)
            .filter_map(|r| {
                let w = s(r, 0).num()?;
                Some(Plate { weight: w, owned: num(r, 1, 0.0), enabled: yes(s(r, 2)) })
            })
            .collect();
        let equipment = (33..=52)
            .filter(|&r| !s(r, 0).is_empty())
            .map(|r| Equipment {
                row: r,
                name: s(r, 0).text(),
                enabled: yes(s(r, 1)),
                load_type: s(r, 2).text(),
                notes: s(r, 3).text(),
            })
            .collect();
        let ladder = (61..=100)
            .filter(|&r| !s(r, 0).is_empty() && s(r, 2).num().is_some())
            .map(|r| LadderStep { equipment: s(r, 0).text(), label: s(r, 1).text(), kg: num(r, 2, 0.0) })
            .collect();
        let settings = Settings {
            bar_weight: num(5, 1, 0.0),
            handle_weight: num(6, 1, 0.0),
            plates,
            jump_pct: num(22, 1, 0.15),
            jump_abs: num(23, 1, 2.5),
            ceiling_streak: num(24, 1, 1.0),
            drop_pct: num(25, 1, 0.25),
            isolation_rep_low: num(18, 1, 10.0),
            equipment,
            ladder,
        };

        let library = library
            .iter()
            .enumerate()
            .filter(|(i, _)| !at(library, *i, 0).is_empty())
            .map(|(i, _)| {
                let c = |col: usize| at(library, i, col);
                LibEx {
                    row: i + LIBRARY_FIRST_ROW,
                    name: c(0).text(),
                    pattern: c(1).text(),
                    primary: c(2).text(),
                    secondary: split_list(&c(3).text()),
                    equipment: c(4).text(),
                    sets: c(5).num().unwrap_or(3.0) as u32,
                    rep_low: c(6).num().unwrap_or(0.0),
                    rep_high: c(7).num().unwrap_or(0.0),
                    start_weight: c(8).num().unwrap_or(0.0),
                    video: extract_url(c(9)),
                    easier: c(10).text(),
                    harder: c(11).text(),
                    fits: split_list(&c(12).text()),
                    notes: c(13).text(),
                }
            })
            .collect();

        let slots = plan
            .iter()
            .enumerate()
            .filter(|(i, _)| !at(plan, *i, 0).is_empty() && !at(plan, *i, 2).is_empty())
            .map(|(i, _)| {
                let c = |col: usize| at(plan, i, col);
                Slot {
                    row: i + PLAN_FIRST_ROW,
                    day: c(0).text(),
                    pattern: c(1).text(),
                    exercise: c(2).text(),
                    sets: c(3).num().map(|n| n as u32),
                    enabled: !c(4).text().eq_ignore_ascii_case("no"),
                }
            })
            .collect();
        // Days table R:S (cols 17-18) and targets U:V (cols 20-21) share the block's rows.
        let days = (0..plan.len())
            .filter(|&i| !at(plan, i, 17).is_empty())
            .map(|i| DayInfo { name: at(plan, i, 17).text(), focus: at(plan, i, 18).text() })
            .collect();
        let targets = (0..plan.len())
            .filter(|&i| !at(plan, i, 20).is_empty())
            .map(|i| Target { muscle: at(plan, i, 20).text(), sets: at(plan, i, 21).num().unwrap_or(0.0) })
            .collect();

        let schedule = (0..7)
            .map(|i| SchedDay {
                day: at(schedule, i, 0).text(),
                session: at(schedule, i, 1).text(),
                notes: at(schedule, i, 2).text(),
                morning: at(schedule, i, 3).text(),
            })
            .collect();

        let log = log
            .iter()
            .enumerate()
            .filter_map(|(i, _)| {
                let c = |col: usize| at(log, i, col);
                if c(2).is_empty() {
                    return None;
                }
                Some(LogEntry {
                    date: parse_date(c(0)).unwrap_or_default(),
                    workout: c(1).text(),
                    exercise: c(2).text(),
                    weight: c(3).num().unwrap_or(0.0),
                    sets: [c(4).num(), c(5).num(), c(6).num(), c(7).num()],
                    row: Some(i + LOG_FIRST_ROW),
                    queued: false,
                })
            })
            .collect();

        let session_order = at(plan_settings, 0, 1).text();
        let deload_every = at(plan_settings, 0, 4).num().unwrap_or(6.0);
        let mut deloads: Vec<NaiveDate> = (0..plan.len()).filter_map(|i| parse_date(at(plan, i, 19))).collect();
        deloads.sort();
        Data { settings, library, plan: slots, days, targets, session_order, deload_every, deloads, schedule, log }
    }
}
