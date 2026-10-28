//! Read the workbook from a local .xlsx export (File → Download → .xlsx).
//! Used for offline/demo mode and for the parity tests against the sheet.

use crate::model::*;
use anyhow::{anyhow, Result};
use calamine::{open_workbook_auto, Data as X, DataType, Reader};
use std::path::Path;

/// Parse "A5:L61" into 0-based (row0, col0, row1, col1).
pub fn parse_a1(r: &str) -> (usize, usize, usize, usize) {
    fn cell(s: &str) -> (usize, usize) {
        let col: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let row: usize = s[col.len()..].parse().unwrap_or(1);
        let c = col.chars().fold(0usize, |a, ch| a * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1));
        (row - 1, c - 1)
    }
    let (a, b) = r.split_once(':').unwrap();
    let (r0, c0) = cell(a);
    let (r1, c1) = cell(b);
    (r0, c0, r1, c1)
}

fn conv(v: &X) -> Cell {
    match v {
        X::Empty | X::Error(_) => Cell::Empty,
        X::Int(i) => Cell::Num(*i as f64),
        X::Float(f) => Cell::Num(*f),
        X::Bool(b) => Cell::Bool(*b),
        X::String(s) => if s.trim().is_empty() { Cell::Empty } else { Cell::Str(s.clone()) },
        X::DateTime(d) => Cell::Num(d.as_f64()),
        other => other.as_string().map(Cell::Str).unwrap_or(Cell::Empty),
    }
}

pub fn load(path: &Path) -> Result<Data> {
    let mut wb = open_workbook_auto(path)?;
    let mut grid = |tab: &str, a1: &str, formula_cols: &[usize]| -> Result<Grid> {
        let vals = wb.worksheet_range(tab).map_err(|e| anyhow!("{tab}: {e}"))?;
        let forms = if formula_cols.is_empty() { None } else { wb.worksheet_formula(tab).ok() };
        let (r0, c0, r1, c1) = parse_a1(a1);
        Ok((r0..=r1)
            .map(|r| {
                (c0..=c1)
                    .map(|c| {
                        if formula_cols.contains(&(c - c0)) {
                            if let Some(f) = forms.as_ref().and_then(|f| f.get_value((r as u32, c as u32))) {
                                if !f.is_empty() {
                                    return Cell::Str(f.clone());
                                }
                            }
                        }
                        vals.get_value((r as u32, c as u32)).map(conv).unwrap_or(Cell::Empty)
                    })
                    .collect()
            })
            .collect())
    };
    let library = grid(TAB_LIBRARY, RANGE_LIBRARY, &[])?;
    let plan = grid(TAB_PLAN, RANGE_PLAN, &[])?;
    let plan_settings = grid(TAB_PLAN, RANGE_PLAN_SETTINGS, &[])?;
    let settings = grid(TAB_SETTINGS, RANGE_SETTINGS, &[])?;
    let schedule = grid(TAB_SCHEDULE, RANGE_SCHEDULE, &[])?;
    let log = grid(TAB_LOG, RANGE_LOG, &[])?;
    let rowing = grid(TAB_ROWING, RANGE_ROWING, &[]).ok();
    let mut data = Data::from_grids(&library, &plan, &plan_settings, &settings, &schedule, &log);
    if let Some(r) = rowing {
        data.rowing = rowing_from_grid(&r);
        data.rowing_tab = true;
    }
    Ok(data)
}

/// Cached values of arbitrary log columns (used by the parity test).
pub fn log_column(path: &Path, col: usize) -> Result<Vec<String>> {
    let mut wb = open_workbook_auto(path)?;
    let vals = wb.worksheet_range(TAB_LOG).map_err(|e| anyhow!("{e}"))?;
    Ok((1..LOG_LAST_FORMULA_ROW as u32)
        .map(|r| vals.get_value((r, col as u32)).map(|v| v.to_string()).unwrap_or_default())
        .collect())
}
