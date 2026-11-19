//! Editing: friendly methods that run GridCraft engine commands.
//!
//! Every method here ends in `GridDoc::run(command_id, json_params)`. The engine then does the
//! real work: it changes a copy of the workbook, recalculates the formulas that depend on the
//! change, and pushes an undo step. So every edit below is undoable with [`GridDoc::undo`].
//!
//! Edits apply to the *active sheet* (see `GridDoc::set_active_sheet`). Ranges are passed to the
//! engine explicitly, so the engine's own "current selection" never matters, except for the few
//! commands that only work on it (paste special) — those set it first.
//!
//! The groups follow DocumentSlate/docs/GridSlate-features.md (Cell, Formula, Data, Insert, File
//! tabs and the long-press menu).

use serde_json::{Value as Json, json};

use crate::{Addr, Area, GridDoc};

/// Horizontal alignment buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlignChoice {
    Left,
    Center,
    Right,
}

/// Vertical alignment buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlignChoice {
    Top,
    Middle,
    Bottom,
}

/// Border presets for the Cell tab (the common ones from Excel's Borders menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderPreset {
    None,
    All,
    Outside,
    ThickOutside,
    Bottom,
    Top,
    Left,
    Right,
    DoubleBottom,
    Inside,
}

impl BorderPreset {
    fn id(self) -> &'static str {
        match self {
            BorderPreset::None => "none",
            BorderPreset::All => "all",
            BorderPreset::Outside => "outside",
            BorderPreset::ThickOutside => "thickOutside",
            BorderPreset::Bottom => "bottom",
            BorderPreset::Top => "top",
            BorderPreset::Left => "left",
            BorderPreset::Right => "right",
            BorderPreset::DoubleBottom => "doubleBottom",
            BorderPreset::Inside => "inside",
        }
    }
}

/// Number formats offered in the Cell tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumberFormat {
    General,
    /// Two decimals, e.g. 1234.50
    Number,
    /// Currency with the engine's default symbol.
    Currency,
    /// Accounting with a chosen symbol, e.g. "€" or "$".
    Accounting(String),
    Percent,
    /// Thousands separators with two decimals (Excel's "comma style").
    Comma,
    ShortDate,
    LongDate,
    Time,
    /// Store as text (no number conversion).
    Text,
    /// Any Excel format code, e.g. `0.000` or `dd/mm/yyyy`.
    Custom(String),
}

/// One key of a custom sort.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortKey {
    /// Column letter(s), e.g. "B".
    pub column: String,
    pub descending: bool,
}

/// One function in the "insert function" list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInfo {
    pub name: String,
    pub category: String,
    /// E.g. `SUM(number1, [number2], …)`.
    pub signature: String,
    pub description: String,
}

/// Function categories for the Formula tab: (engine id, label to show). The approved v1 list
/// is the first six; the rest are there for "more".
pub const FUNCTION_CATEGORIES: &[(&str, &str)] = &[
    ("MathTrig", "Maths"),
    ("Text", "Text"),
    ("DateTime", "Date & time"),
    ("Logical", "Logical"),
    ("Lookup", "Lookup"),
    ("Financial", "Financial"),
    ("Statistical", "Statistical"),
    ("Information", "Information"),
    ("Engineering", "Engineering"),
    ("Database", "Database"),
];

/// Column letters for a 0-based column index ("A", "B", ... "AA").
pub(crate) fn col_letters(col: u32) -> String {
    let a1 = Addr::new(0, col).a1();
    a1.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()
}

impl GridDoc {
    /// Small helper: run a command whose only parameter besides its own is the target range.
    fn on_range(&mut self, id: &str, area: Area, mut extra: Json) -> Result<(), String> {
        if let Some(obj) = extra.as_object_mut() {
            obj.insert("range".into(), Json::String(area.a1()));
        }
        self.run(id, extra).map(|_| ())
    }

    // ------------------------------------------------------------------ undo / redo

    pub fn undo(&mut self) -> Result<(), String> {
        self.run("edit.undo", json!({})).map(|_| ())
    }
    pub fn redo(&mut self) -> Result<(), String> {
        self.run("edit.redo", json!({})).map(|_| ())
    }

    // ------------------------------------------------------------------ cell contents

    /// Types into a cell, exactly like the formula bar: `42`, `hello`, `12/3/2026`, `=SUM(A1:A3)`.
    /// Formulas recalculate at once.
    pub fn set_cell(&mut self, at: Addr, input: &str) -> Result<(), String> {
        self.run("cell.set", json!({ "cell": at.a1(), "input": input })).map(|_| ())
    }
    /// Clears values and formulas, keeps formatting.
    pub fn clear_contents(&mut self, area: Area) -> Result<(), String> {
        self.on_range("edit.clearContents", area, json!({}))
    }
    /// Clears formatting, keeps values.
    pub fn clear_formats(&mut self, area: Area) -> Result<(), String> {
        self.on_range("edit.clearFormats", area, json!({}))
    }
    /// Clears everything (values, formats, notes, links).
    pub fn clear_all(&mut self, area: Area) -> Result<(), String> {
        self.on_range("edit.clearAll", area, json!({}))
    }

    // ------------------------------------------------------------------ clipboard (long-press menu)

    /// Copies cells into the engine's clipboard. Returns tab-separated text for Android's
    /// system clipboard.
    pub fn copy(&mut self, area: Area) -> Result<String, String> {
        let r = self.run("edit.copy", json!({ "range": area.a1() }))?;
        Ok(r.get("text").and_then(Json::as_str).unwrap_or_default().to_string())
    }
    /// Cut: the cells move when pasted (like Excel).
    pub fn cut(&mut self, area: Area) -> Result<String, String> {
        let r = self.run("edit.cut", json!({ "range": area.a1() }))?;
        Ok(r.get("text").and_then(Json::as_str).unwrap_or_default().to_string())
    }
    /// Pastes the engine clipboard at `at` (formulas, values and formats).
    pub fn paste(&mut self, at: Addr) -> Result<(), String> {
        self.run("edit.paste", json!({ "at": at.a1() })).map(|_| ())
    }
    /// Pastes plain text from another app (tab/newline separated) at `at`.
    pub fn paste_text(&mut self, at: Addr, text: &str) -> Result<(), String> {
        self.run("edit.paste", json!({ "at": at.a1(), "text": text })).map(|_| ())
    }
    /// "Paste values only": pastes results, not formulas or formats.
    pub fn paste_values(&mut self, at: Addr) -> Result<(), String> {
        self.select(Area::cell(at))?;
        self.run("edit.pasteSpecial", json!({ "what": "values" })).map(|_| ())
    }
    /// Sets the engine's selection (only needed before commands that act on it).
    pub fn select(&mut self, area: Area) -> Result<(), String> {
        self.run("selection.set", json!({ "range": area.a1() })).map(|_| ())
    }

    // ------------------------------------------------------------------ Cell tab: font

    pub fn set_bold(&mut self, area: Area, on: bool) -> Result<(), String> {
        self.on_range("home.bold", area, json!({ "on": on }))
    }
    pub fn set_italic(&mut self, area: Area, on: bool) -> Result<(), String> {
        self.on_range("home.italic", area, json!({ "on": on }))
    }
    pub fn set_underline(&mut self, area: Area, on: bool) -> Result<(), String> {
        self.on_range("home.underline", area, json!({ "on": on }))
    }
    pub fn set_strikethrough(&mut self, area: Area, on: bool) -> Result<(), String> {
        self.on_range("home.strikethrough", area, json!({ "on": on }))
    }
    pub fn set_font_name(&mut self, area: Area, name: &str) -> Result<(), String> {
        self.on_range("home.fontName", area, json!({ "name": name }))
    }
    /// Font size in points.
    pub fn set_font_size(&mut self, area: Area, size: f32) -> Result<(), String> {
        self.on_range("home.fontSize", area, json!({ "size": size }))
    }
    /// Font colour as `#RRGGBB`, or `None` for automatic.
    pub fn set_font_color(&mut self, area: Area, hex: Option<&str>) -> Result<(), String> {
        self.on_range("home.fontColor", area, json!({ "color": hex.unwrap_or("auto") }))
    }
    /// Fill colour as `#RRGGBB`, or `None` for no fill.
    pub fn set_fill_color(&mut self, area: Area, hex: Option<&str>) -> Result<(), String> {
        self.on_range("home.fillColor", area, json!({ "color": hex.unwrap_or("none") }))
    }

    // ------------------------------------------------------------------ Cell tab: alignment

    pub fn set_align(&mut self, area: Area, h: HAlignChoice) -> Result<(), String> {
        let id = match h {
            HAlignChoice::Left => "home.alignLeft",
            HAlignChoice::Center => "home.alignCenter",
            HAlignChoice::Right => "home.alignRight",
        };
        self.on_range(id, area, json!({}))
    }
    pub fn set_valign(&mut self, area: Area, v: VAlignChoice) -> Result<(), String> {
        let id = match v {
            VAlignChoice::Top => "home.alignTop",
            VAlignChoice::Middle => "home.alignMiddle",
            VAlignChoice::Bottom => "home.alignBottom",
        };
        self.on_range(id, area, json!({}))
    }
    pub fn set_wrap(&mut self, area: Area, on: bool) -> Result<(), String> {
        self.on_range("home.wrapText", area, json!({ "on": on }))
    }
    /// Merge & Center. If more than the top-left cell has content, the engine refuses with a
    /// "confirm:" error (Excel's "only the upper-left value is kept"); pass `force` to go ahead.
    pub fn merge(&mut self, area: Area, force: bool) -> Result<(), String> {
        self.on_range("home.mergeCenter", area, json!({ "overwrite": force }))
    }
    pub fn unmerge(&mut self, area: Area) -> Result<(), String> {
        self.on_range("home.unmergeCells", area, json!({}))
    }
    pub fn set_borders(&mut self, area: Area, preset: BorderPreset) -> Result<(), String> {
        self.on_range("home.borders", area, json!({ "preset": preset.id() }))
    }

    // ------------------------------------------------------------------ Cell tab: numbers

    pub fn set_number_format(&mut self, area: Area, f: &NumberFormat) -> Result<(), String> {
        match f {
            NumberFormat::Accounting(sym) => self.on_range("home.accounting", area, json!({ "symbol": sym })),
            NumberFormat::Percent => self.on_range("home.percent", area, json!({})),
            NumberFormat::Comma => self.on_range("home.comma", area, json!({})),
            NumberFormat::Custom(code) => self.on_range("home.numberFormat", area, json!({ "code": code })),
            other => {
                let name = match other {
                    NumberFormat::General => "General",
                    NumberFormat::Number => "Number",
                    NumberFormat::Currency => "Currency",
                    NumberFormat::ShortDate => "Short Date",
                    NumberFormat::LongDate => "Long Date",
                    NumberFormat::Time => "Time",
                    _ => "Text",
                };
                self.on_range("home.numberFormat", area, json!({ "format": name }))
            }
        }
    }
    /// One more decimal place. The engine starts from the format of the range's first cell
    /// (its "active cell"), so we select the range first.
    pub fn increase_decimals(&mut self, area: Area) -> Result<(), String> {
        self.select(area)?;
        self.on_range("home.increaseDecimal", area, json!({}))
    }
    pub fn decrease_decimals(&mut self, area: Area) -> Result<(), String> {
        self.select(area)?;
        self.on_range("home.decreaseDecimal", area, json!({}))
    }

    // ------------------------------------------------------------------ rows, columns, sizes

    /// Inserts `count` empty rows before row `at` (0-based). Formulas below are adjusted.
    pub fn insert_rows(&mut self, at: u32, count: u32) -> Result<(), String> {
        let (a, b) = (at + 1, at + count.max(1));
        self.run("home.insertRows", json!({ "rows": format!("{a}:{b}") })).map(|_| ())
    }
    pub fn delete_rows(&mut self, at: u32, count: u32) -> Result<(), String> {
        let (a, b) = (at + 1, at + count.max(1));
        self.run("home.deleteRows", json!({ "rows": format!("{a}:{b}") })).map(|_| ())
    }
    /// Inserts `count` empty columns before column `at` (0-based).
    pub fn insert_cols(&mut self, at: u32, count: u32) -> Result<(), String> {
        let (a, b) = (col_letters(at), col_letters(at + count.max(1) - 1));
        self.run("home.insertColumns", json!({ "cols": format!("{a}:{b}") })).map(|_| ())
    }
    pub fn delete_cols(&mut self, at: u32, count: u32) -> Result<(), String> {
        let (a, b) = (col_letters(at), col_letters(at + count.max(1) - 1));
        self.run("home.deleteColumns", json!({ "cols": format!("{a}:{b}") })).map(|_| ())
    }
    /// Column width in pixels (at 100 %), e.g. after dragging a header edge.
    pub fn set_col_width(&mut self, col: u32, px: f32) -> Result<(), String> {
        let c = col_letters(col);
        self.run("home.columnWidth", json!({ "cols": format!("{c}:{c}"), "width": px })).map(|_| ())
    }
    /// Row height in pixels (at 100 %).
    pub fn set_row_height(&mut self, row: u32, px: f32) -> Result<(), String> {
        let r = row + 1;
        self.run("home.rowHeight", json!({ "rows": format!("{r}:{r}"), "height": px })).map(|_| ())
    }
    pub fn autofit_col(&mut self, col: u32) -> Result<(), String> {
        let c = col_letters(col);
        self.run("home.autofitColumnWidth", json!({ "cols": format!("{c}:{c}") })).map(|_| ())
    }

    // ------------------------------------------------------------------ sheets

    /// Adds a sheet at the end (or `None` name for "Sheet2", "Sheet3"...). Returns its index.
    pub fn add_sheet(&mut self, name: Option<&str>) -> Result<usize, String> {
        let before = self.sheet_count();
        let mut p = json!({ "before": before });
        if let (Some(n), Some(obj)) = (name, p.as_object_mut()) {
            obj.insert("name".into(), Json::String(n.to_string()));
        }
        self.run("home.insertSheet", p)?;
        Ok(self.active_sheet())
    }
    /// Deletes a sheet (refused for the last visible sheet).
    pub fn delete_sheet(&mut self, index: usize) -> Result<(), String> {
        self.run("home.deleteSheet", json!({ "sheet": index, "overwrite": true })).map(|_| ())
    }
    pub fn rename_sheet(&mut self, index: usize, name: &str) -> Result<(), String> {
        self.run("sheet.rename", json!({ "sheet": index, "name": name })).map(|_| ())
    }

    // ------------------------------------------------------------------ Formula tab

    /// AutoSum: `function` is SUM, AVERAGE, COUNT, MAX or MIN. With a range of numbers, the
    /// result goes in the cell below (or right of) it, as in Excel.
    pub fn autosum(&mut self, area: Area, function: &str) -> Result<(), String> {
        self.select(area)?;
        self.on_range("formulas.autoSum", area, json!({ "function": function.to_ascii_uppercase() }))
    }
    /// Functions for "insert function", optionally one category id from
    /// [`FUNCTION_CATEGORIES`] (e.g. "MathTrig") and/or a search word.
    pub fn functions(&mut self, category: Option<&str>, search: Option<&str>) -> Result<Vec<FunctionInfo>, String> {
        let mut p = json!({});
        if let Some(obj) = p.as_object_mut() {
            if let Some(c) = category {
                obj.insert("category".into(), Json::String(c.to_string()));
            }
            if let Some(s) = search {
                obj.insert("search".into(), Json::String(s.to_string()));
            }
        }
        let r = self.run("formulas.functions", p)?;
        let list = r.as_array().or_else(|| r.get("functions").and_then(Json::as_array)).cloned().unwrap_or_default();
        let s = |v: &Json, k: &str| v.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
        Ok(list
            .iter()
            .map(|f| FunctionInfo { name: s(f, "name"), category: s(f, "category"), signature: s(f, "signature"), description: s(f, "description") })
            .collect())
    }
    /// Functions most recently used in formulas, newest first.
    pub fn recent_functions(&mut self) -> Result<Vec<String>, String> {
        let r = self.run("formulas.recentlyUsed", json!({}))?;
        let list = r.as_array().or_else(|| r.get("functions").and_then(Json::as_array)).cloned().unwrap_or_default();
        Ok(list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string).or_else(|| v.get("name").and_then(Json::as_str).map(str::to_string)))
            .collect())
    }

    // ------------------------------------------------------------------ Data tab

    /// Sort A→Z (or Z→A) by one column of `area`. The engine detects a header row.
    pub fn sort(&mut self, area: Area, column: u32, descending: bool) -> Result<(), String> {
        let id = if descending { "data.sortDescending" } else { "data.sortAscending" };
        self.on_range(id, area, json!({ "column": col_letters(column) }))
    }
    /// Custom sort by several keys. `header`: the first row holds titles and stays on top.
    pub fn sort_by(&mut self, area: Area, keys: &[SortKey], header: bool) -> Result<(), String> {
        let keys: Vec<Json> = keys.iter().map(|k| json!({ "column": k.column, "order": if k.descending { "desc" } else { "asc" } })).collect();
        self.on_range("data.sort", area, json!({ "keys": keys, "header": header }))
    }
    /// Turns the AutoFilter on/off for the table around `area`.
    pub fn toggle_filter(&mut self, area: Area) -> Result<(), String> {
        self.select(area)?;
        self.on_range("data.filter", area, json!({}))
    }
    /// Shows only rows whose `column` holds one of `values` (filter must be on).
    pub fn filter_values(&mut self, column: u32, values: &[&str]) -> Result<(), String> {
        self.run("data.filterBy", json!({ "column": col_letters(column), "values": values })).map(|_| ())
    }
    /// Removes duplicate rows in `area`. Returns the engine's summary (e.g. removed count).
    pub fn remove_duplicates(&mut self, area: Area, header: bool) -> Result<Json, String> {
        self.run("data.removeDuplicates", json!({ "range": area.a1(), "header": header }))
    }
    pub fn freeze_top_row(&mut self) -> Result<(), String> {
        self.run("view.freezeTopRow", json!({})).map(|_| ())
    }
    pub fn freeze_first_column(&mut self) -> Result<(), String> {
        self.run("view.freezeFirstColumn", json!({})).map(|_| ())
    }
    /// Freezes the rows above and the columns left of `at`.
    pub fn freeze_panes(&mut self, at: Addr) -> Result<(), String> {
        self.run("view.freezePanes", json!({ "cell": at.a1() })).map(|_| ())
    }
    pub fn unfreeze(&mut self) -> Result<(), String> {
        self.run("view.unfreezePanes", json!({})).map(|_| ())
    }
    /// Imports CSV text into a new sheet (Data › Import CSV).
    pub fn import_csv(&mut self, text: &str, sheet_name: Option<&str>) -> Result<(), String> {
        let mut p = json!({ "text": text });
        if let (Some(n), Some(obj)) = (sheet_name, p.as_object_mut()) {
            obj.insert("sheet".into(), Json::String(n.to_string()));
        }
        self.run("data.fromTextCsv", p).map(|_| ())
    }

    // ------------------------------------------------------------------ search

    /// Finds every cell on the active sheet whose shown text contains `what` (any case).
    pub fn find_all(&mut self, what: &str) -> Result<Vec<Addr>, String> {
        let r = self.run("edit.find", json!({ "what": what, "all": true, "lookIn": "values", "within": "sheet" }))?;
        let list = r.get("results").and_then(Json::as_array).cloned().unwrap_or_default();
        Ok(list.iter().filter_map(|h| h.get("cell").and_then(Json::as_str)).filter_map(Addr::parse).collect())
    }
    /// Replaces every `what` with `with` on the active sheet. Returns how many were replaced.
    pub fn replace_all(&mut self, what: &str, with: &str) -> Result<u64, String> {
        let r = self.run("edit.replace", json!({ "what": what, "with": with, "all": true, "within": "sheet" }))?;
        Ok(r.get("replaced").and_then(Json::as_u64).unwrap_or(0))
    }

    // ------------------------------------------------------------------ Insert tab

    /// Inserts a chart of `area`'s data. `kind`: column, bar, line or pie (others work too).
    pub fn insert_chart(&mut self, area: Area, kind: &str, title: Option<&str>) -> Result<(), String> {
        let mut p = json!({ "type": kind });
        if let (Some(t), Some(obj)) = (title, p.as_object_mut()) {
            obj.insert("title".into(), Json::String(t.to_string()));
        }
        self.on_range("insert.chart", area, p)
    }
    /// Inserts a picture (PNG/JPEG bytes) with its top-left corner at `at`. `width_px` sets the
    /// width (height keeps the aspect ratio); phone photos are huge otherwise.
    pub fn insert_picture(&mut self, at: Addr, bytes: &[u8], mime: &str, width_px: Option<f32>) -> Result<(), String> {
        let mut p = json!({ "base64": gridcraft_engine::io::base64_encode(bytes), "mime": mime, "at": at.a1() });
        if let (Some(w), Some((iw, ih))) = (width_px, gridcraft_engine::io::image_size(bytes))
            && iw > 0
            && let Some(obj) = p.as_object_mut()
        {
            obj.insert("width".into(), json!(w));
            obj.insert("height".into(), json!(w * ih as f32 / iw as f32));
        }
        self.run("insert.picture", p).map(|_| ())
    }
    /// Adds a hyperlink to a cell (`url` may also be "Sheet2!A1"). `text` replaces the shown text.
    pub fn insert_link(&mut self, at: Addr, url: &str, text: Option<&str>) -> Result<(), String> {
        let mut p = json!({ "cell": at.a1(), "target": url });
        if let (Some(t), Some(obj)) = (text, p.as_object_mut()) {
            obj.insert("text".into(), Json::String(t.to_string()));
        }
        self.run("insert.link", p).map(|_| ())
    }
    /// Adds a note (the classic yellow comment box).
    pub fn add_note(&mut self, at: Addr, text: &str) -> Result<(), String> {
        self.run("review.newNote", json!({ "cell": at.a1(), "text": text })).map(|_| ())
    }
    /// Adds a threaded comment.
    pub fn add_comment(&mut self, at: Addr, text: &str, author: &str) -> Result<(), String> {
        self.run("review.newComment", json!({ "cell": at.a1(), "text": text, "author": author })).map(|_| ())
    }

    // ------------------------------------------------------------------ File tab: page setup

    pub fn set_landscape(&mut self, landscape: bool) -> Result<(), String> {
        self.run("pageLayout.orientation", json!({ "orientation": if landscape { "landscape" } else { "portrait" } })).map(|_| ())
    }
    /// Paper: "A4", "Letter", "Legal", "A3", "A5"...
    pub fn set_paper(&mut self, paper: &str) -> Result<(), String> {
        self.run("pageLayout.size", json!({ "paper": paper })).map(|_| ())
    }
    /// Margins preset: "normal", "narrow" or "wide".
    pub fn set_margins(&mut self, preset: &str) -> Result<(), String> {
        self.run("pageLayout.margins", json!({ "preset": preset })).map(|_| ())
    }
    /// Fit to page: `Some((w, h))` = fit on w pages wide by h tall (0 = automatic); `None` = 100 %.
    pub fn set_fit_to_page(&mut self, fit: Option<(u32, u32)>) -> Result<(), String> {
        let p = match fit {
            Some((w, h)) => json!({ "width": w, "height": h }),
            None => json!({ "scale": 100, "width": 0, "height": 0 }),
        };
        self.run("pageLayout.scaleToFit", p).map(|_| ())
    }
    /// Print area: `Some(area)` sets it, `None` clears it.
    pub fn set_print_area(&mut self, area: Option<Area>) -> Result<(), String> {
        let p = match area {
            Some(a) => json!({ "range": a.a1() }),
            None => json!({ "clear": true }),
        };
        self.run("pageLayout.printArea", p).map(|_| ())
    }
    pub fn set_print_gridlines(&mut self, on: bool) -> Result<(), String> {
        self.run("pageLayout.printGridlines", json!({ "on": on })).map(|_| ())
    }
}
