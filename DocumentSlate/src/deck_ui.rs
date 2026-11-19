//! Deck Slate: the presentations screen (viewer, editor, slideshow) on the `slate-deck` back-end.
//!
//! - Viewer: slides stacked top to bottom, each as wide as the screen; scroll/swipe between them,
//!   pinch to zoom, the shell's "n / N" pill counts slides. ⋮ menu: Slideshow, Notes.
//! - Editor: a strip of slide thumbnails on top, the current slide fitted below it. Tap selects,
//!   drag moves, the round handles resize, the top knob rotates (all through the engine's own
//!   pointer tool, so every gesture is one undo step). Tap text in a selected box to type.
//!   The bottom bar (src/deck_ui/tools.rs) has the tabs Slides · Text · Insert · Design · File,
//!   and shape/picture/table tools while an object is selected.
//! - Slideshow: full screen on black, tap/swipe forward and back, Back or Esc ends it.
//!
//! Slides are drawn on a background thread (src/deck_ui/render.rs); the UI never waits.

mod render;
mod tools;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::mpsc;

use eframe::egui;
use slate_deck::touch::{ObjectKind, PointerKind};
use slate_deck::links::DeckLink;
use slate_deck::{DeckDoc, PdfLayout, PdfOptions, SaveFormat, ShapeId};

use crate::doc_screen::{DocScreen, ScreenCtx};
use crate::edit_bar::FileAction;
use crate::theme::Tool;

use render::{Key, Kind, Renderer};

/// Space between slides in the viewer, points.
const GAP: f32 = 12.0;
/// Height of the thumbnail strip in edit mode, points.
const STRIP_H: f32 = 72.0;
/// Finger tolerance for hit tests, screen points.
const FINGER: f32 = 18.0;

/// What a tapped link does on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LinkGo {
    /// Show this slide (0-based).
    Slide(usize),
    /// Slideshow: one step forward (1) or back (-1).
    Step(isize),
    /// End the slideshow.
    End,
    /// Handled already (pop-up or message shown).
    Shown,
}

/// A slideshow in progress.
struct Show {
    /// Slides it plays (hidden ones left out).
    order: Vec<usize>,
    /// Position in `order`; `order.len()` = the black "end of show" screen.
    pos: usize,
    /// How far the finger has moved in the swipe under way.
    drag: egui::Vec2,
}

/// What the edit-mode slide is doing with the finger right now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// Engine pointer drag (move, resize, rotate, line end, text selection…).
    Engine,
    /// Panning a zoomed-in slide.
    Pan,
}

pub struct DeckScreen {
    name: String,
    pub(crate) doc: DeckDoc,
    editing: bool,
    renderer: Renderer,
    // ---- viewer ----
    zoom: f32,
    tapped: bool,
    scrolled: bool,
    current: usize,
    last_offset: f32,
    last_view_h: f32,
    jump_to: Option<usize>,
    notes_open: bool,
    show: Option<Show>,
    // ---- search ----
    search_open: bool,
    focus_search: bool,
    query: String,
    searched: String,
    /// Slide of each match.
    hits: Vec<usize>,
    hit: usize,
    // ---- editor ----
    edit_zoom: f32,
    pan: egui::Vec2,
    gesture: Option<Gesture>,
    /// Handle position minus finger position at the start of a handle drag.
    grab: egui::Vec2,
    keyboard_wanted: bool,
    blink_from: f64,
    /// Engine revision the format/look caches were read at.
    seen_rev: u64,
    pub(crate) fmt: serde_json::Value,
    pub(crate) tabs: tools::TabState,
    /// Was text being typed last frame (to switch to the Text tab once when typing starts).
    was_typing: bool,
    // ---- files ----
    /// Layout of the next PDF (File › PDF / Print menus), back to Slides after use.
    pdf_layout: PdfLayout,
    /// Paper for notes and handouts (File › Paper): "letter", "legal" or "a4". Slides print at
    /// the slide's own size.
    pub(crate) pdf_paper: &'static str,
    fidelity_rx: Option<mpsc::Receiver<slate_deck::FidelityReport>>,
    /// Messages to show (open warnings, errors from tools), handed to the shell by the edit bar.
    pub(crate) notices: Vec<String>,
}

impl DeckScreen {
    pub fn open(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let doc = DeckDoc::open_bytes(name, bytes)?;
        Ok(Self::with_doc(name, doc))
    }

    pub fn new_blank() -> Self {
        let doc = DeckDoc::new_blank().or_else(|_| DeckDoc::sample()).unwrap_or_else(|e| panic!("the presentation engine can't start: {e}"));
        Self::with_doc("Untitled.pptx", doc)
    }

    fn with_doc(name: &str, mut doc: DeckDoc) -> Self {
        // Working out what a save would lose costs one export: do it in the background now, so
        // the first Save doesn't wait for it.
        let fidelity_rx = doc.fidelity_inputs().map(|(orig, opened)| {
            let (tx, rx) = mpsc::channel();
            let spawned = std::thread::Builder::new().name("deck-fidelity".into()).spawn(move || {
                let _ = tx.send(slate_deck::fidelity::probe(&orig, &opened));
            });
            if let Err(e) = spawned {
                log::warn!("fidelity thread: {e}");
            }
            rx
        });
        // Missing fonts are normal on a phone (a similar font is used): not worth a message.
        let notices: Vec<String> = doc.take_messages().into_iter().filter(|m| !m.starts_with("Fonts not on this device")).collect();
        Self {
            name: name.to_owned(),
            doc,
            editing: false,
            renderer: Renderer::default(),
            zoom: 1.0,
            tapped: false,
            scrolled: false,
            current: 0,
            last_offset: 0.0,
            last_view_h: 0.0,
            jump_to: None,
            notes_open: false,
            show: None,
            search_open: false,
            focus_search: false,
            query: String::new(),
            searched: String::new(),
            hits: Vec::new(),
            hit: 0,
            edit_zoom: 1.0,
            pan: egui::Vec2::ZERO,
            gesture: None,
            grab: egui::Vec2::ZERO,
            keyboard_wanted: false,
            blink_from: 0.0,
            seen_rev: u64::MAX,
            fmt: serde_json::Value::Null,
            tabs: tools::TabState::default(),
            was_typing: false,
            pdf_layout: PdfLayout::Slides,
            pdf_paper: "letter",
            fidelity_rx,
            notices,
        }
    }

    // ------------------------------------------------------------------ helpers for the tools

    /// Run a back-end edit; an error becomes a message for the user.
    pub(crate) fn act<T>(&mut self, r: Result<T, String>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("deck: {e}");
                self.notices.push(e);
                None
            }
        }
    }

    /// The single selected object (kind and id), when not typing in it.
    pub(crate) fn selected_object(&self) -> Option<(ShapeId, ObjectKind)> {
        if self.doc.editing_text().is_some() {
            return None;
        }
        let sel = self.doc.selected();
        let first = *sel.first()?;
        let kind = self.doc.objects().into_iter().find(|o| o.id == first).map(|o| o.kind)?;
        Some((first, kind))
    }

    pub(crate) fn typing(&self) -> bool {
        self.doc.editing_text().is_some()
    }

    /// Refresh caches that depend on the document/selection (format state for the Text tab).
    fn refresh(&mut self) {
        let rev = self.doc.revision();
        if rev == self.seen_rev {
            return;
        }
        self.seen_rev = rev;
        self.fmt = if self.typing() || !self.doc.selected().is_empty() { self.doc.format_state().unwrap_or_default() } else { serde_json::Value::Null };
        let n = self.doc.slide_count();
        if self.current >= n {
            self.current = n.saturating_sub(1);
        }
    }

    pub(crate) fn fmt_on(&self, key: &str) -> bool {
        self.fmt.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false)
    }

    /// Where a new object goes: the middle of the slide, `w` x `h` points.
    pub(crate) fn centred(&self, w: f64, h: f64) -> slate_deck::edit::Rect {
        let (sw, sh) = self.doc.slide_size();
        slate_deck::edit::Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h)
    }

    pub(crate) fn start_show(&mut self, from_current: bool) {
        let order = self.doc.show_order();
        if order.is_empty() {
            self.notices.push("Every slide is hidden: nothing to show".into());
            return;
        }
        let cur = if self.editing { self.doc.current_slide() } else { self.current };
        let pos = if from_current { order.iter().position(|&i| i >= cur).unwrap_or(0) } else { 0 };
        self.show = Some(Show { order, pos, drag: egui::Vec2::ZERO });
    }

    fn pdf_options(&self) -> PdfOptions {
        // Slides use the slide's own size; notes and handouts go on the chosen paper (portrait).
        PdfOptions { layout: self.pdf_layout, paper: self.pdf_paper.into(), ..Default::default() }
    }

    // ------------------------------------------------------------------ search

    fn run_search(&mut self) {
        self.searched = self.query.clone();
        self.hits.clear();
        self.hit = 0;
        let q = self.query.trim().to_owned();
        if q.is_empty() {
            return;
        }
        match self.doc.run("text.find", serde_json::json!({ "text": q })) {
            Ok(v) => {
                self.hits = v
                    .as_array()
                    .map(|a| a.iter().filter_map(|m| m.get("slide").and_then(serde_json::Value::as_u64)).map(|s| s as usize).collect())
                    .unwrap_or_default();
                if let Some(&s) = self.hits.first() {
                    self.go_to(s);
                }
            }
            Err(e) => log::warn!("find: {e}"),
        }
    }

    fn step_hit(&mut self, d: isize) {
        if self.hits.is_empty() {
            return;
        }
        self.hit = (self.hit as isize + d).rem_euclid(self.hits.len() as isize) as usize;
        let s = self.hits[self.hit];
        self.go_to(s);
    }

    /// Show slide `i` (viewer: scroll to it; editor: make it current).
    pub(crate) fn go_to(&mut self, i: usize) {
        if self.editing {
            let r = self.doc.go_to_slide(i);
            self.act(r);
        } else {
            self.jump_to = Some(i);
        }
        self.current = i;
    }

    // ------------------------------------------------------------------ typing

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let typing = self.typing();
        for e in events {
            let r = match e {
                egui::Event::Text(t) if typing && !t.is_empty() => self.doc.type_text(&t),
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Z if modifiers.command => self.doc.undo(),
                    egui::Key::Y if modifiers.command => self.doc.redo(),
                    egui::Key::Backspace if typing => self.doc.backspace(),
                    egui::Key::Delete if typing => self.doc.delete_forward(),
                    egui::Key::Enter if typing && modifiers.shift => self.doc.type_text("\u{b}"),
                    egui::Key::Enter if typing => self.doc.type_text("\n"),
                    egui::Key::ArrowLeft if typing => self.doc.move_caret("left"),
                    egui::Key::ArrowRight if typing => self.doc.move_caret("right"),
                    egui::Key::ArrowUp if typing => self.doc.move_caret("up"),
                    egui::Key::ArrowDown if typing => self.doc.move_caret("down"),
                    egui::Key::Home if typing => self.doc.move_caret("lineStart"),
                    egui::Key::End if typing => self.doc.move_caret("lineEnd"),
                    egui::Key::Escape if typing => self.doc.exit_text(),
                    egui::Key::Escape => self.doc.deselect(),
                    egui::Key::Delete | egui::Key::Backspace if !self.doc.selected().is_empty() => self.doc.delete_objects(&[]),
                    _ => continue,
                },
                _ => continue,
            };
            self.blink_from = ctx.input(|i| i.time);
            self.act(r);
        }
    }

    // ------------------------------------------------------------------ viewer

    fn show_viewer(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let pinch = ctx.input(|i| i.zoom_delta());
        if pinch != 1.0 {
            self.zoom = (self.zoom * pinch).clamp(1.0, 5.0);
        }
        let ppp = ctx.pixels_per_point();
        let (sw, sh) = self.doc.slide_size();
        let n = self.doc.slide_count();
        let view_w = ui.available_width();
        let view_h = ui.available_height();
        let base = ((view_w - 2.0 * GAP) / sw as f32).max(0.05);
        let scale = base * self.zoom;
        let (pw, ph) = (sw as f32 * scale, sh as f32 * scale);
        let total_h = GAP + n as f32 * (ph + GAP);
        let total_w = (pw + 2.0 * GAP).max(view_w);
        let mut area = egui::ScrollArea::both().auto_shrink(false);
        if let Some(i) = self.jump_to.take() {
            area = area.vertical_scroll_offset((GAP + i as f32 * (ph + GAP) - GAP).max(0.0));
        }
        let showing = self.show.is_some();
        let out = area.show_viewport(ui, |ui, viewport| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(total_w, total_h), egui::Sense::click());
            // A tap on a link follows it; any other tap shows/hides the bars.
            let mut link_tap: Option<(usize, f64, f64)> = None;
            for i in 0..n {
                let local = egui::Rect::from_min_size(egui::pos2((total_w - pw) / 2.0, GAP + i as f32 * (ph + GAP)), egui::vec2(pw, ph));
                if resp.clicked()
                    && let Some(p) = resp.interact_pointer_pos().filter(|p| local.translate(rect.min.to_vec2()).contains(*p))
                {
                    let o = p - local.translate(rect.min.to_vec2()).min;
                    link_tap = Some((i, (o.x / scale) as f64, (o.y / scale) as f64));
                }
                let key = Key { slide: i, kind: Kind::Plain };
                if !local.intersects(viewport.expand(viewport.height())) {
                    // Not during a slideshow: it uses the same textures for slides that may be
                    // far from where the viewer is scrolled.
                    if !showing {
                        self.renderer.drop_texture(key);
                    }
                    continue;
                }
                let screen = local.translate(rect.min.to_vec2());
                if !showing {
                    self.renderer.want(key, scale * ppp, local.intersects(viewport));
                }
                paint_slide(ui.painter(), screen, self.renderer.texture(key), true);
                self.renderer.scanner.draw_badges(ui.painter(), i, screen, scale, egui::Color32::from_rgb(0xD9, 0x73, 0x2B));
                if self.search_open && self.hits.get(self.hit) == Some(&i) {
                    ui.painter().rect_stroke(screen.expand(3.0), 4.0, egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 160, 0)), egui::StrokeKind::Outside);
                }
            }
            if resp.clicked() {
                let go = link_tap.and_then(|(i, x, y)| self.link_tap(i, x, y, n));
                match go {
                    Some(LinkGo::Slide(i)) => {
                        self.jump_to = Some(i);
                        self.current = i;
                    }
                    Some(LinkGo::Shown) => {}
                    Some(LinkGo::Step(_) | LinkGo::End) | None => {
                        let code = link_tap.and_then(|(i, x, y)| self.renderer.scanner.hit_at(i, [x as f32, y as f32], 6.0).map(|h| h.found.clone()));
                        match code {
                            Some(f) => crate::barcode_scan::show(&f),
                            None => self.tapped = true,
                        }
                    }
                }
            }
            // Long-press reads a barcode too small to find at screen resolution.
            if resp.long_touched()
                && let Some(p) = resp.interact_pointer_pos()
            {
                for i in 0..n {
                    let screen = egui::Rect::from_min_size(egui::pos2((total_w - pw) / 2.0, GAP + i as f32 * (ph + GAP)), egui::vec2(pw, ph)).translate(rect.min.to_vec2());
                    if screen.contains(p) {
                        let o = (p - screen.min) / scale;
                        self.renderer.read_spot(&ctx, i, [o.x, o.y]);
                    }
                }
            }
            viewport
        });
        self.scrolled = (out.inner.min.y - self.last_offset).abs() > 0.5 && self.last_view_h > 0.0;
        let mid = out.inner.center().y;
        self.current = (((mid - GAP) / (ph + GAP)).floor().max(0.0) as usize).min(n.saturating_sub(1));
        self.last_offset = out.inner.min.y;
        self.last_view_h = view_h;
        if self.notes_open {
            self.notes_sheet(&ctx);
        }
    }

    /// Speaker notes of the slide in view, in a sheet at the bottom (viewer ⋮ › Notes).
    fn notes_sheet(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let width = (screen.width() - 16.0).min(640.0);
        let height = (screen.height() * 0.32).clamp(140.0, 320.0);
        let pos = egui::pos2(screen.center().x - width / 2.0, screen.bottom() - height - 16.0);
        let notes = self.doc.notes(self.current);
        let fill = ctx.global_style().visuals.panel_fill;
        let mut close = false;
        egui::Area::new(egui::Id::unique("deck-notes-sheet")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::new().fill(fill).corner_radius(16).inner_margin(12).shadow(egui::Shadow { offset: [0, 2], blur: 12, spread: 0, color: egui::Color32::from_black_alpha(80) }).show(ui, |ui| {
                ui.set_width(width - 24.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Notes · slide {}", self.current + 1)).size(16.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::chrome::bar_button(ui, "close", "Close", ui.visuals().text_color()).clicked() {
                            close = true;
                        }
                    });
                });
                egui::ScrollArea::vertical().max_height(height - 70.0).show(ui, |ui| {
                    if notes.trim().is_empty() {
                        ui.label(egui::RichText::new("No notes on this slide.").weak());
                    } else {
                        ui.label(egui::RichText::new(&notes).size(15.0));
                    }
                });
            });
        });
        if close {
            self.notes_open = false;
        }
    }

    // ------------------------------------------------------------------ editor

    fn show_editor(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) {
        let ctx = ui.ctx().clone();
        let typing_focus = ctx.memory(|m| m.focused().is_none());
        if typing_focus && self.show.is_none() {
            self.handle_keys(&ctx);
        }
        self.refresh();
        let typing = self.typing();
        if typing && !self.was_typing {
            self.tabs.tab = tools::Tab::Text;
        }
        if !typing {
            self.keyboard_wanted = false;
        }
        self.was_typing = typing;
        self.current = self.doc.current_slide();

        self.slide_strip(ui, cx.accent);

        // The slide, fitted into what's left.
        let pinch = ctx.input(|i| i.zoom_delta());
        if pinch != 1.0 {
            self.edit_zoom = (self.edit_zoom * pinch).clamp(1.0, 4.0);
            if self.edit_zoom <= 1.0 {
                self.pan = egui::Vec2::ZERO;
            }
        }
        let area = ui.available_rect_before_wrap();
        let (sw, sh) = self.doc.slide_size();
        let fit = ((area.width() - 16.0) / sw as f32).min((area.height() - 16.0) / sh as f32).max(0.05);
        let scale = fit * self.edit_zoom;
        let size = egui::vec2(sw as f32 * scale, sh as f32 * scale);
        // Keep the zoomed slide from leaving the screen entirely.
        let max_pan = ((size - area.size()) / 2.0 + egui::vec2(24.0, 24.0)).max(egui::Vec2::ZERO);
        self.pan = self.pan.clamp(-max_pan, max_pan);
        let slide = egui::Rect::from_center_size(area.center() + self.pan, size);
        let resp = ui.allocate_rect(area, egui::Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let ppp = ctx.pixels_per_point();
        let key = Key { slide: self.current, kind: Kind::Edit };
        self.renderer.want(key, scale * ppp, true);
        paint_slide(&painter, slide, self.renderer.texture(key), true);

        // Finger -> engine (slide points). Handles are picked here in screen space (a finger
        // is big next to a small slide), then handed to the engine at their exact spot; bodies
        // use a small tolerance so a handle never steals a drag that starts inside a shape.
        let to_slide = |p: egui::Pos2| (((p.x - slide.min.x) / scale) as f64, ((p.y - slide.min.y) / scale) as f64);
        let pt = |p: (f64, f64)| egui::pos2(slide.min.x + p.0 as f32 * scale, slide.min.y + p.1 as f32 * scale);
        let fine = (4.0 / scale as f64).max(1.0);
        let finger = (FINGER / scale) as f64;
        if resp.drag_started() {
            let origin = ctx.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            if let Some(o) = origin {
                self.gesture = Some(Gesture::Pan);
                self.grab = egui::Vec2::ZERO;
                if ctx.input(|i| i.multi_touch().is_none()) {
                    let (x, y) = to_slide(o);
                    // Inside a shape only a handle right under the finger counts (small shapes
                    // on a phone are barely bigger than a finger); outside, a finger's reach.
                    let on_body = self.doc.hit_test(x, y, fine).is_some_and(|h| matches!(h, slate_deck::touch::TouchHit::Object { .. }));
                    let reach = if on_body { 12.0 } else { 26.0 };
                    let handle = if self.typing() { None } else { self.handle_near(o, &pt, reach) };
                    // A resize handle keeps the finger's offset from it (no jump); the rotate
                    // knob follows the finger's angle around the centre directly.
                    let start = match handle {
                        Some((h, rotate)) => Some((to_slide(h), if rotate { egui::Vec2::ZERO } else { h - o })),
                        None if self.doc.hit_test(x, y, fine).is_some() => Some(((x, y), egui::Vec2::ZERO)),
                        None => None,
                    };
                    if let Some(((sx, sy), grab)) = start {
                        self.grab = grab;
                        let r = self.doc.pointer(PointerKind::Down, sx, sy, fine);
                        self.act(r);
                        self.gesture = Some(Gesture::Engine);
                    }
                }
            }
        }
        if resp.dragged() {
            match self.gesture {
                Some(Gesture::Engine) => {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let (x, y) = to_slide(p + self.grab);
                        let r = self.doc.pointer(PointerKind::Drag, x, y, fine);
                        self.act(r);
                    }
                }
                Some(Gesture::Pan) if self.edit_zoom > 1.0 => self.pan += resp.drag_delta(),
                _ => {}
            }
        }
        if resp.drag_stopped() {
            if self.gesture == Some(Gesture::Engine) {
                let p = resp.interact_pointer_pos().or(ctx.input(|i| i.pointer.latest_pos())).unwrap_or(slide.center());
                let (x, y) = to_slide(p + self.grab);
                let r = self.doc.pointer(PointerKind::Up, x, y, fine);
                self.act(r);
            }
            self.gesture = None;
        }
        // Two fingers pan a zoomed slide.
        if let Some(mt) = ctx.input(|i| i.multi_touch())
            && self.edit_zoom > 1.0
        {
            self.pan += mt.translation_delta;
        }
        let double = resp.double_clicked() || resp.long_touched();
        if (resp.clicked() || double)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let (x, y) = to_slide(p);
            let tol = if self.doc.hit_test(x, y, fine).is_some() { fine } else { finger };
            let was_typing = self.typing();
            let sel_before = self.doc.selected();
            if double {
                let r = self.doc.pointer(PointerKind::DoubleClick, x, y, tol);
                self.act(r);
            } else {
                let r = self.doc.pointer(PointerKind::Down, x, y, tol);
                self.act(r);
                let r = self.doc.pointer(PointerKind::Up, x, y, tol);
                self.act(r);
            }
            // Tapping a selected shape that has no text yet (a fresh box, bubble or arrow)
            // starts typing in it, like a double tap.
            if !self.typing() && !double && !was_typing {
                self.maybe_type_into_selected(x, y, tol, &sel_before);
            }
            self.keyboard_wanted = self.typing();
            self.blink_from = ctx.input(|i| i.time);
        }

        // Selection frame, handles, caret.
        let accent = cx.accent;
        for q in self.doc.text_selection_quads() {
            let pts: Vec<egui::Pos2> = q.iter().map(|p| pt(*p)).collect();
            painter.add(egui::Shape::convex_polygon(pts, accent.gamma_multiply(0.35), egui::Stroke::NONE));
        }
        if typing {
            if let Some((a, b)) = self.doc.caret() {
                let (a, b) = (pt(a), pt(b));
                let since = ctx.input(|i| i.time) - self.blink_from;
                if since < 0.5 || (since * 2.0) as i64 % 2 == 0 {
                    painter.line_segment([a, b], egui::Stroke::new(2.0, accent));
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                if typing_focus && self.keyboard_wanted {
                    let caret = egui::Rect::from_two_pos(a, b).expand2(egui::vec2(1.0, 0.0));
                    let ime = egui::output::IMEOutput::new(ui.scope_id().with("deck-editor"), 1, egui::IMEPurpose::Normal, slide, caret);
                    ctx.output_mut(|o| o.ime = Some(ime));
                }
            }
        }
        for h in self.doc.selection_handles() {
            let frame_col = accent;
            if let Some([a, b]) = h.line_ends {
                painter.line_segment([pt(a), pt(b)], egui::Stroke::new(1.0, frame_col.gamma_multiply(0.6)));
                for e in [a, b] {
                    knob(&painter, pt(e), accent);
                }
                continue;
            }
            // The frame sits a few pixels outside the shape, so its own outline stays visible.
            let corners = [h.resize[0], h.resize[2], h.resize[4], h.resize[6]].map(pt);
            let mid = corners.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
            let corners = corners.map(|c| c + (c - mid.to_pos2()).normalized() * 5.0);
            let mut poly = corners.to_vec();
            poly.push(corners[0]);
            painter.add(egui::Shape::line(poly, egui::Stroke::new(1.5, frame_col)));
            if typing {
                continue; // typing: just the frame
            }
            let rot = rotate_spot(pt(h.resize[1]), pt(h.rotate), pt(h.resize[5]));
            painter.line_segment([pt(h.resize[1]), rot], egui::Stroke::new(1.0, frame_col));
            for r in h.resize {
                knob(&painter, pt(r), accent);
            }
            rotate_knob(&painter, rot, accent);
        }
        if !typing {
            for (_, _, q) in self.doc.adjust_handles() {
                adjust_knob(&painter, pt(q));
            }
        }
    }

    /// The selection handle (resize, rotate, line end) within a finger's reach of `p`, on screen.
    /// Returns where the engine has it on screen, and whether it is the rotate knob.
    fn handle_near(&self, p: egui::Pos2, pt: &impl Fn((f64, f64)) -> egui::Pos2, reach: f32) -> Option<(egui::Pos2, bool)> {
        let mut best: Option<(f32, egui::Pos2, bool)> = None;
        // Shape-adjust handles (bubble tails, corner radius, arrow heads) win ties: listed first.
        let adjust: Vec<egui::Pos2> = self.doc.adjust_handles().into_iter().map(|(_, _, q)| pt(q)).collect();
        for q in &adjust {
            let d = q.distance(p);
            if d <= reach && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d - 4.0, *q, false));
            }
        }
        for h in self.doc.selection_handles() {
            let spots: Vec<(f64, f64)> = match h.line_ends {
                Some(ends) => ends.to_vec(),
                None => h.resize.to_vec(),
            };
            // (where it's drawn, where the engine has it)
            let mut spots: Vec<(egui::Pos2, egui::Pos2, bool)> = spots.into_iter().map(|s| (pt(s), pt(s), false)).collect();
            if h.line_ends.is_none() {
                spots.push((rotate_spot(pt(h.resize[1]), pt(h.rotate), pt(h.resize[5])), pt(h.rotate), true));
            }
            for (drawn, engine, rotate) in spots {
                let d = drawn.distance(p);
                if d <= reach && best.is_none_or(|(bd, ..)| d < bd) {
                    best = Some((d, engine, rotate));
                }
            }
        }
        best.map(|(_, q, r)| (q, r))
    }

    /// A tap on a shape that was already selected and has no text: start typing in it (a new
    /// box, bubble or arrow only gets text once you tap it again).
    pub(crate) fn maybe_type_into_selected(&mut self, x: f64, y: f64, tol: f64, sel_before: &[ShapeId]) {
        let Some(slate_deck::touch::TouchHit::Object { id, .. }) = self.doc.hit_test(x, y, tol) else { return };
        if sel_before != [id] || self.doc.selected() != [id] {
            return;
        }
        let Some(look) = self.doc.shape_look(id) else { return };
        let empty = !self.doc.objects().iter().any(|o| o.id == id && o.has_text);
        if look.has_text_body && !look.is_line && empty {
            let r = self.doc.edit_text(id, None);
            self.act(r);
        }
    }

    /// Thumbnails of every slide across the top; tap one to edit it, + adds a slide.
    fn slide_strip(&mut self, ui: &mut egui::Ui, accent: egui::Color32) {
        let n = self.doc.slide_count();
        let (sw, sh) = self.doc.slide_size();
        let th = STRIP_H - 18.0;
        let tw = th * (sw / sh) as f32;
        let ppp = ui.ctx().pixels_per_point();
        let hidden: Vec<bool> = self.doc.slides().iter().map(|s| s.hidden).collect();
        let mut go = None;
        let mut add = false;
        let text = ui.visuals().text_color();
        let out = egui::ScrollArea::horizontal().id_salt("deck-strip").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_height(STRIP_H);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(4.0);
                for i in 0..n {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(tw, STRIP_H), egui::Sense::click());
                    let thumb = egui::Rect::from_min_size(r.min + egui::vec2(0.0, 4.0), egui::vec2(tw, th));
                    if ui.is_rect_visible(r) {
                        let key = Key { slide: i, kind: Kind::Thumb };
                        self.renderer.want(key, (th / sh as f32) * ppp, true);
                        paint_slide(ui.painter(), thumb, self.renderer.texture(key), false);
                        if hidden.get(i).copied().unwrap_or(false) {
                            ui.painter().rect_filled(thumb, 2.0, egui::Color32::from_black_alpha(110));
                        }
                    }
                    if i == self.current {
                        ui.painter().rect_stroke(thumb.expand(2.0), 3.0, egui::Stroke::new(2.5, accent), egui::StrokeKind::Outside);
                    }
                    let label = if hidden.get(i).copied().unwrap_or(false) { format!("{} (hidden)", i + 1) } else { format!("{}", i + 1) };
                    ui.painter().text(egui::pos2(r.center().x, r.bottom() - 6.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(10.5), text.gamma_multiply(0.8));
                    if resp.clicked() {
                        go = Some(i);
                    }
                    if i == self.current && self.tabs.scroll_strip {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                }
                let (r, resp) = ui.allocate_exact_size(egui::vec2(th, STRIP_H), egui::Sense::click());
                let b = egui::Rect::from_center_size(egui::pos2(r.center().x, r.top() + 4.0 + th / 2.0), egui::vec2(th * 0.8, th * 0.8));
                ui.painter().rect_stroke(b, 6.0, egui::Stroke::new(1.0, text.gamma_multiply(0.4)), egui::StrokeKind::Inside);
                crate::chrome::icon(ui.painter(), b, "plus", text, accent);
                if resp.on_hover_text("New slide").clicked() {
                    add = true;
                }
            });
        });
        self.tabs.scroll_strip = false;
        crate::chrome::scroll_hints(ui, &out);
        if let Some(i) = go {
            self.go_to(i);
        }
        if add {
            let r = self.doc.new_slide(None);
            self.act(r);
            self.tabs.scroll_strip = true;
        }
        // Thumbnails of slides that no longer exist.
        self.renderer.forget_beyond(n);
    }

    // ------------------------------------------------------------------ slideshow

    fn show_slideshow(&mut self, ctx: &egui::Context) {
        let Some(show) = self.show.as_mut() else { return };
        let screen = ctx.content_rect();
        let (sw, sh) = self.doc.slide_size();
        let scale = (screen.width() / sw as f32).min(screen.height() / sh as f32);
        let slide = egui::Rect::from_center_size(screen.center(), egui::vec2(sw as f32 * scale, sh as f32 * scale));
        let ppp = ctx.pixels_per_point();
        let mut step: isize = 0;
        let mut end = false;
        let len = show.order.len();
        let pos = show.pos;
        let current = show.order.get(pos).copied();
        let next = show.order.get(pos + 1).copied();
        if let Some(i) = current {
            self.renderer.want(Key { slide: i, kind: Kind::Plain }, scale * ppp, true);
        }
        if let Some(i) = next {
            self.renderer.want(Key { slide: i, kind: Kind::Plain }, scale * ppp, false);
        }
        let tex = current.and_then(|i| self.renderer.texture(Key { slide: i, kind: Kind::Plain }));
        let mut link_go: Option<LinkGo> = None;
        let (mut swiped, mut swipe_done) = (egui::Vec2::ZERO, false);
        // Below the link pop-up while it's open (the pop-up is a Foreground layer).
        let order = if crate::links::is_open() { egui::Order::Middle } else { egui::Order::Tooltip };
        egui::Area::new(egui::Id::unique("deck-slideshow")).order(order).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            let p = ui.painter();
            p.rect_filled(r, 0.0, egui::Color32::BLACK);
            if current.is_some() {
                paint_slide(p, slide, tex, false);
            } else {
                p.text(r.center(), egui::Align2::CENTER_CENTER, "End of slide show · tap to exit", egui::FontId::proportional(16.0), egui::Color32::from_gray(200));
            }
            // A swipe: add up the finger's movement while it drags (the press position is gone
            // by the time the drag stops), and decide when it lifts.
            if resp.dragged() || resp.drag_stopped() {
                swiped += resp.drag_delta();
            }
            if resp.drag_stopped() {
                swipe_done = true;
            } else if resp.clicked() {
                // A tap on a link or click action runs it (web addresses: the link pop-up).
                let at = resp.interact_pointer_pos().filter(|p| slide.contains(*p)).map(|p| (((p.x - slide.min.x) / scale) as f64, ((p.y - slide.min.y) / scale) as f64));
                if let Some((i, (x, y))) = current.zip(at)
                    && let Some(link) = self.doc.link_at(i, x, y)
                {
                    link_go = Some(self.resolve_link(link, i, self.doc.slide_count()));
                } else if current.is_none() {
                    end = true;
                } else if resp.interact_pointer_pos().is_some_and(|p| p.x < r.left() + r.width() / 3.0) {
                    step = -1;
                } else {
                    step = 1;
                }
            }
            ui.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    end = true;
                }
                if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::PageDown) {
                    step = 1;
                }
                if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::PageUp) {
                    step = -1;
                }
            });
        });
        if let Some(show) = self.show.as_mut() {
            show.drag += swiped;
            if swipe_done {
                let total = std::mem::take(&mut show.drag);
                if total.x < -50.0 {
                    step = 1;
                } else if total.x > 50.0 {
                    step = -1;
                }
            }
        }
        match link_go {
            Some(LinkGo::End) => end = true,
            Some(LinkGo::Step(d)) => step = d,
            Some(LinkGo::Slide(i)) => {
                if let Some(show) = self.show.as_mut()
                    && let Some(at) = show.order.iter().position(|s| *s == i)
                {
                    show.pos = at;
                }
            }
            Some(LinkGo::Shown) | None => {}
        }
        if end {
            self.end_show();
            return;
        }
        if let Some(show) = self.show.as_mut() {
            if step > 0 {
                show.pos = (show.pos + 1).min(len);
            } else if step < 0 {
                show.pos = show.pos.saturating_sub(1);
            }
        }
    }

    /// A tap at slide point (x, y) on slide `slide` (of `n`): what its link does, if it has one.
    pub(crate) fn link_tap(&mut self, slide: usize, x: f64, y: f64, n: usize) -> Option<LinkGo> {
        let link = self.doc.link_at(slide, x, y)?;
        Some(self.resolve_link(link, slide, n))
    }

    /// Web/mail addresses show the shared link pop-up (never open directly); slide links say
    /// where to go; actions Deck Slate doesn't run get a message.
    fn resolve_link(&mut self, link: DeckLink, from: usize, n: usize) -> LinkGo {
        let last = n.saturating_sub(1);
        match link {
            DeckLink::Url(u) => {
                crate::links::show_link(&u);
                LinkGo::Shown
            }
            DeckLink::Slide(i) => LinkGo::Slide(i.min(last)),
            DeckLink::FirstSlide => LinkGo::Slide(0),
            DeckLink::LastSlide => LinkGo::Slide(last),
            DeckLink::NextSlide if self.show.is_some() => LinkGo::Step(1),
            DeckLink::PreviousSlide if self.show.is_some() => LinkGo::Step(-1),
            DeckLink::NextSlide => LinkGo::Slide((from + 1).min(last)),
            DeckLink::PreviousSlide => LinkGo::Slide(from.saturating_sub(1)),
            DeckLink::EndShow => LinkGo::End,
            DeckLink::Other(what) => {
                self.notices.push(format!("This link opens {what} — not supported"));
                LinkGo::Shown
            }
        }
    }

    fn end_show(&mut self) {
        if let Some(show) = self.show.take()
            && let Some(&i) = show.order.get(show.pos.min(show.order.len().saturating_sub(1)))
        {
            self.go_to(i);
        }
    }
}

/// White page, shadow and the slide picture (stretched while a sharper one is drawn).
fn paint_slide(p: &egui::Painter, r: egui::Rect, tex: Option<egui::TextureId>, shadow: bool) {
    if shadow {
        p.rect_filled(r.translate(egui::vec2(2.0, 3.0)), 2.0, egui::Color32::from_black_alpha(60));
    }
    p.rect_filled(r, 0.0, egui::Color32::WHITE);
    if let Some(t) = tex {
        p.image(t, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
}

/// Where the rotate knob is drawn: a finger's length beyond the top-middle handle `n`, away from
/// the bottom-middle one `s` (the engine's own spot `engine` is too close on a small slide).
fn rotate_spot(n: egui::Pos2, engine: egui::Pos2, s: egui::Pos2) -> egui::Pos2 {
    let dir = (n - s).normalized();
    let dir = if dir.is_finite() && dir.length() > 0.5 { dir } else { (engine - n).normalized() };
    if dir.is_finite() { n + dir * 30.0 } else { engine }
}

/// A round resize handle (white with an accent ring).
fn knob(p: &egui::Painter, c: egui::Pos2, accent: egui::Color32) {
    p.circle_filled(c, 7.0, egui::Color32::WHITE);
    p.circle_stroke(c, 7.0, egui::Stroke::new(2.0, accent));
}

/// A shape-adjust handle: a yellow diamond (drag a bubble's tail, a corner's roundness…).
fn adjust_knob(p: &egui::Painter, c: egui::Pos2) {
    let r = 7.5;
    let pts = vec![c + egui::vec2(0.0, -r), c + egui::vec2(r, 0.0), c + egui::vec2(0.0, r), c + egui::vec2(-r, 0.0)];
    p.add(egui::Shape::convex_polygon(pts, egui::Color32::from_rgb(0xFF, 0xD0, 0x30), egui::Stroke::new(1.5, egui::Color32::from_rgb(0x50, 0x40, 0x10))));
}

/// The rotate handle: a filled accent circle with a curved arrow.
fn rotate_knob(p: &egui::Painter, c: egui::Pos2, accent: egui::Color32) {
    p.circle_filled(c, 9.0, accent);
    let pts: Vec<egui::Pos2> = (0..=16)
        .map(|i| {
            let a = -0.6 + i as f32 / 16.0 * 4.6;
            egui::pos2(c.x + 4.5 * a.cos(), c.y + 4.5 * a.sin())
        })
        .collect();
    let tip = *pts.last().unwrap_or(&c);
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.6, egui::Color32::WHITE)));
    p.circle_filled(tip, 1.8, egui::Color32::WHITE);
}

impl DocScreen for DeckScreen {
    fn name(&self) -> &str {
        &self.name
    }
    fn tool(&self) -> Tool {
        Tool::Deck
    }
    fn dirty(&self) -> bool {
        self.doc.is_dirty()
    }
    fn editing(&self) -> bool {
        self.editing
    }
    fn set_editing(&mut self, on: bool) {
        if self.editing == on {
            return;
        }
        if on {
            let r = self.doc.go_to_slide(self.current);
            self.act(r);
            self.tabs.scroll_strip = true;
        } else {
            self.flush_notes();
            let _ = self.doc.exit_text();
            let _ = self.doc.deselect();
            self.current = self.doc.current_slide();
            self.jump_to = Some(self.current);
            self.keyboard_wanted = false;
        }
        self.editing = on;
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) {
        let ctx = ui.ctx().clone();
        if let Some(rx) = &self.fidelity_rx
            && let Ok(rep) = rx.try_recv()
        {
            self.doc.set_fidelity_report(rep);
            self.fidelity_rx = None;
        }
        self.renderer.collect(&ctx);
        self.renderer.sync(&self.doc, &ctx);
        match self.renderer.scanner.poll() {
            Some(crate::barcode_scan::SpotResult::Found(h)) => crate::barcode_scan::show(&h.found),
            Some(crate::barcode_scan::SpotResult::Nothing) => crate::links::note("No barcode found there"),
            None => {}
        }
        if self.search_open && self.query != self.searched {
            self.run_search();
        }
        if self.editing {
            self.show_editor(ui, cx);
        } else {
            self.show_viewer(ui);
        }
        if self.show.is_some() {
            self.show_slideshow(&ctx);
        }
    }

    fn take_tapped(&mut self) -> bool {
        std::mem::take(&mut self.tapped) && self.show.is_none()
    }
    fn scrolled(&self) -> bool {
        self.scrolled && self.show.is_none()
    }
    fn position(&self) -> (usize, usize) {
        (self.current, self.doc.slide_count().max(1))
    }

    // The shell's Back button closes search first: while a slideshow plays it reports itself
    // as "search", so Back ends the show (see the report: a proper back hook would be nicer).
    fn search_open(&self) -> bool {
        self.search_open || self.show.is_some()
    }
    fn toggle_search(&mut self) {
        if self.show.is_some() {
            self.end_show();
            return;
        }
        self.search_open = !self.search_open;
        self.focus_search = self.search_open;
        if !self.search_open {
            self.hits.clear();
            self.searched.clear();
            self.query.clear();
        }
    }
    fn search_bar(&mut self, ui: &mut egui::Ui) {
        if self.show.is_some() {
            return;
        }
        ui.horizontal(|ui| {
            ui.set_min_height(44.0);
            let resp = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Find in slides").desired_width((ui.available_width() - 230.0).max(110.0)));
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            if self.query != self.searched {
                self.run_search();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.step_hit(1);
                resp.request_focus();
            }
            let big = |t: &str| egui::Button::new(t).min_size(egui::vec2(52.0, 36.0));
            if ui.add(big("Prev")).clicked() {
                self.step_hit(-1);
            }
            if ui.add(big("Next")).clicked() {
                self.step_hit(1);
            }
            let count = match (self.hits.len(), self.query.trim().is_empty()) {
                (_, true) => String::new(),
                (0, false) => "No matches".into(),
                (n, false) => format!("{} of {} · slide {}", self.hit + 1, n, self.hits[self.hit] + 1),
            };
            ui.label(count);
        });
    }

    fn view_menu(&self) -> Vec<(String, &'static str)> {
        vec![
            ("Slideshow from start".into(), "show"),
            ("Slideshow from this slide".into(), "show-here"),
            (if self.notes_open { "Hide notes".into() } else { "Show notes".into() }, "notes"),
        ]
    }
    fn view_action(&mut self, id: &str) {
        match id {
            "show" => self.start_show(false),
            "show-here" => self.start_show(true),
            "notes" => self.notes_open = !self.notes_open,
            _ => {}
        }
    }

    fn undo(&mut self) {
        let r = self.doc.undo();
        self.act(r);
    }
    fn redo(&mut self) {
        let r = self.doc.redo();
        self.act(r);
    }

    fn edit_bar(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) -> Option<FileAction> {
        self.refresh();
        let action = tools::show(ui, self, cx.accent);
        if action.is_some() {
            return action;
        }
        if !self.notices.is_empty() {
            let msg = self.notices.drain(..).collect::<Vec<_>>().join(" · ");
            return Some(FileAction::Status(msg));
        }
        None
    }

    fn insert_picture(&mut self, path: &str, width_pt: f32) {
        let Ok(bytes) = std::fs::read(path) else { return };
        let (sw, sh) = self.doc.slide_size();
        let (iw, ih) = if bytes.len() > 24 { (u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]), u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]])) } else { (1, 1) };
        let w = f64::from(width_pt).min(sw * 0.8);
        let h = w * f64::from(ih) / f64::from(iw.max(1));
        let rect = slate_deck::edit::Rect { x: (sw - w) / 2.0, y: (sh - h) / 2.0, w, h };
        let r = self.doc.insert_picture(&bytes, "barcode.png", Some(rect));
        self.act(r);
    }

    fn picked_image(&mut self, path: &str) {
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "picture.png".into());
        match std::fs::read(path) {
            Ok(bytes) => {
                let r = self.doc.insert_picture(&bytes, &name, None);
                self.act(r);
            }
            Err(e) => self.notices.push(format!("Couldn't read the picture: {e}")),
        }
    }

    fn can_save_in_place(&self) -> bool {
        matches!(SaveFormat::from_file_name(&self.name), Some(f) if f.is_full_save())
            && matches!(self.doc.source_format(), slate_deck::SourceFormat::Pptx | slate_deck::SourceFormat::Native)
    }
    fn default_ext(&self) -> &'static str {
        "pptx"
    }
    fn write_to(&mut self, path: &Path) -> Result<Vec<String>, String> {
        self.flush_notes();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let format = SaveFormat::from_file_name(&name).unwrap_or(SaveFormat::Pptx);
        let out = self.doc.export_bytes(format)?;
        std::fs::write(path, &out.bytes).map_err(|e| e.to_string())?;
        if format == SaveFormat::OutlineText {
            return Ok(Vec::new());
        }
        Ok(self.doc.fidelity_report().issues.iter().map(|i| i.kind.clone()).collect())
    }
    fn mark_saved(&mut self, name: &str, _path: &Path) {
        self.name = name.to_owned();
        let r = self.doc.mark_saved();
        self.act(r);
    }
    fn export_pdf(&mut self, path: &Path) -> Result<(), String> {
        self.flush_notes();
        let opts = self.pdf_options();
        self.pdf_layout = PdfLayout::Slides;
        self.doc.export_pdf(path, &opts)
    }
    fn info_line(&mut self) -> String {
        let n = self.doc.slide_count();
        let (w, h) = self.doc.slide_size();
        let ratio = if (w / h - 16.0 / 9.0).abs() < 0.02 {
            "16:9".to_owned()
        } else if (w / h - 4.0 / 3.0).abs() < 0.02 {
            "4:3".to_owned()
        } else {
            format!("{:.0} × {:.0} pt", w, h)
        };
        let hidden = self.doc.slides().iter().filter(|s| s.hidden).count();
        let notes = self.doc.slides().iter().filter(|s| s.has_notes).count();
        let mut s = format!("{} · {} slide{} · {}", self.name, n, if n == 1 { "" } else { "s" }, ratio);
        if hidden > 0 {
            s += &format!(" · {hidden} hidden");
        }
        if notes > 0 {
            s += &format!(" · notes on {notes}");
        }
        s
    }
}
