//! Helpers the phone UI needs that are not edits: searching the whole workbook, the block of data
//! around a cell (for "sort" or "chart" with one cell selected), the AutoFilter's values, page
//! setup state for the File tab, and decoded pictures for drawing.

use std::collections::BTreeSet;

use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_engine::model::{Orientation, Visibility};

use crate::{Addr, Area, GridDoc};

/// A sheet's page setup, as the File tab shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSetup {
    pub landscape: bool,
    /// "Letter", "A4"...
    pub paper: String,
    /// Fit to (pages wide, pages tall), 0 = automatic; `None` = normal scale.
    pub fit: Option<(u32, u32)>,
    pub gridlines: bool,
    pub print_area: Option<Area>,
}

impl GridDoc {
    /// Every cell on every visible sheet whose shown text contains `what` (any case), in sheet,
    /// row, column order; at most `max` results.
    pub fn find_in_workbook(&self, what: &str, max: usize) -> Vec<(usize, Addr)> {
        let needle = what.trim().to_lowercase();
        let mut out = Vec::new();
        let Some(wb) = self.workbook() else { return out };
        if needle.is_empty() {
            return out;
        }
        for (si, sh) in wb.sheets.iter().enumerate() {
            if sh.visibility != Visibility::Visible {
                continue;
            }
            let mut hits: Vec<Addr> = sh
                .cells
                .iter()
                .filter(|(c, _)| sh.value(*c) != Value::Empty)
                .filter(|(c, _)| gridcraft_engine::display::value_text(wb, sh, *c).to_lowercase().contains(&needle))
                .map(|(c, _)| Addr::new(c.row, c.col))
                .collect();
            hits.sort();
            for a in hits {
                if out.len() >= max {
                    return out;
                }
                out.push((si, a));
            }
        }
        out
    }

    /// The block of filled cells around `at` (Excel's "current region"): grows while a cell next
    /// to the block's edge has content. A lone empty cell gives just that cell.
    pub fn current_region(&self, sheet: usize, at: Addr) -> Area {
        let Some(sh) = self.workbook().and_then(|wb| wb.sheet(sheet)) else { return Area::cell(at) };
        let filled = |r: u32, c: u32| sh.value(CellRef::new(r, c)) != Value::Empty;
        let (mut r0, mut c0, mut r1, mut c1) = (at.row, at.col, at.row, at.col);
        // Bounded: a region never grows past the used range, which is finite.
        let used = sh.used_range();
        for _ in 0..10_000 {
            let mut grew = false;
            if r0 > 0 && (c0.saturating_sub(1)..=(c1 + 1).min(MAX_COLS - 1)).any(|c| filled(r0 - 1, c)) {
                r0 -= 1;
                grew = true;
            }
            if r1 + 1 < MAX_ROWS && (c0.saturating_sub(1)..=(c1 + 1).min(MAX_COLS - 1)).any(|c| filled(r1 + 1, c)) {
                r1 += 1;
                grew = true;
            }
            if c0 > 0 && (r0.saturating_sub(1)..=(r1 + 1).min(MAX_ROWS - 1)).any(|r| filled(r, c0 - 1)) {
                c0 -= 1;
                grew = true;
            }
            if c1 + 1 < MAX_COLS && (r0.saturating_sub(1)..=(r1 + 1).min(MAX_ROWS - 1)).any(|r| filled(r, c1 + 1)) {
                c1 += 1;
                grew = true;
            }
            if let Some(u) = used {
                r1 = r1.min(u.end.row.max(at.row));
                c1 = c1.min(u.end.col.max(at.col));
            }
            if !grew {
                break;
            }
        }
        Area::new(Addr::new(r0, c0), Addr::new(r1, c1))
    }

    /// The AutoFilter's range on a sheet, if a filter is on.
    pub fn filter_range(&self, sheet: usize) -> Option<Area> {
        let sh = self.workbook()?.sheet(sheet)?;
        sh.autofilter.as_ref().map(|a| Area::from_range(a.range))
    }

    /// Distinct shown values of `col` inside `area` (header row excluded), sorted, at most `max`.
    pub fn column_values(&self, sheet: usize, col: u32, area: Area, max: usize) -> Vec<String> {
        let Some(wb) = self.workbook() else { return Vec::new() };
        let Some(sh) = wb.sheet(sheet) else { return Vec::new() };
        let mut set = BTreeSet::new();
        for row in area.first.row.saturating_add(1)..=area.last.row {
            let t = gridcraft_engine::display::value_text(wb, sh, CellRef::new(row, col));
            if !t.is_empty() {
                set.insert(t);
                if set.len() >= max {
                    break;
                }
            }
        }
        set.into_iter().collect()
    }

    /// Turns every filter criterion off (all rows show again); the filter buttons stay.
    pub fn clear_filter(&mut self) -> Result<(), String> {
        self.run("data.clearFilter", serde_json::json!({})).map(|_| ())
    }

    /// The active sheet's page setup.
    pub fn page_setup(&self, sheet: usize) -> Option<PageSetup> {
        let p = &self.workbook()?.sheet(sheet)?.print;
        Some(PageSetup {
            landscape: p.orientation == Orientation::Landscape,
            paper: p.paper.clone(),
            fit: p.fit_to.map(|(w, h)| (u32::from(w), u32::from(h))),
            gridlines: p.gridlines,
            print_area: p.print_area.map(Area::from_range),
        })
    }

    /// A floating picture decoded to RGBA, scaled down so its longer side is at most `max_side`
    /// pixels (phone memory). Returns (width, height, pixels).
    pub fn picture_rgba(&self, sheet: usize, index: usize, max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
        let (bytes, _mime) = self.picture(sheet, index)?;
        let img = image::load_from_memory(&bytes).ok()?;
        let img = if img.width().max(img.height()) > max_side { img.thumbnail(max_side, max_side) } else { img };
        let rgba = img.to_rgba8();
        Some((rgba.width(), rgba.height(), rgba.into_raw()))
    }
}
