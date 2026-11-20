//! Start screen (approved design): the Document Slate suite as four colour-coded app bars
//! (Word blue, Grid green, Deck orange, Page red) with the app names in Audiowide — charcoal with a
//! black outline — and the Recent documents underneath.

use eframe::egui::{self, RichText};

use crate::settings::{self, Settings};
use crate::theme::{self, Mode, Tool};

/// What the user asked for on the start screen.
pub enum HomeAction {
    OpenPicker,
    /// An app bar whose app isn't built yet.
    ComingSoon(Tool),
    OpenRecent(String),
    Forget(String),
    ToggleTheme,
    /// The Document Slate icon (top left): open the preferences drawer.
    OpenPrefs,
    /// An app bar was tapped: show that app's own screen.
    OpenApp(Tool),
    /// "New Word Slate": a blank document.
    NewDocument,
    /// The file picker, starting in a common folder.
    OpenPickerAt(Folder),
    /// From an app's screen back to the suite's start screen.
    BackToSuite,
    /// The Recent tab was tapped.
    ShowRecent,
}

/// The phone folders behind the folder tabs. Each tab opens Android's file picker in that
/// folder (the app has no permission to list folders itself, by design).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    Downloads,
    Documents,
    Pictures,
    Camera,
    Movies,
    Music,
    /// The picker wherever it starts (other storage, SD cards, other apps' files).
    Browse,
}

impl Folder {
    /// Tab order after Recent.
    pub const TABS: [Folder; 7] = [Folder::Downloads, Folder::Documents, Folder::Pictures, Folder::Camera, Folder::Movies, Folder::Music, Folder::Browse];

    pub fn label(self) -> &'static str {
        match self {
            Folder::Downloads => "Downloads",
            Folder::Documents => "Documents",
            Folder::Pictures => "Pictures",
            Folder::Camera => "Camera",
            Folder::Movies => "Movies",
            Folder::Music => "Music",
            Folder::Browse => "Browse",
        }
    }

    /// The folder's name on the phone's shared storage; None = no particular folder.
    pub fn storage_dir(self) -> Option<&'static str> {
        match self {
            Folder::Downloads => Some("Download"),
            Folder::Documents => Some("Documents"),
            Folder::Pictures => Some("Pictures"),
            Folder::Camera => Some("DCIM"),
            Folder::Movies => Some("Movies"),
            Folder::Music => Some("Music"),
            Folder::Browse => None,
        }
    }
}

/// The folder tab that is lit: Recent (its list shows below the tabs) or a folder whose picker
/// is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeTab {
    Recent,
    Folder(Folder),
}

/// File types each app opens (Recent on an app's screen only lists these).
pub fn belongs(tool: Tool, name: &str) -> bool {
    tool_for(name) == tool
}

/// Which app a file belongs to, by extension (decides the badge colour and letter).
pub fn tool_for(name: &str) -> Tool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "xls" | "ods" | "csv" | "tsv" => Tool::Grid,
        "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" | "ppt" | "odp" => Tool::Deck,
        "pdf" => Tool::Page,
        // Everything else goes to Word Slate: its engine opens Word, OpenDocument, RTF, HTML,
        // Markdown, LaTeX… and any other text file as plain text (.json, .log, .csv-like…).
        _ => Tool::Word,
    }
}

/// Extensions the Word engine reads by name (anything else it reads by content).
const WORD_EXTS: &[&str] = &[
    "docx", "docm", "dotx", "dotm", "doc", "dot", "odt", "ott", "fodt", "rtf", "txt", "text", "md", "markdown",
    "html", "htm", "xhtml", "mht", "mhtml", "xml", "tex", "latex", "ltx",
];

/// [`tool_for`], but a file whose extension says nothing (none, ".bin", ".download"…) is sent by
/// what it holds: a PDF, a PowerPoint or an Excel package.
pub fn tool_for_file(name: &str, bytes: &[u8]) -> Tool {
    let by_name = tool_for(name);
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if by_name != Tool::Word || WORD_EXTS.contains(&ext.as_str()) {
        return by_name;
    }
    let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    if bytes.starts_with(b"%PDF") {
        Tool::Page
    } else if bytes.starts_with(b"PK") && has(b"ppt/presentation.xml") {
        Tool::Deck
    } else if bytes.starts_with(b"PK") && has(b"xl/workbook.") {
        Tool::Grid
    } else {
        Tool::Word
    }
}

fn badge_letter(tool: Tool) -> &'static str {
    match tool {
        Tool::Word => "W",
        Tool::Grid => "G",
        Tool::Deck => "D",
        Tool::Page => "P",
    }
}

pub fn show(ui: &mut egui::Ui, settings: &Settings, mode: Mode, top_inset: f32, tab: HomeTab) -> Option<HomeAction> {
    let mut action = None;
    let p = mode.palette();

    // Header: neutral slate (no app accent until a document is open).
    let frame = egui::Frame::new()
        .fill(p.surface)
        .inner_margin(egui::Margin { left: 16, right: 8, top: (top_inset + 6.0).min(120.0) as i8, bottom: 6 });
    egui::Panel::top("home_header").frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(66.0);
            // The app icon is the menu button (preferences drawer).
            // Same raised button as each app's own screen (shadow, lighter face, dark edge, a small
            // down-arrow), holding the colored suite icon.
            let (r, resp) = ui.allocate_exact_size(egui::vec2(66.0, 66.0), egui::Sense::click());
            let pressed = resp.is_pointer_button_down_on();
            let face = r.translate(egui::vec2(0.0, if pressed { 2.0 } else { 0.0 }));
            let painter = ui.painter();
            let dark = mode == Mode::Dark;
            // The header is slate gray, so the face is a clearly lighter key with a deep shadow
            // and a lit top edge; a ring of it shows around the (dark) icon.
            if !pressed {
                painter.add(egui::Shadow { offset: [0, 4], blur: 10, spread: 1, color: egui::Color32::from_black_alpha(if dark { 190 } else { 90 }) }.as_shape(face, 14.0));
            }
            let (key, edge) = if dark {
                (egui::Color32::from_rgb(if pressed { 0x44 } else { 0x52 }, if pressed { 0x4A } else { 0x59 }, if pressed { 0x50 } else { 0x60 }), egui::Color32::from_rgb(0x15, 0x17, 0x19))
            } else {
                (if pressed { egui::Color32::from_rgb(0xE4, 0xE7, 0xEA) } else { egui::Color32::WHITE }, egui::Color32::from_rgb(0x9A, 0xA3, 0xAB))
            };
            painter.rect_filled(face, 14.0, key);
            if !pressed {
                let lit = egui::Rect::from_min_max(face.min + egui::vec2(12.0, 1.5), egui::pos2(face.max.x - 12.0, face.min.y + 3.0));
                painter.rect_filled(lit, 1.5, egui::Color32::from_white_alpha(if dark { 70 } else { 200 }));
            }
            painter.rect_stroke(face, 14.0, egui::Stroke::new(1.5, edge), egui::StrokeKind::Inside);
            crate::prefs::suite_icon(painter, face.shrink(8.0));
            let c = face.right_bottom() + egui::vec2(-9.0, -9.0);
            painter.circle_filled(c, 7.5, edge);
            painter.add(egui::Shape::convex_polygon(
                vec![c + egui::vec2(-5.0, -3.0), c + egui::vec2(5.0, -3.0), c + egui::vec2(0.0, 3.0)],
                theme::ON_ACCENT,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
            ));
            if resp.clicked() {
                action = Some(HomeAction::OpenPrefs);
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Document Slate").font(theme::brand_font(22.0)).color(p.text));
        });
    });

    egui::CentralPanel::default().frame(egui::Frame::new().fill(p.backdrop).inner_margin(16)).show(ui, |ui| {
        // One left-aligned column, centred on wide tablets.
        let width = ui.available_width().min(720.0);
        let side = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.horizontal_top(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_width(width);

                // The four apps of the suite.
                for tool in Tool::ALL {
                    if app_bar(ui, tool, width) {
                        action = Some(if tool.available() { HomeAction::OpenApp(tool) } else { HomeAction::ComingSoon(tool) });
                    }
                    ui.add_space(8.0);
                }
                ui.add_space(8.0);

                if let Some(a) = folder_tabs(ui, tab, p.text, width, &p) {
                    action = Some(a);
                }
                ui.add_space(10.0);
                if !settings.keep_recent {
                    ui.label(RichText::new("Recent is turned off in Preferences.").weak());
                    return;
                }
                if settings.recent.is_empty() {
                    ui.label(RichText::new("Documents you open will appear here.").weak());
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(ui.available_height().max(120.0))
                    .show(ui, |ui| {
                        for r in &settings.recent {
                            if let Some(a) = recent_row(ui, r, width, &p) {
                                action = Some(a);
                            }
                        }
                    });

            });
        });
    });
    action
}

/// One app's own screen (e.g. Word Slate): the app banner (its symbol, boxed as a button that
/// opens a menu of file places, and the name), a "New Word Slate" bar, and Recent files of the
/// types this app opens.
pub fn app_home(ui: &mut egui::Ui, tool: Tool, settings: &Settings, mode: Mode, top_inset: f32, tab: HomeTab) -> Option<HomeAction> {
    let mut action = None;
    let p = mode.palette();
    let accent = tool.accent();

    // Banner in the app's colour, running up under the status bar.
    let frame = egui::Frame::new()
        .fill(accent)
        .inner_margin(egui::Margin { left: 14, right: 14, top: (top_inset + 10.0).min(120.0) as i8, bottom: 12 });
    egui::Panel::top("app_banner").frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            // The symbol, raised like a button (shadow, lighter face, edge, a small down-arrow).
            let (r, resp) = ui.allocate_exact_size(egui::vec2(66.0, 66.0), egui::Sense::click());
            let pressed = resp.is_pointer_button_down_on();
            let face = r.translate(egui::vec2(0.0, if pressed { 2.0 } else { 0.0 }));
            let painter = ui.painter();
            painter.rect_filled(r.translate(egui::vec2(0.0, 3.0)), 14.0, egui::Color32::from_black_alpha(80));
            painter.rect_filled(face, 14.0, egui::Color32::from_white_alpha(if pressed { 30 } else { 52 }));
            painter.rect_stroke(face, 14.0, egui::Stroke::new(1.5, egui::Color32::from_black_alpha(110)), egui::StrokeKind::Inside);
            app_glyph(painter, face.shrink(9.0), tool, theme::BRAND_TEXT, egui::Color32::BLACK);
            let c = face.right_bottom() + egui::vec2(-10.0, -10.0);
            painter.add(egui::Shape::convex_polygon(
                vec![c + egui::vec2(-5.0, -3.0), c + egui::vec2(5.0, -3.0), c + egui::vec2(0.0, 3.0)],
                theme::ON_ACCENT,
                egui::Stroke::new(1.0, egui::Color32::BLACK),
            ));
            egui::Popup::menu(&resp).align(egui::RectAlign::BOTTOM_START).show(|ui| {
                ui.set_min_width(220.0);
                ui.style_mut().spacing.button_padding = egui::vec2(12.0, 10.0);
                if crate::chrome::menu_item(ui, "Browse files\u{2026}") {
                    action = Some(HomeAction::OpenPicker);
                }
                ui.separator();
                let other = match mode {
                    Mode::Dark => "Light mode",
                    Mode::Light => "Dark mode",
                };
                if crate::chrome::menu_item(ui, other) {
                    action = Some(HomeAction::ToggleTheme);
                }
                if crate::chrome::menu_item(ui, "Document Slate home") {
                    action = Some(HomeAction::BackToSuite);
                }
            });
            ui.add_space(12.0);
            let (nr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 66.0), egui::Sense::hover());
            outlined_text(ui.painter(), egui::pos2(nr.left(), nr.center().y), tool.name(), theme::brand_font(32.0), theme::BRAND_TEXT, egui::Color32::BLACK);
        });
    });

    egui::CentralPanel::default().frame(egui::Frame::new().fill(p.backdrop).inner_margin(16)).show(ui, |ui| {
        let width = ui.available_width().min(720.0);
        let side = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.horizontal_top(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_width(width);
                if new_bar(ui, tool, width, &p) {
                    action = Some(HomeAction::NewDocument);
                }
                ui.add_space(16.0);
                if let Some(a) = folder_tabs(ui, tab, accent, width, &p) {
                    action = Some(a);
                }
                ui.add_space(10.0);
                if !settings.keep_recent {
                    ui.label(RichText::new("Recent is turned off in Preferences.").weak());
                    return;
                }
                let mine: Vec<&settings::Recent> = settings.recent.iter().filter(|r| belongs(tool, &r.name)).collect();
                if mine.is_empty() {
                    ui.label(RichText::new(format!("{} you open will appear here.", tool.blurb().split(" \u{b7} ").next().unwrap_or("Files"))).weak());
                }
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for r in mine {
                        if let Some(a) = recent_row(ui, r, width, &p) {
                            action = Some(a);
                        }
                    }
                });
            });
        });
    });
    action
}

/// The "New Word Slate" bar: a raised card with the app's colour as its edge and a round "+".
fn new_bar(ui: &mut egui::Ui, tool: Tool, width: f32, p: &theme::Palette) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 76.0), egui::Sense::click());
    let painter = ui.painter();
    let accent = tool.accent();
    let fill = if resp.is_pointer_button_down_on() { p.control } else { p.surface };
    painter.rect_filled(rect.translate(egui::vec2(0.0, 3.0)), 14.0, egui::Color32::from_black_alpha(50));
    painter.rect_filled(rect, 14.0, fill);
    painter.rect_stroke(rect, 14.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Inside);
    let c = rect.left_center() + egui::vec2(40.0, 0.0);
    painter.circle_filled(c, 24.0, accent);
    let s = egui::Stroke::new(4.0, theme::ON_ACCENT);
    painter.line_segment([c - egui::vec2(10.0, 0.0), c + egui::vec2(10.0, 0.0)], s);
    painter.line_segment([c - egui::vec2(0.0, 10.0), c + egui::vec2(0.0, 10.0)], s);
    let x = rect.left() + 80.0;
    painter.text(egui::pos2(x, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, format!("New {}", tool.name()), theme::brand_font(22.0), p.text);
    let sub = match tool {
        Tool::Word => "Blank document",
        Tool::Grid => "Blank spreadsheet",
        Tool::Deck => "Blank presentation",
        Tool::Page => "Blank PDF",
    };
    painter.text(egui::pos2(x + 1.0, rect.center().y + 15.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(13.0), p.text.gamma_multiply(0.65));
    resp.clicked()
}

/// One app bar: rounded bar in the app's colour, the name in Audiowide (charcoal, black outline),
/// what it opens underneath. Returns true when tapped.
fn app_bar(ui: &mut egui::Ui, tool: Tool, width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 59.0), egui::Sense::click());
    let painter = ui.painter();
    let mut fill = tool.accent();
    if resp.is_pointer_button_down_on() {
        fill = fill.gamma_multiply(0.85);
    }
    // Soft shadow, then the bar.
    // Everything at 70% of the first design (84 px bars), to make room for the folder tabs.
    painter.rect_filled(rect.translate(egui::vec2(0.0, 2.0)), 10.0, egui::Color32::from_black_alpha(50));
    painter.rect_filled(rect, 10.0, fill);

    // The app's symbol from the app icon, enlarged, then the name beside it.
    let glyph = egui::Rect::from_min_size(egui::pos2(rect.left() + 9.0, rect.center().y - 21.0), egui::vec2(42.0, 42.0));
    app_glyph(painter, glyph, tool, theme::BRAND_TEXT, egui::Color32::BLACK);
    let x = glyph.right() + 7.0;
    outlined_text(painter, egui::pos2(x, rect.top() + 22.0), tool.name(), theme::brand_font(21.0), theme::BRAND_TEXT, egui::Color32::BLACK);
    painter.text(egui::pos2(x + 1.5, rect.bottom() - 13.0), egui::Align2::LEFT_CENTER, tool.blurb(), egui::FontId::proportional(11.0), theme::ON_ACCENT);
    if !tool.available() {
        let tag = egui::Rect::from_min_size(egui::pos2(rect.right() - 66.0, rect.bottom() - 21.0), egui::vec2(59.0, 15.0));
        painter.rect_filled(tag, 7.5, egui::Color32::from_black_alpha(70));
        painter.text(tag.center(), egui::Align2::CENTER_CENTER, "Coming soon", egui::FontId::proportional(8.5), theme::ON_ACCENT);
    }
    resp.clicked()
}

/// Height of a folder tab (the strip under the app bars).
const TAB_H: f32 = 40.0;

/// The side-scrolling folder tabs: Recent first, then the phone's folders. The lit tab has a
/// frame in `highlight` (the app's color on an app screen) and opens into the line under the row.
fn folder_tabs(ui: &mut egui::Ui, selected: HomeTab, highlight: egui::Color32, width: f32, p: &theme::Palette) -> Option<HomeAction> {
    let mut action = None;
    let left = ui.cursor().left();
    let mut bottom = None;
    let mut lit_rect = None;
    let out = egui::ScrollArea::horizontal()
        .id_salt("folder_tabs")
        .max_width(width)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let tabs = std::iter::once(HomeTab::Recent).chain(Folder::TABS.into_iter().map(HomeTab::Folder));
                for tab in tabs {
                    let (clicked, shape) = folder_tab(ui, tab, tab == selected, highlight, p);
                    bottom = Some(shape.bottom());
                    if tab == selected {
                        lit_rect = Some(shape);
                    }
                    if clicked {
                        action = Some(match tab {
                            HomeTab::Recent => HomeAction::ShowRecent,
                            HomeTab::Folder(f) => HomeAction::OpenPickerAt(f),
                        });
                    }
                }
            });
        });
    // The line the tabs stand on, then the lit tab's opening cut into it (both where the tabs
    // really ended up, inside the visible part of the strip).
    if let Some(y) = bottom {
        let painter = ui.painter();
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(left, y), egui::pos2(left + width, y + 2.0)), 0.0, highlight);
        if let Some(r) = lit_rect {
            let view = out.inner_rect;
            let (x0, x1) = ((r.left() + 2.0).max(view.left()), (r.right() - 2.0).min(view.right()));
            if x1 > x0 {
                painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, y - 2.0), egui::pos2(x1, y + 2.0)), 0.0, p.control);
            }
        }
    }
    action
}

/// One folder tab: rounded top corners, a small symbol (clock, folder, magnifier) and the name.
/// Returns (tapped, the tab's outline).
fn folder_tab(ui: &mut egui::Ui, tab: HomeTab, lit: bool, highlight: egui::Color32, p: &theme::Palette) -> (bool, egui::Rect) {
    let label = match tab {
        HomeTab::Recent => "Recent",
        HomeTab::Folder(f) => f.label(),
    };
    let ink = if lit { p.text } else { p.text.gamma_multiply(0.7) };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(15.0), ink);
    let w = 12.0 + 18.0 + 8.0 + galley.size().x + 14.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TAB_H), egui::Sense::click());
    let shape = egui::Rect::from_min_size(rect.min, egui::vec2(w, TAB_H));
    let corners = egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 };
    let fill = if lit || resp.is_pointer_button_down_on() { p.control } else { p.surface };
    let painter = ui.painter();
    painter.rect_filled(shape, corners, fill);
    if lit {
        painter.rect_stroke(shape, corners, egui::Stroke::new(2.0, highlight), egui::StrokeKind::Inside);
    } else {
        painter.rect_stroke(shape, corners, egui::Stroke::new(1.0, p.text.gamma_multiply(0.12)), egui::StrokeKind::Inside);
    }

    // Symbol (about 18 x 14), then the name.
    let c = egui::pos2(shape.left() + 12.0 + 9.0, shape.center().y);
    let s = egui::Stroke::new(1.6, ink);
    match tab {
        HomeTab::Recent => {
            painter.circle_stroke(c, 7.5, s);
            painter.line_segment([c, c + egui::vec2(0.0, -4.5)], s);
            painter.line_segment([c, c + egui::vec2(3.5, 1.5)], s);
        }
        HomeTab::Folder(Folder::Browse) => {
            let m = c + egui::vec2(-1.5, -1.5);
            painter.circle_stroke(m, 5.0, s);
            painter.line_segment([m + egui::vec2(3.6, 3.6), m + egui::vec2(8.0, 8.0)], egui::Stroke::new(2.2, ink));
        }
        HomeTab::Folder(_) => {
            let body = egui::Rect::from_min_max(c + egui::vec2(-9.0, -4.0), c + egui::vec2(9.0, 7.0));
            painter.rect_stroke(body, 2.0, s, egui::StrokeKind::Middle);
            painter.add(egui::Shape::line(
                vec![c + egui::vec2(-8.0, -4.0), c + egui::vec2(-7.0, -7.0), c + egui::vec2(-2.0, -7.0), c + egui::vec2(0.0, -4.0)],
                s,
            ));
        }
    }
    painter.galley(egui::pos2(shape.left() + 38.0, shape.center().y - galley.size().y / 2.0), galley, ink);
    (resp.clicked(), shape)
}

/// One app's symbol from the app icon (Branding/Icon/master/*.svg: each symbol sits in a 150 x 150
/// tile, drawn with round-cap 8-unit lines). Coordinates below are tile-local, copied from the SVG.
/// Drawn twice: a wider `outline` stroke first, then `fg` on top (matches the outlined app names).
fn app_glyph(painter: &egui::Painter, rect: egui::Rect, tool: Tool, fg: egui::Color32, outline: egui::Color32) {
    glyph(painter, rect, tool, fg, Some(outline), 1.0);
}

/// The same symbol in one color with no outline, lines `weight` times the icon's width
/// (used for the small app icon drawn in code).
pub fn app_glyph_lines(painter: &egui::Painter, rect: egui::Rect, tool: Tool, fg: egui::Color32, weight: f32) {
    glyph(painter, rect, tool, fg, None, weight);
}

fn glyph(painter: &egui::Painter, rect: egui::Rect, tool: Tool, fg: egui::Color32, outline: Option<egui::Color32>, weight: f32) {
    let k = rect.width() / 100.0; // tile units 25..125 -> rect
    let at = |x: f32, y: f32| rect.min + egui::vec2((x - 25.0) * k, (y - 25.0) * k);
    let lines: Vec<Vec<(f32, f32)>>;
    let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new(); // rounded rects: x, y, w, h (radius 8)
    match tool {
        Tool::Word => {
            lines = vec![
                vec![(39.0, 29.0), (89.0, 29.0), (111.0, 51.0), (111.0, 121.0), (39.0, 121.0), (39.0, 29.0)],
                vec![(89.0, 29.0), (89.0, 51.0), (111.0, 51.0)],
                vec![(54.0, 69.0), (96.0, 69.0)],
                vec![(54.0, 87.0), (96.0, 87.0)],
                vec![(54.0, 105.0), (80.0, 105.0)],
            ];
        }
        Tool::Grid => {
            boxes.push((29.0, 37.0, 92.0, 76.0));
            lines = vec![
                vec![(60.0, 37.0), (60.0, 113.0)],
                vec![(91.0, 37.0), (91.0, 113.0)],
                vec![(29.0, 62.0), (121.0, 62.0)],
                vec![(29.0, 88.0), (121.0, 88.0)],
            ];
        }
        Tool::Deck => {
            boxes.push((27.0, 39.0, 96.0, 62.0));
            lines = vec![
                vec![(75.0, 101.0), (75.0, 119.0)],
                vec![(57.0, 123.0), (93.0, 123.0)],
                vec![(43.0, 87.0), (61.0, 71.0), (78.0, 80.0), (107.0, 57.0)],
            ];
        }
        Tool::Page => {
            boxes.push((26.0, 69.0, 70.0, 30.0));
            // The page's left edge is cut where the "PDF" label overlaps it (the SVG's mask).
            lines = vec![
                vec![(39.0, 65.0), (39.0, 29.0), (89.0, 29.0), (111.0, 51.0), (111.0, 121.0), (39.0, 121.0), (39.0, 103.0)],
                vec![(89.0, 29.0), (89.0, 51.0), (111.0, 51.0)],
            ];
        }
    }
    let passes: Vec<(egui::Color32, f32)> = match outline {
        Some(o) => vec![(o, 8.0 * k + 3.0), (fg, 8.0 * k)],
        None => vec![(fg, 8.0 * k * weight)],
    };
    for (colour, width) in passes {
        let stroke = egui::Stroke::new(width, colour);
        for l in &lines {
            let pts: Vec<egui::Pos2> = l.iter().map(|&(x, y)| at(x, y)).collect();
            painter.add(egui::Shape::line(pts.clone(), stroke));
            // Round caps and joins (egui lines are butt-ended).
            for p in pts {
                painter.circle_filled(p, width / 2.0, colour);
            }
        }
        for &(x, y, w, h) in &boxes {
            let r = egui::Rect::from_min_max(at(x, y), at(x + w, y + h));
            painter.rect_stroke(r, 8.0 * k, stroke, egui::StrokeKind::Middle);
        }
    }
    if tool == Tool::Page {
        let c = at(61.0, 84.0);
        let font = egui::FontId::new(19.0 * k, egui::FontFamily::Name("semibold".into()));
        if let Some(outline) = outline {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                painter.text(c + egui::vec2(a.cos(), a.sin()) * 1.5, egui::Align2::CENTER_CENTER, "PDF", font.clone(), outline);
            }
        }
        painter.text(c, egui::Align2::CENTER_CENTER, "PDF", font, fg);
    }
}

/// Text with an outline: the outline colour drawn around it, then the text on top.
fn outlined_text(painter: &egui::Painter, pos: egui::Pos2, text: &str, font: egui::FontId, fill: egui::Color32, outline: egui::Color32) {
    let w = (font.size / 18.0).clamp(1.0, 2.5);
    for i in 0..12 {
        let a = i as f32 * std::f32::consts::TAU / 12.0;
        painter.text(pos + egui::vec2(a.cos() * w, a.sin() * w), egui::Align2::LEFT_CENTER, text, font.clone(), outline);
    }
    painter.text(pos, egui::Align2::LEFT_CENTER, text, font, fill);
}

/// One Recent entry: coloured badge, name, "opened 5 min ago". Tap opens; long-press offers removal.
fn recent_row(ui: &mut egui::Ui, r: &settings::Recent, width: f32, p: &theme::Palette) -> Option<HomeAction> {
    let tool = tool_for(&r.name);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 60.0), egui::Sense::click());
    let painter = ui.painter();
    let fill = if resp.hovered() || resp.is_pointer_button_down_on() { p.control } else { p.surface };
    painter.rect_filled(rect.shrink2(egui::vec2(0.0, 2.0)), 8.0, fill);

    let badge = egui::Rect::from_center_size(rect.left_center() + egui::vec2(30.0, 0.0), egui::vec2(36.0, 36.0));
    painter.rect_filled(badge, 6.0, tool.accent());
    painter.text(badge.center(), egui::Align2::CENTER_CENTER, badge_letter(tool), egui::FontId::proportional(18.0), theme::ON_ACCENT);

    let x = badge.right() + 14.0;
    painter.text(egui::pos2(x, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, &r.name, egui::FontId::proportional(16.0), p.text);
    painter.text(
        egui::pos2(x, rect.center().y + 12.0),
        egui::Align2::LEFT_CENTER,
        format!("Opened {}", settings::ago(r.opened)),
        egui::FontId::proportional(13.0),
        p.text.gamma_multiply(0.6),
    );

    // Long-press (touch) or right-click (desktop): a small menu instead of removing straight away.
    let mut forget = false;
    resp.context_menu(|ui| {
        if ui.button("Remove from Recent").clicked() {
            forget = true;
            ui.close();
        }
    });
    if forget {
        return Some(HomeAction::Forget(r.uri.clone()));
    }
    if resp.clicked() {
        return Some(HomeAction::OpenRecent(r.uri.clone()));
    }
    None
}
