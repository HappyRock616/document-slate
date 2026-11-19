//! WordSlate edit mode: the WPS-style bottom bar (approved list: docs/WordSlate-features.md).
//! An icon tool row for the current tab above an icon tab strip: Text · Paragraph · Insert ·
//! Review · File. Icons are upstream WordCraft's (src/icons.rs); buttons and menus come from
//! src/chrome.rs so the other apps can share them. Less-used choices sit in small pop-up menus.

use eframe::egui::{self, RichText};
use serde_json::json;

use crate::chrome;
use crate::word_viewer::{ObjKind, WordViewer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Text,
    Paragraph,
    Insert,
    Review,
    File,
}

/// (tab, icon, label).
const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Text, "fontcolor", "Text"),
    (Tab::Paragraph, "pilcrow", "Paragraph"),
    (Tab::Insert, "picture", "Insert"),
    (Tab::Review, "spelling", "Review"),
    (Tab::File, "file", "File"),
];

/// Fonts offered in Text › Font. The first four are bundled (OFL); the Microsoft names are
/// common in documents people receive and are drawn with the engine's metric-compatible substitutes
/// (the document keeps the name, so it looks right again in Word).
const FONTS: [&str; 8] = ["Source Sans 3", "Source Serif 4", "Inter", "JetBrains Mono", "Calibri", "Arial", "Times New Roman", "Courier New"];

/// Text colours (Text › Colour), RRGGBB.
const TEXT_COLOURS: [&str; 10] = ["000000", "595959", "C00000", "E36C09", "C9A100", "00A050", "2F6DB5", "1F3864", "7030A0", "FFFFFF"];

/// Highlight colours: engine name and how to show it.
const HIGHLIGHTS: [(&str, &str); 8] = [
    ("yellow", "FFFF00"),
    ("brightGreen", "00FF00"),
    ("turquoise", "00FFFF"),
    ("pink", "FF00FF"),
    ("blue", "0000FF"),
    ("red", "FF0000"),
    ("gray25", "C0C0C0"),
    ("darkYellow", "808000"),
];

/// A row of colour circles; returns the tapped one's RRGGBB.
fn swatches<'a>(ui: &mut egui::Ui, colours: &[&'a str]) -> Option<&'a str> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(5.0 * 44.0);
        for hex in colours {
            let c = u32::from_str_radix(hex, 16).unwrap_or(0);
            let col = egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
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

/// Paragraph styles offered in the Text tab (engine style names).
const STYLES: [&str; 6] = ["Normal", "Title", "Heading 1", "Heading 2", "Heading 3", "Quote"];

/// Actions that need the app (files, Android pickers, messages), handled in src/lib.rs.
pub enum FileAction {
    Save,
    /// Save a copy in another format ("docx", "odt", "txt") through Android's "save to" screen.
    SaveAs(&'static str),
    ExportPdf,
    Share,
    Print,
    Info,
    /// Show a short message (word count, "Replaced 3", "No spelling mistakes").
    Status(String),
    /// Insert › Picture: open Android's image picker (the picture is inserted when it arrives).
    PickImage,
    /// A copy in the document's own format through "save to", named "<name><suffix>.<ext>"
    /// (e.g. " (signed)"). `adopt`: the copy becomes the open document.
    SaveCopy { suffix: &'static str, adopt: bool },
    /// Insert › Barcode: open the barcode maker (the picture is inserted when it's made).
    InsertBarcode,
}

/// Which Insert item's input row is open under the Insert tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InsertRow {
    #[default]
    None,
    Table,
    Link,
    HeaderFooter,
    Comment,
    Symbol,
}

/// What the user is typing into the Insert rows (kept between frames).
pub struct InsertState {
    pub row: InsertRow,
    pub rows: u32,
    pub cols: u32,
    pub url: String,
    pub link_text: String,
    pub hf_text: String,
    pub comment: String,
}

impl Default for InsertState {
    fn default() -> Self {
        Self { row: InsertRow::None, rows: 3, cols: 3, url: String::new(), link_text: String::new(), hf_text: String::new(), comment: String::new() }
    }
}

/// Common symbols for Insert › Symbol (all in egui's built-in font, so the buttons can show them).
const SYMBOLS: [&str; 16] = ["©", "®", "™", "€", "£", "¥", "°", "±", "×", "÷", "§", "¶", "…", "—", "½", "µ"];

/// Draw the tool row for `tab` above the tab strip. `accent` colours active buttons and the tab.
pub fn show(ui: &mut egui::Ui, v: &mut WordViewer, tab: &mut Tab, ins: &mut InsertState, accent: egui::Color32) -> Option<FileAction> {
    let mut action = None;
    let object = v.selected_object().map(|o| o.kind);
    // Tool row (swipes sideways when it doesn't fit, like WPS). A selected picture/shape/chart
    // replaces the tab's tools with the Object tools until it's deselected.
    // Caret in a table (and no object selected): a Table row with rows/columns tools.
    if object.is_none() && v.in_table() {
        let out = egui::ScrollArea::horizontal().id_salt("table_tools").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                table_tools(ui, v, accent);
            });
        });
        chrome::scroll_hints(ui, &out);
        ui.separator();
    }
    let out = egui::ScrollArea::horizontal().id_salt("edit_tools").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if let Some(kind) = object {
                object_tools(ui, v, kind, accent);
                return;
            }
            match tab {
                Tab::Text => text_tools(ui, v, accent),
                Tab::Paragraph => paragraph_tools(ui, v, accent),
                Tab::Insert => action = insert_tools(ui, v, ins, accent),
                Tab::Review => action = review_tools(ui, v, accent),
                Tab::File => action = file_tools(ui, v, accent),
            }
        });
    });
    chrome::scroll_hints(ui, &out);
    // Chart data grid (Object › Data).
    if object == Some(ObjKind::Chart) && v.chart_edit.is_some() {
        chart_grid(ui, v, accent);
    } else if object.is_none() {
        v.chart_edit = None;
    }
    // Extra rows: Review (spelling suggestions, find & replace), Insert (the open item's inputs).
    if object.is_some() {
        // (no tab rows while an object is selected)
    } else if *tab == Tab::Review {
        if let Some(a) = review_rows(ui, v, accent) {
            action = Some(a);
        }
    }
    if object.is_none() && *tab == Tab::Insert {
        if let Some(a) = insert_rows(ui, v, ins, accent) {
            action = Some(a);
        }
    }
    // Tab strip: icon + label, the current tab in the accent colour with a bar under it.
    let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::hover());
    let w = strip.width() / TABS.len() as f32;
    for (i, (t, icon, label)) in TABS.into_iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(strip.left() + i as f32 * w, strip.top()), egui::vec2(w, strip.height()));
        let resp = ui.interact(r, ui.scope_id().with(("tab", i)), egui::Sense::click());
        let selected = *tab == t;
        let fg = if selected { accent } else { ui.visuals().text_color().gamma_multiply(0.7) };
        let p = ui.painter();
        chrome::icon(p, egui::Rect::from_center_size(egui::pos2(r.center().x, r.top() + 20.0), egui::Vec2::splat(chrome::ICON)), icon, fg, accent);
        p.text(egui::pos2(r.center().x, r.bottom() - 12.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.5), fg);
        if selected {
            let bar = egui::Rect::from_center_size(egui::pos2(r.center().x, r.bottom() - 2.0), egui::vec2(28.0, 3.0));
            p.rect_filled(bar, 1.5, accent);
        }
        if resp.clicked() {
            *tab = t;
        }
    }
    action
}

fn text_tools(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) {
    // Most used first (what fits on a phone without swiping): font, size, B/I/U, colour,
    // highlight. The rest is a swipe away (the row shows a fade + chevron).
    chrome::tool_menu(ui, "fonts", "Font", accent, |ui| {
        let current = v.font_name();
        for f in FONTS {
            let label = if current == f { format!("• {f}") } else { f.to_owned() };
            if chrome::menu_item(ui, &label) {
                v.run("format.font", json!({ "name": f }));
                ui.close();
            }
        }
    });
    let size_label = v.font_size().map(|s| format!("{s} pt")).unwrap_or_else(|| "Size".into());
    chrome::tool_menu(ui, "size", &size_label, accent, |ui| {
        for sz in [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0, 72.0] {
            let label = if v.font_size() == Some(sz) { format!("• {sz} pt") } else { format!("{sz} pt") };
            if chrome::menu_item(ui, &label) {
                v.run("format.size", json!({ "size": sz }));
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "bold", "Bold", v.fmt_on("bold"), accent).clicked() {
        v.run("format.bold", json!({}));
    }
    if chrome::tool(ui, "italic", "Italic", v.fmt_on("italic"), accent).clicked() {
        v.run("format.italic", json!({}));
    }
    if chrome::tool(ui, "underline", "Underline", v.fmt_on("underline"), accent).clicked() {
        v.run("format.underline", json!({}));
    }
    chrome::tool_menu(ui, "fontcolor", "Color", accent, |ui| {
        if chrome::menu_item(ui, "Automatic") {
            v.run("format.color", json!({ "color": "auto" }));
            ui.close();
        }
        if let Some(hex) = swatches(ui, &TEXT_COLOURS) {
            v.run("format.color", json!({ "color": hex }));
            ui.close();
        }
    });
    chrome::tool_menu(ui, "highlight", "Highlight", accent, |ui| {
        if chrome::menu_item(ui, "No highlight") {
            v.run("format.highlight", json!({ "color": "none" }));
            ui.close();
        }
        let names: Vec<&str> = HIGHLIGHTS.iter().map(|(n, _)| *n).collect();
        let hexes: Vec<&str> = HIGHLIGHTS.iter().map(|(_, h)| *h).collect();
        if let Some(hex) = swatches(ui, &hexes) {
            if let Some(i) = hexes.iter().position(|h| *h == hex) {
                v.run("format.highlight", json!({ "color": names[i] }));
            }
            ui.close();
        }
    });
    chrome::divider(ui);
    if chrome::tool(ui, "strike", "Strike", v.fmt_on("strike"), accent).clicked() {
        v.run("format.strikethrough", json!({}));
    }
    if chrome::tool(ui, "superscript", "Super", v.fmt_on("superscript"), accent).clicked() {
        v.run("format.superscript", json!({}));
    }
    if chrome::tool(ui, "subscript", "Sub", v.fmt_on("subscript"), accent).clicked() {
        v.run("format.subscript", json!({}));
    }
    chrome::tool_menu(ui, "case", "Case", accent, |ui| {
        for (label, mode) in [("Sentence case.", "sentence"), ("lowercase", "lower"), ("UPPERCASE", "upper"), ("Capitalize Each Word", "title"), ("tOGGLE cASE", "toggle")] {
            if chrome::menu_item(ui, label) {
                v.run("format.changeCase", json!({ "mode": mode }));
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "shrink", "Smaller", false, accent).clicked() {
        v.run("format.shrinkFont", json!({}));
    }
    if chrome::tool(ui, "grow", "Larger", false, accent).clicked() {
        v.run("format.growFont", json!({}));
    }
    chrome::tool_menu(ui, "styles", "Style", accent, |ui| {
        let current = v.style_name();
        for s in STYLES {
            let label = if current == s { format!("• {s}") } else { s.to_owned() };
            if chrome::menu_item(ui, &label) {
                v.run("para.style", json!({ "style": s }));
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "clear", "Clear", false, accent).clicked() {
        v.run("format.clear", json!({}));
    }
}

fn paragraph_tools(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) {
    for (icon, label, cmd) in [
        ("alignLeft", "Left", "para.alignLeft"),
        ("alignCenter", "Center", "para.alignCenter"),
        ("alignRight", "Right", "para.alignRight"),
        ("justify", "Justify", "para.justify"),
    ] {
        if chrome::tool(ui, icon, label, false, accent).clicked() {
            v.run(cmd, json!({}));
        }
    }
    chrome::divider(ui);
    for (icon, label, cmd) in [
        ("bullets", "Bullets", "para.bullets"),
        ("numbering", "Numbers", "para.numbering"),
        ("outdent", "Outdent", "para.outdent"),
        ("indent", "Indent", "para.indent"),
    ] {
        if chrome::tool(ui, icon, label, false, accent).clicked() {
            v.run(cmd, json!({}));
        }
    }
    chrome::divider(ui);
    chrome::tool_menu(ui, "lineSpacing", "Spacing", accent, |ui| {
        for (label, cmd) in [("Single (1.0)", "para.single"), ("1.5 lines", "para.oneAndHalf"), ("Double (2.0)", "para.double")] {
            if chrome::menu_item(ui, label) {
                v.run(cmd, json!({}));
                ui.close();
            }
        }
        ui.separator();
        ui.label(RichText::new("Space before / after (pt)").weak());
        for (label, before, after) in [("Before 0", Some(0.0), None), ("Before 6", Some(6.0), None), ("Before 12", Some(12.0), None), ("After 0", None, Some(0.0)), ("After 6", None, Some(6.0)), ("After 12", None, Some(12.0))] {
            if chrome::menu_item(ui, label) {
                let mut p = json!({});
                if let Some(b) = before {
                    p["before"] = json!(b);
                }
                if let Some(a) = after {
                    p["after"] = json!(a);
                }
                v.run("para.spacing", p);
                ui.close();
            }
        }
    });
}

/// File tab: Save · Save as (menu) · PDF · Share · Print · Page setup (menu) · Info.
fn file_tools(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) -> Option<FileAction> {
    let mut action = None;
    if chrome::tool(ui, "save", "Save", false, accent).clicked() {
        action = Some(FileAction::Save);
    }
    chrome::tool_menu(ui, "saveAs", "Save as", accent, |ui| {
        for (label, ext) in [("Word document (.docx)", "docx"), ("OpenDocument (.odt)", "odt"), ("Plain text (.txt)", "txt")] {
            if chrome::menu_item(ui, label) {
                action = Some(FileAction::SaveAs(ext));
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "export", "PDF", false, accent).clicked() {
        action = Some(FileAction::ExportPdf);
    }
    if chrome::tool(ui, "share", "Share", false, accent).clicked() {
        action = Some(FileAction::Share);
    }
    if chrome::tool(ui, "print", "Print", false, accent).clicked() {
        action = Some(FileAction::Print);
    }
    chrome::divider(ui);
    let current = v.paper_name();
    chrome::tool_menu(ui, "size", "Paper", accent, |ui| {
        for (name, w, h) in crate::paper::PAPERS {
            if chrome::menu_item(ui, &crate::paper::label(name, current == Some(name))) {
                v.run("layout.size", json!({ "width": w, "height": h }));
                ui.close();
            }
        }
    });
    chrome::tool_menu(ui, "orientation", "Orientation", accent, |ui| {
        for (label, value) in [("Portrait", "portrait"), ("Landscape", "landscape")] {
            if chrome::menu_item(ui, label) {
                v.run("layout.orientation", json!({ "value": value }));
                ui.close();
            }
        }
    });
    chrome::tool_menu(ui, "margins", "Margins", accent, |ui| {
        for (label, preset) in [("Normal", "normal"), ("Narrow", "narrow"), ("Wide", "wide")] {
            if chrome::menu_item(ui, label) {
                v.run("layout.margins", json!({ "preset": preset }));
                ui.close();
            }
        }
    });
    chrome::divider(ui);
    if chrome::tool(ui, "info", "Info", false, accent).clicked() {
        action = Some(FileAction::Info);
    }
    action
}

/// Review tab: Spelling · Word count · Replace | Comments (menu) | Track changes · Changes (menu).
fn review_tools(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) -> Option<FileAction> {
    let mut action = None;
    if chrome::tool(ui, "spelling", "Spelling", false, accent).clicked() && !v.next_mistake() {
        action = Some(FileAction::Status("No spelling mistakes found".into()));
    }
    if chrome::tool(ui, "wordCount", "Count", false, accent).clicked() {
        action = Some(FileAction::Status(v.word_count_line()));
    }
    if chrome::tool(ui, "replace", "Replace", v.replace_open, accent).clicked() {
        v.replace_open = !v.replace_open;
    }
    chrome::divider(ui);
    if chrome::tool(ui, "prevComment", "Prev note", false, accent).clicked() {
        v.run("review.previousComment", json!({}));
    }
    if chrome::tool(ui, "nextComment", "Next note", false, accent).clicked() {
        v.run("review.nextComment", json!({}));
    }
    if chrome::tool(ui, "deleteComment", "Delete note", false, accent).clicked() {
        v.run("review.deleteComment", json!({}));
    }
    chrome::divider(ui);
    let tracking = v.tracking();
    if chrome::tool(ui, "trackChanges", "Track", tracking, accent).clicked() {
        v.run("review.trackChanges", json!({ "value": !tracking }));
    }
    if chrome::tool(ui, "accept", "Accept", false, accent).clicked() {
        v.run("review.accept", json!({ "next": true }));
    }
    if chrome::tool(ui, "reject", "Reject", false, accent).clicked() {
        v.run("review.reject", json!({ "next": true }));
    }
    chrome::tool_menu(ui, "nextChange", "Changes", accent, |ui| {
        for (label, cmd) in [
            ("Previous change", "review.previousChange"),
            ("Next change", "review.nextChange"),
            ("Accept all", "review.acceptAll"),
            ("Reject all", "review.rejectAll"),
        ] {
            if chrome::menu_item(ui, label) {
                v.run(cmd, json!({}));
                ui.close();
            }
        }
    });
    action
}

/// Rows under the Review tools: suggestions for a misspelled word at the caret, and the
/// find & replace fields when Replace is on.
fn review_rows(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) -> Option<FileAction> {
    let mut action = None;
    if let Some(issue) = v.issue.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("\u{201C}{}\u{201D} may be misspelled:", issue.word)).color(egui::Color32::from_rgb(0xD0, 0x40, 0x40)));
            for s in &issue.suggestions {
                if ui.add(egui::Button::new(RichText::new(s).strong()).min_size(egui::vec2(52.0, 40.0))).clicked() {
                    v.apply_suggestion(s);
                }
            }
            if issue.suggestions.is_empty() {
                ui.label(RichText::new("no suggestions").weak());
            }
            if ui.add(egui::Button::new("Ignore all").min_size(egui::vec2(0.0, 40.0))).clicked() {
                v.run("review.ignoreAll", json!({ "word": issue.word }));
            }
            if ui.add(egui::Button::new("Add to dictionary").min_size(egui::vec2(0.0, 40.0))).clicked() {
                v.run("review.addToDictionary", json!({ "word": issue.word }));
            }
        });
    }
    if v.replace_open {
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut v.replace_find).hint_text("Find").desired_width(160.0));
            ui.add(egui::TextEdit::singleline(&mut v.replace_with).hint_text("Replace with").desired_width(160.0));
            if ui.add(egui::Button::new("Replace all").fill(accent.gamma_multiply(0.3)).min_size(egui::vec2(0.0, 40.0))).clicked() {
                action = Some(FileAction::Status(v.replace_all()));
            }
        });
    }
    action
}

/// Insert tab: Picture · Table · Link · Page break · Page number · Header & footer · Comment · Symbol.
fn insert_tools(ui: &mut egui::Ui, v: &mut WordViewer, ins: &mut InsertState, accent: egui::Color32) -> Option<FileAction> {
    let mut action = None;
    if chrome::tool(ui, "picture", "Picture", false, accent).clicked() {
        action = Some(FileAction::PickImage);
    }
    if chrome::tool(ui, "qr", "Barcode", false, accent).clicked() {
        action = Some(FileAction::InsertBarcode);
    }
    // Items that open an input row under the tools.
    let mut toggle = |ui: &mut egui::Ui, icon: &str, label: &str, row: InsertRow| {
        if chrome::tool(ui, icon, label, ins.row == row, accent).clicked() {
            ins.row = if ins.row == row { InsertRow::None } else { row };
        }
    };
    toggle(ui, "table", "Table", InsertRow::Table);
    toggle(ui, "link", "Link", InsertRow::Link);
    if chrome::tool(ui, "pageBreak", "Page break", false, accent).clicked() {
        v.run("insert.pageBreak", json!({}));
    }
    if chrome::tool(ui, "pageNumber", "Page no.", false, accent).clicked() {
        v.run("insert.pageNumber", json!({ "position": "bottom", "align": "center" }));
    }
    let mut toggle = |ui: &mut egui::Ui, icon: &str, label: &str, row: InsertRow| {
        if chrome::tool(ui, icon, label, ins.row == row, accent).clicked() {
            ins.row = if ins.row == row { InsertRow::None } else { row };
        }
    };
    toggle(ui, "header", "Header", InsertRow::HeaderFooter);
    toggle(ui, "comment", "Comment", InsertRow::Comment);
    toggle(ui, "symbol", "Symbol", InsertRow::Symbol);
    action
}

/// The input row for the Insert item that's open.
fn insert_rows(ui: &mut egui::Ui, v: &mut WordViewer, ins: &mut InsertState, accent: egui::Color32) -> Option<FileAction> {
    let mut action = None;
    let go = |ui: &mut egui::Ui, label: &str| ui.add(egui::Button::new(label).fill(accent.gamma_multiply(0.3)).min_size(egui::vec2(0.0, 40.0))).clicked();
    match ins.row {
        InsertRow::None => {}
        InsertRow::Table => {
            ui.horizontal_wrapped(|ui| {
                ui.label("Rows");
                ui.add(egui::DragValue::new(&mut ins.rows).range(1..=50));
                ui.label("Columns");
                ui.add(egui::DragValue::new(&mut ins.cols).range(1..=20));
                if go(ui, "Insert table") {
                    v.run("insert.table", json!({ "rows": ins.rows, "cols": ins.cols }));
                    ins.row = InsertRow::None;
                }
            });
        }
        InsertRow::Link => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut ins.url).hint_text("https://…").desired_width(200.0));
                ui.add(egui::TextEdit::singleline(&mut ins.link_text).hint_text("Text to show (optional)").desired_width(180.0));
                if go(ui, "Insert link") {
                    if ins.url.trim().is_empty() {
                        action = Some(FileAction::Status("Type the link address first".into()));
                    } else {
                        let mut p = json!({ "url": ins.url.trim() });
                        if !ins.link_text.trim().is_empty() {
                            p["text"] = json!(ins.link_text.trim());
                        }
                        v.run("insert.link", p);
                        ins.url.clear();
                        ins.link_text.clear();
                        ins.row = InsertRow::None;
                    }
                }
            });
        }
        InsertRow::HeaderFooter => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut ins.hf_text).hint_text("Header or footer text").desired_width(220.0));
                // Back to the body afterwards, so typing doesn't land in the header/footer.
                if go(ui, "Set header") {
                    v.run("insert.header", json!({ "text": ins.hf_text }));
                    v.run("insert.closeHeader", json!({}));
                }
                if go(ui, "Set footer") {
                    v.run("insert.footer", json!({ "text": ins.hf_text }));
                    v.run("insert.closeHeader", json!({}));
                }
                if go(ui, "Remove header") {
                    v.run("insert.removeHeader", json!({}));
                }
                if go(ui, "Remove footer") {
                    v.run("insert.removeFooter", json!({}));
                }
            });
        }
        InsertRow::Comment => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut ins.comment).hint_text("Comment on the selected text").desired_width(260.0));
                if go(ui, "Add comment") && !ins.comment.trim().is_empty() {
                    v.run("review.newComment", json!({ "text": ins.comment.trim() }));
                    ins.comment.clear();
                    ins.row = InsertRow::None;
                    action = Some(FileAction::Status("Comment added (Review tab: Prev / Next / Delete note)".into()));
                }
            });
        }
        InsertRow::Symbol => {
            ui.horizontal_wrapped(|ui| {
                for s in SYMBOLS {
                    if ui.add(egui::Button::new(RichText::new(s).size(18.0)).min_size(egui::vec2(44.0, 44.0))).clicked() {
                        v.run("insert.symbol", json!({ "char": s }));
                    }
                }
            });
        }
    }
    action
}

/// Table row (caret in a table): add/delete rows and columns, merge, delete the table.
fn table_tools(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) {
    for (icon, label, cmd) in [
        ("insertAbove", "Row above", "table.insertRowAbove"),
        ("insertBelow", "Row below", "table.insertRowBelow"),
        ("insertLeft", "Col left", "table.insertColumnLeft"),
        ("insertRight", "Col right", "table.insertColumnRight"),
    ] {
        if chrome::tool(ui, icon, label, false, accent).clicked() {
            v.run(cmd, json!({}));
        }
    }
    chrome::divider(ui);
    for (icon, label, cmd) in [
        ("rowHeight", "Del row", "table.deleteRow"),
        ("colWidth", "Del col", "table.deleteColumn"),
        ("merge", "Merge", "table.merge"),
        ("deleteTable", "Del table", "table.deleteTable"),
    ] {
        if chrome::tool(ui, icon, label, false, accent).clicked() {
            v.run(cmd, json!({}));
        }
    }
}

/// Object tools (a picture, shape or chart is selected). Common: rotate, wrap, front/back,
/// delete. Pictures: size presets, reset. Shapes: fill and outline colour. Charts: type, title,
/// legend, data labels, edit data (grid). Drag the corner handle on the page to resize; drag a
/// floating object to move it.
fn object_tools(ui: &mut egui::Ui, v: &mut WordViewer, kind: ObjKind, accent: egui::Color32) {
    match kind {
        ObjKind::Picture => {
            chrome::tool_menu(ui, "size", "Size", accent, |ui| {
                for (label, pct) in [("50 %", 50), ("75 %", 75), ("125 %", 125), ("150 %", 150), ("200 %", 200)] {
                    if chrome::menu_item(ui, label) {
                        v.run("picture.size", json!({ "scale": pct, "original": true }));
                        ui.close();
                    }
                }
                if chrome::menu_item(ui, "Original size") {
                    v.run("picture.size", json!({ "scale": 100, "original": true }));
                    ui.close();
                }
            });
            if chrome::tool(ui, "update", "Reset", false, accent).clicked() {
                v.run("picture.reset", json!({}));
            }
        }
        ObjKind::Shape => {
            chrome::tool_menu(ui, "shapes", "Fill", accent, |ui| {
                if chrome::menu_item(ui, "No fill") {
                    v.run("shape.fill", json!({ "color": null }));
                    ui.close();
                }
                if let Some(hex) = swatches(ui, &TEXT_COLOURS) {
                    v.run("shape.fill", json!({ "color": hex }));
                    ui.close();
                }
            });
            chrome::tool_menu(ui, "pen", "Outline", accent, |ui| {
                if let Some(hex) = swatches(ui, &TEXT_COLOURS) {
                    v.run("shape.outline", json!({ "color": hex }));
                    ui.close();
                }
                for (label, w) in [("Thin (0.75 pt)", 0.75), ("Medium (2 pt)", 2.0), ("Thick (4 pt)", 4.0)] {
                    if chrome::menu_item(ui, label) {
                        v.run("shape.outline", json!({ "width": w }));
                        ui.close();
                    }
                }
            });
        }
        ObjKind::Chart => {
            chrome::tool_menu(ui, "changeChartType", "Type", accent, |ui| {
                for (label, t) in [("Column", "column"), ("Stacked column", "columnStacked"), ("Bar", "bar"), ("Line", "line"), ("Area", "area"), ("Pie", "pie"), ("Doughnut", "doughnut"), ("Scatter", "scatter")] {
                    if chrome::menu_item(ui, label) {
                        v.run("chart.type", json!({ "type": t }));
                        ui.close();
                    }
                }
            });
            if chrome::tool(ui, "chartData", "Data", v.chart_edit.is_some(), accent).clicked() {
                v.chart_edit = if v.chart_edit.is_some() { None } else { v.chart_table() };
            }
            if chrome::tool(ui, "chartTitle", "Title", false, accent).clicked() {
                v.run("chart.title", json!({}));
            }
            chrome::tool_menu(ui, "legend", "Legend", accent, |ui| {
                for (label, pos) in [("Right", "right"), ("Bottom", "bottom"), ("Top", "top"), ("Left", "left"), ("None", "none")] {
                    if chrome::menu_item(ui, label) {
                        v.run("chart.legend", json!({ "pos": pos }));
                        ui.close();
                    }
                }
            });
            if chrome::tool(ui, "dataLabels", "Labels", false, accent).clicked() {
                v.run("chart.dataLabels", json!({}));
            }
        }
        ObjKind::Diagram => {}
    }
    chrome::divider(ui);
    if !kind.arrangeable() {
        if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
            v.run("text.delete", json!({}));
        }
        return;
    }
    if chrome::tool(ui, "rotate", "Rotate", false, accent).clicked() {
        v.run("arrange.rotate", json!({ "direction": "right" }));
    }
    chrome::tool_menu(ui, "wrapText", "Wrap", accent, |ui| {
        for (label, w) in [
            ("In line with text", "inline"),
            ("Square", "square"),
            ("Tight", "tight"),
            ("Top and bottom", "topAndBottom"),
            ("Behind text", "behindText"),
            ("In front of text", "inFrontOfText"),
        ] {
            if chrome::menu_item(ui, label) {
                v.run("arrange.wrap", json!({ "wrap": w }));
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "bringForward", "Front", false, accent).clicked() {
        v.run("arrange.bringForward", json!({}));
    }
    if chrome::tool(ui, "sendBackward", "Back", false, accent).clicked() {
        v.run("arrange.sendBackward", json!({}));
    }
    if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
        v.run("text.delete", json!({}));
    }
}

/// The chart's data as a small editable grid: one row per category, one column per series.
fn chart_grid(ui: &mut egui::Ui, v: &mut WordViewer, accent: egui::Color32) {
    let Some(mut t) = v.chart_edit.take() else { return };
    let mut apply = false;
    let mut cancel = false;
    egui::ScrollArea::both().id_salt("chart-grid").max_height(220.0).show(ui, |ui| {
        egui::Grid::new("chart-grid-cells").spacing([4.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Category").weak());
            for (name, _) in t.series.iter_mut() {
                ui.add(egui::TextEdit::singleline(name).desired_width(80.0));
            }
            ui.end_row();
            for (row, cat) in t.categories.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(cat).desired_width(100.0));
                for (_, vals) in t.series.iter_mut() {
                    if let Some(cell) = vals.get_mut(row) {
                        ui.add(egui::TextEdit::singleline(cell).desired_width(80.0));
                    }
                }
                ui.end_row();
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add(egui::Button::new("+ Row").min_size(egui::vec2(0.0, 40.0))).clicked() {
            t.categories.push(format!("Item {}", t.categories.len() + 1));
            for (_, vals) in t.series.iter_mut() {
                vals.push(String::new());
            }
        }
        if ui.add(egui::Button::new("+ Series").min_size(egui::vec2(0.0, 40.0))).clicked() {
            let n = t.series.len() + 1;
            t.series.push((format!("Series {n}"), vec![String::new(); t.categories.len()]));
        }
        if ui.add(egui::Button::new("Apply").fill(accent.gamma_multiply(0.3)).min_size(egui::vec2(0.0, 40.0))).clicked() {
            apply = true;
        }
        if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 40.0))).clicked() {
            cancel = true;
        }
    });
    if apply {
        v.apply_chart_table(&t);
    }
    if !apply && !cancel {
        v.chart_edit = Some(t);
    }
}
