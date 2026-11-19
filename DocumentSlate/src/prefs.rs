//! Preferences: a side drawer that slides in from the left when the Document Slate icon on the
//! start screen is tapped (layout after the Third Coast Sailing app's side menu: logo and name,
//! rows of icon · one word · switch, thin dividers, an outlined "About ›" button).
//!
//! Minimal on purpose: symbols first, one or two words per row, on/off as switches.
//! Everything is stored in settings.json on the phone; nothing leaves the device.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::settings::Settings;
use crate::theme::{self, Mode, Tool};

/// Spelling languages built into the app (craft-i18n dictionaries): (BCP 47 tag, shown name).
pub const LANGUAGES: [(&str, &str); 9] = [
    ("en-US", "English (US)"),
    ("en-GB", "English (UK)"),
    ("en-CA", "English (Canada)"),
    ("en-AU", "English (Australia)"),
    ("de-DE", "Deutsch"),
    ("it-IT", "Italiano"),
    ("pl-PL", "Polski"),
    ("pt-PT", "Português"),
    ("ru-RU", "Русский"),
];

/// "On" color of the switches.
const ON: Color32 = Color32::from_rgb(0x2E, 0x8B, 0x57);
/// Widest the drawer gets (tablets); on phones it is half the screen.
const DRAWER_W: f32 = 230.0;

#[derive(Default)]
pub struct Prefs {
    pub open: bool,
    /// The About page is showing inside the drawer.
    pub about: bool,
    /// "Digital IDs" was tapped (the shell opens the manager).
    pub open_ids: bool,
}

impl Prefs {
    /// Back pressed while the drawer is open: About → menu → closed. False if it was closed.
    pub fn back(&mut self) -> bool {
        if self.about {
            self.about = false;
        } else if self.open {
            self.open = false;
        } else {
            return false;
        }
        true
    }

    /// Draw the drawer (if open or closing). Changes go straight into `settings` and are saved.
    pub fn show(&mut self, ctx: &egui::Context, settings: &mut Settings, mode: Mode, top: f32, bottom: f32) {
        let t = ctx.animate_bool_with_time(egui::Id::unique("prefs-drawer"), self.open, 0.18);
        if t <= 0.0 {
            self.about = false;
            return;
        }
        let screen = ctx.content_rect();
        let p = mode.palette();

        // Dim the start screen; a tap on it closes the drawer.
        egui::Area::new(egui::Id::unique("prefs-scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((130.0 * t) as u8));
                if resp.clicked() {
                    self.open = false;
                }
            });

        let w = DRAWER_W.min(screen.width() * 0.5);
        let before = snapshot(settings);
        egui::Area::new(egui::Id::unique("prefs-panel"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(screen.left() - w * (1.0 - t), screen.top()))
            .show(ctx, |ui| {
                let frame = egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin { left: 12, right: 12, top: (top + 14.0).min(120.0) as i8, bottom: (bottom + 12.0).min(120.0) as i8 })
                    .shadow(egui::Shadow { offset: [4, 0], blur: 16, spread: 0, color: Color32::from_black_alpha(90) });
                frame.show(ui, |ui| {
                    ui.set_width(w - 24.0);
                    ui.set_min_height(screen.height() - (top + 14.0).min(120.0) - (bottom + 12.0).min(120.0));
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if self.about {
                            self.about_page(ui, p.text);
                        } else {
                            self.menu(ui, settings, p.text);
                        }
                    });
                });
            });
        if snapshot(settings) != before {
            settings.save();
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui, s: &mut Settings, fg: Color32) {
        header(ui, fg);
        ui.add_space(8.0);

        let mut dark = s.dark;
        switch_row(ui, Glyph::Moon, "Dark", &mut dark, fg);
        s.dark = dark;
        switch_row(ui, Glyph::Awake, "Screen on", &mut s.keep_awake, fg);
        // Only the default: with it off, each app's Edit button still works.
        label_row(ui, Glyph::Pencil, fg, |ui| plain_label(ui, "Edit mode", Some("on open"), fg), |ui| {
            switch(ui, &mut s.open_in_edit);
        });
        // Recent off: stop adding files (the list is hidden, not wiped). While old entries remain,
        // a box around the word offers "hold to clear": a 1 s hold empties the list.
        let can_clear = !s.keep_recent && !s.recent.is_empty();
        let mut clear = false;
        label_row(ui, Glyph::Clock, fg, |ui| {
            if can_clear {
                clear = hold_to_clear(ui, "Recent", fg);
            } else {
                plain_label(ui, "Recent", None, fg);
            }
        }, |ui| {
            switch(ui, &mut s.keep_recent);
        });
        if clear {
            s.recent.clear();
        }

        divider(ui, fg);
        row(ui, Glyph::Spelling, "", fg, |ui| {
            let current = LANGUAGES.iter().find(|l| l.0 == s.spell_lang).map_or(s.spell_lang.as_str(), |l| l.1).to_owned();
            egui::ComboBox::from_id_salt("prefs-lang")
                .selected_text(current)
                .width(ui.available_width() - 4.0)
                .height(420.0)
                .show_ui(ui, |ui| {
                    ui.style_mut().spacing.button_padding = egui::vec2(12.0, 10.0);
                    for (tag, name) in LANGUAGES {
                        ui.selectable_value(&mut s.spell_lang, tag.to_owned(), name);
                    }
                });
        });
        row(ui, Glyph::Person, "", fg, |ui| {
            ui.add(egui::TextEdit::singleline(&mut s.author).hint_text("Your name").desired_width(ui.available_width() - 4.0).margin(egui::vec2(6.0, 7.0)));
        });

        divider(ui, fg);
        if cfg!(feature = "pdf") && outlined_button(ui, Glyph::Key, "Digital IDs", fg).clicked() {
            self.open_ids = true;
            self.open = false;
        }
        ui.add_space(6.0);
        if outlined_button(ui, Glyph::Info, "About", fg).clicked() {
            self.about = true;
        }
    }

    fn about_page(&mut self, ui: &mut egui::Ui, fg: Color32) {
        if outlined_back(ui, fg).clicked() {
            self.about = false;
        }
        ui.add_space(18.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
            suite_icon(ui.painter(), r);
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Document Slate").font(theme::brand_font(15.0)).color(fg));
            ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
            ui.add_space(10.0);
            // The four promises, as symbols with one word each, two by two.
            let promises = [(Glyph::Free, "Free"), (Glyph::NoAds, "No ads"), (Glyph::NoTrack, "Private"), (Glyph::NoAi, "No AI")];
            for pair in promises.chunks(2) {
                ui.horizontal(|ui| {
                    let total = 2.0 * 72.0;
                    ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                    for &(g, word) in pair {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(72.0, 52.0), Sense::hover());
                        draw_glyph(ui.painter(), Rect::from_center_size(egui::pos2(r.center().x, r.top() + 14.0), Vec2::splat(24.0)), g, fg);
                        ui.painter().text(egui::pos2(r.center().x, r.bottom() - 9.0), egui::Align2::CENTER_CENTER, word, egui::FontId::proportional(12.0), fg.gamma_multiply(0.8));
                    }
                });
            }
        });
        ui.add_space(14.0);
        divider(ui, fg);
        let small = |ui: &mut egui::Ui, title: &str, body: &str| {
            ui.label(egui::RichText::new(title).size(13.0).strong());
            ui.label(egui::RichText::new(body).size(12.0).weak());
            ui.add_space(8.0);
        };
        small(ui, "Privacy", "Document Slate collects nothing and has no internet access. Your files, settings and \
             signatures stay on this device; documents leave it only when you share, print or save them elsewhere. \
             Your name, if set, is written into documents you save.");
        small(ui, "Engines", "Built on the open-source office engines by storytold (github.com/storytold) — MIT or Apache-2.0.");
        small(ui, "Fonts", "Audiowide by Astigmatic; Inter by Rasmus Andersson — SIL Open Font License 1.1.");
        small(ui, "Dictionaries", "SCOWL and craft-i18n word lists — see their licenses in the source.");
        small(ui, "Icons", "Toolbar icons from the same engines (MIT or Apache-2.0). App icon: Document Slate.");
    }
}

/// The values the drawer can change, to spot a change and save once.
fn snapshot(s: &Settings) -> (bool, bool, bool, bool, String, String, usize) {
    (s.dark, s.keep_awake, s.open_in_edit, s.keep_recent, s.spell_lang.clone(), s.author.clone(), s.recent.len())
}

// ===================================================================== pieces

/// Icon and "Document Slate" at the top of the drawer.
fn header(ui: &mut egui::Ui, fg: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
    suite_icon(ui.painter(), r);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Document Slate").font(theme::brand_font(15.0)).color(fg));
}

/// One 52-pt row: symbol, a word (may be empty), then `right` (a switch, a list…) at the end.
fn row(ui: &mut egui::Ui, g: Glyph, label: &str, fg: Color32, right: impl FnOnce(&mut egui::Ui)) {
    let h = 46.0;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_height(h);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        draw_glyph(ui.painter(), r, g, fg);
        ui.add_space(10.0);
        if !label.is_empty() {
            ui.label(egui::RichText::new(label).size(15.0).color(fg));
            // Right side, right-aligned: the first item drawn sits at the far right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        } else {
            right(ui);
        }
    });
}

/// A row whose left part (after the symbol) is drawn by `left`, with `right` at the far right.
fn label_row(ui: &mut egui::Ui, g: Glyph, fg: Color32, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    let h = 46.0;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_height(h);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
        draw_glyph(ui.painter(), r, g, fg);
        ui.add_space(4.0);
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

/// A word with an optional small line under it, inset like the hold box's text.
fn plain_label(ui: &mut egui::Ui, label: &str, sub: Option<&str>, fg: Color32) {
    let (r, _) = ui.allocate_exact_size(label_size(ui, label), Sense::hover());
    paint_label(ui.painter(), r, label, sub, fg);
}

fn label_size(_ui: &egui::Ui, _label: &str) -> Vec2 {
    egui::vec2(90.0, 40.0)
}

fn paint_label(p: &egui::Painter, r: Rect, label: &str, sub: Option<&str>, fg: Color32) {
    let x = r.left() + 6.0;
    match sub {
        Some(sub) => {
            p.text(egui::pos2(x, r.center().y - 7.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), fg);
            p.text(egui::pos2(x, r.center().y + 10.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.0), fg.gamma_multiply(0.6));
        }
        None => {
            p.text(egui::pos2(x, r.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), fg);
        }
    }
}

/// "Recent" in an outlined box with "hold to clear" under it. Holding it fills the box over one
/// second; true once the hold completes (the caller clears the list, and the box goes away).
fn hold_to_clear(ui: &mut egui::Ui, label: &str, fg: Color32) -> bool {
    const HOLD: f64 = 1.0;
    let (r, resp) = ui.allocate_exact_size(label_size(ui, label), Sense::click_and_drag());
    let id = resp.id.with("hold-start");
    let now = ui.ctx().input(|i| i.time);
    // Read the finger directly: a press that started on the box and is still on it (the
    // scrolling drawer around it may claim the press as a possible drag).
    let down = resp.is_pointer_button_down_on()
        || ui.ctx().input(|i| {
            i.pointer.primary_down()
                && i.pointer.press_origin().is_some_and(|o| r.contains(o))
                && i.pointer.latest_pos().is_some_and(|q| r.expand(12.0).contains(q))
        });
    let start: Option<f64> = ui.ctx().data(|d| d.get_temp(id));
    let mut done = false;
    let progress = match (down, start) {
        (true, None) => {
            ui.ctx().data_mut(|d| d.insert_temp(id, now));
            0.0
        }
        (true, Some(t0)) => ((now - t0) / HOLD).min(1.0) as f32,
        (false, _) => {
            ui.ctx().data_mut(|d| d.remove::<f64>(id));
            0.0
        }
    };
    if down {
        ui.ctx().request_repaint();
        if progress >= 1.0 {
            done = true;
            ui.ctx().data_mut(|d| d.remove::<f64>(id));
        }
    }
    let warn = Color32::from_rgb(0xC2, 0x42, 0x3A);
    let p = ui.painter();
    if progress > 0.0 {
        let fill = Rect::from_min_max(r.min, egui::pos2(r.left() + r.width() * progress, r.bottom()));
        p.rect_filled(fill, 8.0, warn.gamma_multiply(0.45));
    }
    p.rect_stroke(r, 8.0, Stroke::new(1.5, warn), egui::StrokeKind::Inside);
    paint_label(p, r, label, Some("hold to clear"), fg);
    done
}

fn switch_row(ui: &mut egui::Ui, g: Glyph, label: &str, on: &mut bool, fg: Color32) {
    row(ui, g, label, fg, |ui| {
        switch(ui, on);
    });
}

/// A pill switch: green track with a white knob when on, gray when off; the knob slides.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(44.0, 26.0);
    let (r, mut resp) = ui.allocate_exact_size(egui::vec2(size.x, 44.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let track = Rect::from_center_size(r.center(), size);
    let off_track = ui.visuals().widgets.inactive.bg_fill.gamma_multiply(1.4);
    let p = ui.painter();
    p.rect_filled(track, 13.0, lerp(off_track, ON, t));
    let x = egui::lerp(track.left() + 13.0..=track.right() - 13.0, t);
    p.circle_filled(egui::pos2(x, track.center().y + 1.0), 10.0, Color32::from_black_alpha(50));
    p.circle_filled(egui::pos2(x, track.center().y), 10.0, lerp(Color32::from_gray(0xB8), Color32::WHITE, t));
    resp
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn divider(ui: &mut egui::Ui, fg: Color32) {
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, fg.gamma_multiply(0.15));
    ui.add_space(8.0);
}

/// Outlined button to a page behind the menu: symbol, word, ›.
fn outlined_button(ui: &mut egui::Ui, g: Glyph, label: &str, fg: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, 12.0, fg.gamma_multiply(0.10));
    }
    p.rect_stroke(r.shrink(1.0), 12.0, Stroke::new(1.5, fg.gamma_multiply(0.55)), egui::StrokeKind::Inside);
    draw_glyph(p, Rect::from_center_size(egui::pos2(r.left() + 20.0, r.center().y), Vec2::splat(22.0)), g, fg);
    p.text(egui::pos2(r.left() + 42.0, r.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), fg);
    chevron(p, egui::pos2(r.right() - 20.0, r.center().y), 1.0, fg);
    resp
}

fn outlined_back(ui: &mut egui::Ui, fg: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.circle_filled(r.center(), 22.0, fg.gamma_multiply(0.12));
    }
    chevron(p, r.center(), -1.0, fg);
    resp
}

/// A › (dir 1) or ‹ (dir −1) drawn as two lines.
fn chevron(p: &egui::Painter, c: Pos2, dir: f32, fg: Color32) {
    let s = Stroke::new(2.4, fg);
    let tip = c + egui::vec2(4.0 * dir, 0.0);
    p.line_segment([tip + egui::vec2(-8.0 * dir, -8.0), tip], s);
    p.line_segment([tip + egui::vec2(-8.0 * dir, 8.0), tip], s);
}

/// The app icon (v2 color design), drawn in code so it is crisp at any size: slate squircle,
/// four faint tiles, each app's symbol in its color.
pub fn suite_icon(p: &egui::Painter, r: Rect) {
    let s = r.width();
    let k = s / 512.0;
    p.rect_filled(r, 120.0 * k, Color32::from_rgb(0x2B, 0x2F, 0x33));
    p.rect_stroke(r.shrink(22.0 * k), 100.0 * k, Stroke::new((3.0 * k).max(0.6), Color32::from_white_alpha(14)), egui::StrokeKind::Middle);
    let tile = 186.0 * k;
    for (i, tool) in [Tool::Word, Tool::Grid, Tool::Deck, Tool::Page].into_iter().enumerate() {
        let (cx, cy) = (i % 2, i / 2);
        let min = r.min + egui::vec2(60.0 * k + cx as f32 * (tile + 20.0 * k), 60.0 * k + cy as f32 * (tile + 20.0 * k));
        let tr = Rect::from_min_size(min, Vec2::splat(tile));
        p.rect_filled(tr, 36.0 * k, tool.accent().gamma_multiply(0.12));
        // The symbol's drawing (100 units) fills ~2/3 of the tile.
        let g = Rect::from_center_size(tr.center(), Vec2::splat(tile * 0.74));
        crate::home::app_glyph_lines(p, g, tool, tool.accent(), 1.35);
    }
}

// ===================================================================== symbols

#[derive(Clone, Copy)]
pub enum Glyph {
    Moon,
    Awake,
    Pencil,
    Clock,
    Trash,
    Spelling,
    Person,
    Info,
    Key,
    Free,
    NoAds,
    NoTrack,
    NoAi,
}

/// Line symbols on a 24-unit grid, 2-unit round strokes (the Third Coast menu style).
pub fn draw_glyph(p: &egui::Painter, r: Rect, g: Glyph, fg: Color32) {
    let k = r.width() / 24.0;
    let o = |x: f32, y: f32| r.min + egui::vec2(x * k, y * k);
    let st = Stroke::new(2.0 * k, fg);
    let line = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| o(x, y)).collect();
        for pt in &v {
            p.circle_filled(*pt, st.width / 2.0, fg);
        }
        p.add(egui::Shape::line(v, st));
    };
    let arc = |c: (f32, f32), rad: f32, a0: f32, a1: f32| {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (c.0 + rad * a.cos(), c.1 + rad * a.sin())
            })
            .collect();
        line(&pts);
    };
    let slash = || {
        p.line_segment([o(3.5, 20.5), o(20.5, 3.5)], Stroke::new(2.0 * k, Color32::from_rgb(0xC2, 0x42, 0x3A)));
    };
    match g {
        Glyph::Moon => {
            // A crescent: the part of a circle not covered by a second, offset circle.
            let (c1, r1) = ((11.0f32, 13.0f32), 8.5f32);
            let (c2, r2) = ((16.5f32, 8.5f32), 7.5f32);
            let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
            let d = (dx * dx + dy * dy).sqrt();
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let hh = (r1 * r1 - a * a).max(0.0).sqrt();
            let m = (c1.0 + a * dx / d, c1.1 + a * dy / d);
            let pa = (m.0 - hh * dy / d, m.1 + hh * dx / d);
            let pb = (m.0 + hh * dy / d, m.1 - hh * dx / d);
            let ang = |c: (f32, f32), q: (f32, f32)| (q.1 - c.1).atan2(q.0 - c.0).to_degrees();
            let toward2 = dy.atan2(dx).to_degrees();
            // Outer arc: from pa to pb the long way (away from circle 2).
            let (oa, mut ob) = (ang(c1, pa), ang(c1, pb));
            while ob < oa { ob += 360.0; }
            let mid = (oa + ob) / 2.0;
            let near = ((mid - toward2).rem_euclid(360.0) - 180.0).abs() > 90.0;
            let (o0, o1) = if near { (oa, ob - 360.0) } else { (oa, ob) };
            // Inner arc on circle 2: from pb back to pa, through the side facing circle 1.
            let (ia, mut ib) = (ang(c2, pb), ang(c2, pa));
            while ib < ia { ib += 360.0; }
            let toward1 = (toward2 + 180.0).rem_euclid(360.0);
            let imid = (ia + ib) / 2.0;
            let inear = ((imid - toward1).rem_euclid(360.0) - 180.0).abs() > 90.0;
            let (i0, i1) = if inear { (ia, ib) } else { (ia, ib - 360.0) };
            let mut pts: Vec<Pos2> = Vec::new();
            for i in 0..=32 {
                let t = (o0 + (o1 - o0) * i as f32 / 32.0).to_radians();
                pts.push(o(c1.0 + r1 * t.cos(), c1.1 + r1 * t.sin()));
            }
            for i in 0..=32 {
                let t = (i0 + (i1 - i0) * i as f32 / 32.0).to_radians();
                pts.push(o(c2.0 + r2 * t.cos(), c2.1 + r2 * t.sin()));
            }
            p.add(egui::Shape::closed_line(pts, st));
        }
        Glyph::Awake => {
            // A phone with rays: the screen stays lit.
            p.rect_stroke(Rect::from_min_max(o(8.0, 3.0), o(16.0, 21.0)), 2.0 * k, st, egui::StrokeKind::Middle);
            line(&[(11.0, 18.0), (13.0, 18.0)]);
            line(&[(3.0, 8.0), (5.0, 9.0)]);
            line(&[(2.5, 12.0), (5.0, 12.0)]);
            line(&[(3.0, 16.0), (5.0, 15.0)]);
            line(&[(21.0, 8.0), (19.0, 9.0)]);
            line(&[(21.5, 12.0), (19.0, 12.0)]);
            line(&[(21.0, 16.0), (19.0, 15.0)]);
        }
        Glyph::Pencil => {
            line(&[(4.0, 20.0), (5.0, 15.5), (16.0, 4.5), (19.5, 8.0), (8.5, 19.0), (4.0, 20.0)]);
            line(&[(13.5, 7.0), (17.0, 10.5)]);
        }
        Glyph::Clock => {
            arc((12.0, 12.0), 9.0, 0.0, 360.0);
            line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }
        Glyph::Trash => {
            line(&[(4.0, 6.5), (20.0, 6.5)]);
            line(&[(9.5, 6.5), (9.5, 3.5), (14.5, 3.5), (14.5, 6.5)]);
            line(&[(6.0, 6.5), (7.0, 20.5), (17.0, 20.5), (18.0, 6.5)]);
            line(&[(10.0, 10.5), (10.0, 16.5)]);
            line(&[(14.0, 10.5), (14.0, 16.5)]);
        }
        Glyph::Spelling => {
            p.text(o(10.5, 9.0), egui::Align2::CENTER_CENTER, "abc", egui::FontId::new(10.5 * k, egui::FontFamily::Name("semibold".into())), fg);
            line(&[(8.0, 17.5), (11.0, 20.5), (18.5, 13.0)]);
        }
        Glyph::Person => {
            arc((12.0, 8.0), 4.0, 0.0, 360.0);
            arc((12.0, 22.0), 8.0, 200.0, 340.0);
        }
        Glyph::Key => {
            arc((8.0, 12.0), 4.5, 0.0, 360.0);
            line(&[(12.5, 12.0), (21.0, 12.0)]);
            line(&[(17.5, 12.0), (17.5, 15.5)]);
            line(&[(20.5, 12.0), (20.5, 15.0)]);
        }
        Glyph::Info => {
            arc((12.0, 12.0), 9.5, 0.0, 360.0);
            p.circle_filled(o(12.0, 7.5), 1.5 * k, fg);
            line(&[(12.0, 11.0), (12.0, 17.0)]);
        }
        Glyph::Free => {
            // A gift-tag style "0": a price tag with a 0.
            line(&[(1.5, 12.0), (8.0, 4.0), (22.5, 4.0), (22.5, 20.0), (8.0, 20.0), (1.5, 12.0)]);
            p.text(o(15.0, 12.0), egui::Align2::CENTER_CENTER, "$0", egui::FontId::new(8.0 * k, egui::FontFamily::Name("semibold".into())), fg);
        }
        Glyph::NoAds => {
            p.rect_stroke(Rect::from_min_max(o(3.0, 6.0), o(21.0, 18.0)), 2.0 * k, st, egui::StrokeKind::Middle);
            p.text(o(12.0, 12.0), egui::Align2::CENTER_CENTER, "AD", egui::FontId::new(8.0 * k, egui::FontFamily::Name("semibold".into())), fg);
            slash();
        }
        Glyph::NoTrack => {
            // An eye, crossed out.
            arc((12.0, 19.0), 11.0, 220.0, 320.0);
            arc((12.0, 5.0), 11.0, 40.0, 140.0);
            arc((12.0, 12.0), 3.0, 0.0, 360.0);
            slash();
        }
        Glyph::NoAi => {
            // A sparkle, crossed out.
            line(&[(12.0, 3.0), (13.8, 10.2), (21.0, 12.0), (13.8, 13.8), (12.0, 21.0), (10.2, 13.8), (3.0, 12.0), (10.2, 10.2), (12.0, 3.0)]);
            slash();
        }
    }
}
