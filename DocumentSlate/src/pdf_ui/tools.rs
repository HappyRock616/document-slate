//! Page Slate edit mode: the bottom bar (tool row + icon tab strip), same look as Word Slate.
//! Tabs (docs/PageSlate-features.md): Annotate · Edit · Fill & Sign · Pages · File.
//! Touch tools stay on until tapped again (like WPS), so several marks can be made in a row.

use eframe::egui::{self, Color32, RichText};
use slate_pdf::{AddedItem, CommentInfo, FillMark, Look, MarkupKind, PageLink, Rgb, ShapeKind, TextFont, TextStyle, ViewRect};

use super::sheets::Sheet;
use super::PdfScreen;
use crate::chrome;
use crate::doc_screen::ScreenCtx;
use crate::edit_bar::FileAction;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tab {
    Annotate,
    Edit,
    FillSign,
    Pages,
    File,
}

const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Annotate, "comment", "Annotate"),
    (Tab::Edit, "editor", "Edit"),
    (Tab::FillSign, "signature", "Fill & Sign"),
    (Tab::Pages, "multiplePages", "Pages"),
    (Tab::File, "file", "File"),
];

/// What a tap or finger drag on the page does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum TouchTool {
    None,
    Markup(MarkupKind),
    Note,
    TextBox,
    Ink,
    Shape(ShapeKind),
    FillText,
    Mark(FillMark),
    Sign,
    Date,
    EditText,
    AddText,
    Link,
    /// Certificate signature: drag its box (or tap a signature field).
    CertBox,
}

impl TouchTool {
    /// Tools that work by dragging a finger (the page doesn't scroll while one is on).
    pub fn is_drag(self) -> bool {
        matches!(self, TouchTool::Markup(_) | TouchTool::Ink | TouchTool::Shape(_) | TouchTool::Link | TouchTool::CertBox)
    }

    fn hint(self) -> &'static str {
        match self {
            TouchTool::None => "",
            TouchTool::Markup(MarkupKind::Highlight) => "Drag across text to highlight it",
            TouchTool::Markup(MarkupKind::Underline) => "Drag across text to underline it",
            TouchTool::Markup(_) => "Drag across text to strike it through",
            TouchTool::Note => "Tap where the note should go",
            TouchTool::TextBox => "Tap where the text box should go",
            TouchTool::Ink => "Draw with your finger (the page stays still)",
            TouchTool::Shape(_) => "Drag to draw the shape",
            TouchTool::FillText => "Tap where to type",
            TouchTool::Mark(_) => "Tap to place the mark",
            TouchTool::Sign => "Tap where the signature goes",
            TouchTool::Date => "Tap to place today's date",
            TouchTool::EditText => "Tap a line of text to change it",
            TouchTool::AddText => "Tap where the new text goes",
            TouchTool::Link => "Drag a box over the link area",
            TouchTool::CertBox => "Drag a box for the signature, or tap a signature field",
        }
    }
}

/// The selected item (edit mode, no tool on).
#[derive(Clone, Debug)]
pub(super) enum Sel {
    Comment(CommentInfo),
    Added(AddedItem),
    Link(PageLink),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum EntryKind {
    Note,
    TextBox,
    FillText,
    AddText,
    EditLine(usize),
    Field(String),
    NewLink,
    LinkUrl(usize),
    CommentText(usize),
    Reply(usize),
    AddedText(usize),
}

/// The text input row above the tools (a note's words, a field's value…).
#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub kind: EntryKind,
    pub page: usize,
    /// Where it applies (page view space).
    pub rect: ViewRect,
    pub text: String,
    pub focus: bool,
}

pub(super) struct EditState {
    pub tab: Tab,
    pub tool: TouchTool,
    pub selected: Option<Sel>,
    pub entry: Option<Entry>,
    /// Pen / shape / underline colour and line width (points).
    pub color: Rgb,
    pub width: f64,
    pub hl_color: Rgb,
    /// Fill & Sign text size; tick / cross size.
    pub text_size: f64,
    pub mark_size: f32,
    /// Edit › Add text style.
    pub style: TextStyle,
    /// The saved signature to place (index in the store).
    pub sig_choice: Option<usize>,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            tab: Tab::Annotate,
            tool: TouchTool::None,
            selected: None,
            entry: None,
            color: [0.85, 0.1, 0.1],
            width: 2.0,
            hl_color: [1.0, 0.9, 0.0],
            text_size: 12.0,
            mark_size: 14.0,
            style: TextStyle::default(),
            sig_choice: None,
        }
    }
}

fn rgb32(c: Rgb) -> Color32 {
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

impl EditState {
    pub fn look(&self) -> Look {
        Look { color: Some(self.color), width: Some(self.width), ..Look::default() }
    }
    pub fn highlight_look(&self) -> Look {
        Look { color: Some(self.hl_color), ..Look::default() }
    }
    pub fn colour32(&self) -> Color32 {
        rgb32(self.color)
    }
    pub fn highlight32(&self) -> Color32 {
        rgb32(self.hl_color)
    }
}

/// Pen colours, RRGGBB.
const PEN_COLOURS: [&str; 10] = ["000000", "595959", "D91A1A", "E36C09", "C9A100", "00A050", "2F6DB5", "1F3864", "7030A0", "FFFFFF"];
/// Highlighter colours.
const HL_COLOURS: [&str; 6] = ["FFE600", "7CFC00", "00E5FF", "FF66CC", "FFA500", "B0B0FF"];

fn hex_rgb(hex: &str) -> Rgb {
    let c = u32::from_str_radix(hex, 16).unwrap_or(0);
    [((c >> 16) & 255) as f64 / 255.0, ((c >> 8) & 255) as f64 / 255.0, (c & 255) as f64 / 255.0]
}

/// A row of colour circles; returns the tapped one (same as Word Slate's).
fn swatches<'a>(ui: &mut egui::Ui, colours: &[&'a str]) -> Option<&'a str> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(5.0 * 44.0);
        for hex in colours {
            let c = hex_rgb(hex);
            let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
            ui.painter().circle_filled(r.center(), 15.0, rgb32(c));
            ui.painter().circle_stroke(r.center(), 15.0, egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.4)));
            if resp.clicked() {
                picked = Some(*hex);
            }
        }
    });
    picked
}

impl PdfScreen {
    /// The whole bottom bar.
    pub(super) fn tool_bar(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) -> Option<FileAction> {
        let accent = cx.accent;
        let mut action = None;
        // Text input row (note words, field value, link address…).
        if self.e.entry.is_some() {
            self.entry_row(ui, accent);
            ui.separator();
        } else if self.e.tool != TouchTool::None {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.e.tool.hint()).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cert = self.e.tool == TouchTool::CertBox;
                    if ui.add(egui::Button::new(if cert { "Cancel" } else { "Done" }).min_size(egui::vec2(64.0, 32.0))).clicked() {
                        self.e.tool = TouchTool::None;
                        self.v.drag = None;
                        if cert {
                            self.cert.stage = super::cert::Stage::None;
                        }
                    }
                    if cert && ui.add(egui::Button::new("Invisible").min_size(egui::vec2(80.0, 32.0))).clicked() {
                        self.v.drag = None;
                        self.cert_target(super::cert::Target::Invisible);
                    }
                });
            });
        }
        let out = egui::ScrollArea::horizontal().id_salt("pdf_tools").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if let Some(sel) = self.e.selected.clone() {
                    self.selection_tools(ui, sel, accent);
                    return;
                }
                match self.e.tab {
                    Tab::Annotate => self.annotate_tools(ui, accent),
                    Tab::Edit => action = self.edit_tools(ui, accent),
                    Tab::FillSign => self.fill_sign_tools(ui, accent),
                    Tab::Pages => action = self.page_tools(ui, accent),
                    Tab::File => action = self.file_tools(ui, accent),
                }
            });
        });
        chrome::scroll_hints(ui, &out);
        // Tab strip.
        let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::hover());
        let w = strip.width() / TABS.len() as f32;
        for (i, (t, icon, label)) in TABS.into_iter().enumerate() {
            let r = egui::Rect::from_min_size(egui::pos2(strip.left() + i as f32 * w, strip.top()), egui::vec2(w, strip.height()));
            let resp = ui.interact(r, ui.scope_id().with(("pdf-tab", i)), egui::Sense::click());
            let selected = self.e.tab == t;
            let fg = if selected { accent } else { ui.visuals().text_color().gamma_multiply(0.7) };
            let p = ui.painter();
            chrome::icon(p, egui::Rect::from_center_size(egui::pos2(r.center().x, r.top() + 20.0), egui::Vec2::splat(chrome::ICON)), icon, fg, accent);
            p.text(egui::pos2(r.center().x, r.bottom() - 12.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.5), fg);
            if selected {
                let bar = egui::Rect::from_center_size(egui::pos2(r.center().x, r.bottom() - 2.0), egui::vec2(28.0, 3.0));
                p.rect_filled(bar, 1.5, accent);
            }
            if resp.clicked() && !selected {
                self.e.tab = t;
                self.e.tool = TouchTool::None;
                self.e.selected = None;
                self.e.entry = None;
            }
        }
        action
    }

    /// A tool button that turns a touch tool on / off.
    fn touch_tool(&mut self, ui: &mut egui::Ui, icon: &str, label: &str, tool: TouchTool, accent: Color32) {
        if chrome::tool(ui, icon, label, self.e.tool == tool, accent).clicked() {
            self.e.tool = if self.e.tool == tool { TouchTool::None } else { tool };
            self.e.selected = None;
            self.e.entry = None;
        }
    }

    fn annotate_tools(&mut self, ui: &mut egui::Ui, accent: Color32) {
        self.touch_tool(ui, "highlight", "Highlight", TouchTool::Markup(MarkupKind::Highlight), accent);
        self.touch_tool(ui, "underline", "Underline", TouchTool::Markup(MarkupKind::Underline), accent);
        self.touch_tool(ui, "strike", "Strike", TouchTool::Markup(MarkupKind::Strikeout), accent);
        self.touch_tool(ui, "addNote", "Note", TouchTool::Note, accent);
        self.touch_tool(ui, "textBox", "Text box", TouchTool::TextBox, accent);
        self.touch_tool(ui, "pen", "Pen", TouchTool::Ink, accent);
        let shape_on = matches!(self.e.tool, TouchTool::Shape(_));
        let shape_icon = match self.e.tool {
            TouchTool::Shape(ShapeKind::Circle) => "ellipse",
            TouchTool::Shape(ShapeKind::Line) => "line",
            TouchTool::Shape(ShapeKind::Arrow) => "arrow",
            _ => "rectangle",
        };
        let resp = chrome::tool(ui, shape_icon, "Shape", shape_on, accent);
        let mut pick = None;
        egui::Popup::menu(&resp).align(egui::RectAlign::TOP_START).show(|ui| {
            ui.set_min_width(180.0);
            for (label, k) in [("Rectangle", ShapeKind::Rectangle), ("Circle / oval", ShapeKind::Circle), ("Line", ShapeKind::Line), ("Arrow", ShapeKind::Arrow)] {
                if chrome::menu_item(ui, label) {
                    pick = Some(k);
                    ui.close();
                }
            }
            if shape_on && chrome::menu_item(ui, "Stop drawing shapes") {
                pick = None;
                self.e.tool = TouchTool::None;
                ui.close();
            }
        });
        if let Some(k) = pick {
            self.e.tool = TouchTool::Shape(k);
            self.e.selected = None;
        }
        self.colour_menu(ui, accent);
        chrome::divider(ui);
        if chrome::tool(ui, "showComments", "Comments", false, accent).clicked() {
            self.sheet = Sheet::comments();
        }
    }

    /// Colour (pen, shapes, underline) + highlighter colour + line width.
    fn colour_menu(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let (color, hl, width) = (&mut self.e.color, &mut self.e.hl_color, &mut self.e.width);
        chrome::tool_menu(ui, "fontcolor", "Color", accent, |ui| {
            ui.label(RichText::new("Pen, shapes, lines").weak());
            if let Some(hex) = swatches(ui, &PEN_COLOURS) {
                *color = hex_rgb(hex);
                ui.close();
            }
            ui.separator();
            ui.label(RichText::new("Highlighter").weak());
            if let Some(hex) = swatches(ui, &HL_COLOURS) {
                *hl = hex_rgb(hex);
                ui.close();
            }
            ui.separator();
            ui.label(RichText::new("Line width").weak());
            for (label, w) in [("Fine (1 pt)", 1.0), ("Medium (2 pt)", 2.0), ("Thick (4 pt)", 4.0), ("Marker (8 pt)", 8.0)] {
                let label = if (*width - w).abs() < 0.01 { format!("• {label}") } else { label.to_owned() };
                if chrome::menu_item(ui, &label) {
                    *width = w;
                    ui.close();
                }
            }
        });
    }

    fn edit_tools(&mut self, ui: &mut egui::Ui, accent: Color32) -> Option<FileAction> {
        let mut action = None;
        self.touch_tool(ui, "editor", "Edit text", TouchTool::EditText, accent);
        self.touch_tool(ui, "addText", "Add text", TouchTool::AddText, accent);
        let style = &mut self.e.style;
        chrome::tool_menu(ui, "fonts", "Text style", accent, |ui| {
            for (label, f) in [("Sans (Helvetica)", TextFont::Sans), ("Serif (Times)", TextFont::Serif), ("Mono (Courier)", TextFont::Mono)] {
                let label = if style.font == f { format!("• {label}") } else { label.to_owned() };
                if chrome::menu_item(ui, &label) {
                    style.font = f;
                    ui.close();
                }
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for sz in [9.0, 10.0, 12.0, 14.0, 18.0, 24.0, 36.0] {
                    let on = (style.size - sz).abs() < 0.01;
                    if ui.add(egui::Button::new(format!("{sz}")).selected(on).min_size(egui::vec2(40.0, 36.0))).clicked() {
                        style.size = sz;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.toggle_value(&mut style.bold, RichText::new(" B ").strong());
                ui.toggle_value(&mut style.italic, RichText::new(" I ").italics());
            });
            if let Some(hex) = swatches(ui, &PEN_COLOURS) {
                style.color = hex_rgb(hex);
                ui.close();
            }
        });
        if chrome::tool(ui, "picture", "Picture", false, accent).clicked() {
            action = Some(FileAction::PickImage);
            self.say("Choose a picture — it lands on the current page");
        }
        if chrome::tool(ui, "qr", "Barcode", false, accent).clicked() {
            action = Some(FileAction::InsertBarcode);
        }
        self.touch_tool(ui, "link", "Link", TouchTool::Link, accent);
        action
    }

    fn fill_sign_tools(&mut self, ui: &mut egui::Ui, accent: Color32) {
        self.touch_tool(ui, "addText", "Text", TouchTool::FillText, accent);
        self.touch_tool(ui, "check", "Tick", TouchTool::Mark(FillMark::Check), accent);
        self.touch_tool(ui, "close", "Cross", TouchTool::Mark(FillMark::Cross), accent);
        // Dot: drawn here (no icon for it upstream).
        let on = self.e.tool == TouchTool::Mark(FillMark::Dot);
        let resp = chrome::tool(ui, "", "Dot", on, accent);
        let c = egui::pos2(resp.rect.center().x, resp.rect.top() + 22.0);
        ui.painter().circle_filled(c, 5.0, ui.visuals().text_color());
        if resp.clicked() {
            self.e.tool = if on { TouchTool::None } else { TouchTool::Mark(FillMark::Dot) };
            self.e.selected = None;
        }
        chrome::divider(ui);
        // Sign: pick a saved signature (then tap the page), or make a new one.
        let sign_on = self.e.tool == TouchTool::Sign;
        let resp = chrome::tool(ui, "signature", "Sign", sign_on, accent);
        let mut choose = None;
        let mut new_pad = None;
        let mut remove = None;
        let labels: Vec<String> = self.sigs.list.iter().enumerate().map(|(i, s)| s.label(i)).collect();
        egui::Popup::menu(&resp).align(egui::RectAlign::TOP_START).show(|ui| {
            ui.set_min_width(220.0);
            for (i, label) in labels.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(label).min_size(egui::vec2(170.0, 40.0))).clicked() {
                        choose = Some(i);
                        ui.close();
                    }
                    if ui.add(egui::Button::new("✕").min_size(egui::vec2(36.0, 40.0))).on_hover_text("Forget").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if !labels.is_empty() {
                ui.separator();
            }
            if chrome::menu_item(ui, "Draw a new signature…") {
                new_pad = Some(false);
                ui.close();
            }
            if chrome::menu_item(ui, "Type a signature / initials…") {
                new_pad = Some(true);
                ui.close();
            }
        });
        if let Some(i) = choose {
            self.e.sig_choice = Some(i);
            self.e.tool = TouchTool::Sign;
            self.e.selected = None;
        }
        if let Some(i) = remove {
            self.sigs.remove(i);
            self.e.sig_choice = None;
            if self.e.tool == TouchTool::Sign {
                self.e.tool = TouchTool::None;
            }
        }
        if let Some(typed) = new_pad {
            self.sheet = Sheet::sign_pad(typed);
        }
        self.touch_tool(ui, "dateTime", "Date", TouchTool::Date, accent);
        if chrome::tool(ui, "certificate", "Certificate", self.e.tool == TouchTool::CertBox, accent).clicked() {
            self.start_cert_sign();
        }
        let (ts, ms) = (&mut self.e.text_size, &mut self.e.mark_size);
        chrome::tool_menu(ui, "size", "Size", accent, |ui| {
            ui.label(RichText::new("Text and marks").weak());
            for (label, t, m) in [("Small", 9.0, 10.0), ("Normal", 12.0, 14.0), ("Large", 16.0, 20.0), ("Extra large", 22.0, 28.0)] {
                let label = if (*ts - t).abs() < 0.01 { format!("• {label}") } else { label.to_owned() };
                if chrome::menu_item(ui, &label) {
                    *ts = t;
                    *ms = m;
                    ui.close();
                }
            }
        });
    }

    fn page_tools(&mut self, ui: &mut egui::Ui, accent: Color32) -> Option<FileAction> {
        let mut action = None;
        let n = self.page_count();
        let cur = self.v.current_page.min(n.saturating_sub(1));
        if chrome::tool(ui, "multiplePages", &format!("Page {}/{}", cur + 1, n), false, accent).clicked() {
            self.sheet = Sheet::Thumbs;
        }
        chrome::divider(ui);
        let mut op: Option<(&str, Box<dyn FnOnce(&mut slate_pdf::PdfDoc) -> Result<(), slate_pdf::PdfError>>, Option<usize>)> = None;
        if chrome::tool(ui, "rotate", "Turn left", false, accent).clicked() {
            op = Some(("Turned", Box::new(move |d| d.rotate_pages(&[cur], -90)), Some(cur)));
        }
        if chrome::tool(ui, "rotate", "Turn right", false, accent).clicked() {
            op = Some(("Turned", Box::new(move |d| d.rotate_pages(&[cur], 90)), Some(cur)));
        }
        if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
            op = Some(("Deleted page", Box::new(move |d| d.delete_pages(&[cur])), Some(cur.min(n.saturating_sub(2)))));
        }
        if chrome::tool(ui, "chevronUp", "Move up", false, accent).clicked() && cur > 0 {
            op = Some(("Moved", Box::new(move |d| d.move_pages(&[cur], cur - 1)), Some(cur - 1)));
        }
        if chrome::tool(ui, "chevronDown", "Move down", false, accent).clicked() && cur + 1 < n {
            op = Some(("Moved", Box::new(move |d| d.move_pages(&[cur], cur + 1)), Some(cur + 1)));
        }
        chrome::divider(ui);
        if chrome::tool(ui, "blankPage", "Blank page", false, accent).clicked() {
            op = Some(("Added a blank page", Box::new(move |d| d.insert_blank_page(cur + 1, None)), Some(cur + 1)));
        }
        if chrome::tool(ui, "copy", "Duplicate", false, accent).clicked() {
            op = Some(("Duplicated", Box::new(move |d| d.duplicate_pages(&[cur])), Some(cur + 1)));
        }
        let mut crop: Option<Option<f32>> = None;
        chrome::tool_menu(ui, "crop", "Crop", accent, |ui| {
            ui.label(RichText::new("Trim the edges of this page").weak());
            for (label, f) in [("Trim 5 %", 0.05), ("Trim 10 %", 0.10), ("Trim 15 %", 0.15)] {
                if chrome::menu_item(ui, label) {
                    crop = Some(Some(f));
                    ui.close();
                }
            }
            if chrome::menu_item(ui, "Undo crop (full page)") {
                crop = Some(None);
                ui.close();
            }
        });
        if let Some(c) = crop {
            let (w, h) = self.cache.pages.get(cur).map(|p| (p.width, p.height)).unwrap_or((0.0, 0.0));
            op = match c {
                Some(f) => Some(("Cropped", Box::new(move |d| d.crop_pages(&[cur], [w * f, h * f, w * (1.0 - f), h * (1.0 - f)])), Some(cur))),
                None => Some(("Crop removed", Box::new(move |d| d.reset_crop(&[cur])), Some(cur))),
            };
        }
        if chrome::tool(ui, "export", "Extract", false, accent).clicked() {
            self.extract_pending = Some(vec![cur]);
            // A copy that never becomes the open document (`write_to` writes just this page).
            action = Some(FileAction::SaveCopy { suffix: " (extract)", adopt: false });
        }
        if let Some((done, f, go)) = op {
            if self.edit(f) {
                self.v.clear_textures = true;
                self.say(format!("{done} (Undo takes it back)"));
                if let Some(p) = go {
                    self.v.current_page = p;
                    self.v.jump_to = Some((p, 0.0));
                }
            }
        }
        action
    }

    fn file_tools(&mut self, ui: &mut egui::Ui, accent: Color32) -> Option<FileAction> {
        let mut action = None;
        if chrome::tool(ui, "save", "Save", false, accent).clicked() {
            self.cert.save_direct = true;
            action = Some(FileAction::Save);
        }
        if chrome::tool(ui, "flatten", "Save flattened", false, accent).clicked() {
            self.cert.stage = super::cert::Stage::FlattenSave { all: false };
        }
        if chrome::tool(ui, "saveAs", "Save as", false, accent).clicked() {
            action = Some(FileAction::SaveAs("pdf"));
        }
        if chrome::tool(ui, "share", "Share", false, accent).clicked() {
            action = Some(FileAction::Share);
        }
        if chrome::tool(ui, "print", "Print", false, accent).clicked() {
            action = Some(FileAction::Print);
        }
        let current = self.print_paper;
        chrome::tool_menu(ui, "size", "Paper", accent, |ui| {
            if chrome::menu_item(ui, &crate::paper::label("Actual size", current.is_none())) {
                self.print_paper = None;
                ui.close();
            }
            for (name, w, h) in crate::paper::PAPERS {
                if chrome::menu_item(ui, &crate::paper::label(name, current == Some((w, h)))) {
                    self.print_paper = Some((w, h));
                    ui.close();
                }
            }
        });
        chrome::divider(ui);
        if chrome::tool(ui, "protect", "Password", false, accent).clicked() {
            self.sheet = Sheet::Password { pw: String::new(), pw2: String::new(), owner: String::new() };
        }
        if chrome::tool(ui, "properties", "Properties", false, accent).clicked() {
            self.sheet = self.properties_sheet();
        }
        if chrome::tool(ui, "info", "Info", false, accent).clicked() {
            action = Some(FileAction::Info);
        }
        action
    }

    /// Tools for the selected comment / added item / link.
    fn selection_tools(&mut self, ui: &mut egui::Ui, sel: Sel, accent: Color32) {
        if chrome::tool(ui, "check", "Done", false, accent).clicked() {
            self.e.selected = None;
            return;
        }
        chrome::divider(ui);
        match sel {
            Sel::Comment(c) => {
                if chrome::tool(ui, "editor", "Edit note", false, accent).clicked() {
                    self.e.entry = Some(Entry { kind: EntryKind::CommentText(c.index), page: c.page, rect: c.rect, text: c.text.clone(), focus: true });
                }
                if chrome::tool(ui, "comment", "Reply", false, accent).clicked() {
                    self.e.entry = Some(Entry { kind: EntryKind::Reply(c.index), page: c.page, rect: c.rect, text: String::new(), focus: true });
                }
                if !c.locked && chrome::tool(ui, "deleteComment", "Delete", false, accent).clicked() {
                    let (p, i) = (c.page, c.index);
                    self.edit(|d| d.delete_comment(p, i));
                }
                ui.label(RichText::new(format!("{} · drag to move", kind_name(&c.kind))).weak());
            }
            Sel::Added(a) => {
                if let Some(t) = &a.text {
                    if chrome::tool(ui, "editor", "Edit text", false, accent).clicked() {
                        self.e.entry = Some(Entry { kind: EntryKind::AddedText(a.index), page: a.page, rect: a.rect, text: t.clone(), focus: true });
                    }
                }
                if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
                    let (p, i) = (a.page, a.index);
                    self.edit(|d| d.delete_added(p, i));
                }
                ui.label(RichText::new("Drag to move · corner to resize").weak());
            }
            Sel::Link(l) => {
                if chrome::tool(ui, "link", "Change", false, accent).clicked() {
                    let text = match &l.target {
                        slate_pdf::LinkTarget::Uri(u) => u.clone(),
                        _ => "https://".into(),
                    };
                    self.e.entry = Some(Entry { kind: EntryKind::LinkUrl(l.index), page: l.page, rect: l.rect, text, focus: true });
                }
                if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
                    let (p, i) = (l.page, l.index);
                    self.edit(|d| d.delete_link(p, i));
                }
                let target = match &l.target {
                    slate_pdf::LinkTarget::Page(p) => format!("Goes to page {}", p + 1),
                    slate_pdf::LinkTarget::Uri(u) => u.clone(),
                    slate_pdf::LinkTarget::Other(o) => o.clone(),
                };
                ui.label(RichText::new(target).weak());
            }
        }
    }

    /// The input row: what it's for, the text, OK / Cancel.
    fn entry_row(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let Some(mut en) = self.e.entry.take() else { return };
        let (hint, multi) = match &en.kind {
            EntryKind::Note => ("Note text", true),
            EntryKind::TextBox => ("Text box text", true),
            EntryKind::FillText => ("Text to type on the page", false),
            EntryKind::AddText => ("New text", true),
            EntryKind::EditLine(_) => ("Change this line", false),
            EntryKind::Field(_) => ("Field value", false),
            EntryKind::NewLink | EntryKind::LinkUrl(_) => ("Web address", false),
            EntryKind::CommentText(_) => ("Note text", true),
            EntryKind::Reply(_) => ("Your reply", true),
            EntryKind::AddedText(_) => ("Text", true),
        };
        let mut ok = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).max(120.0);
            let edit = if multi {
                egui::TextEdit::multiline(&mut en.text).desired_rows(2).hint_text(hint).desired_width(w)
            } else {
                egui::TextEdit::singleline(&mut en.text).hint_text(hint).desired_width(w)
            };
            let resp = ui.add(edit);
            if std::mem::take(&mut en.focus) {
                resp.request_focus();
            }
            if !multi && resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            if ui.add(egui::Button::new(RichText::new("OK").strong()).fill(accent.gamma_multiply(0.35)).min_size(egui::vec2(60.0, 40.0))).clicked() {
                ok = true;
            }
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 40.0))).clicked() {
                cancel = true;
            }
        });
        if ok {
            self.commit_entry(en);
        } else if !cancel {
            self.e.entry = Some(en);
        }
    }

    pub(super) fn commit_entry(&mut self, en: Entry) {
        let (page, r, text) = (en.page, en.rect, en.text.clone());
        let at = [r[0], r[1]];
        if text.trim().is_empty() && !matches!(en.kind, EntryKind::Field(_) | EntryKind::EditLine(_)) {
            return;
        }
        match en.kind {
            EntryKind::Note => {
                self.edit(|d| d.add_note(page, at, &text));
            }
            EntryKind::TextBox => {
                let lines = text.lines().count().max(1) as f32;
                let rect = [r[0], r[1], r[0] + 180.0, r[1] + lines * 12.0 * 1.35 + 10.0];
                let look = Look { color: Some(self.e.color), width: Some(1.0), ..Look::default() };
                self.edit(|d| d.add_text_box(page, rect, &text, 12.0, look));
            }
            EntryKind::FillText => {
                let size = self.e.text_size;
                self.edit(|d| d.add_typed_text(page, at, &text, size, [0.0, 0.0, 0.0]));
            }
            EntryKind::AddText => {
                let style = self.e.style;
                let w = self.cache.pages.get(page).map(|p| p.width).unwrap_or(600.0);
                let rect = [r[0], r[1], (r[0] + 240.0).min(w - 8.0).max(r[0] + 40.0), r[1] + style.size as f32 * 1.3];
                self.edit(|d| d.add_text(page, rect, &text, style));
            }
            EntryKind::EditLine(i) => {
                self.edit(|d| d.edit_text_line(page, i, &text));
            }
            EntryKind::Field(name) => {
                self.edit(|d| d.set_field_text(&name, &text));
            }
            EntryKind::NewLink => {
                self.edit(|d| d.add_link_uri(page, r, &text));
            }
            EntryKind::LinkUrl(i) => {
                self.edit(|d| d.set_link_uri(page, i, &text));
            }
            EntryKind::CommentText(i) => {
                self.edit(|d| d.set_comment_text(page, i, &text));
            }
            EntryKind::Reply(i) => {
                if self.edit(|d| d.reply_to_comment(page, i, &text)) {
                    self.say("Reply added");
                }
            }
            EntryKind::AddedText(i) => {
                self.edit(|d| d.set_added_text(page, i, &text));
            }
        }
    }

    /// Fill & Sign › Sign: put the chosen saved signature at `pt`.
    pub(super) fn place_signature(&mut self, page: usize, pt: slate_pdf::ViewPoint) {
        let Some(sig) = self.e.sig_choice.and_then(|i| self.sigs.list.get(i)).cloned() else {
            self.sheet = Sheet::sign_pad(false);
            return;
        };
        match sig {
            super::sig::SavedSig::Drawn { strokes, w, h } => {
                // About 150 × 50 points, centred on the tap.
                let s = (150.0 / w.max(1.0)).min(50.0 / h.max(1.0));
                let (ox, oy) = (pt[0] - w * s / 2.0, pt[1] - h * s / 2.0);
                let placed: Vec<Vec<[f32; 2]>> = strokes.iter().map(|st| st.iter().map(|p| [ox + p[0] * s, oy + p[1] * s]).collect()).collect();
                self.edit(|d| d.add_drawn_signature(page, &placed, Some([0.05, 0.1, 0.45])));
            }
            super::sig::SavedSig::Typed { name, initials } => {
                let height = if initials { 22.0 } else { 32.0 };
                let half = name.chars().count() as f32 * height as f32 * 0.22;
                self.edit(|d| d.add_typed_signature(page, [pt[0] - half, pt[1]], &name, height));
            }
        }
    }
}

/// Comment kinds in words.
pub(super) fn kind_name(kind: &str) -> &str {
    match kind {
        "Text" => "Note",
        "FreeText" => "Text box",
        "Ink" => "Drawing",
        "Square" => "Rectangle",
        "Circle" => "Oval",
        "StrikeOut" => "Strike-through",
        "Highlight" | "Underline" | "Squiggly" | "Line" | "Polygon" | "PolyLine" | "Stamp" => kind,
        other => other,
    }
}
