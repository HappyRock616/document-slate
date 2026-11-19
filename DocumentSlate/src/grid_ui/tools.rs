//! Grid Slate edit mode: the bottom bar (approved list: docs/GridSlate-features.md). An icon tool
//! row for the current tab above an icon tab strip: Cell · Formula · Data · Insert · File, in the
//! same style as Word Slate's (src/edit_bar.rs). Less-used choices sit in pop-up menus; things
//! that need typing (find & replace, link, note, sheet name, function search) open an input row.

use eframe::egui::{self, Color32, RichText};
use slate_grid::{BorderPreset, FUNCTION_CATEGORIES, HAlignChoice, NumberFormat, SortKey, VAlignChoice};

use super::icons::{self, tool, tool_menu};
use super::{GridScreen, Row, Tab};
use crate::chrome;
use crate::edit_bar::FileAction;

/// (tab, icon, label).
const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Cell, "g.cell", "Cell"),
    (Tab::Formula, "g.fx", "Formula"),
    (Tab::Data, "sort", "Data"),
    (Tab::Insert, "g.insert", "Insert"),
    (Tab::File, "file", "File"),
];

/// Fonts offered in Cell › Font (common in workbooks people receive; drawn with the bundled
/// look-alikes, the file keeps the name).
const FONTS: [&str; 8] = ["Calibri", "Aptos", "Arial", "Verdana", "Times New Roman", "Cambria", "Georgia", "Courier New"];
const SIZES: [f32; 13] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0];
/// Text and fill colours, RRGGBB (our own picks).
const TEXT_COLOURS: [&str; 10] = ["000000", "595959", "C00000", "E36C09", "C9A100", "00A050", "2F6DB5", "1F3864", "7030A0", "FFFFFF"];
const FILL_COLOURS: [&str; 15] = [
    "FFF2CC", "FCE4D6", "E2EFDA", "DDEBF7", "EDE7F6", "F2F2F2", "FFFF00", "F4B183", "A9D08E", "9DC3E6", "D9D9D9", "C00000", "2E8B57", "2F6DB5", "404040",
];

/// A grid of colour circles; returns the tapped one's RRGGBB.
fn swatches<'a>(ui: &mut egui::Ui, colours: &[&'a str]) -> Option<&'a str> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(5.0 * 44.0);
        for hex in colours {
            let c = u32::from_str_radix(hex, 16).unwrap_or(0);
            let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
            let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
            ui.painter().circle_filled(r.center(), 15.0, col);
            ui.painter().circle_stroke(r.center(), 15.0, egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.4)));
            if resp.clicked() {
                picked = Some(*hex);
            }
        }
    });
    picked
}

fn tick(on: bool, label: &str) -> String {
    // (✔ has no glyph in the bundled UI fonts; a bullet does.)
    if on { format!("•  {label}") } else { format!("    {label}") }
}

impl GridScreen {
    /// The whole bottom bar: tool row, the open input row, tab strip.
    pub(super) fn tools(&mut self, ui: &mut egui::Ui, accent: Color32) -> Option<FileAction> {
        let mut action = None;
        let out = egui::ScrollArea::horizontal().id_salt("grid_tools").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                match self.tab {
                    Tab::Cell => self.cell_tools(ui, accent),
                    Tab::Formula => self.formula_tools(ui, accent),
                    Tab::Data => self.data_tools(ui, accent),
                    Tab::Insert => action = self.insert_tools(ui, accent),
                    Tab::File => action = self.file_tools(ui, accent),
                }
            });
        });
        chrome::scroll_hints(ui, &out);
        if self.row != Row::None {
            ui.separator();
            self.input_row(ui, accent);
        }
        // Tab strip (as Word's): icon + label, the current one in the accent colour.
        let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::hover());
        let w = strip.width() / TABS.len() as f32;
        for (i, (t, icon, label)) in TABS.into_iter().enumerate() {
            let r = egui::Rect::from_min_size(egui::pos2(strip.left() + i as f32 * w, strip.top()), egui::vec2(w, strip.height()));
            let resp = ui.interact(r, ui.scope_id().with(("grid-tab", i)), egui::Sense::click());
            let selected = self.tab == t;
            let fg = if selected { accent } else { ui.visuals().text_color().gamma_multiply(0.7) };
            let p = ui.painter();
            icons::icon(p, egui::Rect::from_center_size(egui::pos2(r.center().x, r.top() + 20.0), egui::Vec2::splat(chrome::ICON)), icon, fg, accent);
            p.text(egui::pos2(r.center().x, r.bottom() - 12.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.5), fg);
            if selected {
                let bar = egui::Rect::from_center_size(egui::pos2(r.center().x, r.bottom() - 2.0), egui::vec2(28.0, 3.0));
                p.rect_filled(bar, 1.5, accent);
            }
            if resp.clicked() && self.tab != t {
                self.tab = t;
                self.row = Row::None;
            }
        }
        action
    }

    /// Cell tab: font, size, B/I/U, colours | alignment, wrap, merge, borders | number formats |
    /// strike, clear formats, row/column size.
    fn cell_tools(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let info = self.doc.cell(self.sheet(), self.sel.first);
        let t = self.target();
        let (bold, italic, underline, strike, wrap, merged) =
            info.as_ref().map(|c| (c.bold, c.italic, c.underline, c.strike, c.wrap, c.merged.is_some())).unwrap_or_default();
        let font = info.as_ref().map(|c| c.font_name.clone()).unwrap_or_default();
        let size = info.as_ref().map(|c| c.font_size).unwrap_or(11.0);
        tool_menu(ui, "fonts", "Font", accent, |ui| {
            for f in FONTS {
                if chrome::menu_item(ui, &tick(font == f, f)) {
                    let r = self.doc.set_font_name(t, f);
                    self.act(r);
                    ui.close();
                }
            }
        });
        tool_menu(ui, "size", &format!("{size} pt"), accent, |ui| {
            for s in SIZES {
                if chrome::menu_item(ui, &tick((size - s).abs() < 0.01, &format!("{s} pt"))) {
                    let r = self.doc.set_font_size(t, s);
                    self.act(r);
                    ui.close();
                }
            }
        });
        if tool(ui, "bold", "Bold", bold, accent).clicked() {
            let r = self.doc.set_bold(t, !bold);
            self.act(r);
        }
        if tool(ui, "italic", "Italic", italic, accent).clicked() {
            let r = self.doc.set_italic(t, !italic);
            self.act(r);
        }
        if tool(ui, "underline", "Underline", underline, accent).clicked() {
            let r = self.doc.set_underline(t, !underline);
            self.act(r);
        }
        tool_menu(ui, "fontcolor", "Color", accent, |ui| {
            if chrome::menu_item(ui, "Automatic") {
                let r = self.doc.set_font_color(t, None);
                self.act(r);
                ui.close();
            }
            if let Some(hex) = swatches(ui, &TEXT_COLOURS) {
                let r = self.doc.set_font_color(t, Some(&format!("#{hex}")));
                self.act(r);
                ui.close();
            }
        });
        tool_menu(ui, "g.fill", "Fill", accent, |ui| {
            if chrome::menu_item(ui, "No fill") {
                let r = self.doc.set_fill_color(t, None);
                self.act(r);
                ui.close();
            }
            if let Some(hex) = swatches(ui, &FILL_COLOURS) {
                let r = self.doc.set_fill_color(t, Some(&format!("#{hex}")));
                self.act(r);
                ui.close();
            }
        });
        chrome::divider(ui);
        for (icon, label, h) in [("alignLeft", "Left", HAlignChoice::Left), ("alignCenter", "Center", HAlignChoice::Center), ("alignRight", "Right", HAlignChoice::Right)] {
            if tool(ui, icon, label, false, accent).clicked() {
                let r = self.doc.set_align(t, h);
                self.act(r);
            }
        }
        tool_menu(ui, "g.alignMiddle", "Vertical", accent, |ui| {
            for (label, v) in [("Top", VAlignChoice::Top), ("Middle", VAlignChoice::Middle), ("Bottom", VAlignChoice::Bottom)] {
                if chrome::menu_item(ui, label) {
                    let r = self.doc.set_valign(t, v);
                    self.act(r);
                    ui.close();
                }
            }
        });
        if tool(ui, "g.wrap", "Wrap", wrap, accent).clicked() {
            let r = self.doc.set_wrap(t, !wrap);
            self.act(r);
        }
        if tool(ui, "g.merge", if merged { "Unmerge" } else { "Merge" }, merged, accent).clicked() {
            if merged {
                let area = info.as_ref().and_then(|c| c.merged).unwrap_or(t);
                let r = self.doc.unmerge(area);
                self.act(r);
            } else if t.first == t.last {
                self.say("Select two or more cells to merge (drag the round handle)");
            } else if self.merge_would_clear(t) {
                // More than the upper-left cell holds something: ask first (the engine merges
                // without asking and keeps only that one value).
                self.merge_pending = Some(t);
                self.row = Row::ConfirmMerge;
            } else {
                let r = self.doc.merge(t, true);
                self.act(r);
            }
        }
        tool_menu(ui, "borders", "Borders", accent, |ui| {
            for (label, b) in [
                ("All borders", BorderPreset::All),
                ("Outside", BorderPreset::Outside),
                ("Thick outside", BorderPreset::ThickOutside),
                ("Inside", BorderPreset::Inside),
                ("Bottom", BorderPreset::Bottom),
                ("Top", BorderPreset::Top),
                ("Left", BorderPreset::Left),
                ("Right", BorderPreset::Right),
                ("Double bottom", BorderPreset::DoubleBottom),
                ("No borders", BorderPreset::None),
            ] {
                if chrome::menu_item(ui, label) {
                    let r = self.doc.set_borders(t, b);
                    self.act(r);
                    ui.close();
                }
            }
        });
        chrome::divider(ui);
        let code = info.as_ref().map(|c| c.number_format.clone()).unwrap_or_default();
        tool_menu(ui, "g.numfmt", "Number", accent, |ui| {
            ui.label(RichText::new(format!("Now: {code}")).weak());
            for (label, f) in [
                ("General", NumberFormat::General),
                ("Number (1234.50)", NumberFormat::Number),
                ("Currency", NumberFormat::Currency),
                ("Accounting €", NumberFormat::Accounting("€".into())),
                ("Accounting $", NumberFormat::Accounting("$".into())),
                ("Accounting £", NumberFormat::Accounting("£".into())),
                ("Percent (%)", NumberFormat::Percent),
                ("Comma style (1,234.50)", NumberFormat::Comma),
                ("Short date", NumberFormat::ShortDate),
                ("Long date", NumberFormat::LongDate),
                ("Time", NumberFormat::Time),
                ("Text", NumberFormat::Text),
            ] {
                if chrome::menu_item(ui, label) {
                    let r = self.doc.set_number_format(t, &f);
                    self.act(r);
                    ui.close();
                }
            }
        });
        if tool(ui, "g.decInc", "More .0", false, accent).clicked() {
            let r = self.doc.increase_decimals(t);
            self.act(r);
        }
        if tool(ui, "g.decDec", "Fewer .0", false, accent).clicked() {
            let r = self.doc.decrease_decimals(t);
            self.act(r);
        }
        chrome::divider(ui);
        if tool(ui, "strike", "Strike", strike, accent).clicked() {
            let r = self.doc.set_strikethrough(t, !strike);
            self.act(r);
        }
        if tool(ui, "clear", "Clear fmt", false, accent).clicked() {
            let r = self.doc.clear_formats(t);
            self.act(r);
        }
        let sheet = self.sheet();
        tool_menu(ui, "g.size", "Size", accent, |ui| {
            let col_w = self.doc.cell_rect(sheet, slate_grid::Addr::new(0, t.first.col)).map(|r| r.2).unwrap_or(64.0);
            let row_h = self.doc.cell_rect(sheet, slate_grid::Addr::new(t.first.row, 0)).map(|r| r.3).unwrap_or(20.0);
            let mut res = Ok(());
            let mut done = true;
            if chrome::menu_item(ui, "Autofit column width") {
                for c in t.first.col..=t.last.col.min(t.first.col + 200) {
                    res = res.and(self.doc.autofit_col(c));
                }
            } else if chrome::menu_item(ui, "Wider column") {
                res = (t.first.col..=t.last.col.min(t.first.col + 200)).try_for_each(|c| self.doc.set_col_width(c, col_w + 24.0));
            } else if chrome::menu_item(ui, "Narrower column") {
                res = (t.first.col..=t.last.col.min(t.first.col + 200)).try_for_each(|c| self.doc.set_col_width(c, (col_w - 24.0).max(12.0)));
            } else if chrome::menu_item(ui, "Taller row") {
                res = (t.first.row..=t.last.row.min(t.first.row + 1000)).try_for_each(|r| self.doc.set_row_height(r, row_h + 10.0));
            } else if chrome::menu_item(ui, "Shorter row") {
                res = (t.first.row..=t.last.row.min(t.first.row + 1000)).try_for_each(|r| self.doc.set_row_height(r, (row_h - 10.0).max(8.0)));
            } else {
                done = false;
            }
            if done {
                self.act(res);
                ui.close();
            }
        });
    }

    /// The functions of one category (cached: the list doesn't change).
    fn functions_of(&mut self, key: &str, category: Option<&str>, search: Option<&str>) -> Vec<slate_grid::FunctionInfo> {
        if let Some(v) = self.fn_cache.get(key) {
            return v.clone();
        }
        let list = self.doc.functions(category, search).unwrap_or_default();
        self.fn_cache.insert(key.to_owned(), list.clone());
        list
    }

    /// A menu body listing functions (the popular ones of the category first); tapping one
    /// inserts it.
    fn function_list(&mut self, ui: &mut egui::Ui, list: &[slate_grid::FunctionInfo], popular: &[&str]) {
        if list.is_empty() {
            ui.label(RichText::new("No functions").weak());
        }
        let top: Vec<&slate_grid::FunctionInfo> = popular.iter().filter_map(|p| list.iter().find(|f| f.name == *p)).collect();
        if !top.is_empty() {
            ui.label(RichText::new("Popular").strong());
            for f in &top {
                self.function_button(ui, f);
            }
            ui.separator();
            ui.label(RichText::new("All").strong());
        }
        for f in list {
            self.function_button(ui, f);
        }
    }

    fn function_button(&mut self, ui: &mut egui::Ui, f: &slate_grid::FunctionInfo) {
        ui.set_max_width(300.0);
        let resp = ui.add(egui::Button::new(RichText::new(&f.name).strong()).min_size(egui::vec2(ui.available_width(), 36.0)));
        let about = if f.description.is_empty() { &f.signature } else { &f.description };
        if !about.is_empty() {
            let short: String = about.chars().take(70).collect();
            ui.label(RichText::new(if short.len() < about.len() { format!("{short}…") } else { short }).small().weak());
        }
        if resp.clicked() {
            self.insert_function(&f.name);
            ui.close();
        }
    }

    /// Formula tab: AutoSum (+ average, count, max, min) | recent | the six categories, more,
    /// search.
    fn formula_tools(&mut self, ui: &mut egui::Ui, accent: Color32) {
        if tool(ui, "g.sum", "AutoSum", false, accent).clicked() {
            self.autosum("SUM");
        }
        tool_menu(ui, "g.sum", "More Σ", accent, |ui| {
            for (label, f) in [("Average", "AVERAGE"), ("Count numbers", "COUNT"), ("Max", "MAX"), ("Min", "MIN"), ("Sum", "SUM")] {
                if chrome::menu_item(ui, label) {
                    self.autosum(f);
                    ui.close();
                }
            }
        });
        tool_menu(ui, "g.recent", "Recent", accent, |ui| {
            let recent = self.doc.recent_functions().unwrap_or_default();
            if recent.is_empty() {
                ui.label(RichText::new("Functions you use show here").weak());
            }
            for name in recent.iter().take(12) {
                if chrome::menu_item(ui, name) {
                    self.insert_function(name);
                    ui.close();
                }
            }
        });
        chrome::divider(ui);
        for (id, label) in FUNCTION_CATEGORIES.iter().take(6) {
            let short = match *id {
                "DateTime" => "Date",
                "Financial" => "Money",
                _ => label,
            };
            let popular: &[&str] = match *id {
                "MathTrig" => &["SUM", "SUMIF", "ROUND", "PRODUCT", "ABS", "INT", "MOD", "SUMPRODUCT"],
                "Text" => &["CONCAT", "TEXTJOIN", "LEFT", "RIGHT", "MID", "LEN", "TRIM", "UPPER", "TEXT"],
                "DateTime" => &["TODAY", "NOW", "DATE", "YEAR", "MONTH", "DAY", "DATEDIF", "WEEKDAY", "EDATE"],
                "Logical" => &["IF", "IFS", "AND", "OR", "NOT", "IFERROR"],
                "Lookup" => &["XLOOKUP", "VLOOKUP", "INDEX", "MATCH", "HLOOKUP", "FILTER", "UNIQUE"],
                "Financial" => &["PMT", "FV", "PV", "NPV", "IRR", "RATE"],
                _ => &[],
            };
            tool_menu(ui, "g.cat", short, accent, |ui| {
                let list = self.functions_of(id, Some(id), None);
                self.function_list(ui, &list, popular);
            });
        }
        tool_menu(ui, "g.cat", "More", accent, |ui| {
            for (id, label) in FUNCTION_CATEGORIES.iter().skip(6) {
                ui.label(RichText::new(*label).strong());
                let list = self.functions_of(id, Some(id), None);
                self.function_list(ui, &list, &[]);
                ui.separator();
            }
        });
        if tool(ui, "search", "Find fx", self.row == Row::FnSearch, accent).clicked() {
            self.row = if self.row == Row::FnSearch { Row::None } else { Row::FnSearch };
        }
    }

    fn autosum(&mut self, f: &str) {
        self.commit();
        let t = self.target();
        let r = self.doc.autosum(t, f);
        self.act(r);
        self.input_for = None;
    }

    /// Data tab: sort | filter | duplicates | freeze | find & replace.
    fn data_tools(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let sheet = self.sheet();
        let col = self.sel.first.col;
        for (icon, label, desc) in [("g.sortAsc", "A to Z", false), ("g.sortDesc", "Z to A", true)] {
            if tool(ui, icon, label, false, accent).clicked() {
                self.commit();
                let area = self.data_area();
                let r = self.doc.sort(area, col, desc);
                if self.act(r).is_some() {
                    self.say(format!("Sorted {} by column {}", area.a1(), super::paint::col_name(col)));
                }
            }
        }
        tool_menu(ui, "g.sortCustom", "Custom", accent, |ui| {
            let area = self.data_area();
            ui.checkbox(&mut self.sort_header, "First row has titles");
            ui.label(RichText::new(format!("Sorting {}", area.a1())).weak());
            for c in area.first.col..=area.last.col.min(area.first.col + 30) {
                let letters = super::paint::col_name(c);
                let title = if self.sort_header { self.doc.cell_text(sheet, slate_grid::Addr::new(area.first.row, c)) } else { String::new() };
                let name = if title.is_empty() { format!("Column {letters}") } else { format!("{letters} · {title}") };
                for (arrow, desc) in [("A to Z", false), ("Z to A", true)] {
                    if chrome::menu_item(ui, &format!("{name}  {arrow}")) {
                        let r = self.doc.sort_by(area, &[SortKey { column: letters.clone(), descending: desc }], self.sort_header);
                        self.act(r);
                        ui.close();
                    }
                }
            }
        });
        chrome::divider(ui);
        let filter = self.doc.filter_range(sheet);
        if tool(ui, "g.filter", "Filter", filter.is_some(), accent).clicked() {
            let area = filter.unwrap_or_else(|| self.data_area());
            let r = self.doc.toggle_filter(area);
            self.act(r);
        }
        tool_menu(ui, "g.filterCol", "Filter by", accent, |ui| {
            let Some(f) = filter else {
                ui.label(RichText::new("Turn the filter on first").weak());
                return;
            };
            if col < f.first.col || col > f.last.col {
                ui.label(RichText::new("Select a cell in the filtered table").weak());
                return;
            }
            ui.label(RichText::new(format!("Column {}: show only", super::paint::col_name(col))).weak());
            if chrome::menu_item(ui, "Show all") {
                let r = self.doc.run("data.filterBy", serde_json::json!({ "column": super::paint::col_name(col), "clear": true }));
                self.act(r);
                ui.close();
            }
            for v in self.doc.column_values(sheet, col, f, 60) {
                if chrome::menu_item(ui, &v) {
                    let r = self.doc.filter_values(col, &[v.as_str()]);
                    self.act(r);
                    ui.close();
                }
            }
            ui.separator();
            if chrome::menu_item(ui, "Clear all filters") {
                let r = self.doc.clear_filter();
                self.act(r);
                ui.close();
            }
        });
        tool_menu(ui, "g.duplicates", "Duplicates", accent, |ui| {
            for (label, header) in [("Remove duplicates (first row = titles)", true), ("Remove duplicates (no titles)", false)] {
                if chrome::menu_item(ui, label) {
                    let area = self.data_area();
                    let r = self.doc.remove_duplicates(area, header);
                    if let Some(j) = self.act(r) {
                        let n = j.get("removed").and_then(serde_json::Value::as_u64).unwrap_or(0);
                        self.say(format!("Removed {n} duplicate row{}", if n == 1 { "" } else { "s" }));
                    }
                    ui.close();
                }
            }
        });
        chrome::divider(ui);
        tool_menu(ui, "g.freeze", "Freeze", accent, |ui| {
            let at = self.sel.first;
            let mut r = None;
            if chrome::menu_item(ui, "Freeze top row") {
                r = Some(self.doc.freeze_top_row());
            } else if chrome::menu_item(ui, "Freeze first column") {
                r = Some(self.doc.freeze_first_column());
            } else if chrome::menu_item(ui, &format!("Freeze above and left of {}", at.a1())) {
                r = Some(self.doc.freeze_panes(at));
            } else if chrome::menu_item(ui, "Unfreeze") {
                r = Some(self.doc.unfreeze());
            }
            if let Some(r) = r {
                self.act(r);
                self.scroll = egui::Vec2::ZERO;
                ui.close();
            }
        });
        if tool(ui, "replace", "Replace", self.row == Row::Replace, accent).clicked() {
            self.row = if self.row == Row::Replace { Row::None } else { Row::Replace };
        }
    }

    /// Insert tab: rows, columns, sheets | chart, picture, link, note.
    fn insert_tools(&mut self, ui: &mut egui::Ui, accent: Color32) -> Option<FileAction> {
        let mut action = None;
        let t = self.target();
        tool_menu(ui, "g.rows", "Rows", accent, |ui| {
            let n = t.rows().min(1000);
            let mut r = None;
            if chrome::menu_item(ui, &format!("Insert {n} row(s) above")) {
                r = Some(self.doc.insert_rows(t.first.row, n));
            } else if chrome::menu_item(ui, &format!("Insert {n} row(s) below")) {
                r = Some(self.doc.insert_rows(t.last.row + 1, n));
            } else if chrome::menu_item(ui, "Delete selected row(s)") {
                r = Some(if self.sel.rows() > 10_000 { Err("Select rows first (tap a row number)".into()) } else { self.doc.delete_rows(self.sel.first.row, self.sel.rows()) });
            }
            if let Some(r) = r {
                self.commit();
                self.act(r);
                ui.close();
            }
        });
        tool_menu(ui, "g.cols", "Columns", accent, |ui| {
            let n = t.cols().min(200);
            let mut r = None;
            if chrome::menu_item(ui, &format!("Insert {n} column(s) left")) {
                r = Some(self.doc.insert_cols(t.first.col, n));
            } else if chrome::menu_item(ui, &format!("Insert {n} column(s) right")) {
                r = Some(self.doc.insert_cols(t.last.col + 1, n));
            } else if chrome::menu_item(ui, "Delete selected column(s)") {
                r = Some(if self.sel.cols() > 1000 { Err("Select columns first (tap a column letter)".into()) } else { self.doc.delete_cols(self.sel.first.col, self.sel.cols()) });
            }
            if let Some(r) = r {
                self.commit();
                self.act(r);
                ui.close();
            }
        });
        tool_menu(ui, "g.sheet", "Sheet", accent, |ui| {
            if chrome::menu_item(ui, "Add sheet") {
                self.commit();
                self.per_sheet.insert(self.shown_sheet, (self.scroll, self.sel));
                let r = self.doc.add_sheet(None);
                self.act(r);
                self.arrive_on_sheet();
                ui.close();
            }
            if chrome::menu_item(ui, "Rename sheet…") {
                self.fields.rename = self.doc.sheet_names().get(self.sheet()).cloned().unwrap_or_default();
                self.row = Row::Rename;
                ui.close();
            }
            if chrome::menu_item(ui, "Delete sheet") {
                if self.visible_sheets().len() <= 1 {
                    self.say("A workbook needs at least one sheet");
                } else {
                    self.commit();
                    let s = self.sheet();
                    self.per_sheet.clear();
                    let r = self.doc.delete_sheet(s);
                    if self.act(r).is_some() {
                        self.say("Sheet deleted (Undo brings it back)");
                    }
                    self.arrive_on_sheet();
                }
                ui.close();
            }
        });
        chrome::divider(ui);
        tool_menu(ui, "chart", "Chart", accent, |ui| {
            for (label, kind) in [("Column", "column"), ("Bar", "bar"), ("Line", "line"), ("Pie", "pie")] {
                if chrome::menu_item(ui, label) {
                    self.commit();
                    let area = self.data_area();
                    let r = self.doc.insert_chart(area, kind, None);
                    if self.act(r).is_some() {
                        self.say(format!("{label} chart of {}", area.a1()));
                    }
                    ui.close();
                }
            }
        });
        if tool(ui, "picture", "Picture", false, accent).clicked() {
            action = Some(FileAction::PickImage);
        }
        if tool(ui, "qr", "Barcode", false, accent).clicked() {
            action = Some(FileAction::InsertBarcode);
        }
        if tool(ui, "link", "Link", self.row == Row::Link, accent).clicked() {
            let info = self.doc.cell(self.sheet(), self.sel.first);
            self.fields.url = info.as_ref().and_then(|c| c.link.clone()).unwrap_or_default();
            self.fields.link_text = info.map(|c| c.display).unwrap_or_default();
            self.row = if self.row == Row::Link { Row::None } else { Row::Link };
        }
        if tool(ui, "g.note", "Note", self.row == Row::Note, accent).clicked() {
            self.fields.note = self.doc.cell(self.sheet(), self.sel.first).and_then(|c| c.comment).unwrap_or_default();
            self.row = if self.row == Row::Note { Row::None } else { Row::Note };
        }
        action
    }

    /// File tab: Save · Save as · PDF · Share · Print | paper, orientation, margins, fit, print
    /// area, gridlines | Info.
    fn file_tools(&mut self, ui: &mut egui::Ui, accent: Color32) -> Option<FileAction> {
        let mut action = None;
        if tool(ui, "save", "Save", false, accent).clicked() {
            action = Some(FileAction::Save);
        }
        tool_menu(ui, "saveAs", "Save as", accent, |ui| {
            for (label, ext) in [("Excel workbook (.xlsx)", "xlsx"), ("CSV, this sheet's values (.csv)", "csv")] {
                if chrome::menu_item(ui, label) {
                    action = Some(FileAction::SaveAs(ext));
                    ui.close();
                }
            }
            ui.label(RichText::new("OpenDocument (.ods) can be opened, not saved yet").small().weak());
        });
        if tool(ui, "export", "PDF", false, accent).clicked() {
            action = Some(FileAction::ExportPdf);
        }
        if tool(ui, "share", "Share", false, accent).clicked() {
            action = Some(FileAction::Share);
        }
        if tool(ui, "print", "Print", false, accent).clicked() {
            action = Some(FileAction::Print);
        }
        chrome::divider(ui);
        let ps = self.doc.page_setup(self.sheet());
        let paper = ps.as_ref().map(|p| p.paper.clone()).unwrap_or_default();
        let landscape = ps.as_ref().is_some_and(|p| p.landscape);
        let fit = ps.as_ref().and_then(|p| p.fit);
        let grid = ps.as_ref().is_some_and(|p| p.gridlines);
        let area = ps.as_ref().and_then(|p| p.print_area);
        tool_menu(ui, "size", "Paper", accent, |ui| {
            let norm = |s: &str| s.to_ascii_lowercase().replace(['×', ' '], "x").replace("tabloid", "11x17");
            for (p, _, _) in crate::paper::PAPERS {
                let id = norm(p);
                if chrome::menu_item(ui, &crate::paper::label(p, norm(&paper) == id)) {
                    let r = self.doc.set_paper(&id);
                    self.act(r);
                    ui.close();
                }
            }
        });
        tool_menu(ui, "orientation", "Orientation", accent, |ui| {
            for (label, l) in [("Portrait", false), ("Landscape", true)] {
                if chrome::menu_item(ui, &tick(landscape == l, label)) {
                    let r = self.doc.set_landscape(l);
                    self.act(r);
                    ui.close();
                }
            }
        });
        tool_menu(ui, "margins", "Margins", accent, |ui| {
            for (label, m) in [("Normal", "normal"), ("Narrow", "narrow"), ("Wide", "wide")] {
                if chrome::menu_item(ui, label) {
                    let r = self.doc.set_margins(m);
                    self.act(r);
                    ui.close();
                }
            }
        });
        tool_menu(ui, "g.fit", "Fit", accent, |ui| {
            for (label, f) in [("Actual size (100 %)", None), ("Fit sheet on one page", Some((1, 1))), ("Fit all columns on one page", Some((1, 0))), ("Fit all rows on one page", Some((0, 1)))] {
                if chrome::menu_item(ui, &tick(fit == f, label)) {
                    let r = self.doc.set_fit_to_page(f);
                    self.act(r);
                    ui.close();
                }
            }
        });
        tool_menu(ui, "g.printArea", "Print area", accent, |ui| {
            if let Some(a) = area {
                ui.label(RichText::new(format!("Now: {}", a.a1())).weak());
            }
            let t = self.target();
            if chrome::menu_item(ui, &format!("Set to {}", t.a1())) {
                let r = self.doc.set_print_area(Some(t));
                self.act(r);
                ui.close();
            }
            if chrome::menu_item(ui, "Clear print area") {
                let r = self.doc.set_print_area(None);
                self.act(r);
                ui.close();
            }
        });
        if tool(ui, "g.gridlines", "Gridlines", grid, accent).clicked() {
            let r = self.doc.set_print_gridlines(!grid);
            self.act(r);
        }
        chrome::divider(ui);
        if tool(ui, "info", "Info", false, accent).clicked() {
            action = Some(FileAction::Info);
        }
        action
    }

    /// The input row under the tools (only one at a time).
    fn input_row(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let field = |ui: &mut egui::Ui, text: &mut String, hint: &str, w: f32| {
            ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(w).margin(egui::vec2(8.0, 8.0)))
        };
        let big = |t: &str| egui::Button::new(t).min_size(egui::vec2(72.0, 36.0));
        let close = |ui: &mut egui::Ui| {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 36.0), egui::Sense::click());
            icons::icon(ui.painter(), r, "close", ui.visuals().text_color(), accent);
            resp.clicked()
        };
        let mut shut = false;
        match self.row {
            Row::None => {}
            Row::FnSearch => {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 50.0).max(80.0);
                    field(ui, &mut self.fields.fn_search, "Search functions (e.g. VLOOKUP, date)", w);
                    shut = close(ui);
                });
                let q = self.fields.fn_search.trim().to_owned();
                if !q.is_empty() {
                    let list = self.functions_of(&format!("search:{q}"), None, Some(&q));
                    let out = egui::ScrollArea::horizontal().id_salt("grid-fn-results").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if list.is_empty() {
                                ui.label(RichText::new("No functions found").weak());
                            }
                            for f in list.iter().take(40) {
                                if ui.add(egui::Button::new(RichText::new(&f.name).strong()).min_size(egui::vec2(64.0, 36.0))).on_hover_text(&f.description).clicked() {
                                    self.insert_function(&f.name);
                                }
                            }
                        });
                    });
                    chrome::scroll_hints(ui, &out);
                }
            }
            Row::Replace => {
                ui.horizontal_wrapped(|ui| {
                    field(ui, &mut self.fields.find, "Find", 120.0);
                    field(ui, &mut self.fields.with, "Replace with", 120.0);
                    if ui.add(big("Replace all")).clicked() && !self.fields.find.is_empty() {
                        self.commit();
                        let (what, with) = (self.fields.find.clone(), self.fields.with.clone());
                        let r = self.doc.replace_all(&what, &with);
                        if let Some(n) = self.act(r) {
                            self.say(format!("Replaced {n} on this sheet"));
                        }
                    }
                    shut = close(ui);
                });
            }
            Row::Link => {
                ui.horizontal_wrapped(|ui| {
                    field(ui, &mut self.fields.url, "https://… or Sheet2!A1", 150.0);
                    field(ui, &mut self.fields.link_text, "Text to show", 110.0);
                    if ui.add(big("Insert")).clicked() && !self.fields.url.trim().is_empty() {
                        self.commit();
                        let (url, text) = (self.fields.url.trim().to_owned(), self.fields.link_text.clone());
                        let r = self.doc.insert_link(self.sel.first, &url, (!text.is_empty()).then_some(text.as_str()));
                        if self.act(r).is_some() {
                            shut = true;
                        }
                    }
                    shut |= close(ui);
                });
            }
            Row::Note => {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 130.0).max(80.0);
                    field(ui, &mut self.fields.note, &format!("Note for {}", self.sel.first.a1()), w);
                    if ui.add(big("Add")).clicked() && !self.fields.note.trim().is_empty() {
                        let text = self.fields.note.clone();
                        let r = self.doc.add_note(self.sel.first, &text);
                        if self.act(r).is_some() {
                            shut = true;
                        }
                    }
                    shut |= close(ui);
                });
            }
            Row::Rename => {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 130.0).max(80.0);
                    field(ui, &mut self.fields.rename, "Sheet name", w);
                    if ui.add(big("Rename")).clicked() && !self.fields.rename.trim().is_empty() {
                        let name = self.fields.rename.trim().to_owned();
                        let r = self.doc.rename_sheet(self.sheet(), &name);
                        if self.act(r).is_some() {
                            shut = true;
                        }
                    }
                    shut |= close(ui);
                });
            }
            Row::ConfirmMerge => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Merging keeps only the upper-left value.");
                    if ui.add(big("Merge")).clicked() {
                        if let Some(a) = self.merge_pending.take() {
                            let r = self.doc.merge(a, true);
                            self.act(r);
                        }
                        shut = true;
                    }
                    if ui.add(big("Cancel")).clicked() {
                        self.merge_pending = None;
                        shut = true;
                    }
                });
            }
        }
        if shut {
            self.row = Row::None;
        }
    }
}

impl GridScreen {
    /// Would merging `area` throw values away? True when any cell other than the upper-left one
    /// holds something (a merge keeps only that one value).
    fn merge_would_clear(&self, area: slate_grid::Area) -> bool {
        let sheet = self.sheet();
        // Only cells inside the used part of the sheet can hold anything.
        let Some(used) = self.doc.used_range(sheet) else { return false };
        let (r0, r1) = (area.first.row.max(used.first.row), area.last.row.min(used.last.row));
        let (c0, c1) = (area.first.col.max(used.first.col), area.last.col.min(used.last.col));
        if r0 > r1 || c0 > c1 {
            return false;
        }
        // A very large selection: ask rather than read every cell.
        if u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1) > 50_000 {
            return true;
        }
        (r0..=r1).any(|row| {
            (c0..=c1).any(|col| {
                let at = slate_grid::Addr::new(row, col);
                at != area.first && !self.doc.cell_text(sheet, at).is_empty()
            })
        })
    }
}
