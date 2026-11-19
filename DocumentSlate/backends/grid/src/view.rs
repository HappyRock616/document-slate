//! The viewer's layout model: which columns, rows and cells are visible in a rectangle of the
//! sheet, and how each cell looks. The app paints this with egui (rectangles, lines, text).
//!
//! This mirrors what upstream's own desktop UI does (`ui-egui/src/grid/cell_paint.rs`): read the
//! cell's style, apply table styles and conditional formatting, format the value with its number
//! format, then paint. We stop just before painting, so no egui types appear here.
//!
//! All positions are pixels at 96 dpi / 100 % zoom, measured from the sheet's top-left corner
//! (the corner of cell A1, not counting row/column headers).

use gridcraft_engine::cf::CfCache;
use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_engine::model::{BorderLine, BorderStyle, Color, HAlign, PatternType, Sheet, Style, Underline, VAlign, Workbook};

use crate::{Addr, Area, GridDoc};

/// Most cells one viewport call looks at (rows × columns). A phone screen zoomed far out shows
/// a few thousand; the cap only protects against a caller asking for a whole sheet at once.
const MAX_VIEW_CELLS: usize = 40_000;
/// Most columns one viewport call lists.
const MAX_VIEW_COLS: u32 = 1_000;
/// Width of a digit in the default font (Calibri 11 at 96 dpi), used to fit General numbers to a
/// column the way Excel does (shrinking decimals, then `1E+10`, then `####`).
const DIGIT_PX: f32 = 7.0;

/// One visible column or row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// Column or row index (0-based).
    pub index: u32,
    /// Left (or top) edge in sheet pixels.
    pub start: f32,
    /// Width (or height) in pixels. Hidden columns/rows are left out of the list.
    pub size: f32,
}

/// Horizontal text placement after "General" alignment is resolved (numbers right, text left,
/// booleans/errors centred — as Excel does).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    /// Repeat the text to fill the cell (rare).
    Fill,
    /// Justify / distributed: the app may treat it as Left with wrapping.
    Justify,
}

/// One edge of a cell border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    /// Line width in pixels (1 thin, 2 medium, 3 thick/double).
    pub width: f32,
    pub color: [u8; 3],
    pub dashed: bool,
    pub dotted: bool,
    pub double: bool,
}

/// How one cell looks. Cells that are completely empty and unformatted are not listed.
#[derive(Clone, Debug, PartialEq)]
pub struct CellLook {
    pub addr: Addr,
    /// The cell rectangle in sheet pixels. For a merged block this covers the whole block.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Text to draw (may be empty). Numbers are already formatted.
    pub text: String,
    /// True for numbers/dates: they never spill into neighbouring cells; when they don't fit,
    /// Excel shows `####` (the app can do that once it measures the text).
    pub numeric: bool,
    /// Background fill, `None` = no fill (show the sheet background / gridlines).
    pub fill: Option<[u8; 3]>,
    /// Text colour (`None` = automatic: black on light fills, the theme's text colour otherwise).
    pub text_color: Option<[u8; 3]>,
    pub font_name: String,
    /// Font size in points (multiply by 96/72 for pixels at 100 %).
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub double_underline: bool,
    pub strike: bool,
    pub align: TextAlign,
    /// Vertical placement: 0 = top, 1 = middle, 2 = bottom.
    pub valign: u8,
    pub wrap: bool,
    pub shrink_to_fit: bool,
    /// Indent level (each level is about 3 digit widths, ~ 9 px).
    pub indent: u8,
    /// Text rotation in degrees (-90..=90), or 255 for vertical stacked text.
    pub rotation: i16,
    /// Borders: left, top, right, bottom.
    pub borders: [Option<Border>; 4],
    /// Conditional-format data bar: fraction of the width (0..=1) and colour.
    pub data_bar: Option<(f32, [u8; 3])>,
    /// Conditional-format icon: icon set name and index (0 = lowest).
    pub icon: Option<(String, usize)>,
    /// The cell has a hyperlink (draw text blue/underlined if the style doesn't already).
    pub has_link: bool,
    /// The cell has a note/comment (Excel draws a small red triangle in the top-right corner).
    pub has_comment: bool,
    /// Merged block size in cells (rows, cols) when this cell is a merge's top-left cell.
    pub merge_span: Option<(u32, u32)>,
}

/// What kind of floating object a box is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// Draw with [`GridDoc::chart_prims`] using `index`.
    Chart,
    /// Picture bytes come from [`GridDoc::picture`] using `index`.
    Picture,
    /// A drawn shape or text box (not drawn by this crate yet: show a placeholder outline).
    Shape,
}

/// A floating object (chart, picture, shape) over the grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectBox {
    pub kind: ObjectKind,
    /// Position in the sheet's list of charts / images / shapes.
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Everything needed to paint one rectangle of a sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub sheet: usize,
    pub cols: Vec<Line>,
    pub rows: Vec<Line>,
    /// Non-empty or formatted cells, row by row.
    pub cells: Vec<CellLook>,
    /// Floating objects that overlap the rectangle.
    pub objects: Vec<ObjectBox>,
    /// Draw light gridlines between cells (the sheet's "show gridlines" setting).
    pub gridlines: bool,
    /// Frozen rows and columns, kept from the file.
    pub frozen: Option<(u32, u32)>,
    /// The sheet is written right-to-left (Arabic/Hebrew): the app should mirror it.
    pub right_to_left: bool,
    /// The cell list was cut short by the safety cap (ask for a smaller rectangle).
    pub truncated: bool,
}

impl GridDoc {
    /// The visible part of `sheet` inside the pixel rectangle (`x`, `y`, `w`, `h`), measured
    /// from the top-left of A1 at 100 % zoom. For frozen panes call it once per pane.
    pub fn viewport(&self, sheet: usize, x: f32, y: f32, w: f32, h: f32) -> Option<Viewport> {
        let sh = self.doc()?.wb.sheet(sheet)?;
        let first_col = sh.col_at(f64::from(x.max(0.0)));
        let first_row = sh.row_at(f64::from(y.max(0.0)));
        let last_col = sh.col_at(f64::from((x + w).max(0.0)));
        let last_row = sh.row_at(f64::from((y + h).max(0.0)));
        self.viewport_cells(sheet, Area::new(Addr::new(first_row, first_col), Addr::new(last_row, last_col)))
    }

    /// Like [`GridDoc::viewport`] but for a block of cells (handy for frozen rows/columns).
    pub fn viewport_cells(&self, sheet: usize, area: Area) -> Option<Viewport> {
        let wb = &self.doc()?.wb;
        let sh = wb.sheet(sheet)?;
        let mut area = clamp_area(area);
        // Keep the work bounded: at most MAX_VIEW_COLS columns and MAX_VIEW_CELLS cells.
        let mut truncated = false;
        if area.cols() > MAX_VIEW_COLS {
            area.last.col = area.first.col + MAX_VIEW_COLS - 1;
            truncated = true;
        }
        let max_rows = (MAX_VIEW_CELLS as u32 / area.cols()).max(1);
        if area.rows() > max_rows {
            area.last.row = area.first.row + max_rows - 1;
            truncated = true;
        }
        let cols = lines(area.first.col, area.last.col, |c| sh.col_left(c) as f32, |c| sh.col_width(c));
        let rows = lines(area.first.row, area.last.row, |r| sh.row_top(r) as f32, |r| sh.row_height(r));
        let mut cf = CfCache::new();
        let mut cells = Vec::new();
        let view = area.to_range();
        // Merged blocks that touch the view: their top-left cell is drawn over the whole block,
        // even when that top-left cell itself has scrolled out of view.
        let merges: Vec<RangeRef> = sh.merges.iter().filter(|m| m.intersects(&view)).copied().collect();
        for m in &merges {
            if !view.contains(m.start)
                && let Some(look) = cell_look(wb, sheet, sh, m.start, &mut cf, Some(*m))
            {
                cells.push(look);
            }
        }
        for row in &rows {
            for col in &cols {
                let c = CellRef::new(row.index, col.index);
                let merge = merges.iter().find(|m| m.contains(c)).copied();
                // Cells hidden under a merge are covered by its top-left cell.
                if merge.is_some_and(|m| m.start != c) {
                    continue;
                }
                if let Some(look) = cell_look(wb, sheet, sh, c, &mut cf, merge) {
                    cells.push(look);
                }
            }
        }
        let objects = objects_in(sh, view_rect(sh, area));
        Some(Viewport {
            sheet,
            cols,
            rows,
            cells,
            objects,
            gridlines: sh.show_gridlines,
            frozen: sh.freeze.filter(|(r, c)| *r > 0 || *c > 0),
            right_to_left: sh.right_to_left,
            truncated,
        })
    }

    /// The cell under a point (sheet pixels at 100 %) — for taps.
    pub fn hit(&self, sheet: usize, x: f32, y: f32) -> Option<Addr> {
        let sh = self.doc()?.wb.sheet(sheet)?;
        Some(Addr::new(sh.row_at(f64::from(y.max(0.0))), sh.col_at(f64::from(x.max(0.0)))))
    }

    /// A cell's rectangle (x, y, w, h) in sheet pixels; a merged cell gives the whole block.
    pub fn cell_rect(&self, sheet: usize, at: Addr) -> Option<(f32, f32, f32, f32)> {
        let sh = self.doc()?.wb.sheet(sheet)?;
        let r = sh.merge_at(at.to_ref()).unwrap_or(RangeRef::new(at.to_ref(), at.to_ref()));
        Some(range_rect(sh, r))
    }

    /// How big the scrollable area should be: the used range plus floating objects, plus a margin
    /// of empty cells so there is room to type past the end. (width, height) in pixels.
    pub fn content_size(&self, sheet: usize) -> Option<(f32, f32)> {
        let sh = self.doc()?.wb.sheet(sheet)?;
        let used = sh.used_range().unwrap_or_default();
        let last_row = used.end.row.saturating_add(20).min(MAX_ROWS - 1);
        let last_col = used.end.col.saturating_add(6).min(MAX_COLS - 1);
        let mut w = sh.col_left(last_col + 1) as f32;
        let mut h = sh.row_top(last_row + 1) as f32;
        for o in objects_in(sh, (0.0, 0.0, f32::MAX, f32::MAX)) {
            w = w.max(o.x + o.w);
            h = h.max(o.y + o.h);
        }
        Some((w, h))
    }

    /// A chart's drawing as simple shapes, laid out for a box of `w` × `h` pixels. `measure`
    /// lets the chart place labels using the app's real font widths ([`crate::ApproxMeasure`]
    /// is a fine default). Coordinates are relative to the chart box's top-left.
    pub fn chart_prims(&self, sheet: usize, index: usize, w: f32, h: f32, measure: &dyn gridcraft_chart::Measure) -> Vec<gridcraft_chart::Prim> {
        let Some(wb) = self.doc().map(|d| &d.wb) else { return Vec::new() };
        let Some(ch) = wb.sheet(sheet).and_then(|sh| sh.charts.get(index)) else { return Vec::new() };
        let data = gridcraft_chart::resolve(wb, sheet, ch);
        gridcraft_chart::render(ch, &data, w.max(1.0), h.max(1.0), measure)
    }

    /// A floating picture's encoded bytes (PNG/JPEG...) and MIME type, for the app to decode.
    pub fn picture(&self, sheet: usize, index: usize) -> Option<(std::sync::Arc<Vec<u8>>, String)> {
        let img = self.doc()?.wb.sheet(sheet)?.images.get(index)?;
        Some((img.data.clone(), img.mime.clone()))
    }
}

/// Keeps an area inside the sheet grid.
fn clamp_area(a: Area) -> Area {
    let clamp = |x: Addr| Addr::new(x.row.min(MAX_ROWS - 1), x.col.min(MAX_COLS - 1));
    Area::new(clamp(a.first), clamp(a.last))
}

/// The visible (non-hidden) lines from `first` to `last`.
fn lines(first: u32, last: u32, start: impl Fn(u32) -> f32, size: impl Fn(u32) -> f32) -> Vec<Line> {
    // `start` is costly for sheets with many custom sizes, so measure once and add up.
    let mut pos = start(first);
    let mut out = Vec::new();
    for index in first..=last {
        let s = size(index);
        if s > 0.0 {
            out.push(Line { index, start: pos, size: s });
        }
        pos += s;
    }
    out
}

/// (x, y, w, h) of a range in sheet pixels.
fn range_rect(sh: &Sheet, r: RangeRef) -> (f32, f32, f32, f32) {
    let x = sh.col_left(r.start.col) as f32;
    let y = sh.row_top(r.start.row) as f32;
    let x2 = sh.col_left(r.end.col.saturating_add(1)) as f32;
    let y2 = sh.row_top(r.end.row.saturating_add(1)) as f32;
    (x, y, x2 - x, y2 - y)
}

fn view_rect(sh: &Sheet, a: Area) -> (f32, f32, f32, f32) {
    range_rect(sh, a.to_range())
}

/// Charts, pictures and shapes whose box overlaps `rect`.
fn objects_in(sh: &Sheet, rect: (f32, f32, f32, f32)) -> Vec<ObjectBox> {
    let (rx, ry, rw, rh) = rect;
    let mut out = Vec::new();
    let mut push = |kind, index, a: &gridcraft_engine::model::Anchor| {
        let x = sh.col_left(a.cell.col) as f32 + a.dx;
        let y = sh.row_top(a.cell.row) as f32 + a.dy;
        let (w, h) = (a.width.max(0.0), a.height.max(0.0));
        if x < rx + rw && x + w > rx && y < ry + rh && y + h > ry {
            out.push(ObjectBox { kind, index, x, y, w, h });
        }
    };
    for (i, c) in sh.charts.iter().enumerate() {
        push(ObjectKind::Chart, i, &c.anchor);
    }
    for (i, p) in sh.images.iter().enumerate() {
        push(ObjectKind::Picture, i, &p.anchor);
    }
    for (i, s) in sh.shapes.iter().enumerate() {
        push(ObjectKind::Shape, i, &s.anchor);
    }
    out
}

/// The look of one cell, or `None` when there is nothing to draw (no value, no fill, no border).
fn cell_look(wb: &Workbook, si: usize, sh: &Sheet, c: CellRef, cf: &mut CfCache, merge: Option<RangeRef>) -> Option<CellLook> {
    let v = sh.value(c);
    let (style, cf_fill, data_bar, icon, hide_value) = resolved_style(wb, si, sh, c, cf);
    let fill = cf_fill.or_else(|| fill_rgb(wb, &style));
    let borders = merged_borders(wb, si, sh, c, merge, &style, cf);
    let has_link = sh.hyperlinks.contains_key(&c);
    let has_comment = sh.comments.contains_key(&c);
    if v == Value::Empty && fill.is_none() && borders.iter().all(Option::is_none) && data_bar.is_none() && !has_comment {
        return None;
    }
    let (x, y, w, h) = range_rect(sh, merge.unwrap_or(RangeRef::new(c, c)));
    let (text, color, numeric) = if hide_value { (String::new(), None, false) } else { display_text(wb, sh, c, &v, &style, w) };
    let align = match style.align.h {
        HAlign::Left => TextAlign::Left,
        HAlign::Center | HAlign::CenterAcross => TextAlign::Center,
        HAlign::Right => TextAlign::Right,
        HAlign::Fill => TextAlign::Fill,
        HAlign::Justify | HAlign::Distributed => TextAlign::Justify,
        // General: numbers right, booleans and errors centred, text left.
        HAlign::General => match v {
            Value::Bool(_) | Value::Error(_) => TextAlign::Center,
            _ if numeric => TextAlign::Right,
            _ => TextAlign::Left,
        },
    };
    let valign = match style.align.v {
        VAlign::Top => 0,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => 1,
        VAlign::Bottom => 2,
    };
    let text_color = color.or_else(|| style.font.color.resolve(&wb.theme)).or_else(|| has_link.then_some([0x05, 0x63, 0xC1]));
    Some(CellLook {
        addr: Addr::from_ref(c),
        x,
        y,
        w,
        h,
        text,
        numeric,
        fill,
        text_color,
        font_name: style.font.name.clone(),
        font_size: style.font.size,
        bold: style.font.bold,
        italic: style.font.italic,
        underline: matches!(style.font.underline, Underline::Single | Underline::SingleAccounting),
        double_underline: matches!(style.font.underline, Underline::Double | Underline::DoubleAccounting),
        strike: style.font.strike,
        align,
        valign,
        wrap: style.align.wrap,
        shrink_to_fit: style.align.shrink,
        indent: style.align.indent,
        rotation: style.align.rotation,
        borders,
        data_bar,
        icon,
        has_link,
        has_comment,
        merge_span: merge.map(|m| (m.end.row - m.start.row + 1, m.end.col - m.start.col + 1)),
    })
}

/// A cell's final style after table styles and conditional formatting, plus the extras that
/// conditional formatting adds (colour-scale fill, data bar, icon, hidden value).
type Resolved = (Style, Option<[u8; 3]>, Option<(f32, [u8; 3])>, Option<(String, usize)>, bool);

fn resolved_style(wb: &Workbook, si: usize, sh: &Sheet, c: CellRef, cf: &mut CfCache) -> Resolved {
    let mut style = match sh.table_at(c) {
        Some(t) => gridcraft_engine::tables::table_cell_style(wb, si, t, c),
        None => wb.styles.get(sh.style_id(c)).clone(),
    };
    let Some(look) = cf.look(wb, si, c) else { return (style, None, None, None, false) };
    // A conditional format only overrides the parts it sets (same rules as upstream's painter).
    if let Some(st) = &look.style {
        if st.fill.pattern != PatternType::None {
            style.fill = st.fill;
        }
        if st.font.color != Color::Auto {
            style.font.color = st.font.color;
        }
        style.font.bold |= st.font.bold;
        style.font.italic |= st.font.italic;
        style.font.strike |= st.font.strike;
        if st.font.underline != Underline::None {
            style.font.underline = st.font.underline;
        }
        if st.num_fmt.as_str() != "General" {
            style.num_fmt = st.num_fmt.clone();
        }
        for (dst, src) in [
            (&mut style.borders.left, st.borders.left),
            (&mut style.borders.right, st.borders.right),
            (&mut style.borders.top, st.borders.top),
            (&mut style.borders.bottom, st.borders.bottom),
        ] {
            if !src.is_none() {
                *dst = src;
            }
        }
    }
    let bar = look.bar.map(|(frac, rgb, _gradient)| (frac.clamp(0.0, 1.0), rgb));
    (style, look.scale_fill, bar, look.icon, look.hide_value)
}

/// The fill colour of a style; patterned fills are approximated by a blend (as upstream does).
fn fill_rgb(wb: &Workbook, style: &Style) -> Option<[u8; 3]> {
    match style.fill.pattern {
        PatternType::None => None,
        PatternType::Solid => style.fill.fg.resolve(&wb.theme),
        _ => {
            let fg = style.fill.fg.resolve(&wb.theme).unwrap_or([0, 0, 0]);
            let bg = style.fill.bg.resolve(&wb.theme).unwrap_or([255, 255, 255]);
            let mix = |a: u8, b: u8| ((u16::from(a) + u16::from(b) * 3) / 4) as u8;
            Some([mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2])])
        }
    }
}

fn border(wb: &Workbook, line: BorderLine) -> Option<Border> {
    if line.is_none() {
        return None;
    }
    use BorderStyle as B;
    Some(Border {
        width: line.style.width(),
        color: line.color.resolve(&wb.theme).unwrap_or([0, 0, 0]),
        dashed: matches!(line.style, B::Dashed | B::MediumDashed | B::DashDot | B::MediumDashDot | B::DashDotDot | B::MediumDashDotDot | B::SlantDashDot),
        dotted: matches!(line.style, B::Dotted | B::Hair),
        double: line.style == B::Double,
    })
}

/// Borders of a cell; for a merged block, the outer edges come from the block's edge cells.
fn merged_borders(wb: &Workbook, si: usize, sh: &Sheet, c: CellRef, merge: Option<RangeRef>, style: &Style, cf: &mut CfCache) -> [Option<Border>; 4] {
    let Some(m) = merge else {
        return [border(wb, style.borders.left), border(wb, style.borders.top), border(wb, style.borders.right), border(wb, style.borders.bottom)];
    };
    let edge = |cell: CellRef, cf: &mut CfCache| resolved_style(wb, si, sh, cell, cf).0.borders;
    let _ = c;
    [
        border(wb, edge(m.start, cf).left),
        border(wb, edge(m.start, cf).top),
        border(wb, edge(CellRef::new(m.start.row, m.end.col), cf).right),
        border(wb, edge(CellRef::new(m.end.row, m.start.col), cf).bottom),
    ]
}

/// The text a cell shows, its number-format colour (e.g. `[Red]`), and whether it is numeric.
/// General numbers are narrowed to the column width, as upstream's grid does.
fn display_text(wb: &Workbook, sh: &Sheet, c: CellRef, v: &Value, st: &Style, width_px: f32) -> (String, Option<[u8; 3]>, bool) {
    if sh.show_formulas
        && let Some(f) = sh.cell(c).and_then(|x| x.formula.as_ref())
        && !gridcraft_engine::display::formula_hidden(wb, sh, c)
    {
        return (format!("={}", f.text), None, false);
    }
    if !sh.show_zeros && v.as_f64() == Some(0.0) {
        return (String::new(), None, false);
    }
    let code = st.num_fmt.as_str();
    if code == "checkbox" && matches!(v, Value::Bool(_) | Value::Empty) {
        // A checkbox cell: show a ballot box character the app's font is likely to have.
        let text = if *v == Value::Bool(true) { "\u{2611}" } else { "\u{2610}" };
        return (text.to_string(), None, false);
    }
    if code.eq_ignore_ascii_case("General")
        && let Value::Number(n) = v
    {
        let max_chars = ((width_px - 4.0) / DIGIT_PX).floor().max(1.0) as usize;
        return match gridcraft_engine::numfmt::format_general_fit(*n, max_chars.min(11)) {
            Some(t) => (t, None, true),
            None => ("#".repeat(max_chars.max(1)), None, true),
        };
    }
    let f = gridcraft_engine::display::format(v, code, wb);
    let mut text = f.text;
    // `*x` repeat-fill (accounting formats): show one space where the filler goes; the app
    // doesn't need to expand it for a viewer.
    if let Some((_, pos)) = f.fill
        && pos <= text.len()
        && text.is_char_boundary(pos)
    {
        text.insert(pos, ' ');
    }
    (text, f.color.and_then(gridcraft_engine::numfmt::FormatColor::rgb), f.numeric)
}
