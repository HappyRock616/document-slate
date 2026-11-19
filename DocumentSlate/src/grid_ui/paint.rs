//! Painting the sheet with egui: the layout model from `slate-grid` (`GridDoc::viewport`) turned
//! into rectangles, lines and text, so it stays crisp at every zoom.
//!
//! Provenance: the order and the rules (fills over gridlines, text spilling into empty
//! neighbours, `###` for numbers that don't fit, chart primitives) follow upstream GridCraft's
//! painter (`GridSlate/crates/ui-egui/src/grid/cell_paint.rs`, `chartview.rs`, MIT OR Apache-2.0);
//! `paint_prims` is adapted from upstream's `chartview.rs`.
//!
//! Screen layout of the grid: row headers on the left, column headers on top, then up to four
//! "quadrants" when panes are frozen (frozen corner, frozen rows, frozen columns, the scrolling
//! part). Sheet coordinates are pixels at 96 dpi / 100 %; `z` turns them into screen points.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use eframe::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, pos2, vec2};
use egui::epaint::{PathShape, PathStroke};
use slate_grid::{Addr, Area, CellLook, ChartHAlign, ChartVAlign, GridDoc, ObjectKind, Prim, TextAlign, Viewport};

/// Column header height and the narrowest row header, screen points.
pub const COL_HEAD: f32 = 24.0;
pub const ROW_HEAD_MIN: f32 = 34.0;

/// Where the cells are on screen and how sheet pixels map to it.
#[derive(Clone, Copy, Debug)]
pub struct Geo {
    /// Screen rectangle of the cells (right of the row headers, under the column headers).
    pub cells: Rect,
    pub z: f32,
    /// Size of the frozen rows / columns in sheet pixels.
    pub fw: f32,
    pub fh: f32,
    /// Scroll offset of the unfrozen part, sheet pixels.
    pub sx: f32,
    pub sy: f32,
}

impl Geo {
    /// Screen x of sheet x (frozen columns don't scroll).
    pub fn x(&self, x: f32, frozen: bool) -> f32 {
        self.cells.left() + (x - if frozen { 0.0 } else { self.sx }) * self.z
    }
    pub fn y(&self, y: f32, frozen: bool) -> f32 {
        self.cells.top() + (y - if frozen { 0.0 } else { self.sy }) * self.z
    }
    /// A sheet rectangle on screen, for a quadrant (`fx`/`fy`: frozen columns / rows).
    pub fn rect_q(&self, x: f32, y: f32, w: f32, h: f32, fx: bool, fy: bool) -> Rect {
        Rect::from_min_max(pos2(self.x(x, fx), self.y(y, fy)), pos2(self.x(x + w, fx), self.y(y + h, fy)))
    }
    /// A sheet rectangle on screen; each edge picks frozen or scrolling by where it lies.
    pub fn rect_auto(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let fx0 = x < self.fw - 0.01;
        let fy0 = y < self.fh - 0.01;
        let fx1 = x + w <= self.fw + 0.01;
        let fy1 = y + h <= self.fh + 0.01;
        // Scrolling edges never reach into the frozen panes (they slide under them).
        let (xm, ym) = (self.cells.left() + self.fw * self.z, self.cells.top() + self.fh * self.z);
        let cx = |v: f32, frozen: bool| if frozen { v } else { v.max(xm) };
        let cy = |v: f32, frozen: bool| if frozen { v } else { v.max(ym) };
        Rect::from_min_max(pos2(cx(self.x(x, fx0), fx0), cy(self.y(y, fy0), fy0)), pos2(cx(self.x(x + w, fx1), fx1), cy(self.y(y + h, fy1), fy1)))
    }
    /// Sheet point under a screen point.
    pub fn sheet_point(&self, p: Pos2) -> (f32, f32) {
        let x = (p.x - self.cells.left()) / self.z;
        let y = (p.y - self.cells.top()) / self.z;
        (if x < self.fw { x } else { x + self.sx }, if y < self.fh { y } else { y + self.sy })
    }
    /// Width / height of the scrolling part in sheet pixels.
    pub fn scroll_w(&self) -> f32 {
        ((self.cells.width() / self.z) - self.fw).max(1.0)
    }
    pub fn scroll_h(&self) -> f32 {
        ((self.cells.height() / self.z) - self.fh).max(1.0)
    }
    /// The four panes: (screen clip, frozen columns?, frozen rows?), empty ones left out.
    pub fn quadrants(&self) -> Vec<(Rect, bool, bool)> {
        let c = self.cells;
        let xm = (c.left() + self.fw * self.z).min(c.right());
        let ym = (c.top() + self.fh * self.z).min(c.bottom());
        let mut out = Vec::new();
        for (fx, fy) in [(false, false), (true, false), (false, true), (true, true)] {
            let (x0, x1) = if fx { (c.left(), xm) } else { (xm, c.right()) };
            let (y0, y1) = if fy { (c.top(), ym) } else { (ym, c.bottom()) };
            if x1 - x0 > 0.5 && y1 - y0 > 0.5 {
                out.push((Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), fx, fy));
            }
        }
        out
    }
}

/// A viewport plus which cells hold text (for text spilling into empty neighbours).
pub struct VpEntry {
    pub vp: Viewport,
    pub occupied: HashSet<(u32, u32)>,
}

/// Recently built viewports, keyed by (document revision, sheet, cell block). Blocks are
/// rounded out so small scrolls reuse them.
#[derive(Default)]
pub struct VpCache {
    entries: Vec<(u64, usize, Area, Rc<VpEntry>)>,
}

impl VpCache {
    pub fn get(&mut self, doc: &GridDoc, sheet: usize, area: Area) -> Option<Rc<VpEntry>> {
        let rev = doc.revision();
        if let Some((_, _, _, e)) = self.entries.iter().find(|(r, s, a, _)| *r == rev && *s == sheet && *a == area) {
            return Some(e.clone());
        }
        let vp = doc.viewport_cells(sheet, area)?;
        let mut occupied = HashSet::new();
        for c in &vp.cells {
            if !c.text.is_empty() || c.merge_span.is_some() {
                let (rows, cols) = c.merge_span.unwrap_or((1, 1));
                for r in 0..rows.min(200) {
                    for k in 0..cols.min(200) {
                        occupied.insert((c.addr.row + r, c.addr.col + k));
                    }
                }
            }
        }
        let e = Rc::new(VpEntry { vp, occupied });
        self.entries.retain(|(r, _, _, _)| *r == rev);
        if self.entries.len() >= 8 {
            self.entries.remove(0);
        }
        self.entries.push((rev, sheet, area, e.clone()));
        Some(e)
    }
}

/// Colours of the sheet (white like paper, or the optional dark sheet).
#[derive(Clone, Copy)]
pub struct SheetColors {
    pub bg: Color32,
    pub text: Color32,
    pub grid: Color32,
    pub head_bg: Color32,
    pub head_text: Color32,
    pub head_line: Color32,
    pub dark: bool,
}

impl SheetColors {
    pub fn new(dark_sheet: bool, dark_app: bool) -> Self {
        let mut c = if dark_sheet {
            SheetColors {
                bg: Color32::from_rgb(0x1E, 0x21, 0x24),
                text: Color32::from_rgb(0xD9, 0xDD, 0xE0),
                grid: Color32::from_rgb(0x3A, 0x3F, 0x44),
                head_bg: Color32::from_rgb(0x2B, 0x2F, 0x33),
                head_text: Color32::from_rgb(0xB0, 0xB6, 0xBC),
                head_line: Color32::from_rgb(0x44, 0x49, 0x4F),
                dark: true,
            }
        } else {
            SheetColors {
                bg: Color32::WHITE,
                text: Color32::BLACK,
                grid: Color32::from_rgb(0xDA, 0xDC, 0xE0),
                head_bg: Color32::from_rgb(0xF1, 0xF3, 0xF4),
                head_text: Color32::from_rgb(0x55, 0x5B, 0x61),
                head_line: Color32::from_rgb(0xCC, 0xD0, 0xD4),
                dark: false,
            }
        };
        // Headers follow the app theme so they blend with the bars.
        if dark_app && !dark_sheet {
            c.head_bg = Color32::from_rgb(0x2B, 0x2F, 0x33);
            c.head_text = Color32::from_rgb(0xB0, 0xB6, 0xBC);
            c.head_line = Color32::from_rgb(0x44, 0x49, 0x4F);
        }
        c
    }
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn luma(c: [u8; 3]) -> f32 {
    0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])
}

/// The egui family for a cell font: the bundled Source Sans / Source Serif / JetBrains Mono
/// (registered by `grid_ui::ensure_fonts`) standing in for the font the file names. Returns the
/// family and whether italics must be faked (no italic face for that family).
pub fn family(name: &str, bold: bool, italic: bool) -> (FontFamily, bool) {
    let n = name.to_ascii_lowercase();
    let serif = ["times", "cambria", "georgia", "garamond", "book", "serif", "palatino", "baskerville", "minion"].iter().any(|s| n.contains(s)) && !n.contains("sans");
    let mono = ["courier", "consolas", "mono", "menlo", "lucida console"].iter().any(|s| n.contains(s));
    let (fam, fake) = if mono {
        ("gs-mono", italic)
    } else if serif {
        (
            match (bold, italic) {
                (true, true) => "gs-serif-bi",
                (true, false) => "gs-serif-b",
                (false, true) => "gs-serif-i",
                _ => "gs-serif",
            },
            false,
        )
    } else {
        match (bold, italic) {
            (true, true) => ("gs-sans-b", true),
            (true, false) => ("gs-sans-b", false),
            (false, true) => ("gs-sans-i", false),
            _ => ("gs-sans", false),
        }
    };
    (FontFamily::Name(fam.into()), fake)
}

/// Visible columns between two sheet x positions: (index, left, width), hidden ones skipped.
pub fn col_lines(doc: &GridDoc, sheet: usize, x0: f32, x1: f32) -> Vec<(u32, f32, f32)> {
    let Some(sh) = doc.workbook().and_then(|wb| wb.sheet(sheet)) else { return Vec::new() };
    let first = sh.col_at(f64::from(x0.max(0.0)));
    let last = sh.col_at(f64::from(x1.max(0.0)));
    let mut out = Vec::new();
    let mut pos = sh.col_left(first) as f32;
    for c in first..=last.min(first + 2000) {
        let w = sh.col_width(c);
        if w > 0.0 {
            out.push((c, pos, w));
        }
        pos += w;
    }
    out
}

/// Visible rows between two sheet y positions: (index, top, height).
pub fn row_lines(doc: &GridDoc, sheet: usize, y0: f32, y1: f32) -> Vec<(u32, f32, f32)> {
    let Some(sh) = doc.workbook().and_then(|wb| wb.sheet(sheet)) else { return Vec::new() };
    let first = sh.row_at(f64::from(y0.max(0.0)));
    let last = sh.row_at(f64::from(y1.max(0.0)));
    let mut out = Vec::new();
    let mut pos = sh.row_top(first) as f32;
    for r in first..=last.min(first + 5000) {
        let h = sh.row_height(r);
        if h > 0.0 {
            out.push((r, pos, h));
        }
        pos += h;
    }
    out
}

/// Column letters ("A", "AB").
pub fn col_name(col: u32) -> String {
    let a = Addr::new(0, col).a1();
    a.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned()
}

/// Paint one quadrant's cells: background, gridlines, fills, text, borders, notes.
pub fn paint_cells(p: &egui::Painter, geo: &Geo, e: &VpEntry, fx: bool, fy: bool, colors: &SheetColors, gridlines: bool) {
    let vp = &e.vp;
    let clip = p.clip_rect();
    p.rect_filled(clip, 0.0, colors.bg);
    let z = geo.z;
    // Gridlines (skipped when zoomed so far out that they would be a grey wash).
    if gridlines {
        let stroke = Stroke::new(1.0, colors.grid);
        if vp.cols.iter().map(|l| l.size).fold(f32::MAX, f32::min) * z > 3.0 {
            for l in &vp.cols {
                let x = geo.x(l.start + l.size, fx).round() - 0.5;
                p.line_segment([pos2(x, clip.top()), pos2(x, clip.bottom())], stroke);
            }
        }
        if vp.rows.iter().map(|l| l.size).fold(f32::MAX, f32::min) * z > 3.0 {
            for l in &vp.rows {
                let y = geo.y(l.start + l.size, fy).round() - 0.5;
                p.line_segment([pos2(clip.left(), y), pos2(clip.right(), y)], stroke);
            }
        }
    }
    // Fills (a merged block hides the gridlines inside it).
    for c in &vp.cells {
        let r = geo.rect_q(c.x, c.y, c.w, c.h, fx, fy);
        if !r.intersects(clip) {
            continue;
        }
        if let Some(f) = c.fill {
            p.rect_filled(r, 0.0, rgb(f));
        } else if c.merge_span.is_some() {
            p.rect_filled(r.shrink(0.5), 0.0, colors.bg);
        }
        if let Some((frac, col)) = c.data_bar {
            let bar = Rect::from_min_size(r.min + vec2(2.0, 3.0 * z), vec2(((r.width() - 4.0) * frac).max(0.0), (r.height() - 6.0 * z).max(1.0)));
            p.rect_filled(bar, 1.0, rgb(col).gamma_multiply(0.75));
        }
    }
    // Text.
    for c in &vp.cells {
        paint_text(p, geo, e, c, fx, fy, colors);
    }
    // Borders on top.
    for c in &vp.cells {
        let r = geo.rect_q(c.x, c.y, c.w, c.h, fx, fy);
        if !r.expand(4.0).intersects(clip) {
            continue;
        }
        let edges = [(r.left_top(), r.left_bottom()), (r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.left_bottom(), r.right_bottom())];
        for (b, (a, bb)) in c.borders.iter().zip(edges) {
            let Some(b) = b else { continue };
            let w = (b.width * z.max(0.5)).clamp(1.0, 4.0);
            let col = if colors.dark && b.color == [0, 0, 0] { colors.text } else { rgb(b.color) };
            if b.double {
                let n = if (a.x - bb.x).abs() < 0.1 { vec2(1.5, 0.0) } else { vec2(0.0, 1.5) };
                p.line_segment([a - n, bb - n], Stroke::new(1.0, col));
                p.line_segment([a + n, bb + n], Stroke::new(1.0, col));
            } else if b.dashed || b.dotted {
                let (dash, gap) = if b.dotted { (1.5, 1.5) } else { (4.0, 2.5) };
                p.extend(egui::Shape::dashed_line(&[a, bb], Stroke::new(w, col), dash, gap));
            } else {
                p.line_segment([a, bb], Stroke::new(w, col));
            }
        }
        // Note / comment: a small red triangle in the top-right corner.
        if c.has_comment {
            let t = 6.0_f32.min(r.width() / 3.0);
            p.add(PathShape::convex_polygon(vec![r.right_top(), r.right_top() + vec2(0.0, t), r.right_top() - vec2(t, 0.0)], Color32::from_rgb(0xD1, 0x3B, 0x3B), Stroke::NONE));
        }
        if let Some((name, idx)) = &c.icon {
            let col = icon_color(name, *idx);
            let s = (r.height() * 0.32).min(7.0 * z);
            p.circle_filled(pos2(r.left() + 3.0 * z + s, r.center().y), s, col);
        }
    }
}

/// A conditional-format icon as a coloured dot: low red, middle amber, high green.
fn icon_color(set: &str, idx: usize) -> Color32 {
    let n = set.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(3).max(2) as usize;
    let t = idx as f32 / (n - 1) as f32;
    if t < 0.34 {
        Color32::from_rgb(0xD1, 0x3B, 0x3B)
    } else if t < 0.67 {
        Color32::from_rgb(0xE8, 0xB0, 0x20)
    } else {
        Color32::from_rgb(0x2E, 0x8B, 0x57)
    }
}

/// One cell's text, clipped to the cell (or to the empty neighbours it spills into).
fn paint_text(p: &egui::Painter, geo: &Geo, e: &VpEntry, c: &CellLook, fx: bool, fy: bool, colors: &SheetColors) {
    if c.text.is_empty() {
        return;
    }
    let z = geo.z;
    let r = geo.rect_q(c.x, c.y, c.w, c.h, fx, fy);
    let clip = p.clip_rect();
    if r.bottom() < clip.top() || r.top() > clip.bottom() {
        return;
    }
    let mut size = c.font_size.max(1.0) * 96.0 / 72.0 * z;
    if size < 3.0 {
        return;
    }
    let pad = 3.0 * z;
    let indent = f32::from(c.indent) * 9.0 * z;
    let inner_w = (r.width() - 2.0 * pad - indent).max(1.0);
    let (fam, fake_italic) = family(&c.font_name, c.bold, c.italic);
    let auto = match c.fill {
        Some(f) if luma(f) > 140.0 => Color32::BLACK,
        Some(_) => Color32::WHITE,
        None => colors.text,
    };
    let color = match c.text_color {
        Some(t) if colors.dark && c.fill.is_none() && luma(t) < 60.0 => colors.text,
        Some(t) => rgb(t),
        None => auto,
    };
    let wrap = c.wrap || c.align == TextAlign::Justify;
    let make = |size: f32, text: &str| {
        let mut job = egui::text::LayoutJob::default();
        let line = Stroke::new((size / 14.0).max(1.0), color);
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, fam.clone()),
                color,
                italics: fake_italic,
                underline: if c.underline || c.double_underline || c.has_link && c.text_color.is_none() { line } else { Stroke::NONE },
                strikethrough: if c.strike { line } else { Stroke::NONE },
                ..Default::default()
            },
        );
        job.wrap.max_width = if wrap { inner_w } else { f32::INFINITY };
        job.halign = match c.align {
            TextAlign::Center => egui::Align::Center,
            TextAlign::Right => egui::Align::RIGHT,
            _ => egui::Align::LEFT,
        };
        job
    };
    let mut galley = p.layout_job(make(size, &c.text));
    if c.shrink_to_fit && !wrap && galley.size().x > inner_w {
        size *= inner_w / galley.size().x;
        galley = p.layout_job(make(size, &c.text));
    }
    // Numbers that don't fit show ### (as Excel does); text spills over empty neighbours.
    let mut text_rect = r;
    if !wrap && galley.size().x > inner_w + 0.5 {
        if c.numeric {
            let hash_w = p.layout_no_wrap("#".into(), FontId::new(size, fam.clone()), color).size().x.max(1.0);
            let n = ((inner_w / hash_w).floor() as usize).max(1);
            galley = p.layout_job(make(size, &"#".repeat(n)));
        } else if c.merge_span.is_none() {
            let need = galley.size().x + 2.0 * pad + indent;
            let row = c.addr.row;
            let vp = &e.vp;
            let right_ok = matches!(c.align, TextAlign::Left | TextAlign::Fill | TextAlign::Center);
            let left_ok = matches!(c.align, TextAlign::Right | TextAlign::Center);
            let mut left = r.left();
            let mut right = r.right();
            if right_ok {
                for l in vp.cols.iter().filter(|l| l.index > c.addr.col) {
                    if right - left >= need || e.occupied.contains(&(row, l.index)) {
                        break;
                    }
                    right = geo.x(l.start + l.size, fx);
                }
            }
            if left_ok {
                for l in vp.cols.iter().rev().filter(|l| l.index < c.addr.col) {
                    if right - left >= need || e.occupied.contains(&(row, l.index)) {
                        break;
                    }
                    left = geo.x(l.start, fx);
                }
            }
            // Centred text grows both ways from the cell's centre.
            if c.align == TextAlign::Center {
                let half = (need / 2.0).min((r.center().x - left).min(right - r.center().x));
                left = r.center().x - half.max(r.width() / 2.0);
                right = r.center().x + half.max(r.width() / 2.0);
            }
            text_rect = Rect::from_min_max(pos2(left, r.top()), pos2(right, r.bottom()));
        }
    }
    let gs = galley.size();
    let x = match c.align {
        TextAlign::Center => r.center().x,
        TextAlign::Right => r.right() - pad,
        _ => r.left() + pad + indent,
    };
    let y = match c.valign {
        0 => r.top() + 1.0 * z,
        1 => r.center().y - gs.y / 2.0,
        _ => r.bottom() - 1.5 * z - gs.y,
    };
    let clip_to = text_rect.intersect(clip);
    if clip_to.width() <= 0.0 || clip_to.height() <= 0.0 {
        return;
    }
    p.with_clip_rect(clip_to).galley(pos2(x, y), galley, color);
}

/// Floating charts, pictures and shapes in one quadrant.
#[allow(clippy::too_many_arguments)]
pub fn paint_objects(p: &egui::Painter, geo: &Geo, vp: &Viewport, fx: bool, fy: bool, doc: &GridDoc, charts: &mut ChartCache, pics: &mut PicCache, ctx: &egui::Context) {
    for o in &vp.objects {
        let r = geo.rect_q(o.x, o.y, o.w, o.h, fx, fy);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        match o.kind {
            ObjectKind::Chart => {
                p.rect_filled(r, 0.0, Color32::WHITE);
                let prims = charts.get(doc, vp.sheet, o.index, o.w, o.h, p);
                paint_prims(&p.with_clip_rect(r.intersect(p.clip_rect())), r.min, &prims, geo.z);
                p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
            }
            ObjectKind::Picture => match pics.get(doc, vp.sheet, o.index, ctx) {
                Some(tex) => {
                    p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    pics.boxes.push((vp.sheet, o.index, [o.x, o.y, o.x + o.w, o.y + o.h]));
                    if let Some((pw, _)) = pics.sizes.get(&(vp.sheet, o.index)) {
                        pics.scanner.draw_badges(p, PicCache::key(vp.sheet, o.index), r, r.width() / (*pw).max(1) as f32, Color32::from_rgb(0x2E, 0x8B, 0x57));
                    }
                }
                None => {
                    p.rect_filled(r, 0.0, Color32::from_gray(235));
                    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(160)), egui::StrokeKind::Inside);
                }
            },
            ObjectKind::Shape => {
                p.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(0x44, 0x72, 0xC4, 40));
                p.rect_stroke(r, 4.0, Stroke::new(1.0, Color32::from_rgb(0x44, 0x72, 0xC4)), egui::StrokeKind::Inside);
            }
        }
    }
}

/// Chart drawings, laid out once per document revision and chart size.
/// (revision, width, height, primitives) of one chart.
type ChartEntry = (u64, u32, u32, Rc<Vec<Prim>>);

#[derive(Default)]
pub struct ChartCache {
    map: HashMap<(usize, usize), ChartEntry>,
}

struct EguiMeasure<'a> {
    painter: &'a egui::Painter,
}

impl slate_grid::Measure for EguiMeasure<'_> {
    fn text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let fam = if bold { FontFamily::Name("gs-sans-b".into()) } else { FontFamily::Name("gs-sans".into()) };
        self.painter.layout_no_wrap(text.to_owned(), FontId::new(size.max(1.0), fam), Color32::BLACK).size().x
    }
}

impl ChartCache {
    fn get(&mut self, doc: &GridDoc, sheet: usize, index: usize, w: f32, h: f32, p: &egui::Painter) -> Rc<Vec<Prim>> {
        let rev = doc.revision();
        let (wi, hi) = (w.round() as u32, h.round() as u32);
        if let Some((r, ww, hh, prims)) = self.map.get(&(sheet, index))
            && *r == rev
            && *ww == wi
            && *hh == hi
        {
            return prims.clone();
        }
        let prims = Rc::new(doc.chart_prims(sheet, index, w, h, &EguiMeasure { painter: p }));
        self.map.insert((sheet, index), (rev, wi, hi, prims.clone()));
        prims
    }
}

/// Decoded pictures as textures, keyed by the picture bytes (they don't change when cells do).
#[derive(Default)]
pub struct PicCache {
    map: HashMap<(usize, usize), (usize, Option<egui::TextureHandle>)>,
    /// Barcodes found in the pictures (positions in picture pixels), and each picture's size.
    pub scanner: crate::barcode_scan::Scanner,
    pub sizes: HashMap<(usize, usize), (u32, u32)>,
    /// Pictures painted this frame: (sheet, index, box in sheet units). Cleared each frame.
    pub boxes: Vec<(usize, usize, [f32; 4])>,
}

impl PicCache {
    fn get(&mut self, doc: &GridDoc, sheet: usize, index: usize, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let (bytes, _) = doc.picture(sheet, index)?;
        let key = std::sync::Arc::as_ptr(&bytes) as usize;
        if let Some((k, tex)) = self.map.get(&(sheet, index))
            && *k == key
        {
            return tex.clone();
        }
        let tex = doc.picture_rgba(sheet, index, 1024).map(|(w, h, px)| {
            self.sizes.insert((sheet, index), (w, h));
            self.scanner.submit_page(ctx, Self::key(sheet, index), key as u64, 1.0, w, h, &px);
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            ctx.load_texture(format!("grid-pic-{sheet}-{index}-{key}"), img, egui::TextureOptions::LINEAR)
        });
        self.map.insert((sheet, index), (key, tex.clone()));
        tex
    }
}

impl PicCache {
    pub fn key(sheet: usize, index: usize) -> usize {
        (sheet << 20) | index
    }

    /// A barcode in a picture under sheet point (x, y).
    pub fn code_at(&self, sheet: usize, x: f32, y: f32) -> Option<slate_barcode::Found> {
        self.boxes.iter().rev().filter(|(s, _, b)| *s == sheet && x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]).find_map(|(s, i, b)| {
            let (pw, ph) = *self.sizes.get(&(*s, *i))?;
            let px = (x - b[0]) / (b[2] - b[0]).max(1.0) * pw as f32;
            let py = (y - b[1]) / (b[3] - b[1]).max(1.0) * ph as f32;
            self.scanner.hit_at(Self::key(*s, *i), [px, py], 8.0).map(|h| h.found.clone())
        })
    }
}

fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Paints chart primitives at `origin`, scaled by `z` (adapted from upstream's chartview.rs).
pub fn paint_prims(p: &egui::Painter, origin: Pos2, prims: &[Prim], z: f32) {
    let pt = |x: f32, y: f32| pos2(origin.x + x * z, origin.y + y * z);
    for prim in prims {
        match prim {
            Prim::Rect { x, y, w, h, fill, stroke, radius } => {
                let r = Rect::from_min_size(pt(*x, *y), vec2(w * z, h * z));
                if let Some(f) = fill {
                    p.rect_filled(r, radius * z, rgba(*f));
                }
                if let Some((c, wd)) = stroke {
                    p.rect_stroke(r, radius * z, Stroke::new(wd * z.max(0.5), rgba(*c)), egui::StrokeKind::Inside);
                }
            }
            Prim::Line { pts, color, width, dash } => {
                let v: Vec<Pos2> = pts.iter().map(|q| pt(q[0], q[1])).collect();
                if v.len() < 2 {
                    continue;
                }
                if *dash {
                    p.extend(egui::Shape::dashed_line(&v, Stroke::new(width * z.max(0.5), rgba(*color)), 4.0 * z, 3.0 * z));
                } else {
                    p.add(PathShape::line(v, PathStroke::new(width * z.max(0.5), rgba(*color))));
                }
            }
            Prim::Polygon { pts, fill, stroke } => {
                let v: Vec<Pos2> = pts.iter().map(|q| pt(q[0], q[1])).collect();
                if v.len() < 3 {
                    continue;
                }
                if is_convex(&v) {
                    p.add(PathShape { points: v.clone(), closed: true, fill: rgba(*fill), stroke: PathStroke::NONE });
                } else {
                    p.add(egui::Shape::mesh(area_mesh(&v, rgba(*fill))));
                }
                if let Some((c, w)) = stroke {
                    p.add(PathShape::closed_line(v, PathStroke::new(w * z.max(0.5), rgba(*c))));
                }
            }
            Prim::Wedge { cx, cy, r_outer, r_inner, a0, a1, fill, stroke } => {
                let c = pt(*cx, *cy);
                let steps = (((a1 - a0).abs() / 0.05).ceil() as usize).clamp(2, 400);
                let at = |a: f32, r: f32| pos2(c.x + r * z * a.sin(), c.y - r * z * a.cos());
                let mut mesh = egui::Mesh::default();
                let col = rgba(*fill);
                for i in 0..=steps {
                    let a = a0 + (a1 - a0) * i as f32 / steps as f32;
                    mesh.colored_vertex(at(a, *r_outer), col);
                    mesh.colored_vertex(if *r_inner > 0.0 { at(a, *r_inner) } else { c }, col);
                }
                for i in 0..steps as u32 {
                    let b = i * 2;
                    mesh.add_triangle(b, b + 1, b + 2);
                    mesh.add_triangle(b + 1, b + 3, b + 2);
                }
                p.add(egui::Shape::mesh(mesh));
                if let Some((sc, w)) = stroke {
                    let mut outline: Vec<Pos2> = (0..=steps).map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, *r_outer)).collect();
                    if *r_inner > 0.0 {
                        outline.extend((0..=steps).rev().map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, *r_inner)));
                    } else {
                        outline.push(c);
                    }
                    p.add(PathShape::closed_line(outline, PathStroke::new(w * z.max(0.5), rgba(*sc))));
                }
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                if let Some(f) = fill {
                    p.circle_filled(pt(*cx, *cy), r * z, rgba(*f));
                }
                if let Some((c, w)) = stroke {
                    p.circle_stroke(pt(*cx, *cy), r * z, Stroke::new(w * z.max(0.5), rgba(*c)));
                }
            }
            Prim::Text { x, y, text, size, color, bold, align, valign, rotation } => {
                let fam = if *bold { FontFamily::Name("gs-sans-b".into()) } else { FontFamily::Name("gs-sans".into()) };
                let galley = p.layout_no_wrap(text.clone(), FontId::new((size * z).max(1.0), fam), rgba(*color));
                let sz = galley.size();
                let ax = match align {
                    ChartHAlign::Left => 0.0,
                    ChartHAlign::Center => 0.5,
                    ChartHAlign::Right => 1.0,
                };
                let ay = match valign {
                    ChartVAlign::Top => 0.0,
                    ChartVAlign::Middle => 0.5,
                    ChartVAlign::Bottom => 1.0,
                };
                let anchor = pt(*x, *y);
                if *rotation == 0.0 {
                    p.galley(pos2(anchor.x - sz.x * ax, anchor.y - sz.y * ay), galley, rgba(*color));
                } else {
                    let (s, c) = rotation.sin_cos();
                    let off = vec2(-sz.x * ax, -sz.y * ay);
                    let rot = vec2(off.x * c - off.y * s, off.x * s + off.y * c);
                    let mut shape = egui::epaint::TextShape::new(anchor + rot, galley, rgba(*color));
                    shape.angle = *rotation;
                    p.add(shape);
                }
            }
        }
    }
}

fn is_convex(v: &[Pos2]) -> bool {
    let n = v.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (v[i], v[(i + 1) % n], v[(i + 2) % n]);
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() > 1e-3 {
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
    }
    true
}

/// Area charts give "mountain" polygons over a straight baseline: fill them as vertical strips.
fn area_mesh(v: &[Pos2], col: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let n = v.len();
    let half = n / 2;
    for i in 0..half.saturating_sub(1) {
        let (a, b, c, d) = (v[i], v[i + 1], v[n - 2 - i], v[n - 1 - i]);
        let base = mesh.vertices.len() as u32;
        for q in [a, b, c, d] {
            mesh.colored_vertex(q, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}

/// Column and row headers (letters / numbers), with the selected ones highlighted.
#[allow(clippy::too_many_arguments)]
pub fn paint_headers(p: &egui::Painter, geo: &Geo, row_head: f32, doc: &GridDoc, sheet: usize, sel: Option<Area>, colors: &SheetColors, accent: Color32) {
    let c = geo.cells;
    let col_bar = Rect::from_min_max(pos2(c.left(), c.top() - COL_HEAD), pos2(c.right(), c.top()));
    let row_bar = Rect::from_min_max(pos2(c.left() - row_head, c.top()), pos2(c.left(), c.bottom()));
    p.rect_filled(col_bar, 0.0, colors.head_bg);
    p.rect_filled(row_bar, 0.0, colors.head_bg);
    let corner = Rect::from_min_max(pos2(c.left() - row_head, c.top() - COL_HEAD), pos2(c.left(), c.top()));
    p.rect_filled(corner, 0.0, colors.head_bg);
    // Little triangle in the corner (select all), as in Excel.
    p.add(PathShape::convex_polygon(
        vec![corner.right_bottom() - vec2(3.0, 3.0), corner.right_bottom() - vec2(3.0, 12.0), corner.right_bottom() - vec2(12.0, 3.0)],
        colors.head_line,
        Stroke::NONE,
    ));
    let line = Stroke::new(1.0, colors.head_line);
    let font = FontId::proportional((12.0 * geo.z.sqrt()).clamp(9.0, 15.0));
    let wash = accent.gamma_multiply(0.25);
    // Columns: frozen part, then the scrolling part.
    let xm = (c.left() + geo.fw * geo.z).min(c.right());
    for (frozen, clip) in [(true, Rect::from_min_max(pos2(c.left(), col_bar.top()), pos2(xm, col_bar.bottom()))), (false, Rect::from_min_max(pos2(xm, col_bar.top()), col_bar.max))] {
        if clip.width() <= 0.5 {
            continue;
        }
        let pc = p.with_clip_rect(clip);
        let (x0, x1) = if frozen { (0.0, geo.fw) } else { (geo.fw + geo.sx, geo.fw + geo.sx + geo.scroll_w()) };
        let lines = col_lines(doc, sheet, x0, x1 - 0.01);
        let step = label_step(lines.iter().map(|l| l.2).fold(64.0, f32::min) * geo.z, 26.0);
        for (i, x, w) in lines {
            let r = Rect::from_min_max(pos2(geo.x(x, frozen), clip.top()), pos2(geo.x(x + w, frozen), clip.bottom()));
            let selected = sel.is_some_and(|s| s.first.col <= i && i <= s.last.col);
            if selected {
                pc.rect_filled(r, 0.0, wash);
                pc.line_segment([r.left_bottom() - vec2(0.0, 1.0), r.right_bottom() - vec2(0.0, 1.0)], Stroke::new(2.0, accent));
            }
            pc.line_segment([r.right_top(), r.right_bottom()], line);
            if i % step == 0 {
                pc.text(r.center(), Align2::CENTER_CENTER, col_name(i), font.clone(), if selected { accent } else { colors.head_text });
            }
        }
    }
    let ym = (c.top() + geo.fh * geo.z).min(c.bottom());
    for (frozen, clip) in [(true, Rect::from_min_max(pos2(row_bar.left(), c.top()), pos2(row_bar.right(), ym))), (false, Rect::from_min_max(pos2(row_bar.left(), ym), row_bar.max))] {
        if clip.height() <= 0.5 {
            continue;
        }
        let pc = p.with_clip_rect(clip);
        let (y0, y1) = if frozen { (0.0, geo.fh) } else { (geo.fh + geo.sy, geo.fh + geo.sy + geo.scroll_h()) };
        let lines = row_lines(doc, sheet, y0, y1 - 0.01);
        let step = label_step(lines.iter().map(|l| l.2).fold(20.0, f32::min) * geo.z, 12.0);
        for (i, y, h) in lines {
            let r = Rect::from_min_max(pos2(clip.left(), geo.y(y, frozen)), pos2(clip.right(), geo.y(y + h, frozen)));
            let selected = sel.is_some_and(|s| s.first.row <= i && i <= s.last.row);
            if selected {
                pc.rect_filled(r, 0.0, wash);
                pc.line_segment([r.right_top() - vec2(1.0, 0.0), r.right_bottom() - vec2(1.0, 0.0)], Stroke::new(2.0, accent));
            }
            pc.line_segment([r.left_bottom(), r.right_bottom()], line);
            if (i + 1) % step == 0 || step == 1 {
                pc.text(r.center(), Align2::CENTER_CENTER, (i + 1).to_string(), font.clone(), if selected { accent } else { colors.head_text });
            }
        }
    }
    p.line_segment([col_bar.left_bottom(), col_bar.right_bottom()], line);
    p.line_segment([row_bar.right_top(), row_bar.right_bottom()], line);
    // The freeze lines.
    if geo.fw > 0.0 && xm < c.right() {
        p.line_segment([pos2(xm, c.top()), pos2(xm, c.bottom())], Stroke::new(1.5, colors.head_text.gamma_multiply(0.7)));
    }
    if geo.fh > 0.0 && ym < c.bottom() {
        p.line_segment([pos2(c.left(), ym), pos2(c.right(), ym)], Stroke::new(1.5, colors.head_text.gamma_multiply(0.7)));
    }
}

/// Label every n-th header when they get too small to read (zoomed far out).
fn label_step(size: f32, need: f32) -> u32 {
    if size >= need {
        1
    } else if size * 2.0 >= need {
        2
    } else if size * 5.0 >= need {
        5
    } else {
        10
    }
}

/// Row header width for the rows on screen (wide enough for the largest row number).
pub fn row_head_width(last_row: u32, z: f32) -> f32 {
    let digits = (last_row + 1).to_string().len() as f32;
    let font = (12.0 * z.sqrt()).clamp(9.0, 15.0);
    (digits * font * 0.62 + 14.0).max(ROW_HEAD_MIN)
}
