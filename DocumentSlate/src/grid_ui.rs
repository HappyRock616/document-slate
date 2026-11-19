//! Grid Slate: the spreadsheet screen (viewer + editor) on top of the `slate-grid` back-end.
//!
//! - Viewer: the sheet fills the screen with column letters / row numbers, gridlines, frozen
//!   panes, charts and pictures; pinch zoom, two-axis scroll with a fling, a slim sheet-tab strip
//!   at the bottom. A tap shows/hides the shell's bars; the counter pill shows "sheet n / N".
//! - Edit mode: a formula bar on top (cell address · contents · ✕/✓), tap a cell to select it,
//!   tap it again (or double-tap) to type, drag the round handles to select a range, long-press
//!   for Cut/Copy/Paste/Insert/Delete. Enter commits and moves down; while a formula is being
//!   typed, tapping a cell inserts its address. The bottom bar (src/grid_ui/tools.rs) has the
//!   Cell · Formula · Data · Insert · File tabs (approved list: docs/GridSlate-features.md).
//!
//! The sheet is drawn from the back-end's layout model (src/grid_ui/paint.rs), visible blocks
//! only, cached per document revision.

mod icons;
mod paint;
mod tools;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use eframe::egui::{self, Color32, FontFamily, Pos2, Rect, Sense, Vec2, pos2, vec2};
use slate_grid::{Addr, Area, FunctionInfo, GridDoc, PdfOptions, SaveFormat};

use crate::chrome;
use crate::doc_screen::{DocScreen, ScreenCtx};
use crate::edit_bar::FileAction;
use crate::theme::Tool;
use paint::{COL_HEAD, ChartCache, Geo, PicCache, SheetColors, VpCache};

/// Most search hits listed (the count shows "500+").
const MAX_HITS: usize = 500;
/// Formula bar and sheet-tab strip heights.
const FORMULA_H: f32 = 48.0;
const TABS_H: f32 = 40.0;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
/// Largest row / column index (0-based) of a sheet.
const LAST_ROW: u32 = slate_grid::engine::core::MAX_ROWS - 1;
const LAST_COL: u32 = slate_grid::engine::core::MAX_COLS - 1;

/// Edit-mode tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Cell,
    Formula,
    Data,
    Insert,
    File,
}

/// An input row open under the tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Row {
    #[default]
    None,
    FnSearch,
    Replace,
    Link,
    Note,
    Rename,
    ConfirmMerge,
}

/// Text typed into the input rows (kept between frames).
#[derive(Default)]
struct Fields {
    fn_search: String,
    find: String,
    with: String,
    url: String,
    link_text: String,
    note: String,
    rename: String,
}

pub struct GridScreen {
    doc: GridDoc,
    name: String,
    editing: bool,
    // ---- view
    zoom: f32,
    /// Scroll offset of the unfrozen part, sheet pixels.
    scroll: Vec2,
    velocity: Vec2,
    /// Scroll + selection remembered per sheet.
    per_sheet: HashMap<usize, (Vec2, Area)>,
    shown_sheet: usize,
    tapped: bool,
    scrolled: bool,
    hide_gridlines: bool,
    dark_sheet: bool,
    cache: VpCache,
    charts: ChartCache,
    pics: PicCache,
    last_geo: Option<Geo>,
    /// Scrollable size of the sheet, per (revision, sheet).
    content: Option<((u64, usize), (f32, f32))>,
    /// Fit the used range to the screen width on the next frame.
    fit_width: bool,
    // ---- selection and the formula bar
    sel: Area,
    input: String,
    /// The cell (and document revision) `input` was loaded from, when not editing.
    input_for: Option<(usize, Addr, u64)>,
    /// The cell being typed into.
    editing_cell: Option<Addr>,
    focus_input: bool,
    cursor_to_end: bool,
    /// A grid tap this frame keeps the cell editor open (formula reference insert).
    keep_editing: bool,
    /// Scroll so this cell is visible.
    want_visible: Option<Addr>,
    /// Long-press menu: where, and when it opened.
    ctx_menu: Option<(Pos2, f64)>,
    /// AutoFilter pop-up: where, which column, when it opened.
    filter_menu: Option<(Pos2, u32, f64)>,
    // ---- edit bar
    tab: Tab,
    row: Row,
    fields: Fields,
    fn_cache: HashMap<String, Vec<FunctionInfo>>,
    sort_header: bool,
    merge_pending: Option<Area>,
    // ---- search
    search_open: bool,
    focus_search: bool,
    query: String,
    searched: String,
    hits: Vec<(usize, Addr)>,
    current: usize,
    jump: bool,
    // ---- messages
    toast: Option<(String, f64)>,
}

impl GridScreen {
    pub fn open(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let doc = GridDoc::open_bytes(name, bytes)?;
        let mut s = Self::with_doc(doc, name);
        if let Some(w) = s.doc.open_warnings().first() {
            s.toast = Some((w.clone(), -1.0));
        }
        Ok(s)
    }

    pub fn new_blank() -> Self {
        Self::with_doc(GridDoc::new_blank("Untitled.xlsx"), "Untitled.xlsx")
    }

    fn with_doc(mut doc: GridDoc, name: &str) -> Self {
        doc.set_author("Author"); // until the app passes the user's name (see set_author)
        let sheet = doc.active_sheet();
        Self {
            doc,
            name: name.to_owned(),
            editing: false,
            zoom: 1.0,
            scroll: Vec2::ZERO,
            velocity: Vec2::ZERO,
            per_sheet: HashMap::new(),
            shown_sheet: sheet,
            tapped: false,
            scrolled: false,
            hide_gridlines: false,
            dark_sheet: false,
            cache: VpCache::default(),
            charts: ChartCache::default(),
            pics: PicCache::default(),
            last_geo: None,
            content: None,
            fit_width: false,
            sel: Area::cell(Addr::new(0, 0)),
            input: String::new(),
            input_for: None,
            editing_cell: None,
            focus_input: false,
            cursor_to_end: false,
            keep_editing: false,
            want_visible: None,
            ctx_menu: None,
            filter_menu: None,
            tab: Tab::Cell,
            row: Row::None,
            fields: Fields::default(),
            fn_cache: HashMap::new(),
            sort_header: true,
            merge_pending: None,
            search_open: false,
            focus_search: false,
            query: String::new(),
            searched: String::new(),
            hits: Vec::new(),
            current: 0,
            jump: false,
            toast: None,
        }
    }

    fn sheet(&self) -> usize {
        self.doc.active_sheet()
    }

    /// Show a short message (toast) for a few seconds.
    fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), -1.0));
    }

    /// Run an edit; a failure becomes a message.
    fn act<T>(&mut self, r: Result<T, String>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.say(friendly(&e));
                None
            }
        }
    }

    /// The selection as a range for formatting commands. Whole rows/columns are cut down to the
    /// used part of the sheet (a million empty cells help nobody).
    fn target(&self) -> Area {
        let s = self.sel;
        if s.rows() <= 5000 && s.cols() <= 500 {
            return s;
        }
        let used = self.doc.used_range(self.sheet()).unwrap_or(Area::cell(s.first));
        let first = Addr::new(s.first.row.max(used.first.row), s.first.col.max(used.first.col));
        let last = Addr::new(s.last.row.min(used.last.row), s.last.col.min(used.last.col));
        if first.row > last.row || first.col > last.col { Area::cell(s.first) } else { Area::new(first, last) }
    }

    /// The data a sort / filter / chart works on: the selection, or with one cell selected, the
    /// block of filled cells around it.
    fn data_area(&self) -> Area {
        if self.sel.first == self.sel.last { self.doc.current_region(self.sheet(), self.sel.first) } else { self.target() }
    }

    fn select(&mut self, a: Area) {
        self.sel = a;
        self.input_for = None;
    }

    // ------------------------------------------------------------------ cell editing

    /// Start typing into the selected cell (opens the keyboard).
    fn begin_edit(&mut self, replace_with: Option<String>) {
        let at = self.sel.first;
        self.editing_cell = Some(at);
        self.input = match replace_with {
            Some(t) => t,
            None => self.doc.cell(self.sheet(), at).map(|c| c.input).unwrap_or_default(),
        };
        self.focus_input = true;
        self.cursor_to_end = true;
    }

    /// Write the formula bar into the cell. False (and still editing) when the engine refused.
    fn commit(&mut self) -> bool {
        let Some(at) = self.editing_cell else { return true };
        let old = self.doc.cell(self.sheet(), at).map(|c| c.input).unwrap_or_default();
        if self.input != old {
            let text = self.input.clone();
            if let Err(e) = self.doc.set_cell(at, &text) {
                self.say(friendly(&e));
                self.focus_input = true;
                return false;
            }
        }
        self.editing_cell = None;
        self.input_for = None;
        true
    }

    fn cancel_edit(&mut self) {
        self.editing_cell = None;
        self.input_for = None;
    }

    /// Enter: commit, move down one cell and keep typing there.
    fn commit_and_next(&mut self, ctx: &egui::Context) {
        let Some(at) = self.editing_cell else { return };
        if !self.commit() {
            return;
        }
        let next = Addr::new((at.row + 1).min(LAST_ROW), at.col);
        self.select(Area::cell(next));
        self.want_visible = Some(next);
        self.begin_edit(None);
        let _ = ctx;
    }

    /// A formula is being typed and ends where a cell address can go: a tapped cell is
    /// inserted as a reference instead of moving the selection.
    fn wants_reference(&self) -> bool {
        self.editing_cell.is_some() && self.input.starts_with('=') && self.input.trim_end().ends_with(['=', '(', ',', '+', '-', '*', '/', '^', '&', '<', '>', ':', ';'])
    }

    /// Insert a function: into the formula being typed, or start a new one in the cell.
    fn insert_function(&mut self, name: &str) {
        if self.editing_cell.is_some() {
            if !self.input.starts_with('=') {
                self.input = format!("={}", self.input);
            }
            self.input.push_str(&format!("{name}("));
        } else {
            self.begin_edit(Some(format!("={name}(")));
        }
        self.focus_input = true;
        self.cursor_to_end = true;
    }

    // ------------------------------------------------------------------ sheets

    fn switch_sheet(&mut self, index: usize) {
        if index == self.sheet() && index == self.shown_sheet {
            return;
        }
        self.commit();
        self.per_sheet.insert(self.shown_sheet, (self.scroll, self.sel));
        if index != self.sheet() {
            let r = self.doc.set_active_sheet(index);
            self.act(r);
        }
        self.arrive_on_sheet();
    }

    /// The active sheet changed (tab tap, sheet added or deleted): restore its view.
    fn arrive_on_sheet(&mut self) {
        let s = self.sheet();
        let (scroll, sel) = self.per_sheet.get(&s).copied().unwrap_or((Vec2::ZERO, Area::cell(Addr::new(0, 0))));
        self.scroll = scroll;
        self.sel = sel;
        self.velocity = Vec2::ZERO;
        self.input_for = None;
        self.shown_sheet = s;
    }

    /// Indices of the sheets that get a tab (hidden sheets don't).
    fn visible_sheets(&self) -> Vec<(usize, String, Option<[u8; 3]>)> {
        // Straight from the model (GridDoc::sheets also measures every used range: too slow
        // for every frame).
        let Some(wb) = self.doc.workbook() else { return Vec::new() };
        wb.sheets
            .iter()
            .enumerate()
            .filter(|(_, s)| s.visibility == slate_grid::engine::model::Visibility::Visible)
            .map(|(i, s)| (i, s.name.clone(), s.tab_color.and_then(|c| c.resolve(&wb.theme))))
            .collect()
    }

    // ------------------------------------------------------------------ search

    fn run_search(&mut self) {
        self.searched = self.query.clone();
        self.hits = self.doc.find_in_workbook(&self.query, MAX_HITS);
        self.current = 0;
        self.jump = !self.hits.is_empty();
    }

    fn step_match(&mut self, delta: isize) {
        let n = self.hits.len();
        if n == 0 {
            return;
        }
        self.current = (self.current as isize + delta).rem_euclid(n as isize) as usize;
        self.jump = true;
    }

    fn go_to_hit(&mut self) {
        let Some(&(s, at)) = self.hits.get(self.current) else { return };
        if s != self.sheet() {
            self.switch_sheet(s);
        }
        self.select(Area::cell(at));
        self.want_visible = Some(at);
    }

    // ------------------------------------------------------------------ geometry helpers

    /// Frozen rows/columns of the active sheet and their size in sheet pixels.
    fn frozen(&self) -> (u32, u32, f32, f32) {
        let s = self.sheet();
        let Some(sh) = self.doc.workbook().and_then(|wb| wb.sheet(s)) else { return (0, 0, 0.0, 0.0) };
        let (fr, fc) = sh.freeze.unwrap_or((0, 0));
        (fr, fc, sh.col_left(fc) as f32, sh.row_top(fr) as f32)
    }

    /// Scroll so `at` is inside the scrolling part of the view.
    fn reveal(&mut self, at: Addr, geo: &Geo) {
        let Some((x, y, w, h)) = self.doc.cell_rect(self.sheet(), at) else { return };
        let (vw, vh) = (geo.scroll_w(), geo.scroll_h());
        if x >= geo.fw {
            let left = x - geo.fw;
            if left < self.scroll.x {
                self.scroll.x = left;
            } else if left + w > self.scroll.x + vw {
                self.scroll.x = (left + w - vw).min(left);
            }
        }
        if y >= geo.fh {
            let top = y - geo.fh;
            if top < self.scroll.y {
                self.scroll.y = top;
            } else if top + h > self.scroll.y + vh {
                self.scroll.y = (top + h - vh).min(top);
            }
        }
    }

    // ------------------------------------------------------------------ drawing

    fn formula_bar(&mut self, ui: &mut egui::Ui, rect: Rect, accent: Color32) {
        let input_id = egui::Id::unique("grid-formula-input");
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.15)));
        let editing = self.editing_cell.is_some();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 5.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let addr = if self.sel.first == self.sel.last { self.sel.first.a1() } else { self.sel.a1() };
            let (r, _) = ui.allocate_exact_size(vec2(64.0, 36.0), Sense::hover());
            ui.painter().rect_filled(r, 8.0, ui.visuals().faint_bg_color);
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, addr, egui::FontId::proportional(14.0), ui.visuals().text_color());
            let (fr, _) = ui.allocate_exact_size(vec2(26.0, 36.0), Sense::hover());
            icons::icon(ui.painter(), fr, "g.fx", ui.visuals().text_color().gamma_multiply(0.7), accent);
            let buttons = if editing { 2.0 * 44.0 + 8.0 } else { 0.0 };
            let edit = egui::TextEdit::singleline(&mut self.input)
                .id(input_id)
                .hint_text("Tap a cell, then type")
                .font(egui::FontId::proportional(16.0))
                .margin(vec2(8.0, 8.0))
                .desired_width((ui.available_width() - buttons).max(60.0));
            let resp = ui.add(edit);
            if self.focus_input {
                resp.request_focus();
                self.focus_input = false;
            }
            if self.cursor_to_end {
                self.cursor_to_end = false;
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), input_id) {
                    let end = egui::text::CCursor::new(self.input.chars().count());
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    st.store(ui.ctx(), input_id);
                }
            }
            if resp.gained_focus() && self.editing_cell.is_none() {
                self.editing_cell = Some(self.sel.first);
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.commit_and_next(ui.ctx());
                resp.request_focus();
                self.cursor_to_end = true;
            }
            if self.editing_cell.is_some() {
                let fg = ui.visuals().text_color();
                let (xr, xresp) = ui.allocate_exact_size(vec2(44.0, 40.0), Sense::click());
                icons::icon(ui.painter(), xr, "close", fg, accent);
                let (cr, cresp) = ui.allocate_exact_size(vec2(44.0, 40.0), Sense::click());
                ui.painter().rect_filled(cr.shrink(3.0), 10.0, accent);
                icons::icon(ui.painter(), cr, "check", Color32::WHITE, Color32::WHITE);
                if xresp.clicked() {
                    self.cancel_edit();
                    ui.memory_mut(|m| m.surrender_focus(input_id));
                }
                if cresp.clicked() && self.commit() {
                    ui.memory_mut(|m| m.surrender_focus(input_id));
                }
            }
        });
    }

    fn grid(&mut self, ui: &mut egui::Ui, rect: Rect, cx: &ScreenCtx) {
        let ctx = ui.ctx().clone();
        let sheet = self.sheet();
        let colors = SheetColors::new(self.dark_sheet, cx.dark);
        let (fr, fc, fw, fh) = self.frozen();
        let resp = ui.interact(rect, ui.scope_id().with("grid-surface"), Sense::click_and_drag());
        let old_zoom = self.zoom;
        let old_scroll = self.scroll;
        // ---- zoom (pinch / ctrl+wheel) about the fingers' centre: remember the sheet point
        // under them, change the zoom, then scroll so that point is under them again.
        let zoom_delta = ctx.input(|i| i.zoom_delta());
        let mut anchor = None;
        if zoom_delta != 1.0 && (resp.hovered() || resp.dragged() || ctx.multi_touch().is_some()) {
            let center = ctx.multi_touch().map(|m| m.center_pos).or_else(|| ctx.pointer_hover_pos()).unwrap_or(rect.center());
            if let Some(g) = self.last_geo {
                anchor = Some((center, g.sheet_point(center)));
            }
            self.zoom = (self.zoom * zoom_delta).clamp(MIN_ZOOM, MAX_ZOOM);
            self.velocity = Vec2::ZERO;
        }
        let est_last_row = self.last_geo.map(|g| g.sheet_point(g.cells.left_bottom()).1).unwrap_or(1000.0);
        let est_row = self.doc.hit(sheet, 0.0, est_last_row).map(|a| a.row).unwrap_or(100);
        let row_head = paint::row_head_width(est_row.max(99), self.zoom);
        let cells = Rect::from_min_max(rect.min + vec2(row_head, COL_HEAD), rect.max);
        if cells.width() < 10.0 || cells.height() < 10.0 {
            return;
        }
        if let (Some(g), Some(at)) = (self.last_geo, self.editing_cell)
            && (g.cells.height() - cells.height()).abs() > 24.0
        {
            self.want_visible = Some(at);
        }
        if let Some((center, (px, py))) = anchor {
            if px >= fw {
                self.scroll.x = px - (center.x - cells.left()) / self.zoom;
            }
            if py >= fh {
                self.scroll.y = py - (center.y - cells.top()) / self.zoom;
            }
        }
        let mut geo = Geo { cells, z: self.zoom, fw, fh, sx: self.scroll.x, sy: self.scroll.y };
        if self.fit_width {
            self.fit_width = false;
            let used = self.doc.used_range(sheet).map(|u| u.last.col).unwrap_or(5);
            let w = self.doc.cell_rect(sheet, Addr::new(0, used)).map(|(x, _, w, _)| x + w).unwrap_or(400.0);
            self.zoom = ((cells.width() - 4.0) / w.max(50.0)).clamp(MIN_ZOOM, MAX_ZOOM);
            self.scroll.x = 0.0;
        }
        // ---- pan: one finger drag (or two-finger move), mouse wheel; fling after release
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let mut pan = Vec2::ZERO;
        if let Some(m) = ctx.multi_touch() {
            pan += m.translation_delta;
        } else if resp.dragged() {
            pan += resp.drag_delta();
        }
        if resp.hovered() && !ctx.input(|i| i.modifiers.command) {
            pan += ctx.input(|i| i.smooth_scroll_delta);
        }
        if resp.drag_stopped() && ctx.multi_touch().is_none() {
            self.velocity = ctx.input(|i| i.pointer.velocity()) / self.zoom;
        }
        if pan != Vec2::ZERO {
            self.scroll -= pan / self.zoom;
            if resp.dragged() {
                self.velocity = Vec2::ZERO;
            }
        } else if self.velocity.length() > 5.0 && !resp.is_pointer_button_down_on() {
            self.scroll -= self.velocity * dt;
            self.velocity *= (-4.0 * dt).exp();
            ctx.request_repaint();
        } else {
            self.velocity = Vec2::ZERO;
        }
        // ---- keep the scroll inside the sheet's content (+ the selection while editing)
        geo.z = self.zoom;
        let key = (self.doc.revision(), sheet);
        let (cw, ch) = match self.content {
            Some((k, size)) if k == key => size,
            _ => {
                let size = self.doc.content_size(sheet).unwrap_or((2000.0, 2000.0));
                self.content = Some((key, size));
                size
            }
        };
        let (mut max_w, mut max_h) = (cw, ch);
        if let Some((x, y, w, h)) = self.doc.cell_rect(sheet, self.sel.last) {
            max_w = max_w.max(x + w + 200.0);
            max_h = max_h.max(y + h + 200.0);
        }
        let max_x = (max_w - fw - geo.scroll_w()).max(0.0);
        let max_y = (max_h - fh - geo.scroll_h()).max(0.0);
        if let Some(at) = self.want_visible.take() {
            geo.sx = self.scroll.x;
            geo.sy = self.scroll.y;
            self.reveal(at, &geo);
        } else {
            self.scroll.x = self.scroll.x.clamp(0.0, max_x);
            self.scroll.y = self.scroll.y.clamp(0.0, max_y);
        }
        self.scroll = self.scroll.max(Vec2::ZERO);
        geo.sx = self.scroll.x;
        geo.sy = self.scroll.y;
        self.scrolled = (self.scroll - old_scroll).length() > 0.3 || (self.zoom - old_zoom).abs() > 1e-4;
        self.last_geo = Some(geo);

        // ---- paint
        let painter = ui.painter_at(rect);
        let gridlines = !self.hide_gridlines;
        for (clip, fx, fy) in geo.quadrants() {
            let (x0, x1) = if fx { (0.0, fw) } else { (fw + geo.sx, fw + geo.sx + geo.scroll_w()) };
            let (y0, y1) = if fy { (0.0, fh) } else { (fh + geo.sy, fh + geo.sy + geo.scroll_h()) };
            let (Some(a), Some(b)) = (self.doc.hit(sheet, x0, y0), self.doc.hit(sheet, (x1 - 0.01).max(x0), (y1 - 0.01).max(y0))) else { continue };
            // Round the block out so small scrolls reuse the cached viewport.
            let (mut first, mut last) = (a, b);
            if fx {
                last.col = last.col.min(fc.saturating_sub(1));
            } else {
                first.col = (first.col / 8 * 8).max(fc);
                last.col = (last.col / 8 * 8 + 15).min(LAST_COL);
            }
            if fy {
                last.row = last.row.min(fr.saturating_sub(1));
            } else {
                first.row = (first.row / 32 * 32).max(fr);
                last.row = (last.row / 32 * 32 + 63).min(LAST_ROW);
            }
            let Some(entry) = self.cache.get(&self.doc, sheet, Area::new(first, last)) else { continue };
            let pq = painter.with_clip_rect(clip);
            paint::paint_cells(&pq, &geo, &entry, fx, fy, &colors, gridlines && entry.vp.gridlines);
            // Search matches on this sheet.
            if self.search_open {
                for (i, (s, at)) in self.hits.iter().enumerate() {
                    if *s != sheet {
                        continue;
                    }
                    if let Some((x, y, w, h)) = self.doc.cell_rect(sheet, *at) {
                        let r = geo.rect_q(x, y, w, h, fx, fy);
                        if r.intersects(clip) {
                            let col = if i == self.current { Color32::from_rgba_unmultiplied(255, 140, 0, 110) } else { Color32::from_rgba_unmultiplied(255, 220, 0, 90) };
                            pq.rect_filled(r, 0.0, col);
                        }
                    }
                }
            }
            paint::paint_objects(&pq, &geo, &entry.vp, fx, fy, &self.doc, &mut self.charts, &mut self.pics, &ctx);
        }
        let accent = cx.accent;
        // AutoFilter buttons on the filter's title row.
        let filter = self.doc.filter_range(sheet);
        if let Some(f) = filter {
            let pc = painter.with_clip_rect(cells);
            for col in f.first.col..=f.last.col.min(f.first.col + 200) {
                if let Some((x, y, w, h)) = self.doc.cell_rect(sheet, Addr::new(f.first.row, col)) {
                    let r = geo.rect_auto(x, y, w, h);
                    if r.intersects(cells) && r.width() > 8.0 {
                        let b = filter_button(r, geo.z);
                        pc.rect_filled(b, 3.0, Color32::from_gray(245));
                        pc.rect_stroke(b, 3.0, egui::Stroke::new(1.0, Color32::from_gray(150)), egui::StrokeKind::Inside);
                        let c = b.center();
                        let t = b.width() * 0.22;
                        pc.add(egui::Shape::convex_polygon(vec![c + vec2(-t, -t * 0.5), c + vec2(t, -t * 0.5), c + vec2(0.0, t * 0.7)], Color32::from_gray(70), egui::Stroke::NONE));
                    }
                }
            }
        }
        // Print area (File › Print area), in edit mode: a dashed frame like Excel's page-break view.
        if self.editing {
            let area = self.doc.page_setup(sheet).and_then(|ps| ps.print_area);
            if let Some(r) = area.and_then(|a| self.area_rect(&geo, a)) {
                let pc = painter.with_clip_rect(cells);
                let st = egui::Stroke::new(2.0, accent.gamma_multiply(0.9));
                let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                pc.extend(egui::Shape::dashed_line(&c, st, 8.0, 5.0));
            }
        }
        // Selection: accent frame (+ a wash for ranges) and, in edit mode, two drag handles.
        let sel_rect = self.area_rect(&geo, self.sel);
        let show_sel = self.editing || (self.search_open && !self.hits.is_empty());
        if show_sel && let Some(sr) = sel_rect {
            let pc = painter.with_clip_rect(cells);
            if self.sel.first != self.sel.last {
                pc.rect_filled(sr, 0.0, accent.gamma_multiply(0.15));
            }
            pc.rect_stroke(sr, 0.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Middle);
        }
        paint::paint_headers(&painter, &geo, row_head, &self.doc, sheet, show_sel.then_some(self.sel), &colors, accent);

        // ---- taps
        let now = ctx.input(|i| i.time);
        let long = resp.long_touched() || resp.secondary_clicked();
        if !self.editing {
            if resp.clicked() && !long {
                // A tap on a linked cell follows the link; any other tap shows/hides the bars.
                let linked = resp.interact_pointer_pos().filter(|p| cells.contains(*p)).and_then(|p| {
                    let (sx, sy) = geo.sheet_point(p);
                    self.doc.hit(sheet, sx, sy)
                });
                if !linked.is_some_and(|at| self.follow_link(at)) {
                    let code = resp.interact_pointer_pos().filter(|p| cells.contains(*p)).and_then(|p| {
                        let (sx, sy) = geo.sheet_point(p);
                        self.pics.code_at(sheet, sx, sy)
                    });
                    match code {
                        Some(f) => crate::barcode_scan::show(&f),
                        None => self.tapped = true,
                    }
                }
            }
        } else if let Some(p) = resp.interact_pointer_pos().filter(|p| cells.contains(*p) && self.ctx_menu.is_none() && self.filter_menu.is_none()) {
            let (sx, sy) = geo.sheet_point(p);
            let hit = self.doc.hit(sheet, sx, sy);
            let on_filter_button = |at: Addr| {
                filter.is_some_and(|f| at.row == f.first.row && f.first.col <= at.col && at.col <= f.last.col)
                    && self.doc.cell_rect(sheet, at).is_some_and(|(x, y, w, h)| filter_button(geo.rect_auto(x, y, w, h), geo.z).expand(6.0).contains(p))
            };
            if let Some(at) = hit {
                if resp.clicked() && on_filter_button(at) {
                    self.commit();
                    self.select(Area::cell(at));
                    self.filter_menu = Some((p, at.col, now));
                } else if long {
                    if !self.sel_contains(at) {
                        self.commit();
                        self.select(Area::cell(at));
                    }
                    self.ctx_menu = Some((p, now));
                } else if resp.double_clicked() {
                    if self.editing_cell.is_none() {
                        self.select(Area::cell(at));
                        self.begin_edit(None);
                    }
                } else if resp.clicked() {
                    self.tap_cell(at);
                }
            }
        }
        // Header taps select whole columns / rows; the corner selects everything used.
        if self.editing {
            let col_bar = Rect::from_min_max(pos2(cells.left(), rect.top()), pos2(cells.right(), cells.top()));
            let row_bar = Rect::from_min_max(pos2(rect.left(), cells.top()), pos2(cells.left(), cells.bottom()));
            let corner = Rect::from_min_max(rect.min, pos2(cells.left(), cells.top()));
            let cr = ui.interact(col_bar, ui.scope_id().with("grid-col-head"), Sense::click());
            if cr.clicked()
                && let Some(p) = cr.interact_pointer_pos()
                && let Some(at) = self.doc.hit(sheet, geo.sheet_point(pos2(p.x, cells.top() + 1.0)).0, 0.0)
            {
                self.commit();
                self.select(Area::new(Addr::new(0, at.col), Addr::new(LAST_ROW, at.col)));
            }
            let rr = ui.interact(row_bar, ui.scope_id().with("grid-row-head"), Sense::click());
            if rr.clicked()
                && let Some(p) = rr.interact_pointer_pos()
                && let Some(at) = self.doc.hit(sheet, 0.0, geo.sheet_point(pos2(cells.left() + 1.0, p.y)).1)
            {
                self.commit();
                self.select(Area::new(Addr::new(at.row, 0), Addr::new(at.row, LAST_COL)));
            }
            if ui.interact(corner, ui.scope_id().with("grid-corner"), Sense::click()).clicked() {
                self.commit();
                let used = self.doc.used_range(sheet).unwrap_or(Area::cell(Addr::new(0, 0)));
                self.select(Area::new(Addr::new(0, 0), used.last));
            }
            if let Some(sr) = sel_rect {
                self.handles(ui, &geo, sr, accent);
            }
        }
        if self.editing {
            self.keys(&ctx);
        }
    }

    fn sel_contains(&self, at: Addr) -> bool {
        let s = self.sel;
        s.first.row <= at.row && at.row <= s.last.row && s.first.col <= at.col && at.col <= s.last.col
    }

    /// A tap on a cell in edit mode.
    fn tap_cell(&mut self, at: Addr) {
        if self.wants_reference() {
            // Typing a formula: the tapped cell's address goes into it.
            self.input.push_str(&at.a1());
            self.keep_editing = true;
            self.focus_input = true;
            self.cursor_to_end = true;
            return;
        }
        if self.editing_cell.is_some() {
            if !self.commit() {
                self.keep_editing = true;
                return;
            }
            self.select(Area::cell(at));
            return;
        }
        if self.sel.first == at && self.sel.last == at {
            // Second tap on the selected cell: type into it.
            self.begin_edit(None);
        } else {
            self.select(Area::cell(at));
        }
    }

    /// Screen rectangle of a range (merged cells included), or None when it's far off-screen.
    fn area_rect(&self, geo: &Geo, a: Area) -> Option<Rect> {
        let s = self.sheet();
        let last = Addr::new(a.last.row.min(LAST_ROW), a.last.col.min(LAST_COL));
        let (x0, y0, _, _) = self.doc.cell_rect(s, a.first)?;
        let (x1, y1, w1, h1) = self.doc.cell_rect(s, last)?;
        let r = geo.rect_auto(x0, y0, (x1 + w1 - x0).max(1.0), (y1 + h1 - y0).max(1.0));
        if r.width() < 0.5 || r.height() < 0.5 {
            return None;
        }
        let big = geo.cells.expand(4000.0);
        Some(Rect::from_min_max(r.min.max(big.min), r.max.min(big.max)))
    }

    /// The two round handles at the selection's corners; dragging one moves that corner.
    fn handles(&mut self, ui: &mut egui::Ui, geo: &Geo, sr: Rect, accent: Color32) {
        const R: f32 = 8.0;
        let sheet = self.sheet();
        let painter = ui.painter_at(geo.cells.expand(R + 2.0));
        for (which, c) in [(0usize, sr.left_top()), (1usize, sr.right_bottom())] {
            if !geo.cells.expand(R).contains(c) {
                continue;
            }
            let hit = Rect::from_center_size(c, Vec2::splat(R * 5.0));
            let resp = ui.interact(hit, ui.scope_id().with(("grid-handle", which)), Sense::drag());
            painter.circle_filled(c, R, Color32::WHITE);
            painter.circle_stroke(c, R, egui::Stroke::new(2.5, accent));
            if resp.dragged()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let p = p.clamp(geo.cells.min, geo.cells.max);
                let (x, y) = geo.sheet_point(p);
                if let Some(at) = self.doc.hit(sheet, x, y) {
                    if self.editing_cell.is_some() {
                        self.commit();
                    }
                    let (first, last) = (self.sel.first, self.sel.last);
                    self.sel = if which == 0 { Area::new(at, last) } else { Area::new(first, at) };
                    self.input_for = None;
                }
                // Dragging near the edge scrolls.
                let edge = 24.0;
                let mut push = Vec2::ZERO;
                if p.x > geo.cells.right() - edge {
                    push.x = 8.0;
                } else if p.x < geo.cells.left() + edge && self.scroll.x > 0.0 {
                    push.x = -8.0;
                }
                if p.y > geo.cells.bottom() - edge {
                    push.y = 8.0;
                } else if p.y < geo.cells.top() + edge && self.scroll.y > 0.0 {
                    push.y = -8.0;
                }
                if push != Vec2::ZERO {
                    self.scroll += push / geo.z;
                    ui.ctx().request_repaint();
                }
            }
        }
    }

    /// Hardware / desktop keyboard while no text box has focus: arrows move, Delete clears,
    /// Enter/F2 edits, typing replaces the cell.
    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) || self.editing_cell.is_some() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Text(t) if !t.is_empty() => {
                    self.begin_edit(Some(t));
                    return;
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let at = self.sel.first;
                    let mv = |r: i64, c: i64| Addr::new((at.row as i64 + r).clamp(0, LAST_ROW as i64) as u32, (at.col as i64 + c).clamp(0, LAST_COL as i64) as u32);
                    let to = match key {
                        egui::Key::ArrowDown => Some(mv(1, 0)),
                        egui::Key::ArrowUp => Some(mv(-1, 0)),
                        egui::Key::ArrowLeft => Some(mv(0, -1)),
                        egui::Key::ArrowRight | egui::Key::Tab => Some(mv(0, 1)),
                        egui::Key::Enter | egui::Key::F2 => {
                            self.begin_edit(None);
                            return;
                        }
                        egui::Key::Delete | egui::Key::Backspace => {
                            let t = self.target();
                            let r = self.doc.clear_contents(t);
                            self.act(r);
                            self.input_for = None;
                            None
                        }
                        egui::Key::Z if modifiers.command => {
                            self.undo();
                            None
                        }
                        egui::Key::Y if modifiers.command => {
                            self.redo();
                            None
                        }
                        _ => None,
                    };
                    if let Some(to) = to {
                        self.select(Area::cell(to));
                        self.want_visible = Some(to);
                    }
                }
                _ => {}
            }
        }
    }

    /// The slim sheet-tab strip at the bottom (tap = switch; edit mode: + adds a sheet,
    /// long-press = rename).
    fn sheet_tabs(&mut self, ui: &mut egui::Ui, rect: Rect, accent: Color32) {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.15)));
        let tabs = self.visible_sheets();
        let active = self.sheet();
        let mut go = None;
        let mut rename = None;
        let mut add = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 3.0))), |ui| {
            egui::ScrollArea::horizontal().id_salt("grid-sheet-tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if self.editing {
                        let (r, resp) = ui.allocate_exact_size(vec2(36.0, 32.0), Sense::click());
                        icons::icon(ui.painter(), r, "plus", ui.visuals().text_color(), accent);
                        if resp.clicked() {
                            add = true;
                        }
                    }
                    for (index, name, color) in &tabs {
                        let fg = ui.visuals().text_color();
                        let galley = ui.painter().layout_no_wrap(name.clone(), egui::FontId::proportional(14.0), fg);
                        let w = (galley.size().x + 28.0).clamp(56.0, 220.0);
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
                        let on = *index == active;
                        let p = ui.painter();
                        if on {
                            p.rect_filled(r, 8.0, accent.gamma_multiply(0.35));
                            p.rect_filled(egui::Rect::from_center_size(pos2(r.center().x, r.bottom() - 1.5), vec2(r.width() - 20.0, 3.0)), 1.5, accent);
                        } else if resp.is_pointer_button_down_on() {
                            p.rect_filled(r, 8.0, fg.gamma_multiply(0.1));
                        }
                        if let Some(c) = color {
                            p.rect_filled(Rect::from_min_size(pos2(r.left() + 8.0, r.bottom() - 4.0), vec2(r.width() - 16.0, 3.0)), 1.5, Color32::from_rgb(c[0], c[1], c[2]));
                        }
                        let col = if on { fg } else { fg.gamma_multiply(0.75) };
                        p.with_clip_rect(r.shrink(4.0)).text(r.center(), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(14.0), col);
                        if resp.clicked() {
                            go = Some(*index);
                        }
                        if self.editing && (resp.long_touched() || resp.secondary_clicked()) {
                            go = Some(*index);
                            rename = Some(name.clone());
                        }
                    }
                });
            });
        });
        if let Some(i) = go {
            self.switch_sheet(i);
        }
        if let Some(n) = rename {
            self.fields.rename = n;
            self.tab = Tab::Insert;
            self.row = Row::Rename;
        }
        if add {
            self.commit();
            self.per_sheet.insert(self.shown_sheet, (self.scroll, self.sel));
            let r = self.doc.add_sheet(None);
            self.act(r);
            self.arrive_on_sheet();
        }
    }

    /// Long-press menu over the grid: clipboard, clear, insert/delete rows and columns.
    fn context_menu(&mut self, ctx: &egui::Context) {
        let Some((pos, opened)) = self.ctx_menu else { return };
        let mut chosen: Option<&'static str> = None;
        let has_link = self.doc.cell(self.sheet(), self.sel.first).is_some_and(|c| c.link.is_some());
        let area = egui::Area::new(egui::Id::unique("grid-ctx-menu")).order(egui::Order::Foreground).fixed_pos(pos + vec2(8.0, 8.0)).constrain(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(220.0);
                ui.set_max_width(260.0);
                ui.style_mut().spacing.button_padding = vec2(12.0, 10.0);
                if has_link && chrome::menu_item(ui, "Open link") {
                    chosen = Some("openLink");
                }
                for (label, id) in [
                    ("Cut", "cut"),
                    ("Copy", "copy"),
                    ("Paste", "paste"),
                    ("Paste values only", "pasteValues"),
                    ("Clear contents", "clear"),
                    ("Insert row above", "rowAbove"),
                    ("Insert column left", "colLeft"),
                    ("Delete row(s)", "delRows"),
                    ("Delete column(s)", "delCols"),
                ] {
                    if chrome::menu_item(ui, label) {
                        chosen = Some(id);
                    }
                }
            });
        });
        let now = ctx.input(|i| i.time);
        let outside = ctx.input(|i| i.pointer.any_pressed()) && !ctx.pointer_interact_pos().is_some_and(|p| area.response.rect.contains(p));
        if let Some(id) = chosen {
            self.ctx_menu = None;
            self.context_action(ctx, id);
        } else if outside && now - opened > 0.3 {
            self.ctx_menu = None;
        }
    }

    /// The AutoFilter pop-up of one column: show all / only one value / clear every filter.
    fn filter_popup(&mut self, ctx: &egui::Context) {
        let Some((pos, col, opened)) = self.filter_menu else { return };
        let sheet = self.sheet();
        let Some(f) = self.doc.filter_range(sheet) else {
            self.filter_menu = None;
            return;
        };
        let values = self.doc.column_values(sheet, col, f, 60);
        let title = self.doc.cell_text(sheet, Addr::new(f.first.row, col));
        let mut pick: Option<Option<String>> = None;
        let mut clear_all = false;
        let area = egui::Area::new(egui::Id::unique("grid-filter-menu")).order(egui::Order::Foreground).fixed_pos(pos + vec2(-40.0, 12.0)).constrain(true).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(220.0);
                ui.set_max_width(280.0);
                ui.style_mut().spacing.button_padding = vec2(12.0, 10.0);
                ui.label(egui::RichText::new(if title.is_empty() { format!("Column {}", paint::col_name(col)) } else { title.clone() }).strong());
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    if chrome::menu_item(ui, "Show all") {
                        pick = Some(None);
                    }
                    for v in &values {
                        if chrome::menu_item(ui, &format!("Only \"{v}\"")) {
                            pick = Some(Some(v.clone()));
                        }
                    }
                    ui.separator();
                    if chrome::menu_item(ui, "Clear all filters") {
                        clear_all = true;
                    }
                });
            });
        });
        let now = ctx.input(|i| i.time);
        let outside = ctx.input(|i| i.pointer.any_pressed()) && !ctx.pointer_interact_pos().is_some_and(|p| area.response.rect.contains(p));
        if let Some(choice) = pick {
            let r = match choice {
                None => self.doc.run("data.filterBy", serde_json::json!({ "column": paint::col_name(col), "clear": true })).map(|_| ()),
                Some(v) => self.doc.filter_values(col, &[v.as_str()]),
            };
            self.act(r);
            self.filter_menu = None;
        } else if clear_all {
            let r = self.doc.clear_filter();
            self.act(r);
            self.filter_menu = None;
        } else if outside && now - opened > 0.3 {
            self.filter_menu = None;
        }
    }

    /// Follow the hyperlink of cell `at` (active sheet), if it has one: a place in this workbook
    /// switches sheet and scrolls there; any other address shows the link pop-up. Returns
    /// whether the cell had a link.
    fn follow_link(&mut self, at: Addr) -> bool {
        let Some(link) = self.doc.cell(self.sheet(), at).and_then(|c| c.link) else { return false };
        match self.doc.link_target(&link) {
            slate_grid::LinkTarget::Place { sheet, area } => {
                self.commit();
                self.switch_sheet(sheet);
                self.select(area);
                self.want_visible = Some(area.first);
            }
            slate_grid::LinkTarget::Address(url) => crate::links::show_link(&url),
            slate_grid::LinkTarget::Refused(why) => self.say(why),
        }
        true
    }

    fn context_action(&mut self, ctx: &egui::Context, id: &str) {
        let t = self.target();
        match id {
            "cut" | "copy" => {
                let r = if id == "cut" { self.doc.cut(t) } else { self.doc.copy(t) };
                if let Some(text) = self.act(r) {
                    ctx.copy_text(text);
                    self.say(if id == "cut" { "Cut — choose where to paste" } else { "Copied" });
                }
            }
            "openLink" => {
                let at = self.sel.first;
                self.follow_link(at);
            }
            "paste" => {
                let r = self.doc.paste(self.sel.first);
                self.act(r);
            }
            "pasteValues" => {
                let r = self.doc.paste_values(self.sel.first);
                self.act(r);
            }
            "clear" => {
                let r = self.doc.clear_contents(t);
                self.act(r);
            }
            "rowAbove" => {
                let r = self.doc.insert_rows(t.first.row, t.rows().min(1000));
                self.act(r);
            }
            "colLeft" => {
                let r = self.doc.insert_cols(t.first.col, t.cols().min(200));
                self.act(r);
            }
            "delRows" if self.sel.rows() > 10_000 => self.say("Select rows first (tap a row number)"),
            "delCols" if self.sel.cols() > 1000 => self.say("Select columns first (tap a column letter)"),
            "delRows" => {
                let r = self.doc.delete_rows(self.sel.first.row, self.sel.rows());
                self.act(r);
            }
            "delCols" => {
                let r = self.doc.delete_cols(self.sel.first.col, self.sel.cols());
                self.act(r);
            }
            _ => {}
        }
        self.input_for = None;
    }

    /// Keep the formula bar in step with the selected cell (when not typing).
    fn sync_input(&mut self) {
        if self.editing_cell.is_some() {
            return;
        }
        let key = (self.sheet(), self.sel.first, self.doc.revision());
        if self.input_for != Some(key) {
            self.input = self.doc.cell(key.0, key.1).map(|c| c.input).unwrap_or_default();
            self.input_for = Some(key);
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context, bottom_gap: f32) {
        let now = ctx.input(|i| i.time);
        let Some((text, since)) = self.toast.as_mut() else { return };
        if *since < 0.0 {
            *since = now;
        }
        if now - *since > 4.0 {
            self.toast = None;
            return;
        }
        let text = text.clone();
        chrome::toast(ctx, &text, bottom_gap);
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// The AutoFilter drop-down button inside a title cell (screen rectangle).
fn filter_button(cell: Rect, z: f32) -> Rect {
    let side = (16.0 * z.clamp(0.8, 1.6)).min(cell.height() - 2.0).max(6.0);
    Rect::from_center_size(pos2(cell.right() - side / 2.0 - 2.0, cell.center().y), Vec2::splat(side))
}

/// Engine errors in plain words (the engine prefixes some with "confirm:").
fn friendly(e: &str) -> String {
    e.trim_start_matches("confirm:").trim().to_owned()
}

/// Cell fonts for the grid, registered next to the app's fonts once: the bundled Source Sans 3 /
/// Source Serif 4 / JetBrains Mono (already in the app through the Word engine) stand in for the
/// fonts files name (Calibri, Times New Roman, Courier...). Returns false on the frame they are
/// added (they become usable from the next frame).
pub(crate) fn ensure_fonts(ctx: &egui::Context) -> bool {
    let have = ctx.fonts(|f| f.definitions().families.contains_key(&FontFamily::Name("gs-sans".into())));
    if have {
        return true;
    }
    let mut defs = ctx.fonts(|f| f.definitions().clone());
    let bundled = wordcraft_fonts::bundled();
    let fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, index) in [("gs-sans", 0), ("gs-sans-b", 2), ("gs-sans-i", 3), ("gs-serif", 4), ("gs-serif-i", 5), ("gs-serif-b", 7), ("gs-serif-bi", 8), ("gs-mono", 12)] {
        let mut list = Vec::new();
        if let Some(bytes) = bundled.get(index) {
            let key = format!("gs-font-{index}");
            defs.font_data.entry(key.clone()).or_insert_with(|| Arc::new(egui::FontData::from_static(bytes)));
            list.push(key);
        }
        list.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(defs);
    ctx.request_repaint();
    false
}

/// Is a font family usable this frame? (Icons fall back to the default font until it is.)
pub(crate) fn has_family(ctx: &egui::Context, name: &str) -> bool {
    ctx.fonts(|f| f.definitions().families.contains_key(&FontFamily::Name(name.into())))
}

impl DocScreen for GridScreen {
    fn name(&self) -> &str {
        &self.name
    }
    fn tool(&self) -> Tool {
        Tool::Grid
    }
    fn dirty(&self) -> bool {
        self.doc.is_dirty()
    }
    fn editing(&self) -> bool {
        self.editing
    }
    fn set_author(&mut self, name: &str) {
        // Blank = "Author" (the engine's own default carries the upstream app name).
        let name = name.trim();
        self.doc.set_author(if name.is_empty() { "Author" } else { name });
    }
    fn set_editing(&mut self, on: bool) {
        if !on {
            self.commit();
            self.editing_cell = None;
            self.ctx_menu = None;
            self.filter_menu = None;
        }
        self.editing = on;
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) {
        let ctx = ui.ctx().clone();
        self.pics.boxes.clear();
        self.pics.scanner.poll();
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        if !ensure_fonts(&ctx) {
            ui.painter().rect_filled(full, 0.0, Color32::WHITE);
            return;
        }
        self.scrolled = false;
        if self.sheet() != self.shown_sheet {
            self.arrive_on_sheet();
        }
        if self.search_open && self.query != self.searched {
            self.run_search();
        }
        if self.jump {
            self.jump = false;
            self.go_to_hit();
        }
        self.sync_input();
        let mut top = full.top();
        if self.editing {
            self.formula_bar(ui, Rect::from_min_max(full.min, pos2(full.right(), top + FORMULA_H)), cx.accent);
            top += FORMULA_H;
        }
        let grid_rect = Rect::from_min_max(pos2(full.left(), top), pos2(full.right(), full.bottom() - TABS_H));
        let tabs_rect = Rect::from_min_max(pos2(full.left(), full.bottom() - TABS_H), full.max);
        self.grid(ui, grid_rect, cx);
        self.sheet_tabs(ui, tabs_rect, cx.accent);
        self.context_menu(&ctx);
        self.filter_popup(&ctx);
        // Typing ended by tapping elsewhere (a tool, another field): keep what was typed.
        // (Another text box taking the focus, e.g. the function search, keeps the typing open.)
        let input_id = egui::Id::unique("grid-formula-input");
        let other_focus = ctx.memory(|m| m.focused().is_some_and(|f| f != input_id));
        if self.editing_cell.is_some() && !self.keep_editing && !self.focus_input && !other_focus && !ctx.memory(|m| m.has_focus(input_id)) {
            self.commit();
        }
        self.keep_editing = false;
        let screen = ctx.content_rect();
        self.draw_toast(&ctx, (screen.bottom() - full.bottom()).max(0.0) + TABS_H - 40.0);
    }

    fn take_tapped(&mut self) -> bool {
        std::mem::take(&mut self.tapped)
    }
    fn scrolled(&self) -> bool {
        self.scrolled
    }
    fn position(&self) -> (usize, usize) {
        let tabs = self.visible_sheets();
        let active = self.sheet();
        (tabs.iter().position(|t| t.0 == active).unwrap_or(0), tabs.len().max(1))
    }

    fn search_open(&self) -> bool {
        self.search_open
    }
    fn toggle_search(&mut self) {
        self.search_open = !self.search_open;
        self.focus_search = self.search_open;
        if !self.search_open {
            self.hits.clear();
            self.searched.clear();
            self.query.clear();
        }
    }
    fn search_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(44.0);
            let edit = egui::TextEdit::singleline(&mut self.query).hint_text("Find in workbook").desired_width((ui.available_width() - 200.0).max(100.0));
            let resp = ui.add(edit);
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if self.query != self.searched {
                self.run_search();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.step_match(1);
                resp.request_focus();
            }
            let big = |t: &str| egui::Button::new(t).min_size(egui::vec2(48.0, 36.0));
            if ui.add(big("Prev")).clicked() {
                self.step_match(-1);
            }
            if ui.add(big("Next")).clicked() {
                self.step_match(1);
            }
            let count = match (self.hits.len(), self.query.trim().is_empty()) {
                (_, true) => String::new(),
                (0, false) => "No matches".into(),
                (n, false) => format!("{} of {}{}", self.current + 1, n, if n >= MAX_HITS { "+" } else { "" }),
            };
            ui.label(count);
        });
    }

    fn view_menu(&self) -> Vec<(String, &'static str)> {
        let tick = |on: bool, t: &str| format!("{t}: {}", if on { "on" } else { "off" });
        let (fr, fc, _, _) = self.frozen();
        let freeze = if fr == 0 && fc == 0 { "Frozen panes: none".to_owned() } else { format!("Frozen: {fr} row(s), {fc} column(s)") };
        vec![
            (tick(!self.hide_gridlines, "Gridlines"), "gridlines"),
            (tick(self.dark_sheet, "Dark sheet"), "dark"),
            ("Fit width".to_owned(), "fit"),
            ("Zoom 100 %".to_owned(), "zoom100"),
            (freeze, "freeze"),
        ]
    }
    fn view_action(&mut self, id: &str) {
        match id {
            "gridlines" => self.hide_gridlines = !self.hide_gridlines,
            "dark" => self.dark_sheet = !self.dark_sheet,
            "fit" => self.fit_width = true,
            "zoom100" => self.zoom = 1.0,
            "freeze" => {
                let (fr, fc, _, _) = self.frozen();
                self.say(if fr == 0 && fc == 0 {
                    "No frozen rows or columns on this sheet (Edit › Data › Freeze)".to_owned()
                } else {
                    format!("The top {fr} row(s) and left {fc} column(s) stay in place while scrolling")
                });
            }
            _ => {}
        }
    }

    fn undo(&mut self) {
        self.cancel_edit();
        if self.doc.can_undo() {
            let r = self.doc.undo();
            self.act(r);
        }
        self.input_for = None;
    }
    fn redo(&mut self) {
        self.cancel_edit();
        if self.doc.can_redo() {
            let r = self.doc.redo();
            self.act(r);
        }
        self.input_for = None;
    }

    fn edit_bar(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) -> Option<FileAction> {
        ensure_fonts(ui.ctx());
        self.tools(ui, cx.accent)
    }

    fn insert_picture(&mut self, path: &str, width_pt: f32) {
        let Ok(bytes) = std::fs::read(path) else { return };
        // Sheet units are pixels at 96 dpi.
        let r = self.doc.insert_picture(self.sel.first, &bytes, "image/png", Some(width_pt * 96.0 / 72.0));
        if self.act(r).is_some() {
            self.say("Barcode inserted");
        }
    }

    fn picked_image(&mut self, path: &str) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.say(format!("Couldn't read the picture: {e}"));
                return;
            }
        };
        let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
        // 3 inches wide (288 px at 96 dpi): a phone photo would cover the sheet otherwise.
        let r = self.doc.insert_picture(self.sel.first, &bytes, mime, Some(288.0));
        if self.act(r).is_some() {
            self.say("Picture inserted");
        }
    }

    fn can_save_in_place(&self) -> bool {
        self.doc.source_kind().saves_in_place()
    }
    fn default_ext(&self) -> &'static str {
        "xlsx"
    }
    fn write_to(&mut self, path: &Path) -> Result<Vec<String>, String> {
        self.commit();
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        let format = match ext.as_str() {
            "xlsx" | "xlsm" | "xltx" | "xltm" => SaveFormat::Xlsx,
            "csv" | "txt" | "tsv" | "tab" => SaveFormat::Csv,
            "ods" => SaveFormat::Ods,
            other => return Err(format!("Grid Slate can't write .{other} files")),
        };
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let saved = self.doc.encode_for(format, &file_name)?;
        std::fs::write(path, &saved.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(saved.report.lost)
    }
    fn mark_saved(&mut self, name: &str, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => self.doc.adopt_saved(name, &bytes),
            Err(_) => self.doc.mark_clean(),
        }
        self.name = name.to_owned();
    }
    fn export_pdf(&mut self, path: &Path) -> Result<(), String> {
        self.commit();
        self.doc.export_pdf(path, PdfOptions { all_sheets: true, fit_on_one_page: false }).map(|_| ())
    }
    fn info_line(&mut self) -> String {
        let sheets = self.visible_sheets().len();
        let s = self.sheet();
        let used = self.doc.used_range(s).map(|u| u.a1()).unwrap_or_else(|| "empty".into());
        let name = self.doc.sheets().get(s).map(|x| x.name.clone()).unwrap_or_default();
        format!("{} · {sheets} sheet{} · {name}: {used}", self.name, if sheets == 1 { "" } else { "s" })
    }
}
