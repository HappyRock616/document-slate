//! # slate-grid — the spreadsheet back-end of Grid Slate
//!
//! This crate wraps upstream GridCraft's engine (`GridSlate/crates/engine`) behind a small API
//! that the Android app (egui) can call. It contains no UI code at all.
//!
//! How it fits together, in plain words:
//!
//! * GridCraft keeps an open workbook inside a [`Session`]. Every change ("make A1 bold", "sort
//!   column B", "insert 3 rows") is a *command* with a text id such as `home.bold` and JSON
//!   parameters. Running commands through the session gives us undo/redo, recalculation of
//!   formulas and the engine's crash guard for free. [`GridDoc`] owns one session with one
//!   workbook and turns friendly Rust methods into those commands (see `edit.rs`).
//! * For drawing, GridCraft's own desktop UI reads the workbook model directly (column widths,
//!   row heights, cell styles) and paints cells itself with egui. We do the same: `view.rs`
//!   builds a [`Viewport`] — a plain list of visible columns, rows and cell "looks" (text,
//!   colours, borders, alignment) — that the app paints. The engine has no pixel renderer for
//!   the grid, so a "render to RGBA" API would have meant writing one; a layout model is
//!   lighter, stays crisp at every zoom level and lets egui draw real fonts.
//! * Saving goes back through the engine's writers (`save.rs`), and `fidelity.rs` compares the
//!   original .xlsx package with the saved one so we can *tell the user* if anything would be
//!   dropped (the full-fidelity rule).
//! * Printing: the engine paginates sheets and writes a PDF itself (`file.print`); the app hands
//!   those bytes to Android's print dialog.
//!
//! Coordinates: rows and columns are 0-based (`Addr { row: 0, col: 0 }` is A1). Sizes are pixels
//! at 96 dpi and 100 % zoom, which is what the engine stores; the app multiplies by its own scale.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod edit;
mod fidelity;
mod save;
mod support;
mod view;

use gridcraft_engine::core::{CellRef, RangeRef, Value};
use gridcraft_engine::model::Visibility;
use gridcraft_engine::{DocState, Session, Trust};
use serde_json::{Value as Json, json};

pub use edit::{BorderPreset, FUNCTION_CATEGORIES, FunctionInfo, HAlignChoice, NumberFormat, SortKey, VAlignChoice};
pub use fidelity::FidelityReport;
pub use save::{PdfOptions, SaveFormat, Saved};
pub use view::{Border, CellLook, Line, ObjectBox, ObjectKind, TextAlign, Viewport};

/// Chart drawing primitives and the text-measuring hook, re-exported so the app can paint charts
/// without depending on GridCraft crates itself.
pub use gridcraft_chart::{ApproxMeasure, HAlign as ChartHAlign, Measure, Prim, VAlign as ChartVAlign};
pub use support::PageSetup;

/// Escape hatch: the whole upstream engine, for anything this wrapper doesn't cover yet.
pub use gridcraft_engine as engine;

/// A cell position, 0-based: `Addr { row: 0, col: 0 }` is A1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Addr {
    pub row: u32,
    pub col: u32,
}

impl Addr {
    pub fn new(row: u32, col: u32) -> Addr {
        Addr { row, col }
    }
    /// Parses "B4" (or "$B$4"). Returns `None` for anything that isn't one cell.
    pub fn parse(a1: &str) -> Option<Addr> {
        CellRef::parse(a1.trim()).map(|c| Addr { row: c.row, col: c.col })
    }
    /// The A1 name, e.g. "B4" — what the formula bar shows next to the contents.
    pub fn a1(self) -> String {
        self.to_ref().a1()
    }
    pub(crate) fn to_ref(self) -> CellRef {
        CellRef::new(self.row, self.col)
    }
    pub(crate) fn from_ref(c: CellRef) -> Addr {
        Addr { row: c.row, col: c.col }
    }
}

/// A rectangular block of cells, both corners included (0-based).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Area {
    pub first: Addr,
    pub last: Addr,
}

impl Area {
    /// The area between two corners, in any order.
    pub fn new(a: Addr, b: Addr) -> Area {
        Area {
            first: Addr::new(a.row.min(b.row), a.col.min(b.col)),
            last: Addr::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }
    pub fn cell(a: Addr) -> Area {
        Area { first: a, last: a }
    }
    /// Parses "A1:C10" or "B4".
    pub fn parse(s: &str) -> Option<Area> {
        RangeRef::parse(s.trim()).map(Area::from_range)
    }
    /// "A1:C10" (or "B4" for a single cell).
    pub fn a1(self) -> String {
        self.to_range().a1()
    }
    pub fn rows(self) -> u32 {
        self.last.row - self.first.row + 1
    }
    pub fn cols(self) -> u32 {
        self.last.col - self.first.col + 1
    }
    pub(crate) fn to_range(self) -> RangeRef {
        RangeRef::new(self.first.to_ref(), self.last.to_ref())
    }
    pub(crate) fn from_range(r: RangeRef) -> Area {
        Area::new(Addr::from_ref(r.start), Addr::from_ref(r.end))
    }
}

/// What kind of file a document came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// .xlsx / .xlsm / .xltx / .xltm — read and written natively.
    Xlsx,
    /// Excel 97-2003 .xls — imported (saved back as .xlsx).
    Xls,
    /// .xlsb — data import only (saved back as .xlsx).
    Xlsb,
    /// OpenDocument .ods — data import only (saved back as .xlsx).
    Ods,
    /// Comma/tab separated text.
    Csv,
    /// Something else the engine understands (SYLK, GridCraft JSON...).
    Other,
    /// Made in the app (no file yet).
    New,
}

impl SourceKind {
    /// Whether saving can write the same format back.
    pub fn saves_in_place(self) -> bool {
        matches!(self, SourceKind::Xlsx | SourceKind::Csv)
    }
}

/// A cell's value, simplified for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    /// An error such as `#DIV/0!`.
    Error(String),
}

impl CellValue {
    fn from_value(v: &Value) -> CellValue {
        match v {
            Value::Empty => CellValue::Empty,
            Value::Number(n) => CellValue::Number(*n),
            Value::Text(t) => CellValue::Text(t.to_string()),
            Value::Bool(b) => CellValue::Bool(*b),
            Value::Error(_) | Value::Array(_) => {
                if v.is_error() {
                    CellValue::Error(v.display())
                } else {
                    CellValue::Text(v.display())
                }
            }
        }
    }
}

/// Everything the formula bar and the format buttons need about one cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellInfo {
    pub addr: Addr,
    /// The text the grid shows (number format applied).
    pub display: String,
    /// What the editor starts from: the formula with its `=`, or the constant as typed.
    pub input: String,
    /// The formula without `=`, if the cell has one (hidden formulas on protected sheets: `None`).
    pub formula: Option<String>,
    pub value: CellValue,
    /// The number format code, e.g. `0.00` or `General`.
    pub number_format: String,
    pub font_name: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub wrap: bool,
    /// The merged block this cell belongs to, if any.
    pub merged: Option<Area>,
    /// Note/comment text, if the cell has one.
    pub comment: Option<String>,
    /// Hyperlink target, if the cell has one.
    pub link: Option<String>,
}

/// One sheet tab.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetInfo {
    pub index: usize,
    pub name: String,
    /// Hidden sheets should not get a tab in viewer mode.
    pub hidden: bool,
    /// Tab colour as RGB, if the file set one.
    pub tab_color: Option<[u8; 3]>,
    /// The used part of the sheet (cells with content), if any.
    pub used: Option<Area>,
    /// Frozen rows and columns at the top/left, kept from the file.
    pub frozen: Option<(u32, u32)>,
}

/// An open spreadsheet: one GridCraft session holding one workbook.
///
/// Not thread-safe to share, but it can be *moved* to a worker thread (it is `Send`), so the app
/// may open/save/print on a background thread and hand the document back.
pub struct GridDoc {
    session: Session,
    name: String,
    kind: SourceKind,
    /// The original .xlsx bytes (only for xlsx files), kept to check nothing is lost on save.
    original_xlsx: Option<Vec<u8>>,
    /// What the workbook held when `original_xlsx` was read (see `fidelity::ModelCounts`).
    opened_counts: fidelity::ModelCounts,
    warnings: Vec<String>,
}

impl GridDoc {
    /// Opens a spreadsheet from its bytes. `name` is the file name (its extension helps pick the
    /// format when the bytes don't say). Reads xlsx/xlsm, xls, xlsb, ods, csv/tsv/txt.
    pub fn open_bytes(name: &str, bytes: &[u8]) -> Result<GridDoc, String> {
        GridDoc::open_bytes_with_password(name, bytes, None)
    }

    /// [`GridDoc::open_bytes`] for a password-protected workbook. If a password is needed and
    /// missing, the error text contains [`PASSWORD_REQUIRED`], so the app can ask for one.
    pub fn open_bytes_with_password(name: &str, bytes: &[u8], password: Option<&str>) -> Result<GridDoc, String> {
        let sniffed = gridcraft_xlsx::sniff(bytes);
        let (mut wb, warnings) = gridcraft_engine::io::open_bytes_with_password(name, bytes, password).map_err(|e| e.to_string())?;
        let kind = source_kind(name, sniffed);
        // What CELL("filename") reports: the name only (we never know a folder on Android).
        wb.source_path = Some(std::path::PathBuf::from(name));
        let opened_counts = fidelity::ModelCounts::of(&wb);
        let mut session = Session::new();
        // A file from elsewhere: its macros never run without asking (we never run them anyway).
        let doc = DocState::new(wb, None, name.to_string(), Trust::FromFile);
        session.add_document(doc);
        let original_xlsx = (kind == SourceKind::Xlsx && sniffed == gridcraft_xlsx::Format::Xlsx).then(|| bytes.to_vec());
        Ok(GridDoc { session, name: name.to_string(), kind, original_xlsx, opened_counts, warnings })
    }

    /// A new, empty workbook with one sheet.
    pub fn new_blank(name: &str) -> GridDoc {
        let mut session = Session::new();
        session.new_workbook();
        GridDoc { session, name: name.to_string(), kind: SourceKind::New, original_xlsx: None, opened_counts: fidelity::ModelCounts::default(), warnings: Vec::new() }
    }

    /// The file name given when opening.
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn source_kind(&self) -> SourceKind {
        self.kind
    }
    /// Messages from opening (things the engine couldn't read, or "this was imported").
    pub fn open_warnings(&self) -> &[String] {
        &self.warnings
    }
    /// Unsaved changes since opening or the last full-fidelity save?
    pub fn is_dirty(&self) -> bool {
        self.session.active().is_some_and(DocState::is_dirty)
    }
    pub fn can_undo(&self) -> bool {
        self.session.active().is_some_and(|d| !d.undo.is_empty())
    }
    pub fn can_redo(&self) -> bool {
        self.session.active().is_some_and(|d| !d.redo.is_empty())
    }
    /// Goes up by one on every change; the app can use it to know when to redraw.
    pub fn revision(&self) -> u64 {
        self.session.active().map(|d| d.revision).unwrap_or(0)
    }

    // ------------------------------------------------------------------ sheets

    pub fn sheet_count(&self) -> usize {
        self.doc().map(|d| d.wb.sheets.len()).unwrap_or(0)
    }

    /// All sheets in tab order (hidden ones included, flagged).
    pub fn sheets(&self) -> Vec<SheetInfo> {
        let Some(d) = self.doc() else { return Vec::new() };
        d.wb
            .sheets
            .iter()
            .enumerate()
            .map(|(index, sh)| SheetInfo {
                index,
                name: sh.name.clone(),
                hidden: sh.visibility != Visibility::Visible,
                tab_color: sh.tab_color.and_then(|c| c.resolve(&d.wb.theme)),
                used: sh.used_range().map(Area::from_range),
                frozen: sh.freeze.filter(|(r, c)| *r > 0 || *c > 0),
            })
            .collect()
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.sheets().into_iter().map(|s| s.name).collect()
    }

    /// The sheet edits apply to (and the one that opens first).
    pub fn active_sheet(&self) -> usize {
        self.doc().map(|d| d.wb.active_sheet()).unwrap_or(0)
    }

    /// The name written as the author of new comments and as the workbook's author.
    pub fn set_author(&mut self, name: &str) {
        self.session.prefs.user_name = name.to_string();
    }

    /// Switches the active sheet (a sheet tab tap).
    pub fn set_active_sheet(&mut self, index: usize) -> Result<(), String> {
        self.run("sheet.activate", json!({ "sheet": index })).map(|_| ())
    }

    /// The used part of a sheet, or `None` when it is empty.
    pub fn used_range(&self, sheet: usize) -> Option<Area> {
        self.doc()?.wb.sheet(sheet)?.used_range().map(Area::from_range)
    }

    // ------------------------------------------------------------------ cells

    /// Full information about one cell (also fine for empty cells).
    pub fn cell(&self, sheet: usize, at: Addr) -> Option<CellInfo> {
        let d = self.doc()?;
        let wb = &d.wb;
        let sh = wb.sheet(sheet)?;
        let c = at.to_ref();
        let style = wb.styles.get(sh.style_id(c));
        let hidden = gridcraft_engine::display::formula_hidden(wb, sh, c);
        let formula = if hidden { None } else { sh.cell(c).and_then(|x| x.formula.as_ref()).map(|f| f.text.to_string()) };
        Some(CellInfo {
            addr: at,
            display: gridcraft_engine::display::value_text(wb, sh, c),
            input: if hidden { gridcraft_engine::display::value_text(wb, sh, c) } else { gridcraft_engine::display::edit_text(wb, sh, c) },
            formula,
            value: CellValue::from_value(&sh.value(c)),
            number_format: style.num_fmt.as_str().to_string(),
            font_name: style.font.name.clone(),
            font_size: style.font.size,
            bold: style.font.bold,
            italic: style.font.italic,
            underline: style.font.underline != gridcraft_engine::model::Underline::None,
            strike: style.font.strike,
            wrap: style.align.wrap,
            merged: sh.merge_at(c).map(Area::from_range),
            comment: sh.comments.get(&c).map(|m| m.text.clone()),
            link: sh.hyperlinks.get(&c).map(|h| h.target.clone()),
        })
    }

    /// Where a cell's hyperlink leads, as upstream's click on a link decides
    /// (`gridcraft_engine::links::classify`): a place in this workbook (`Sheet2!A1`, a defined
    /// name, `#…`) or an address for the system. Looking doesn't move the selection.
    pub fn link_target(&mut self, target: &str) -> LinkTarget {
        use gridcraft_engine::links::{LinkKind, classify};
        match classify(target) {
            LinkKind::Internal(r) => {
                // Go To resolves names and sheet references; then put the selection back.
                let Some(d) = self.doc() else { return LinkTarget::Refused("No workbook open".into()) };
                let (sheet0, sel0) = (d.wb.active_sheet(), d.selection.clone());
                let reference = r.trim_start_matches('#').to_string();
                let found = self.run("edit.goTo", json!({ "reference": reference })).ok().and_then(|_| {
                    let d = self.doc()?;
                    let area = d.selection.ranges.last().copied().map(Area::from_range)?;
                    Some((d.wb.active_sheet(), area))
                });
                let _ = self.run("selection.set", json!({ "sheet": sheet0, "range": sel0.ranges.iter().map(RangeRef::a1).collect::<Vec<_>>().join(",") }));
                match found {
                    Some((sheet, area)) => LinkTarget::Place { sheet, area },
                    None => LinkTarget::Refused(format!("This link points to \"{reference}\", which isn't in this workbook")),
                }
            }
            LinkKind::External(url) => LinkTarget::Address(url),
            // Anything else (file paths, other programs) still shows its address, never opens.
            LinkKind::Refused(_) => LinkTarget::Address(target.trim().to_string()),
        }
    }

    /// The text a cell shows (number format applied).
    pub fn cell_text(&self, sheet: usize, at: Addr) -> String {
        self.cell(sheet, at).map(|c| c.display).unwrap_or_default()
    }

    // ------------------------------------------------------------------ engine access

    /// Runs any engine command by id with JSON parameters (see `GridSlate/crates/engine/src/cmd`).
    /// Everything in this crate's edit API goes through here.
    pub fn run(&mut self, id: &str, params: Json) -> Result<Json, String> {
        self.session.execute(id, params).map_err(|e| e.to_string())
    }

    /// Read-only access to the workbook model, for things this wrapper doesn't expose.
    pub fn workbook(&self) -> Option<&gridcraft_engine::model::Workbook> {
        self.doc().map(|d| d.wb.as_ref())
    }

    pub(crate) fn doc(&self) -> Option<&DocState> {
        self.session.active()
    }
}

/// Where a hyperlink leads ([`GridDoc::link_target`]).
#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    /// A place in this workbook: the sheet index and the cells to select.
    Place { sheet: usize, area: Area },
    /// An address outside the workbook (web, mail, file…), as written.
    Address(String),
    /// A reference that doesn't resolve, with the reason to show.
    Refused(String),
}

/// Text an error contains when a workbook needs a password to open.
pub const PASSWORD_REQUIRED: &str = gridcraft_engine::io::PASSWORD_REQUIRED;

/// Works out the source kind from the bytes (preferred) and the file name.
fn source_kind(name: &str, sniffed: gridcraft_xlsx::Format) -> SourceKind {
    use gridcraft_engine::io::FileKind;
    use gridcraft_xlsx::Format;
    match sniffed {
        Format::Ods => return SourceKind::Ods,
        Format::Xlsb => return SourceKind::Xlsb,
        Format::Xls => return SourceKind::Xls,
        _ => {}
    }
    match FileKind::from_path(name) {
        Some(FileKind::Xlsx) => SourceKind::Xlsx,
        Some(FileKind::Xls) => SourceKind::Xls,
        Some(FileKind::Xlsb) => SourceKind::Xlsb,
        Some(FileKind::Ods) => SourceKind::Ods,
        Some(FileKind::Csv | FileKind::Tsv) => SourceKind::Csv,
        // No/unknown extension: trust the bytes.
        None if sniffed == Format::Xlsx || sniffed == Format::Encrypted => SourceKind::Xlsx,
        None if sniffed == Format::Csv => SourceKind::Csv,
        _ => SourceKind::Other,
    }
}

#[cfg(test)]
mod tests;
