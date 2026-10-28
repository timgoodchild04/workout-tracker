//! Rowing: 3000m pieces with a rest between. Every piece is the same distance, so
//! the numbers that matter are each piece's time and the average split per 500m
//! (what the erg shows), with watts for the trend (2.80 / pace³, pace in s/m).

use crate::model::{RowSession, PIECE_M};
use chrono::NaiveDate;
use serde::Serialize;

/// A gap this long counts as a break: the next target eases off.
const BREAK_DAYS: i64 = 14;
/// Piece 2 this much slower per 500m than piece 1 = went out too hard.
const FADE_SECS: f64 = 2.0;
/// How much faster per 500m to aim for after an even session.
const STEP_SECS: f64 = 0.5;

/// Seconds as "m:ss.s" (or "h:mm:ss.s" past an hour).
pub fn fmt_time(secs: f64) -> String {
    let tenths = (secs * 10.0).round() as i64;
    let (h, m, s) = (tenths / 36000, tenths / 600 % 60, (tenths % 600) as f64 / 10.0);
    if h > 0 { format!("{h}:{m:02}:{s:04.1}") } else { format!("{m}:{s:04.1}") }
}

pub fn split_of(piece_secs: f64) -> f64 {
    piece_secs / (PIECE_M / 500.0)
}

pub fn watts(split_secs: f64) -> f64 {
    (2.80 / (split_secs / 500.0).powi(3)).round()
}

#[derive(Debug, Clone, Serialize)]
pub struct RowView {
    pub date: NaiveDate,
    pub pieces: [Option<f64>; 3],
    pub rest_min: Option<f64>,
    pub spm: Option<f64>,
    pub hr: Option<f64>,
    pub notes: String,
    pub row: Option<usize>,
    pub pending: bool,
    pub metres: f64,
    /// Average split per 500m across the pieces, seconds.
    pub split: f64,
    pub watts: f64,
    /// Piece 2's split minus piece 1's (positive = slowed down).
    pub fade: Option<f64>,
    /// Faster average split than every earlier session.
    pub pb: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    /// Average split to aim for, seconds per 500m.
    pub split: f64,
    /// The 3000m time that split gives.
    pub piece: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Rowing {
    /// The sheet has a 🚣 Rowing tab yet.
    pub tab: bool,
    /// Newest first.
    pub sessions: Vec<RowView>,
    pub best_piece: Option<(f64, NaiveDate)>,
    pub best_split: Option<(f64, NaiveDate)>,
    /// For a session on the date being viewed.
    pub target: Option<Target>,
}

fn view_of(e: &RowSession) -> RowView {
    let done: Vec<f64> = e.pieces.iter().flatten().copied().collect();
    let metres = done.len() as f64 * PIECE_M;
    let split = split_of(done.iter().sum::<f64>() / done.len().max(1) as f64);
    let fade = match (e.pieces[0], e.pieces[1]) {
        (Some(a), Some(b)) => Some(split_of(b) - split_of(a)),
        _ => None,
    };
    RowView {
        date: e.date,
        pieces: e.pieces,
        rest_min: e.rest_min,
        spm: e.spm,
        hr: e.hr,
        notes: e.notes.clone(),
        row: e.row,
        pending: e.queued || e.row.is_none(),
        metres,
        split,
        watts: if done.is_empty() { 0.0 } else { watts(split) },
        fade,
        pb: false,
    }
}

/// Round to the nearest tenth of a second.
fn tenth(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// What to aim for on `date`, from the last session before it.
pub fn target(sessions: &[RowView], date: NaiveDate) -> Target {
    let Some(last) = sessions.iter().filter(|s| s.date < date && s.metres > 0.0).max_by_key(|s| s.date) else {
        return Target {
            split: 0.0,
            piece: 0.0,
            text: "First row logged here. Pick a pace you can hold for both 3000s; that's the one to beat next time.".into(),
        };
    };
    let rest = last.rest_min.map(|r| format!(" Keep the rest at {} min so the times compare.", crate::engine::fmt_num(r))).unwrap_or_default();
    let gap = (date - last.date).num_days();
    let (split, why) = if gap >= BREAK_DAYS {
        (tenth(last.split + 1.0), format!("{gap} days since your last row, so ease in a second slower than last time's {}.", fmt_time(last.split)))
    } else if let Some(f) = last.fade.filter(|&f| f > FADE_SECS) {
        (
            tenth(last.split),
            format!(
                "Piece 2 was {:.1}s/500m slower than piece 1 last time. Hold last time's average, {}, on both: start a bit slower and finish stronger.",
                f,
                fmt_time(last.split)
            ),
        )
    } else {
        let s = tenth(last.split - STEP_SECS);
        (s, format!("Even pieces last time at {} average. Take half a second off.", fmt_time(last.split)))
    };
    let piece = split * PIECE_M / 500.0;
    Target { split, piece, text: format!("{why} Aim for {}/500m, {} per 3000m.{rest}", fmt_time(split), fmt_time(piece)) }
}

pub fn rowing(sessions: &[RowSession], tab: bool, date: NaiveDate) -> Rowing {
    let mut views: Vec<RowView> = sessions.iter().map(view_of).filter(|v| v.metres > 0.0).collect();
    views.sort_by_key(|v| v.date);
    let mut best: Option<f64> = None;
    for v in views.iter_mut() {
        v.pb = best.is_some_and(|b| v.split < b);
        best = Some(best.map_or(v.split, |b| b.min(v.split)));
    }
    let best_piece = sessions
        .iter()
        .flat_map(|s| s.pieces.iter().flatten().map(move |&p| (p, s.date)))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let best_split = views.iter().map(|v| (v.split, v.date)).min_by(|a, b| a.0.total_cmp(&b.0));
    let target = Some(target(&views, date));
    views.reverse();
    Rowing { tab, sessions: views, best_piece, best_split, target }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }
    fn s(day: u32, p1: f64, p2: f64) -> RowSession {
        RowSession { date: d(day), pieces: [Some(p1), Some(p2), None], rest_min: Some(3.0), spm: None, hr: None, notes: String::new(), row: Some(2), queued: false }
    }

    #[test]
    fn piece_cells_read_as_seconds() {
        use crate::model::{parse_secs, Cell};
        assert_eq!(parse_secs(&Cell::Num(702.0 / 86400.0)).map(tenth), Some(702.0)); // a Sheets duration
        assert_eq!(parse_secs(&Cell::Num(702.3)), Some(702.3));
        assert_eq!(parse_secs(&Cell::Str("11:42.3".into())), Some(702.3));
        assert_eq!(parse_secs(&Cell::Str("0:11:42".into())), Some(702.0));
        assert_eq!(parse_secs(&Cell::Str("fast".into())), None);
        assert_eq!(parse_secs(&Cell::Empty), None);
    }

    #[test]
    fn times_format_like_the_erg() {
        assert_eq!(fmt_time(702.3), "11:42.3");
        assert_eq!(fmt_time(117.0), "1:57.0");
        assert_eq!(fmt_time(3723.0), "1:02:03.0");
        assert_eq!(split_of(702.0), 117.0);
        // 2:00/500m is about 203W.
        assert_eq!(watts(120.0), 203.0);
    }

    #[test]
    fn even_pieces_step_down_half_a_second() {
        let r = rowing(&[s(20, 702.0, 704.0)], true, d(22));
        let t = r.target.unwrap();
        assert_eq!(t.split, 116.7); // average 117.17 - 0.5
        assert!(t.text.contains("3 min"));
    }

    #[test]
    fn a_big_fade_holds_the_average() {
        // Piece 2 3s/500m slower than piece 1.
        let r = rowing(&[s(20, 690.0, 708.0)], true, d(22));
        assert_eq!(r.target.unwrap().split, 116.5);
    }

    #[test]
    fn a_break_eases_off() {
        let r = rowing(&[s(1, 702.0, 702.0)], true, d(20));
        assert_eq!(r.target.unwrap().split, 118.0);
    }

    #[test]
    fn third_piece_counts_and_pbs_are_marked() {
        let mut third = s(22, 700.0, 700.0);
        third.pieces[2] = Some(706.0);
        let r = rowing(&[s(20, 702.0, 704.0), third], true, d(22));
        assert_eq!(r.sessions[0].metres, 9000.0);
        assert!(r.sessions[0].pb);
        assert!(!r.sessions[1].pb, "the first session has nothing to beat");
        assert_eq!(r.best_piece, Some((700.0, d(22))));
        // The target for the 22nd ignores the 22nd itself.
        assert_eq!(r.target.unwrap().split, 116.7);
    }
}
