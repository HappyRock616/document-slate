//! Document Slate's mobile "chrome": the bars, buttons and overlays every app (Word, Grid, Deck,
//! Page Slate) shares, so they look and behave the same.
//!
//! - Touch targets are at least 48 dp (Android guideline); icons are upstream WordCraft's
//!   code-drawn set (src/icons.rs), 24 dp, with a short label under them in tool rows.
//! - Viewer mode: floating bars over a full-screen document. They auto-hide a few seconds after
//!   they appear or as soon as the page scrolls; a tap on the page brings them back
//!   ([`AutoHide`]). A page counter pill shows while scrolling ([`page_pill`]).
//! - Edit mode: the bars stay (you need them while editing).

use eframe::egui::{self, Color32, Rect, Sense, Vec2};

use crate::icons;

/// Minimum touch target (dp ≈ egui points on Android).
pub const TOUCH: f32 = 48.0;
/// Icon size inside buttons.
pub const ICON: f32 = 24.0;
/// How long viewer bars stay up after appearing, seconds.
const SHOW_FOR: f64 = 3.5;

/// Draw one of upstream's icons centred in `r`.
pub fn icon(painter: &egui::Painter, r: Rect, name: &str, fg: Color32, accent: Color32) {
    let r = Rect::from_center_size(r.center(), Vec2::splat(ICON.min(r.width()).min(r.height())));
    icons::paint(painter, r, name, fg, accent);
}

/// A round icon-only button for bars (48×48). `fg` draws the icon (light on accent bars).
pub fn bar_button(ui: &mut egui::Ui, name: &str, hint: &str, fg: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(TOUCH), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.circle_filled(r.center(), TOUCH / 2.0 - 2.0, Color32::from_white_alpha(60));
    }
    icon(p, r, name, fg, fg);
    resp.on_hover_text(hint)
}

/// A tool-row button: icon with a short label under it (56×60). `on` = active (e.g. Bold at
/// the caret), shown with a tinted rounded background.
pub fn tool(ui: &mut egui::Ui, name: &str, label: &str, on: bool, accent: Color32) -> egui::Response {
    let fg = ui.visuals().text_color();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(60.0, 60.0), Sense::click());
    let p = ui.painter();
    let bg = if on {
        Some(accent.gamma_multiply(0.30))
    } else if resp.is_pointer_button_down_on() {
        Some(fg.gamma_multiply(0.12))
    } else {
        None
    };
    if let Some(bg) = bg {
        p.rect_filled(r.shrink(2.0), 10.0, bg);
    }
    let icon_r = Rect::from_center_size(egui::pos2(r.center().x, r.top() + 22.0), Vec2::splat(ICON));
    icons::paint(p, icon_r, name, fg, accent);
    p.text(
        egui::pos2(r.center().x, r.bottom() - 11.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.0),
        fg.gamma_multiply(0.85),
    );
    resp
}

/// A tool-row button that opens a small pop-up menu of less-used choices.
pub fn tool_menu(ui: &mut egui::Ui, name: &str, label: &str, accent: Color32, menu: impl FnOnce(&mut egui::Ui)) {
    let resp = tool(ui, name, label, false, accent);
    // Tool rows sit at the bottom, right above the keyboard: open the menu UPWARDS, and let a
    // long menu scroll inside the space above the button instead of running under the keyboard.
    let room = (resp.rect.top() - ui.ctx().content_rect().top() - 48.0).max(160.0);
    egui::Popup::menu(&resp)
        .align(egui::RectAlign::TOP_START)
        .align_alternatives(&[egui::RectAlign::TOP_END, egui::RectAlign::TOP])
        .show(|ui| {
            ui.set_min_width(180.0);
            ui.style_mut().spacing.button_padding = egui::vec2(12.0, 10.0);
            egui::ScrollArea::vertical().max_height(room).show(ui, menu);
        });
}

/// Soft fades at the ends of a sideways-scrolling tool row, with a chevron, so it's clear that
/// more tools are off-screen (a swipe reveals them).
pub fn scroll_hints<R>(ui: &egui::Ui, out: &egui::scroll_area::ScrollAreaOutput<R>) {
    let r = out.inner_rect;
    let hidden_right = out.content_size.x - out.state.clamped_offset().x - r.width();
    let hidden_left = out.state.clamped_offset().x;
    let bg = ui.visuals().panel_fill;
    let fg = ui.visuals().text_color().gamma_multiply(0.8);
    let painter = ui.painter_at(r);
    for (show, right) in [(hidden_right > 2.0, true), (hidden_left > 2.0, false)] {
        if !show {
            continue;
        }
        const W: f32 = 36.0;
        let band = if right {
            egui::Rect::from_min_max(egui::pos2(r.right() - W, r.top()), r.max)
        } else {
            egui::Rect::from_min_max(r.min, egui::pos2(r.left() + W, r.bottom()))
        };
        // Gradient: transparent -> panel colour towards the edge.
        let clear = egui::Color32::TRANSPARENT;
        let (l, rr) = if right { (clear, bg) } else { (bg, clear) };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(band.left_top(), l);
        mesh.colored_vertex(band.right_top(), rr);
        mesh.colored_vertex(band.right_bottom(), rr);
        mesh.colored_vertex(band.left_bottom(), l);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
        let d = if right { 1.0 } else { -1.0 };
        let c = if right { egui::pos2(band.right() - 9.0, band.center().y) } else { egui::pos2(band.left() + 9.0, band.center().y) };
        painter.line(vec![c + egui::vec2(-4.0 * d, -7.0), c + egui::vec2(3.0 * d, 0.0), c + egui::vec2(-4.0 * d, 7.0)], egui::Stroke::new(2.5, fg));
    }
}

/// A full-width menu entry with a comfortable touch height.
pub fn menu_item(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(text).min_size(egui::vec2(ui.available_width(), 40.0))).clicked()
}

/// A thin vertical divider between groups in a tool row.
pub fn divider(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 44.0), Sense::hover());
    let c = ui.visuals().text_color().gamma_multiply(0.2);
    ui.painter().line_segment([r.center_top(), r.center_bottom()], egui::Stroke::new(1.0, c));
}

/// Viewer bars that show for a few seconds, hide on scroll, and come back on a tap.
pub struct AutoHide {
    visible: bool,
    shown_at: f64,
    /// Kept up regardless (search open, a menu open…).
    pub pinned: bool,
}

impl Default for AutoHide {
    fn default() -> Self {
        Self { visible: true, shown_at: -1.0, pinned: false }
    }
}

impl AutoHide {
    pub fn show(&mut self, now: f64) {
        self.visible = true;
        self.shown_at = now;
    }

    pub fn toggle(&mut self, now: f64) {
        if self.visible { self.visible = false } else { self.show(now) }
    }

    /// Call once a frame. Returns how visible the bars are (0 = hidden … 1 = shown), animated.
    pub fn update(&mut self, ctx: &egui::Context, scrolled: bool) -> f32 {
        let now = ctx.input(|i| i.time);
        if self.shown_at < 0.0 {
            self.shown_at = now;
        }
        if self.pinned {
            self.visible = true;
            self.shown_at = now;
        } else if self.visible && (scrolled || now - self.shown_at > SHOW_FOR) {
            self.visible = false;
        }
        if self.visible && !self.pinned {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((SHOW_FOR - (now - self.shown_at)).max(0.05)));
        }
        ctx.animate_bool_with_time(egui::Id::unique("slate-bars"), self.visible, 0.18)
    }
}

/// The "3 / 12" page counter: a dark pill near the bottom, `opacity` 0…1.
pub fn page_pill(ctx: &egui::Context, text: &str, opacity: f32, bottom_inset: f32) {
    if opacity <= 0.01 {
        return;
    }
    let screen = ctx.content_rect();
    let font = egui::FontId::proportional(15.0);
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE));
    let size = galley.size() + egui::vec2(28.0, 14.0);
    let pos = egui::pos2(screen.center().x - size.x / 2.0, screen.bottom() - bottom_inset - size.y - 28.0);
    egui::Area::new(egui::Id::unique("slate-page-pill"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .interactable(false)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(r, size.y / 2.0, Color32::from_black_alpha((170.0 * opacity) as u8));
            ui.painter().galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE.gamma_multiply(opacity));
        });
}

/// A short message pill near the bottom (saved, errors…), like an Android toast.
pub fn toast(ctx: &egui::Context, text: &str, bottom_inset: f32) {
    if text.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    let max_w = (screen.width() - 48.0).max(120.0);
    let font = egui::FontId::proportional(14.0);
    // Engine messages can name the upstream apps; show ours.
    let galley = ctx.fonts_mut(|f| f.layout(crate::rebrand::rename(text), font, Color32::WHITE, max_w - 28.0));
    let size = galley.size() + egui::vec2(28.0, 18.0);
    let pos = egui::pos2(screen.center().x - size.x / 2.0, screen.bottom() - bottom_inset - size.y - 84.0);
    egui::Area::new(egui::Id::unique("slate-toast")).order(egui::Order::Foreground).fixed_pos(pos).interactable(false).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(r, 12.0, Color32::from_rgba_unmultiplied(30, 33, 36, 235));
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
    });
}
