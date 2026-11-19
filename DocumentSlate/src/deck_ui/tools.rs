//! Deck Slate edit mode: the WPS-style bottom bar (approved list: docs/DeckSlate-features.md).
//! An icon tool row for the current tab above the icon tab strip: Slides · Text · Insert ·
//! Design · File. While a shape, picture or table is selected its own tools replace the tab's
//! (fill, outline colour, thickness, dash, arrowheads, text, rotate, order, duplicate, delete).
//! Same look as Word Slate: `chrome::tool` buttons, `chrome::tool_menu` pop-ups opening upwards.

use eframe::egui::{self, Color32, RichText};
use slate_deck::edit::{HAlign, SlideSize, VAlign};
use slate_deck::shapes::{self, Paint, ShapeLook};
use slate_deck::touch::ObjectKind;
use slate_deck::{PdfLayout, ShapeId};

use super::DeckScreen;
use crate::chrome;
use crate::edit_bar::FileAction;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Slides,
    Text,
    Insert,
    Design,
    File,
}

/// (tab, icon, label).
const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Slides, "multiplePages", "Slides"),
    (Tab::Text, "fontcolor", "Text"),
    (Tab::Insert, "shapes", "Insert"),
    (Tab::Design, "themes", "Design"),
    (Tab::File, "file", "File"),
];

/// An input row open under the tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Row {
    #[default]
    None,
    Table,
    Link,
    Footer,
    Notes,
}

/// Tool-bar state kept between frames.
pub struct TabState {
    pub tab: Tab,
    pub row: Row,
    rows: u32,
    cols: u32,
    url: String,
    link_text: String,
    footer: String,
    notes: String,
    /// The slide whose notes are in `notes`.
    notes_slide: Option<usize>,
    /// The notes box was typed in since it was last written to the slide.
    notes_dirty: bool,
    /// Background colour for this slide only (else all slides).
    bg_this_only: bool,
    /// Scroll the slide strip to the current slide next frame.
    pub scroll_strip: bool,
}

impl Default for TabState {
    fn default() -> Self {
        Self {
            tab: Tab::Slides,
            row: Row::None,
            rows: 3,
            cols: 3,
            url: String::new(),
            link_text: String::new(),
            footer: String::new(),
            notes: String::new(),
            notes_slide: None,
            notes_dirty: false,
            bg_this_only: false,
            scroll_strip: false,
        }
    }
}

/// Fill / outline / text colours (RRGGBB): greys, strong colours, then soft ones.
pub const COLOURS: [&str; 20] = [
    "FFFFFF", "D9D9D9", "808080", "404040", "000000", //
    "C00000", "FF0000", "FFC000", "FFFF00", "92D050", //
    "00B050", "00B0F0", "0070C0", "002060", "7030A0", //
    "F8CBAD", "FFE699", "C5E0B4", "BDD7EE", "D9C3E9",
];

/// Outline thicknesses offered, points.
pub const WEIGHTS: [f64; 8] = [0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0];

/// Fonts offered in Text › Font: what slides commonly name (drawn with metric-compatible
/// substitutes where the real font isn't on the phone) plus Android's own.
const FONTS: [&str; 10] = ["Calibri", "Calibri Light", "Arial", "Aptos", "Times New Roman", "Georgia", "Verdana", "Courier New", "Roboto", "Noto Serif"];

/// The shape gallery's sections: (title, presets). Ids are the engine's preset names.
pub const GALLERY: [(&str, &[&str]); 4] = [
    (
        "Basic shapes",
        &["rect", "roundRect", "snip1Rect", "ellipse", "triangle", "rtTriangle", "diamond", "parallelogram", "trapezoid", "pentagon", "hexagon", "octagon", "homePlate", "chevron", "star5", "star4", "star6", "heart", "plus", "cloud", "can", "foldedCorner", "smileyFace", "lightningBolt", "frame"],
    ),
    ("Speech bubbles", &["wedgeRectCallout", "wedgeRoundRectCallout", "wedgeEllipseCallout", "cloudCallout", "borderCallout1", "rightArrowCallout", "downArrowCallout"]),
    (
        "Arrows",
        &["rightArrow", "leftArrow", "upArrow", "downArrow", "leftRightArrow", "upDownArrow", "quadArrow", "bentArrow", "uturnArrow", "bentUpArrow", "leftUpArrow", "curvedRightArrow", "curvedLeftArrow", "notchedRightArrow", "stripedRightArrow", "circularArrow"],
    ),
    ("Lines", &["line", "straightConnector1", LINE_BOTH, "bentConnector3", "curvedConnector3"]),
];

/// Gallery entry for a straight line with arrowheads at both ends (a line + two heads).
pub const LINE_BOTH: &str = "line+both";

fn hex_rgb(hex: &str) -> [u8; 3] {
    let c = u32::from_str_radix(hex, 16).unwrap_or(0);
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A grid of colour circles (5 per row). `current` gets a ring. Returns the tapped RRGGBB.
fn swatches(ui: &mut egui::Ui, current: Option<[u8; 3]>) -> Option<&'static str> {
    let mut picked = None;
    let ring = ui.visuals().text_color();
    for row in COLOURS.chunks(5) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for hex in row {
                let c = hex_rgb(hex);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
                ui.painter().circle_filled(r.center(), 15.0, rgb(c));
                ui.painter().circle_stroke(r.center(), 15.0, egui::Stroke::new(1.0, ring.gamma_multiply(0.4)));
                if current == Some(c) {
                    ui.painter().circle_stroke(r.center(), 18.5, egui::Stroke::new(2.5, ring));
                }
                if resp.clicked() {
                    picked = Some(*hex);
                }
            }
        });
    }
    picked
}

fn solid(p: Paint) -> Option<[u8; 3]> {
    match p {
        Paint::Solid([r, g, b, _]) => Some([r, g, b]),
        _ => None,
    }
}

/// A full-width menu entry that shows when it's the current choice (highlighted).
fn choice(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    ui.add(egui::Button::selectable(on, label).min_size(egui::vec2(ui.available_width(), 40.0))).clicked()
}

/// Draw the tool row(s) and the tab strip. Returns a file action a tool asked for.
pub fn show(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) -> Option<FileAction> {
    let mut action = None;
    let object = s.selected_object();
    let out = egui::ScrollArea::horizontal().id_salt("deck_tools").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if let Some((id, kind)) = object {
                object_tools(ui, s, id, kind, accent);
                return;
            }
            match s.tabs.tab {
                Tab::Slides => slide_tools(ui, s, accent),
                Tab::Text => text_tools(ui, s, accent),
                Tab::Insert => action = insert_tools(ui, s, accent),
                Tab::Design => design_tools(ui, s, accent),
                Tab::File => action = file_tools(ui, s, accent),
            }
        });
    });
    chrome::scroll_hints(ui, &out);
    if object.is_none() {
        rows(ui, s, accent);
    }
    // Tab strip: icon + label, the current tab in the accent colour with a bar under it.
    let (strip, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::hover());
    let w = strip.width() / TABS.len() as f32;
    for (i, (t, icon, label)) in TABS.into_iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(strip.left() + i as f32 * w, strip.top()), egui::vec2(w, strip.height()));
        let resp = ui.interact(r, ui.scope_id().with(("deck-tab", i)), egui::Sense::click());
        let selected = s.tabs.tab == t && object.is_none();
        let fg = if selected { accent } else { ui.visuals().text_color().gamma_multiply(0.7) };
        let p = ui.painter();
        chrome::icon(p, egui::Rect::from_center_size(egui::pos2(r.center().x, r.top() + 20.0), egui::Vec2::splat(chrome::ICON)), icon, fg, accent);
        p.text(egui::pos2(r.center().x, r.bottom() - 12.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.5), fg);
        if selected {
            let bar = egui::Rect::from_center_size(egui::pos2(r.center().x, r.bottom() - 2.0), egui::vec2(28.0, 3.0));
            p.rect_filled(bar, 1.5, accent);
        }
        if resp.clicked() {
            s.tabs.tab = t;
            // A tab tap while an object is selected: leave the object, show that tab.
            if object.is_some() {
                let r = s.doc.deselect();
                s.act(r);
            }
        }
    }
    action
}

// ===================================================================== object tools

fn object_tools(ui: &mut egui::Ui, s: &mut DeckScreen, id: ShapeId, kind: ObjectKind, accent: Color32) {
    let look = s.doc.shape_look(id);
    match kind {
        ObjectKind::Shape | ObjectKind::Connector => {
            if let Some(look) = &look {
                shape_tools(ui, s, id, look, accent);
            }
        }
        ObjectKind::Picture => {
            if let Some(look) = &look {
                outline_tools(ui, s, look, accent, "Border");
            }
            crop_menu(ui, s, id, accent);
        }
        ObjectKind::Table => table_tools(ui, s, id, accent),
        _ => {}
    }
    chrome::divider(ui);
    arrange_tools(ui, s, kind, accent);
}

/// Fill · Outline · Weight · Dash · (Arrows) · Type · Text colour · Change shape.
fn shape_tools(ui: &mut egui::Ui, s: &mut DeckScreen, id: ShapeId, look: &ShapeLook, accent: Color32) {
    if !look.is_line {
        chrome::tool_menu(ui, "shapes", "Fill", accent, |ui| {
            if choice(ui, "No fill", look.fill == Paint::None) {
                let r = s.doc.fill_color(&[], None);
                s.act(r);
                ui.close();
            }
            if let Some(hex) = swatches(ui, solid(look.fill)) {
                let r = s.doc.fill_color(&[], Some(&format!("#{hex}")));
                s.act(r);
                ui.close();
            }
            ui.label(RichText::new("Transparency").weak());
            ui.horizontal(|ui| {
                let cur = match look.fill {
                    Paint::Solid([.., a]) => 1.0 - f64::from(a) / 255.0,
                    _ => -1.0,
                };
                for (label, t) in [("0 %", 0.0), ("25 %", 0.25), ("50 %", 0.5), ("75 %", 0.75)] {
                    let on = (cur - t).abs() < 0.03;
                    if ui.add(egui::Button::new(label).selected(on).min_size(egui::vec2(44.0, 36.0))).clicked() {
                        let r = s.doc.fill_transparency(&[], t);
                        s.act(r);
                    }
                }
            });
        });
    }
    outline_tools(ui, s, look, accent, if look.is_line { "Color" } else { "Outline" });
    if look.is_line {
        arrow_menu(ui, s, look, accent);
    }
    if !look.is_line && look.has_text_body {
        chrome::divider(ui);
        if chrome::tool(ui, "addText", "Type", false, accent).clicked() {
            let r = s.doc.edit_text(id, None);
            s.act(r);
            s.keyboard_wanted = s.typing();
        }
        chrome::tool_menu(ui, "fontcolor", "Text col.", accent, |ui| {
            if let Some(hex) = swatches(ui, None) {
                let r = s.doc.font_color(&format!("#{hex}"));
                s.act(r);
                ui.close();
            }
        });
        size_menu(ui, s, accent);
        if chrome::tool(ui, "bold", "Bold", s.fmt_on("bold"), accent).clicked() {
            let r = s.doc.bold(None);
            s.act(r);
        }
        chrome::tool_menu(ui, "alignCenter", "Align text", accent, |ui| {
            align_items(ui, s);
        });
        chrome::tool_menu(ui, "addShape", "Change", accent, |ui| {
            if let Some(name) = gallery(ui, false) {
                let r = s.doc.change_shape(&[], name);
                s.act(r);
                ui.close();
            }
        });
    }
}

/// Outline colour · Weight · Dash (shapes, lines, picture borders).
fn outline_tools(ui: &mut egui::Ui, s: &mut DeckScreen, look: &ShapeLook, accent: Color32, label: &str) {
    chrome::tool_menu(ui, "pen", label, accent, |ui| {
        if choice(ui, "No line", look.line == Paint::None) {
            let r = s.doc.outline_color(&[], None);
            s.act(r);
            ui.close();
        }
        if let Some(hex) = swatches(ui, solid(look.line)) {
            let r = s.doc.outline_color(&[], Some(&format!("#{hex}")));
            s.act(r);
            ui.close();
        }
    });
    let fg = ui.visuals().text_color();
    let weight_label = if look.line == Paint::None { "Weight".to_owned() } else { format!("{} pt", trim(look.line_width)) };
    chrome::tool_menu(ui, "thickness", &weight_label, accent, |ui| {
        for w in WEIGHTS {
            let on = look.line != Paint::None && (look.line_width - w).abs() < 0.01;
            if line_item(ui, &format!("{} pt", trim(w)), on, fg, w as f32, None) {
                let r = s.doc.outline_width(&[], w);
                s.act(r);
                ui.close();
            }
        }
    });
    chrome::tool_menu(ui, "lineSpacing", "Dash", accent, |ui| {
        for (name, label, pattern) in [
            ("solid", "Solid", None),
            ("sysDot", "Round dots", Some((1.0, 1.0))),
            ("sysDash", "Square dots", Some((3.0, 1.0))),
            ("dash", "Dashes", Some((4.0, 3.0))),
            ("dashDot", "Dash dot", Some((4.0, 3.0))),
            ("lgDash", "Long dashes", Some((8.0, 3.0))),
            ("lgDashDot", "Long dash dot", Some((8.0, 3.0))),
        ] {
            if line_item(ui, label, look.dash == name, fg, 2.5, pattern) {
                let r = s.doc.outline_dash(&[], name);
                s.act(r);
                ui.close();
            }
        }
    });
}

/// Arrowheads of a line: which ends, and their style.
fn arrow_menu(ui: &mut egui::Ui, s: &mut DeckScreen, look: &ShapeLook, accent: Color32) {
    chrome::tool_menu(ui, "straightArrowConnector", "Arrows", accent, |ui| {
        let (h, t) = (look.head != "none", look.tail != "none");
        let style = if t { look.tail.clone() } else if h { look.head.clone() } else { "triangle".into() };
        for (label, head, tail) in [("No arrowheads", false, false), ("Arrow at end", false, true), ("Arrow at start", true, false), ("Arrows at both ends", true, true)] {
            if choice(ui, label, h == head && t == tail) {
                let k = |on: bool| if on { style.as_str() } else { "none" };
                let r = s.doc.arrowheads(&[], Some(k(head)), Some(k(tail)));
                s.act(r);
                ui.close();
            }
        }
        ui.separator();
        ui.label(RichText::new("Arrowhead style").weak());
        for (name, label) in [("triangle", "Triangle"), ("arrow", "Open arrow"), ("stealth", "Stealth"), ("diamond", "Diamond"), ("oval", "Circle")] {
            if choice(ui, label, (h || t) && style == name) {
                let (head, tail) = match (h, t) {
                    (false, false) => (None, Some(name)),
                    _ => (h.then_some(name), t.then_some(name)),
                };
                let r = s.doc.arrowheads(&[], head, tail);
                s.act(r);
                ui.close();
            }
        }
    });
}

/// A menu entry with a drawn line sample (thickness / dash pickers).
fn line_item(ui: &mut egui::Ui, label: &str, on: bool, fg: Color32, width: f32, dash: Option<(f32, f32)>) -> bool {
    let w = ui.available_width().max(170.0);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(r.shrink(1.0), 6.0, ui.visuals().selection.bg_fill.gamma_multiply(0.5));
    } else if resp.hovered() {
        p.rect_filled(r.shrink(1.0), 6.0, fg.gamma_multiply(0.08));
    }
    p.text(egui::pos2(r.left() + 10.0, r.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), fg);
    let (x0, x1, y) = (r.left() + 112.0, r.right() - 10.0, r.center().y);
    let stroke = egui::Stroke::new(width.clamp(0.5, 8.0), fg);
    match dash {
        None => {
            p.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
        }
        Some((d, g)) => {
            let unit = width.max(1.5);
            let mut x = x0;
            while x < x1 {
                let e = (x + d * unit).min(x1);
                p.line_segment([egui::pos2(x, y), egui::pos2(e, y)], stroke);
                x = e + g * unit;
            }
        }
    }
    resp.clicked()
}

fn trim(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Picture crop presets.
fn crop_menu(ui: &mut egui::Ui, s: &mut DeckScreen, id: ShapeId, accent: Color32) {
    chrome::tool_menu(ui, "crop", "Crop", accent, |ui| {
        let rect = s.doc.objects().into_iter().find(|o| o.id == id).map(|o| o.rect);
        if chrome::menu_item(ui, "Trim edges 10 %") {
            let r = s.doc.crop(&[], 0.1, 0.1, 0.1, 0.1);
            s.act(r);
            ui.close();
        }
        if chrome::menu_item(ui, "Square (center)") {
            if let Some((_, _, w, h)) = rect {
                let (l, t) = if w > h { ((1.0 - h / w) / 2.0, 0.0) } else { (0.0, (1.0 - w / h) / 2.0) };
                let r = s.doc.crop(&[], l, t, l, t);
                s.act(r);
            }
            ui.close();
        }
        if chrome::menu_item(ui, "Remove crop") {
            let r = s.doc.crop(&[], 0.0, 0.0, 0.0, 0.0);
            s.act(r);
            ui.close();
        }
    });
}

/// Table: rows and columns at the cell being typed in (else at the end), delete.
fn table_tools(ui: &mut egui::Ui, s: &mut DeckScreen, id: ShapeId, accent: Color32) {
    let (rows, cols) = s.doc.table_size(id).unwrap_or((1, 1));
    let (row, col) = s.doc.current_cell().filter(|(t, ..)| *t == id).map(|(_, r, c)| (r, c)).unwrap_or((rows.saturating_sub(1), cols.saturating_sub(1)));
    let mut r: Option<Result<(), String>> = None;
    if chrome::tool(ui, "insertAbove", "Row above", false, accent).clicked() {
        r = Some(s.doc.table_insert_row(id, row, true));
    }
    if chrome::tool(ui, "insertBelow", "Row below", false, accent).clicked() {
        r = Some(s.doc.table_insert_row(id, row, false));
    }
    if chrome::tool(ui, "insertLeft", "Col left", false, accent).clicked() {
        r = Some(s.doc.table_insert_column(id, col, true));
    }
    if chrome::tool(ui, "insertRight", "Col right", false, accent).clicked() {
        r = Some(s.doc.table_insert_column(id, col, false));
    }
    if chrome::tool(ui, "rowHeight", "Del row", false, accent).clicked() && rows > 1 {
        r = Some(s.doc.table_delete_row(id, row));
    }
    if chrome::tool(ui, "colWidth", "Del col", false, accent).clicked() && cols > 1 {
        r = Some(s.doc.table_delete_column(id, col));
    }
    if chrome::tool(ui, "addText", "Type", false, accent).clicked() {
        r = Some(s.doc.edit_cell(id, row, col));
        s.keyboard_wanted = true;
    }
    if let Some(r) = r {
        s.act(r);
    }
}

/// Rotate · Flip · Align · Front · Back · Duplicate · Delete.
fn arrange_tools(ui: &mut egui::Ui, s: &mut DeckScreen, kind: ObjectKind, accent: Color32) {
    if kind != ObjectKind::Table {
        if chrome::tool(ui, "rotate", "Rotate", false, accent).clicked() {
            let r = s.doc.rotate_right(&[]);
            s.act(r);
        }
        chrome::tool_menu(ui, "transform", "Flip", accent, |ui| {
            for (label, h) in [("Flip horizontal", true), ("Flip vertical", false)] {
                if chrome::menu_item(ui, label) {
                    let r = s.doc.flip(&[], h);
                    s.act(r);
                    ui.close();
                }
            }
        });
    }
    chrome::tool_menu(ui, "align", "Align", accent, |ui| {
        ui.label(RichText::new("On the slide").weak());
        for (label, edge) in [("Left", "left"), ("Center", "center"), ("Right", "right"), ("Top", "top"), ("Middle", "middle"), ("Bottom", "bottom")] {
            if chrome::menu_item(ui, label) {
                let r = s.doc.align_objects(&[], edge);
                s.act(r);
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "bringForward", "Front", false, accent).clicked() {
        let r = s.doc.bring_to_front(&[]);
        s.act(r);
    }
    if chrome::tool(ui, "sendBackward", "Back", false, accent).clicked() {
        let r = s.doc.send_to_back(&[]);
        s.act(r);
    }
    if chrome::tool(ui, "copy", "Duplicate", false, accent).clicked() {
        let r = s.doc.duplicate_objects(&[]);
        s.act(r);
    }
    if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
        let r = s.doc.delete_objects(&[]);
        s.act(r);
    }
}

// ===================================================================== shape gallery

/// The shape gallery (inside a pop-up): drawn previews by section, then every other shape the
/// engine knows under "All shapes". `lines` = include the Lines section. Returns the pick.
pub fn gallery(ui: &mut egui::Ui, lines: bool) -> Option<&'static str> {
    let mut out = None;
    let fg = ui.visuals().text_color();
    let accent = ui.visuals().selection.bg_fill;
    ui.set_max_width(6.0 * 50.0);
    for (title, names) in GALLERY {
        if title == "Lines" && !lines {
            continue;
        }
        ui.label(RichText::new(title).strong());
        if let Some(n) = preview_grid(ui, names.iter().copied(), fg, accent) {
            out = Some(n);
        }
        ui.add_space(4.0);
    }
    egui::CollapsingHeader::new("All shapes").id_salt(("deck-all-shapes", lines)).show(ui, |ui| {
        let mut cat = "";
        let all = shapes::all_presets();
        for (name, _, category) in all.iter().copied() {
            if (!lines && shapes::preset_is_line(name)) || name == "textBox" || name.starts_with("actionButton") {
                continue;
            }
            if category != cat {
                cat = category;
                ui.label(RichText::new(category).weak());
                let names: Vec<&'static str> = all
                    .iter()
                    .filter(|(n, _, c)| *c == category && n != &"textBox" && !n.starts_with("actionButton") && (lines || !shapes::preset_is_line(n)))
                    .map(|(n, _, _)| *n)
                    .collect();
                if let Some(n) = preview_grid(ui, names.into_iter(), fg, accent) {
                    out = Some(n);
                }
            }
        }
    });
    out
}

fn preview_grid(ui: &mut egui::Ui, names: impl Iterator<Item = &'static str>, fg: Color32, accent: Color32) -> Option<&'static str> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
        for name in names {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::click());
            if resp.is_pointer_button_down_on() || resp.hovered() {
                ui.painter().rect_filled(r, 8.0, fg.gamma_multiply(0.10));
            }
            paint_preset(ui.painter(), r.shrink(9.0), name, fg, accent);
            let label = if name == LINE_BOTH { "Double arrow" } else { shapes::preset_label(name).unwrap_or(name) };
            if resp.on_hover_text(label).clicked() {
                out = Some(name);
            }
        }
    });
    out
}

/// A preset's outline drawn in `r` (gallery preview), lightly tinted inside.
pub fn paint_preset(p: &egui::Painter, r: egui::Rect, name: &str, fg: Color32, accent: Color32) {
    let base = if name == LINE_BOTH { "line" } else { name };
    let line = shapes::preset_is_line(base);
    // Lines: drawn left to right, slightly rising, so the arrowheads read.
    let (w, h) = if line { (r.width() as f64, r.height() as f64 * 0.6) } else { (r.width() as f64, r.height() as f64) };
    let origin = if line { egui::pos2(r.left(), r.top() + r.height() * 0.2) } else { r.min };
    let Some(paths) = shapes::preset_outline(base, w, h) else { return };
    for path in &paths {
        let pts: Vec<egui::Pos2> = path.points.iter().map(|(x, y)| egui::pos2(origin.x + x, origin.y + y)).collect();
        if path.filled && pts.len() > 2 {
            // Fan from the centroid: fine for the (nearly) star-shaped outlines in the gallery.
            let c = pts.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2()) / pts.len() as f32;
            let mut mesh = egui::Mesh::default();
            let tint = accent.gamma_multiply(0.35);
            mesh.colored_vertex(c.to_pos2(), tint);
            for q in &pts {
                mesh.colored_vertex(*q, tint);
            }
            let n = pts.len() as u32;
            for i in 0..n {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
            }
            p.add(egui::Shape::mesh(mesh));
        }
        if path.stroke {
            p.add(egui::Shape::Path(egui::epaint::PathShape { points: pts, closed: path.closed, fill: Color32::TRANSPARENT, stroke: egui::Stroke::new(1.6, fg).into() }));
        }
    }
    // Arrowheads for the arrow lines.
    if line {
        let (a, b) = (origin, origin + egui::vec2(w as f32, h as f32));
        let head = |tip: egui::Pos2, from: egui::Pos2| {
            let d = (tip - from).normalized();
            let n = egui::vec2(-d.y, d.x);
            p.add(egui::Shape::convex_polygon(vec![tip, tip - d * 8.0 + n * 4.0, tip - d * 8.0 - n * 4.0], fg, egui::Stroke::NONE));
        };
        if name == "straightConnector1" || name == LINE_BOTH {
            head(b, a);
        }
        if name == LINE_BOTH {
            head(a, b);
        }
    }
}

/// Insert a gallery shape in the middle of the slide (selected, ready to style).
pub fn insert_gallery_shape(s: &mut DeckScreen, name: &str) {
    let base = if name == LINE_BOTH { "line" } else { name };
    let (w, h) = if shapes::preset_is_line(base) {
        (220.0, 0.0)
    } else if base.contains("Callout") {
        (220.0, 130.0)
    } else if matches!(base, "upArrow" | "downArrow" | "upDownArrow") {
        (90.0, 170.0)
    } else if base.contains("Arrow") || matches!(base, "homePlate" | "chevron") {
        (200.0, 100.0)
    } else if matches!(base, "rect" | "roundRect" | "snip1Rect" | "parallelogram" | "trapezoid" | "can" | "foldedCorner" | "frame") {
        (200.0, 120.0)
    } else {
        (140.0, 140.0)
    };
    // Each new shape a little lower-right than the last one, so they don't stack exactly.
    let n = s.doc.objects().len() as f64;
    let mut rect = s.centred(w, h);
    rect.x += (n % 6.0) * 12.0 - 30.0;
    rect.y += (n % 6.0) * 12.0 - 30.0;
    let _ = s.doc.exit_text();
    let r = s.doc.insert_shape(base, rect, None);
    if let Some(id) = s.act(r) {
        if name == LINE_BOTH {
            let r = s.doc.arrowheads(&[id], Some("triangle"), Some("triangle"));
            s.act(r);
        }
        let r = s.doc.select(&[id]);
        s.act(r);
    }
}

// ===================================================================== tabs

fn slide_tools(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) {
    let cur = s.doc.current_slide();
    let n = s.doc.slide_count();
    let layouts = s.doc.layout_names();
    // Play first: the slideshow is what a deck is for (from the slide being edited).
    if chrome::tool(ui, "mediaPlay", "Play", false, accent).clicked() {
        s.start_show(true);
    }
    chrome::divider(ui);
    chrome::tool_menu(ui, "blankPage", "New slide", accent, |ui| {
        for l in &layouts {
            if chrome::menu_item(ui, l) {
                let r = s.doc.new_slide(Some(l));
                s.act(r);
                s.tabs.scroll_strip = true;
                ui.close();
            }
        }
        if layouts.is_empty() && chrome::menu_item(ui, "Blank slide") {
            let r = s.doc.new_slide(Some("blank"));
            s.act(r);
            ui.close();
        }
    });
    if chrome::tool(ui, "copy", "Duplicate", false, accent).clicked() {
        let r = s.doc.duplicate_slide(cur);
        s.act(r);
        s.tabs.scroll_strip = true;
    }
    if chrome::tool(ui, "deleteTable", "Delete", false, accent).clicked() {
        if n > 1 {
            let r = s.doc.delete_slide(cur);
            s.act(r);
        } else {
            s.notices.push("A presentation needs at least one slide".into());
        }
    }
    let hidden = s.doc.slides().get(cur).is_some_and(|x| x.hidden);
    if chrome::tool(ui, "hideInk", "Hide", hidden, accent).clicked() {
        let r = s.doc.set_slide_hidden(cur, !hidden);
        s.act(r);
    }
    if chrome::tool(ui, "chevronUp", "Earlier", false, accent).clicked() && cur > 0 {
        let r = s.doc.move_slide(cur, cur - 1);
        s.act(r);
        s.tabs.scroll_strip = true;
    }
    if chrome::tool(ui, "chevronDown", "Later", false, accent).clicked() && cur + 1 < n {
        let r = s.doc.move_slide(cur, cur + 1);
        s.act(r);
        s.tabs.scroll_strip = true;
    }
    chrome::divider(ui);
    let current_layout = s.doc.slides().get(cur).map(|x| x.layout.clone()).unwrap_or_default();
    chrome::tool_menu(ui, "onePage", "Layout", accent, |ui| {
        for l in &layouts {
            if choice(ui, l, *l == current_layout) {
                let r = s.doc.set_slide_layout(cur, l);
                s.act(r);
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "update", "Reset", false, accent).clicked() {
        let r = s.doc.reset_slide(cur);
        s.act(r);
    }
    if chrome::tool(ui, "showNotes", "Notes", s.tabs.row == Row::Notes, accent).clicked() {
        s.flush_notes();
        s.tabs.row = if s.tabs.row == Row::Notes { Row::None } else { Row::Notes };
        s.tabs.notes_slide = None;
    }
}

impl DeckScreen {
    /// Write typed speaker notes into their slide (once; nothing to do when the box is unchanged).
    pub(super) fn flush_notes(&mut self) {
        if !std::mem::take(&mut self.tabs.notes_dirty) {
            return;
        }
        let Some(slide) = self.tabs.notes_slide else { return };
        if slide < self.doc.slide_count() {
            let text = self.tabs.notes.clone();
            let r = self.doc.set_notes(slide, &text);
            self.act(r);
        }
    }
}

/// Formatting needs text to work on: a text box being typed in, or selected objects.
fn need_text(s: &mut DeckScreen) -> bool {
    if s.typing() || !s.doc.selected().is_empty() {
        return true;
    }
    s.notices.push("Tap a text box or shape first".into());
    false
}

fn size_menu(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) {
    let size = s.fmt.get("size").and_then(serde_json::Value::as_f64);
    let label = size.map(|v| format!("{} pt", trim(v))).unwrap_or_else(|| "Size".into());
    chrome::tool_menu(ui, "size", &label, accent, |ui| {
        for sz in [10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 44.0, 54.0, 60.0, 72.0, 96.0] {
            if choice(ui, &format!("{} pt", trim(sz)), size == Some(sz)) {
                if need_text(s) {
                    let r = s.doc.font_size(sz);
                    s.act(r);
                }
                ui.close();
            }
        }
    });
}

fn align_items(ui: &mut egui::Ui, s: &mut DeckScreen) {
    for (label, a) in [("Top", VAlign::Top), ("Middle", VAlign::Middle), ("Bottom", VAlign::Bottom)] {
        if chrome::menu_item(ui, label) {
            if need_text(s) {
                let r = s.doc.vertical_align(a);
                s.act(r);
            }
            ui.close();
        }
    }
}

fn text_tools(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) {
    let font = s.fmt.get("font").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    chrome::tool_menu(ui, "fonts", "Font", accent, |ui| {
        let mut list: Vec<String> = FONTS.iter().map(|f| (*f).to_owned()).collect();
        if !font.is_empty() && !list.contains(&font) {
            list.insert(0, font.clone());
        }
        for f in list {
            if choice(ui, &f, f == font) {
                if need_text(s) {
                    let r = s.doc.font(&f);
                    s.act(r);
                }
                ui.close();
            }
        }
    });
    size_menu(ui, s, accent);
    for (icon, label, key) in [("bold", "Bold", "bold"), ("italic", "Italic", "italic"), ("underline", "Underline", "underline")] {
        if chrome::tool(ui, icon, label, s.fmt_on(key), accent).clicked() && need_text(s) {
            let r = match key {
                "bold" => s.doc.bold(None),
                "italic" => s.doc.italic(None),
                _ => s.doc.underline(None),
            };
            s.act(r);
        }
    }
    chrome::tool_menu(ui, "fontcolor", "Color", accent, |ui| {
        if let Some(hex) = swatches(ui, None) {
            if need_text(s) {
                let r = s.doc.font_color(&format!("#{hex}"));
                s.act(r);
            }
            ui.close();
        }
    });
    chrome::divider(ui);
    let align = s.fmt.get("align").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
    for (icon, label, a, xml) in [
        ("alignLeft", "Left", HAlign::Left, "l"),
        ("alignCenter", "Center", HAlign::Center, "ctr"),
        ("alignRight", "Right", HAlign::Right, "r"),
        ("justify", "Justify", HAlign::Justify, "just"),
    ] {
        if chrome::tool(ui, icon, label, align == xml, accent).clicked() && need_text(s) {
            let r = s.doc.align(a);
            s.act(r);
        }
    }
    chrome::tool_menu(ui, "cellAlign", "Position", accent, |ui| align_items(ui, s));
    chrome::divider(ui);
    if chrome::tool(ui, "bullets", "Bullets", s.fmt_on("bullets"), accent).clicked() && need_text(s) {
        let r = s.doc.bullets(None);
        s.act(r);
    }
    if chrome::tool(ui, "numbering", "Numbers", s.fmt_on("numbering"), accent).clicked() && need_text(s) {
        let r = s.doc.numbering(None);
        s.act(r);
    }
    if chrome::tool(ui, "outdent", "Level up", false, accent).clicked() && need_text(s) {
        let r = s.doc.list_level_up();
        s.act(r);
    }
    if chrome::tool(ui, "indent", "Level down", false, accent).clicked() && need_text(s) {
        let r = s.doc.list_level_down();
        s.act(r);
    }
    chrome::tool_menu(ui, "lineSpacing", "Spacing", accent, |ui| {
        for (label, v) in [("1.0", 1.0), ("1.15", 1.15), ("1.5", 1.5), ("2.0", 2.0)] {
            if chrome::menu_item(ui, label) {
                if need_text(s) {
                    let r = s.doc.line_spacing(v);
                    s.act(r);
                }
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "clear", "Clear", false, accent).clicked() && need_text(s) {
        let r = s.doc.clear_formatting();
        s.act(r);
    }
}

fn insert_tools(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) -> Option<FileAction> {
    let mut action = None;
    if chrome::tool(ui, "textBox", "Text box", false, accent).clicked() {
        let rect = s.centred(320.0, 60.0);
        let _ = s.doc.exit_text();
        let r = s.doc.insert_text_box(Some(rect), "");
        if let Some(id) = s.act(r) {
            let r = s.doc.edit_text(id, None);
            s.act(r);
            s.keyboard_wanted = s.typing();
        }
    }
    let mut pick = None;
    chrome::tool_menu(ui, "shapes", "Shapes", accent, |ui| {
        if let Some(n) = gallery(ui, true) {
            pick = Some(n);
            ui.close();
        }
    });
    if let Some(n) = pick {
        insert_gallery_shape(s, n);
    }
    if chrome::tool(ui, "picture", "Picture", false, accent).clicked() {
        action = Some(FileAction::PickImage);
    }
    if chrome::tool(ui, "qr", "Barcode", false, accent).clicked() {
        action = Some(FileAction::InsertBarcode);
    }
    let toggle = |ui: &mut egui::Ui, s: &mut DeckScreen, icon: &str, label: &str, row: Row| {
        if chrome::tool(ui, icon, label, s.tabs.row == row, accent).clicked() {
            s.tabs.row = if s.tabs.row == row { Row::None } else { row };
        }
    };
    toggle(ui, s, "table", "Table", Row::Table);
    toggle(ui, s, "link", "Link", Row::Link);
    chrome::divider(ui);
    if chrome::tool(ui, "pageNumber", "Slide no.", false, accent).clicked() {
        let r = s.doc.run("design.headerFooter", serde_json::json!({ "slideNumber": true, "all": true }));
        s.act(r);
    }
    if chrome::tool(ui, "dateTime", "Date", false, accent).clicked() {
        let r = s.doc.run("design.headerFooter", serde_json::json!({ "date": true, "all": true }));
        s.act(r);
    }
    toggle(ui, s, "footer", "Footer", Row::Footer);
    action
}

/// Engine gallery names as shown: the upstream brand name isn't ours to use (rebrand rule), so
/// its own theme/colour/font set shows as "Default".
fn shown(name: &str) -> String {
    name.replace("DeckCraft", "Default").replace("ArtCraft", "Default")
}

fn design_tools(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) {
    let g = s.doc.design_galleries().unwrap_or_default();
    chrome::tool_menu(ui, "themes", "Theme", accent, |ui| {
        for t in g.get("themes").and_then(serde_json::Value::as_array).cloned().unwrap_or_default() {
            let name = t.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
            let colours: Vec<Color32> = t
                .get("colors")
                .and_then(serde_json::Value::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_str()).map(|c| rgb(hex_rgb(c.trim_start_matches('#')))).collect())
                .unwrap_or_default();
            let w = ui.available_width().max(200.0);
            let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 44.0), egui::Sense::click());
            let fg = ui.visuals().text_color();
            if resp.hovered() {
                ui.painter().rect_filled(r, 6.0, fg.gamma_multiply(0.08));
            }
            ui.painter().text(egui::pos2(r.left() + 10.0, r.center().y - 7.0), egui::Align2::LEFT_CENTER, shown(&name), egui::FontId::proportional(14.0), fg);
            for (i, c) in colours.iter().skip(4).take(6).enumerate() {
                let sw = egui::Rect::from_min_size(egui::pos2(r.left() + 10.0 + i as f32 * 18.0, r.center().y + 5.0), egui::vec2(16.0, 10.0));
                ui.painter().rect_filled(sw, 2.0, *c);
            }
            if resp.clicked() {
                let r = s.doc.apply_theme(&name);
                s.act(r);
                ui.close();
            }
        }
    });
    chrome::tool_menu(ui, "colors", "Colors", accent, |ui| {
        for c in g.get("colorSchemes").and_then(serde_json::Value::as_array).cloned().unwrap_or_default() {
            let name = c.as_str().unwrap_or_default().to_owned();
            if chrome::menu_item(ui, &shown(&name)) {
                let r = s.doc.theme_colors(&name);
                s.act(r);
                ui.close();
            }
        }
    });
    chrome::tool_menu(ui, "fonts", "Fonts", accent, |ui| {
        for f in g.get("fontSchemes").and_then(serde_json::Value::as_array).cloned().unwrap_or_default() {
            let name = f.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
            let major = f.get("major").and_then(serde_json::Value::as_str).unwrap_or_default();
            let minor = f.get("minor").and_then(serde_json::Value::as_str).unwrap_or_default();
            if chrome::menu_item(ui, &format!("{}  ·  {major} / {minor}", shown(&name))) {
                let r = s.doc.theme_fonts(&name);
                s.act(r);
                ui.close();
            }
        }
    });
    chrome::tool_menu(ui, "pageColor", "Background", accent, |ui| {
        ui.checkbox(&mut s.tabs.bg_this_only, "This slide only");
        let all = !s.tabs.bg_this_only;
        if let Some(hex) = swatches(ui, None) {
            let r = s.doc.background_color(&format!("#{hex}"), all);
            s.act(r);
            ui.close();
        }
        if chrome::menu_item(ui, "Theme background") {
            let r = s.doc.reset_background(all);
            s.act(r);
            ui.close();
        }
    });
    chrome::divider(ui);
    let (w, h) = s.doc.slide_size();
    let wide = (w / h - 16.0 / 9.0).abs() < 0.02;
    let standard = (w / h - 4.0 / 3.0).abs() < 0.02;
    if chrome::tool(ui, "pageWidth", "16:9", wide, accent).clicked() && !wide {
        let r = s.doc.set_slide_size(SlideSize::Widescreen);
        s.act(r);
    }
    if chrome::tool(ui, "size", "4:3", standard, accent).clicked() && !standard {
        let r = s.doc.set_slide_size(SlideSize::Standard);
        s.act(r);
    }
}

/// File tab: Save · Save as · PDF · Share · Print · Info.
fn file_tools(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) -> Option<FileAction> {
    let mut action = None;
    if chrome::tool(ui, "save", "Save", false, accent).clicked() {
        action = Some(FileAction::Save);
    }
    chrome::tool_menu(ui, "saveAs", "Save as", accent, |ui| {
        for (label, ext) in [("PowerPoint (.pptx)", "pptx"), ("PowerPoint show (.ppsx)", "ppsx")] {
            if chrome::menu_item(ui, label) {
                action = Some(FileAction::SaveAs(ext));
                ui.close();
            }
        }
    });
    let layouts: [(&str, PdfLayout); 6] = [
        ("Slides", PdfLayout::Slides),
        ("Handouts · 2 per page", PdfLayout::Handouts { per_page: 2 }),
        ("Handouts · 3 per page", PdfLayout::Handouts { per_page: 3 }),
        ("Handouts · 6 per page", PdfLayout::Handouts { per_page: 6 }),
        ("Handouts · 9 per page", PdfLayout::Handouts { per_page: 9 }),
        ("Notes pages", PdfLayout::Notes),
    ];
    chrome::tool_menu(ui, "export", "PDF", accent, |ui| {
        for (label, l) in layouts {
            if chrome::menu_item(ui, label) {
                s.pdf_layout = l;
                action = Some(FileAction::ExportPdf);
                ui.close();
            }
        }
    });
    if chrome::tool(ui, "share", "Share", false, accent).clicked() {
        action = Some(FileAction::Share);
    }
    chrome::tool_menu(ui, "print", "Print", accent, |ui| {
        for (label, l) in layouts {
            if chrome::menu_item(ui, label) {
                s.pdf_layout = l;
                action = Some(FileAction::Print);
                ui.close();
            }
        }
    });
    chrome::divider(ui);
    // Paper for notes pages and handouts (the deck engine has no 11×17; slides use the slide size).
    chrome::tool_menu(ui, "size", "Paper", accent, |ui| {
        for (name, id) in [("Letter", "letter"), ("Legal", "legal"), ("A4", "a4")] {
            if chrome::menu_item(ui, &crate::paper::label(name, s.pdf_paper == id)) {
                s.pdf_paper = id;
                ui.close();
            }
        }
        ui.label(RichText::new("For notes and handouts").small().weak());
    });
    if chrome::tool(ui, "info", "Info", false, accent).clicked() {
        action = Some(FileAction::Info);
    }
    action
}

// ===================================================================== input rows

fn rows(ui: &mut egui::Ui, s: &mut DeckScreen, accent: Color32) {
    let go = |ui: &mut egui::Ui, label: &str| ui.add(egui::Button::new(label).fill(accent.gamma_multiply(0.3)).min_size(egui::vec2(0.0, 40.0))).clicked();
    // Rows belong to their tab.
    let row_tab = match s.tabs.row {
        Row::Notes => Tab::Slides,
        Row::None => s.tabs.tab,
        _ => Tab::Insert,
    };
    if row_tab != s.tabs.tab {
        return;
    }
    match s.tabs.row {
        Row::None => {}
        Row::Table => {
            ui.horizontal_wrapped(|ui| {
                ui.label("Rows");
                ui.add(egui::DragValue::new(&mut s.tabs.rows).range(1..=30));
                ui.label("Columns");
                ui.add(egui::DragValue::new(&mut s.tabs.cols).range(1..=12));
                if go(ui, "Insert table") {
                    let (r, c) = (s.tabs.rows as usize, s.tabs.cols as usize);
                    let rect = s.centred((c as f64 * 110.0).min(860.0), r as f64 * 36.0);
                    let _ = s.doc.exit_text();
                    let res = s.doc.insert_table(r, c, Some(rect));
                    s.act(res);
                    s.tabs.row = Row::None;
                }
            });
        }
        Row::Link => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.tabs.url).hint_text("https://…").desired_width(200.0));
                ui.add(egui::TextEdit::singleline(&mut s.tabs.link_text).hint_text("Text (if nothing is selected)").desired_width(200.0));
                if go(ui, "Insert link") {
                    let url = s.tabs.url.trim().to_owned();
                    if url.is_empty() {
                        s.notices.push("Type the link address first".into());
                    } else {
                        let text = s.tabs.link_text.trim().to_owned();
                        let text = if text.is_empty() { url.clone() } else { text };
                        let r = s.doc.insert_link(&url, Some(&text));
                        s.act(r);
                        s.tabs.url.clear();
                        s.tabs.link_text.clear();
                        s.tabs.row = Row::None;
                    }
                }
            });
        }
        Row::Footer => {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.tabs.footer).hint_text("Footer text on every slide").desired_width(220.0));
                if go(ui, "Set footer") {
                    let r = s.doc.run("design.headerFooter", serde_json::json!({ "footer": true, "footerText": s.tabs.footer, "all": true }));
                    s.act(r);
                }
                if go(ui, "Remove") {
                    let r = s.doc.run("design.headerFooter", serde_json::json!({ "footer": false, "all": true }));
                    s.act(r);
                }
            });
        }
        Row::Notes => {
            let cur = s.doc.current_slide();
            if s.tabs.notes_slide != Some(cur) {
                s.flush_notes();
                s.tabs.notes = s.doc.notes(cur);
                s.tabs.notes_slide = Some(cur);
            }
            ui.label(RichText::new(format!("Speaker notes · slide {}", cur + 1)).weak());
            let notes_box = ui.add(egui::TextEdit::multiline(&mut s.tabs.notes).desired_rows(3).desired_width(f32::INFINITY).hint_text("Notes only you see while presenting"));
            if notes_box.changed() {
                s.tabs.notes_dirty = true;
            }
            // Typed notes go into the slide as soon as the box is left (a tap on another slide,
            // Save, Done), so they can't be lost by forgetting "Save notes".
            if notes_box.lost_focus() {
                s.flush_notes();
            }
            ui.horizontal(|ui| {
                if go(ui, "Save notes") {
                    s.tabs.notes_dirty = true;
                    s.flush_notes();
                }
                if ui.add(egui::Button::new("Close").min_size(egui::vec2(0.0, 40.0))).clicked() {
                    s.tabs.row = Row::None;
                }
            });
        }
    }
}
