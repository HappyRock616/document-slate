//! The page column: background drawing, layout, scroll / pinch zoom, search highlights, and the
//! touch side of the edit tools (taps, finger drags, moving a selected item).

use std::sync::mpsc;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke};
use slate_pdf::{FieldKind, LinkTarget, MarkupKind, PageSnapshot, RenderedImage, SearchHit, ViewPoint, ViewRect};

use super::tools::{Entry, EntryKind, Sel, Tab, TouchTool};
use super::PdfScreen;
use crate::doc_screen::ScreenCtx;

/// Gap between pages, in screen points.
const PAGE_GAP: f32 = 14.0;
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 5.0;
/// Longest side of a page picture, pixels (keeps textures inside every GPU's limit).
const MAX_PX: f32 = 4096.0;
/// Highlight at most this many search matches.
pub(super) const MAX_HITS: usize = 500;
/// Thumbnail width, pixels.
pub(super) const THUMB_PX: f32 = 220.0;
/// Night mode: chalk text on the slate page (the app's dark palette).
pub(super) const NIGHT_TEXT: [u8; 3] = [0xD9, 0xDD, 0xE0];
pub(super) const NIGHT_PAGE: [u8; 3] = [0x2B, 0x2F, 0x33];

// ------------------------------------------------------------------ background worker

enum Job {
    /// A new version of the document (after an edit, or night mode switched): later pages are
    /// drawn from it. Tagged with the view generation.
    Snapshot(u64, Box<PageSnapshot>),
    Render { page: usize, scale: f32, px: u32, thumb: bool },
}

struct Done {
    page: usize,
    px: u32,
    generation: u64,
    thumb: bool,
    img: Result<RenderedImage, String>,
}

struct Worker {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
}

impl Worker {
    /// Start the drawing thread; it ends when the screen is dropped (the job channel closes).
    fn start(ctx: egui::Context) -> Option<Self> {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel::<Done>();
        let spawned = std::thread::Builder::new().name("pdf-render".into()).spawn(move || {
            let mut snap: Option<(u64, Box<PageSnapshot>)> = None;
            // Pending requests: at most one per (page, thumbnail), the newest.
            let mut queue: Vec<(usize, f32, u32, bool)> = Vec::new();
            let take = |j: Job, snap: &mut Option<(u64, Box<PageSnapshot>)>, queue: &mut Vec<(usize, f32, u32, bool)>| match j {
                Job::Snapshot(g, s) => *snap = Some((g, s)),
                Job::Render { page, scale, px, thumb } => {
                    queue.retain(|q| !(q.0 == page && q.3 == thumb));
                    queue.push((page, scale, px, thumb));
                }
            };
            loop {
                if queue.is_empty() {
                    match job_rx.recv() {
                        Ok(j) => take(j, &mut snap, &mut queue),
                        Err(_) => break,
                    }
                }
                while let Ok(j) = job_rx.try_recv() {
                    take(j, &mut snap, &mut queue);
                }
                let Some((generation, s)) = snap.as_mut() else { continue };
                if queue.is_empty() {
                    continue;
                }
                // Full pages before thumbnails; newest request first (after a fast fling, the page now
                // on screen comes before the ones scrolled past).
                let idx = queue.iter().rposition(|q| !q.3).unwrap_or(queue.len() - 1);
                let (page, scale, px, thumb) = queue.remove(idx);
                if page >= s.page_count() {
                    continue;
                }
                let img = s.render_page(page, scale).map_err(|e| e.to_string());
                if done_tx.send(Done { page, px, generation: *generation, thumb, img }).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        match spawned {
            Ok(_) => Some(Self { jobs, done }),
            Err(e) => {
                log::error!("pdf-render thread: {e}");
                None
            }
        }
    }
}

pub(super) struct PageTex {
    pub tex: egui::TextureHandle,
    px: u32,
    generation: u64,
}

/// A finger drag in progress with a drawing tool (page view space, points).
pub(super) struct Drag {
    pub page: usize,
    pub points: Vec<ViewPoint>,
    /// Highlight / underline / strike: the text rectangles the drag covers so far.
    pub preview: Vec<ViewRect>,
}

/// Viewer state.
pub(super) struct View {
    worker: Option<Worker>,
    /// Barcodes found on the drawn pages (tap one to read it).
    pub scanner: crate::barcode_scan::Scanner,
    textures: Vec<Option<PageTex>>,
    in_flight: Vec<Option<(u32, u64)>>,
    pub thumbs: Vec<Option<PageTex>>,
    thumb_flight: Vec<Option<u64>>,
    /// Bumped when the document or night mode changes; pictures from older ones are redrawn.
    generation: u64,
    sent: Option<(u64, bool)>,
    pub clear_textures: bool,
    pub zoom: f32,
    pub night: bool,
    last_offset: f32,
    last_view_h: f32,
    set_offset: Option<f32>,
    /// Scroll so that (page, y in points) is at the top.
    pub jump_to: Option<(usize, f32)>,
    pub current_page: usize,
    pub scrolled: bool,
    pub tapped: bool,
    pub drag: Option<Drag>,
    /// Moving / resizing the selected item: offset (dx, dy) and size change (dw, dh), points.
    obj_drag: Option<[f32; 4]>,
    // search
    pub search_open: bool,
    pub query: String,
    pub searched: String,
    query_changed: f64,
    pub hits: Vec<SearchHit>,
    pub current: usize,
    pub focus_search: bool,
    jump: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            worker: None,
            scanner: crate::barcode_scan::Scanner::default(),
            textures: Vec::new(),
            in_flight: Vec::new(),
            thumbs: Vec::new(),
            thumb_flight: Vec::new(),
            generation: 1,
            sent: None,
            clear_textures: false,
            zoom: 1.0,
            night: false,
            last_offset: 0.0,
            last_view_h: 0.0,
            set_offset: None,
            jump_to: None,
            current_page: 0,
            scrolled: false,
            tapped: false,
            drag: None,
            obj_drag: None,
            search_open: false,
            query: String::new(),
            searched: String::new(),
            query_changed: 0.0,
            hits: Vec::new(),
            current: 0,
            focus_search: false,
            jump: false,
        }
    }
}

impl View {
    pub fn reset_pages(&mut self) {
        self.textures.clear();
        self.in_flight.clear();
        self.thumbs.clear();
        self.thumb_flight.clear();
        self.sent = None;
    }

    /// The page list changed (count, sizes or order): old pictures may belong to other pages.
    pub fn pages_changed(&mut self, n: usize) {
        self.scanner.clear();
        self.textures = (0..n).map(|_| None).collect();
        self.in_flight = vec![None; n];
        self.thumbs = (0..n).map(|_| None).collect();
        self.thumb_flight = vec![None; n];
    }
}

fn to_rect(r: &ViewRect) -> Rect {
    Rect::from_min_max(egui::pos2(r[0].min(r[2]), r[1].min(r[3])), egui::pos2(r[0].max(r[2]), r[1].max(r[3])))
}

pub(super) fn contains(r: &ViewRect, p: ViewPoint, slack: f32) -> bool {
    to_rect(r).expand(slack).contains(egui::pos2(p[0], p[1]))
}

/// Today's date for Fill & Sign ("17 Nov 2026" — unambiguous in every country).
pub(super) fn date_text(y: i64, m: u32, d: u32) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{d} {} {y}", MONTHS.get(m.saturating_sub(1) as usize).copied().unwrap_or("?"))
}

impl PdfScreen {
    /// Send the worker a fresh snapshot when the document or night mode changed.
    fn sync_snapshot(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        let key = (doc.generation(), self.v.night);
        if self.v.sent == Some(key) {
            return;
        }
        self.v.generation += 1;
        let mut snap = doc.snapshot();
        if self.v.night {
            snap.set_reading_colors(Some((NIGHT_TEXT, NIGHT_PAGE)));
        }
        if let Some(w) = &self.v.worker {
            if w.jobs.send(Job::Snapshot(self.v.generation, Box::new(snap))).is_ok() {
                self.v.sent = Some(key);
            }
        }
        let n = self.page_count();
        self.v.in_flight = vec![None; n];
        self.v.thumb_flight = vec![None; n];
        if std::mem::take(&mut self.v.clear_textures) {
            self.v.pages_changed(n);
        }
    }

    fn collect_finished(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.v.worker else { return };
        let mut failed = None;
        while let Ok(d) = w.done.try_recv() {
            let (slots, flights) = if d.thumb { (&mut self.v.thumbs, None) } else { (&mut self.v.textures, Some(&mut self.v.in_flight)) };
            if let Some(f) = flights {
                if f.get(d.page).copied().flatten().is_some_and(|(px, g)| px == d.px && g == d.generation) {
                    f[d.page] = None;
                }
            } else if let Some(f) = self.v.thumb_flight.get_mut(d.page) {
                *f = None;
            }
            let Some(slot) = slots.get_mut(d.page) else { continue };
            if slot.as_ref().is_some_and(|t| t.generation > d.generation) {
                continue;
            }
            match d.img {
                Ok(img) => {
                    if !d.thumb {
                        if let Some(p) = self.cache.pages.get(d.page) {
                            let unit = p.width / img.width.max(1) as f32;
                            self.v.scanner.submit_page(ctx, d.page, d.generation, unit, img.width, img.height, &img.rgba);
                        }
                    }
                    let color = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.rgba);
                    let name = format!("pdf-{}-{}", if d.thumb { "thumb" } else { "page" }, d.page);
                    let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                    *slot = Some(PageTex { tex, px: d.px, generation: d.generation });
                }
                Err(e) => failed = Some(e),
            }
        }
        if let Some(e) = failed {
            log::warn!("page render: {e}");
        }
    }

    /// Ask for a page picture if what's shown is missing, blurry or out of date.
    fn request_page(&mut self, i: usize, page_w_pt: f32, want_px: f32) {
        let want = want_px.round().max(1.0) as u32;
        let off_by = |px: u32| (px as f32 - want as f32).abs() / want as f32;
        let g = self.v.generation;
        let stale = self.v.textures.get(i).and_then(|t| t.as_ref()).is_none_or(|t| off_by(t.px) > 0.25 || t.generation < g);
        let asked = self.v.in_flight.get(i).copied().flatten().is_some_and(|(px, ag)| off_by(px) <= 0.25 && ag == g);
        if !stale || asked {
            return;
        }
        if let Some(w) = &self.v.worker {
            if w.jobs.send(Job::Render { page: i, scale: want as f32 / page_w_pt.max(1.0), px: want, thumb: false }).is_ok() {
                if let Some(f) = self.v.in_flight.get_mut(i) {
                    *f = Some((want, g));
                }
            }
        }
    }

    /// Ask for a thumbnail (Page thumbnails sheet).
    pub(super) fn request_thumb(&mut self, i: usize) {
        let g = self.v.generation;
        let fresh = self.v.thumbs.get(i).and_then(|t| t.as_ref()).is_some_and(|t| t.generation >= g);
        let asked = self.v.thumb_flight.get(i).copied().flatten() == Some(g);
        if fresh || asked {
            return;
        }
        let Some(p) = self.cache.pages.get(i) else { return };
        let scale = THUMB_PX / p.width.max(p.height).max(1.0);
        if let Some(w) = &self.v.worker {
            if w.jobs.send(Job::Render { page: i, scale, px: THUMB_PX as u32, thumb: true }).is_ok() {
                if let Some(f) = self.v.thumb_flight.get_mut(i) {
                    *f = Some(g);
                }
            }
        }
    }

    // ------------------------------------------------------------------ search

    pub(super) fn run_search(&mut self) {
        self.v.searched = self.v.query.clone();
        self.v.hits.clear();
        self.v.current = 0;
        let q = self.v.query.trim().to_owned();
        let Some(doc) = self.doc.as_mut() else { return };
        if q.is_empty() {
            return;
        }
        self.v.hits = doc.search(&q, false, false, MAX_HITS);
        // Start at the first match on or after the page being read.
        self.v.current = self.v.hits.iter().position(|h| h.page >= self.v.current_page).unwrap_or(0);
        self.v.jump = !self.v.hits.is_empty();
    }

    fn step_match(&mut self, delta: isize) {
        let n = self.v.hits.len();
        if n == 0 {
            return;
        }
        self.v.current = (self.v.current as isize + delta).rem_euclid(n as isize) as usize;
        self.v.jump = true;
    }

    /// The search row (drawn by the shell under the viewer bar).
    pub(super) fn search_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(44.0);
            let edit = egui::TextEdit::singleline(&mut self.v.query).hint_text("Find in PDF").desired_width((ui.available_width() - 200.0).max(110.0));
            let resp = ui.add(edit);
            if self.v.focus_search {
                resp.request_focus();
                self.v.focus_search = false;
            }
            if resp.changed() {
                self.v.query_changed = ui.input(|i| i.time);
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if self.v.query != self.v.searched {
                    self.run_search();
                } else {
                    self.step_match(1);
                }
                resp.request_focus();
            }
            let big = |t: &str| egui::Button::new(t).min_size(egui::vec2(48.0, 36.0));
            if ui.add(big("‹")).on_hover_text("Previous").clicked() {
                self.step_match(-1);
            }
            if ui.add(big("›")).on_hover_text("Next").clicked() {
                self.step_match(1);
            }
            let count = match (self.v.hits.len(), self.v.query.trim().is_empty(), self.v.query == self.v.searched) {
                (_, true, _) => String::new(),
                (_, false, false) => "…".into(),
                (0, false, true) => "No matches".into(),
                (n, false, true) => format!("{} of {}{}", self.v.current + 1, n, if n >= MAX_HITS { "+" } else { "" }),
            };
            ui.label(count);
        });
    }

    // ------------------------------------------------------------------ the page column

    pub(super) fn show_pages(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) {
        let ctx = ui.ctx().clone();
        if self.v.worker.is_none() {
            self.v.worker = Worker::start(ctx.clone());
        }
        self.sync_snapshot();
        self.collect_finished(&ctx);
        match self.v.scanner.poll() {
            Some(crate::barcode_scan::SpotResult::Found(h)) => crate::barcode_scan::show(&h.found),
            Some(crate::barcode_scan::SpotResult::Nothing) => self.say("No barcode found there"),
            None => {}
        }
        let now = ctx.input(|i| i.time);
        // Search as you type, a moment after the typing stops (long PDFs take a while).
        if self.v.search_open && self.v.query != self.v.searched {
            if now - self.v.query_changed > 0.4 {
                self.run_search();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
        }

        // Pinch zoom (Ctrl+wheel on desktop), keeping the middle of the screen in place.
        let pinch = ctx.input(|i| i.zoom_delta());
        if pinch != 1.0 && self.v.obj_drag.is_none() {
            let old = self.v.zoom;
            self.v.zoom = (old * pinch).clamp(MIN_ZOOM, MAX_ZOOM);
            let ratio = self.v.zoom / old;
            let mid = self.v.last_offset + self.v.last_view_h / 2.0;
            self.v.set_offset = Some((mid * ratio - self.v.last_view_h / 2.0).max(0.0));
        }

        let ppp = ctx.pixels_per_point();
        let view_w = ui.available_width();
        let view_h = ui.available_height();
        let widest = self.cache.pages.iter().map(|p| p.width).fold(1.0_f32, f32::max);
        let base = ((view_w - 2.0 * PAGE_GAP) / widest).max(0.05);
        let scale = base * self.v.zoom;
        let mut tops = Vec::with_capacity(self.page_count());
        let mut y = PAGE_GAP;
        for p in &self.cache.pages {
            tops.push(y);
            y += p.height * scale + PAGE_GAP;
        }
        let total_h = y;
        let total_w = (widest * scale + 2.0 * PAGE_GAP).max(view_w);

        let mut area = egui::ScrollArea::both().auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
        let drawing = self.editing && self.e.tool.is_drag();
        if drawing || self.v.obj_drag.is_some() {
            area = area.scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
        }
        if let Some((page, y)) = self.v.jump_to.take() {
            let target = tops.get(page).copied().unwrap_or(0.0) + y * scale - 8.0;
            area = area.vertical_scroll_offset(target.max(0.0));
        } else if std::mem::take(&mut self.v.jump) {
            if let Some((page, r)) = self.v.hits.get(self.v.current).and_then(|h| h.rects.first().map(|r| (h.page, *r))) {
                let target = tops.get(page).copied().unwrap_or(0.0) + r[1] * scale - view_h / 3.0;
                area = area.vertical_scroll_offset(target.max(0.0));
            }
        } else if let Some(off) = self.v.set_offset.take() {
            area = area.vertical_scroll_offset(off);
        }

        let accent = cx.accent;
        let paper = if self.v.night { Color32::from_rgb(NIGHT_PAGE[0], NIGHT_PAGE[1], NIGHT_PAGE[2]) } else { Color32::WHITE };
        let out = area.show_viewport(ui, |ui, viewport| {
            let sense = if drawing { Sense::click_and_drag() } else { Sense::click() };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(total_w, total_h), sense);
            let mut page_rects: Vec<Rect> = Vec::with_capacity(tops.len());
            for i in 0..tops.len() {
                let p = &self.cache.pages[i];
                let (pw, ph) = (p.width * scale, p.height * scale);
                let local = Rect::from_min_size(egui::pos2((total_w - pw) / 2.0, tops[i]), egui::vec2(pw, ph));
                let screen = local.translate(rect.min.to_vec2());
                page_rects.push(screen);
                if !local.intersects(viewport.expand(viewport.height())) {
                    if let Some(t) = self.v.textures.get_mut(i) {
                        *t = None;
                    }
                    continue;
                }
                if local.intersects(viewport.expand(viewport.height() * 0.5)) {
                    // Pixels for this page at the current zoom (capped for huge pages / zooms).
                    let mut want = pw * ppp;
                    let longest = pw.max(ph) * ppp;
                    if longest > MAX_PX {
                        want *= MAX_PX / longest;
                    }
                    let w_pt = p.width;
                    self.request_page(i, w_pt, want);
                }
                let painter = ui.painter();
                painter.rect_filled(screen.translate(egui::vec2(2.0, 3.0)), 2.0, Color32::from_black_alpha(60));
                painter.rect_filled(screen, 0.0, paper);
                if let Some(t) = self.v.textures.get(i).and_then(|t| t.as_ref()) {
                    painter.image(t.tex.id(), screen, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                self.page_overlays(ui, i, screen, scale, accent);
            }

            // ---- touch
            let pos = resp.interact_pointer_pos();
            let on_page = |p: Pos2| -> Option<(usize, ViewPoint)> {
                page_rects.iter().position(|r| r.contains(p)).map(|i| {
                    let l = (p - page_rects[i].min) / scale;
                    (i, [l.x, l.y])
                })
            };
            if drawing {
                if resp.drag_started() {
                    if let Some((page, pt)) = pos.and_then(on_page) {
                        self.v.drag = Some(Drag { page, points: vec![pt], preview: Vec::new() });
                    }
                }
                if resp.dragged() {
                    if let (Some(p), Some(d)) = (pos, self.v.drag.as_mut()) {
                        let r = page_rects[d.page];
                        let l = (p.clamp(r.min, r.max) - r.min) / scale;
                        let pt = [l.x, l.y];
                        if d.points.last().is_none_or(|q| (q[0] - pt[0]).abs() + (q[1] - pt[1]).abs() > 0.6) {
                            d.points.push(pt);
                            if let TouchTool::Markup(_) = self.e.tool {
                                let (page, from) = (d.page, d.points[0]);
                                let span = self.doc.as_mut().and_then(|doc| doc.text_span(page, from, pt, 14.0));
                                if let Some(d) = self.v.drag.as_mut() {
                                    d.preview = span.map(|s| s.0).unwrap_or_default();
                                }
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    if let Some(d) = self.v.drag.take() {
                        self.finish_drag(d);
                    }
                }
            }
            if resp.clicked() {
                match pos.and_then(on_page) {
                    Some((page, pt)) => {
                        self.v.current_page = page;
                        if self.editing {
                            self.on_tap(page, pt);
                        } else if !self.follow_link(page, pt) {
                            match self.v.scanner.hit_at(page, pt, 6.0) {
                                Some(h) => crate::barcode_scan::show(&h.found),
                                None => self.v.tapped = true,
                            }
                        }
                    }
                    None => {
                        if self.editing {
                            self.e.selected = None;
                        } else {
                            self.v.tapped = true;
                        }
                    }
                }
            }
            // Viewer: long-press reads a barcode too small to find at screen resolution.
            if !self.editing && resp.long_touched() {
                if let Some((page, pt)) = pos.and_then(on_page) {
                    self.read_spot(ui.ctx(), page, pt);
                }
            }
            if !self.editing {
                for (i, r) in page_rects.iter().enumerate() {
                    if r.intersects(viewport.translate(rect.min.to_vec2())) {
                        self.v.scanner.draw_badges(ui.painter(), i, *r, scale, accent);
                    }
                }
            }
            if self.editing && self.e.selected.is_some() {
                self.selection_overlay(ui, &page_rects, scale, accent);
            }
            viewport
        });
        self.v.scrolled = (out.inner.min.y - self.v.last_offset).abs() > 0.5 && self.v.last_view_h > 0.0;
        if self.v.scrolled || !self.editing {
            let mid = out.inner.center().y;
            self.v.current_page = tops.iter().rposition(|t| *t <= mid).unwrap_or(0);
        }
        self.v.last_offset = out.inner.min.y;
        self.v.last_view_h = out.inner.height();
    }

    /// Search highlights, form-field boxes, text-line boxes, links and the live drag preview.
    fn page_overlays(&mut self, ui: &egui::Ui, i: usize, screen: Rect, scale: f32, accent: Color32) {
        let to_screen = |r: &ViewRect| {
            let r = to_rect(r);
            Rect::from_min_size(screen.min + r.min.to_vec2() * scale, r.size() * scale)
        };
        let painter = ui.painter_at(screen.expand(2.0));
        for (m, hit) in self.v.hits.iter().enumerate() {
            if hit.page != i {
                continue;
            }
            let c = if m == self.v.current { Color32::from_rgba_unmultiplied(255, 140, 0, 110) } else { Color32::from_rgba_unmultiplied(255, 220, 0, 90) };
            for r in &hit.rects {
                painter.rect_filled(to_screen(r), 1.0, c);
            }
        }
        if !self.editing {
            return;
        }
        let thin = Stroke::new(1.0, accent.gamma_multiply(0.7));
        match self.e.tab {
            Tab::FillSign if self.e.tool == TouchTool::None => {
                for f in &self.cache.fields {
                    if f.read_only {
                        continue;
                    }
                    for w in f.widgets.iter().filter(|w| w.page == i) {
                        let r = to_screen(&w.rect);
                        painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(80, 140, 255, 40));
                        painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_rgb(80, 140, 255)), egui::StrokeKind::Middle);
                    }
                }
            }
            Tab::Edit if self.e.tool == TouchTool::None => {
                for a in self.cache.added.iter().filter(|a| a.page == i) {
                    painter.rect_stroke(to_screen(&a.rect), 1.0, thin, egui::StrokeKind::Middle);
                }
                for l in self.cache.links.iter().filter(|l| l.page == i) {
                    painter.rect_stroke(to_screen(&l.rect), 1.0, Stroke::new(1.0, Color32::from_rgb(80, 140, 255)), egui::StrokeKind::Middle);
                }
            }
            _ => {}
        }
        if self.e.tool == TouchTool::EditText {
            let lines: Vec<ViewRect> = self.lines(i).iter().map(|l| l.rect).collect();
            for r in &lines {
                let r = to_screen(r).expand(1.0);
                painter.rect_filled(r, 1.0, accent.gamma_multiply(0.10));
                painter.rect_stroke(r, 1.0, thin, egui::StrokeKind::Middle);
            }
        }
        // Where the entry row will put its text.
        if let Some(e) = self.e.entry.as_ref().filter(|e| e.page == i) {
            let r = to_screen(&e.rect);
            painter.rect_stroke(r.expand(2.0), 2.0, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
        }
        let Some(d) = self.v.drag.as_ref().filter(|d| d.page == i) else { return };
        let pts: Vec<Pos2> = d.points.iter().map(|p| screen.min + egui::vec2(p[0], p[1]) * scale).collect();
        let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) else { return };
        let color = self.e.colour32();
        match self.e.tool {
            TouchTool::Markup(kind) => {
                if d.preview.is_empty() {
                    painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
                }
                for r in &d.preview {
                    let r = to_screen(r);
                    match kind {
                        MarkupKind::Highlight => {
                            painter.rect_filled(r, 0.0, self.e.highlight32().gamma_multiply(0.45));
                        }
                        MarkupKind::Underline | MarkupKind::Squiggly => {
                            painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(2.0, color));
                        }
                        MarkupKind::Strikeout => {
                            painter.line_segment([r.left_center(), r.right_center()], Stroke::new(2.0, color));
                        }
                    }
                }
            }
            TouchTool::Ink => {
                painter.add(egui::Shape::line(pts, Stroke::new((self.e.width as f32 * scale).max(1.0), color)));
            }
            TouchTool::Shape(kind) => {
                let s = Stroke::new((self.e.width as f32 * scale).max(1.0), color);
                match kind {
                    slate_pdf::ShapeKind::Rectangle => {
                        painter.rect_stroke(Rect::from_two_pos(a, b), 0.0, s, egui::StrokeKind::Middle);
                    }
                    slate_pdf::ShapeKind::Circle => {
                        let r = Rect::from_two_pos(a, b);
                        let n = 48;
                        let ring: Vec<Pos2> = (0..=n)
                            .map(|k| {
                                let t = k as f32 / n as f32 * std::f32::consts::TAU;
                                r.center() + egui::vec2(t.cos() * r.width() / 2.0, t.sin() * r.height() / 2.0)
                            })
                            .collect();
                        painter.add(egui::Shape::line(ring, s));
                    }
                    slate_pdf::ShapeKind::Line => {
                        painter.line_segment([a, b], s);
                    }
                    slate_pdf::ShapeKind::Arrow => {
                        painter.arrow(a, b - a, s);
                    }
                }
            }
            TouchTool::CertBox => {
                let r = Rect::from_two_pos(a, b);
                painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(194, 66, 58, 36));
                painter.rect_stroke(r, 4.0, Stroke::new(1.5, Color32::from_rgb(194, 66, 58)), egui::StrokeKind::Middle);
            }
            TouchTool::Link => {
                let r = Rect::from_two_pos(a, b);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(80, 140, 255, 40));
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::from_rgb(80, 140, 255)), egui::StrokeKind::Middle);
            }
            _ => {}
        }
    }

    /// A drawing-tool drag ended: make the comment / shape / link.
    pub(super) fn finish_drag(&mut self, d: Drag) {
        let (Some(a), Some(b)) = (d.points.first().copied(), d.points.last().copied()) else { return };
        let far = (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) > 4.0;
        let page = d.page;
        let look = self.e.look();
        match self.e.tool {
            TouchTool::Markup(kind) => {
                let mut rects = d.preview;
                if rects.is_empty() {
                    rects = self.doc.as_mut().and_then(|doc| doc.text_span(page, a, b, 14.0)).map(|s| s.0).unwrap_or_default();
                }
                if rects.is_empty() && far {
                    // No text there (a scan?): mark the drawn area instead.
                    rects = vec![[a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]];
                }
                if rects.is_empty() {
                    self.say("Drag across the text to mark it");
                    return;
                }
                let look = if kind == MarkupKind::Highlight { self.e.highlight_look() } else { look };
                self.edit(|doc| doc.add_text_markup(page, kind, &rects, look, ""));
            }
            TouchTool::Ink => {
                let pts = d.points;
                if pts.len() >= 2 {
                    self.edit(|doc| doc.add_ink(page, &[pts], look));
                }
            }
            TouchTool::Shape(kind) => {
                if far {
                    self.edit(|doc| doc.add_shape(page, kind, a, b, look));
                }
            }
            TouchTool::Link => {
                if far {
                    let rect = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
                    self.e.entry = Some(Entry { kind: EntryKind::NewLink, page, rect, text: "https://".into(), focus: true });
                } else {
                    self.say("Drag a box over the area that should be a link");
                }
            }
            TouchTool::CertBox => {
                let rect = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
                if rect[2] - rect[0] >= 40.0 && rect[3] - rect[1] >= 16.0 {
                    self.cert_target(super::cert::Target::Rect(page, rect));
                } else if let Some(name) = self.signature_field_at(page, a) {
                    self.cert_target(super::cert::Target::Field(name));
                } else {
                    self.say("Drag a bigger box (about 2 inches wide) where the signature goes");
                }
            }
            _ => {}
        }
    }

    /// The name of an unsigned signature field under `pt`.
    fn signature_field_at(&self, page: usize, pt: ViewPoint) -> Option<String> {
        self.cache
            .fields
            .iter()
            .find(|f| f.kind == FieldKind::Signature && f.value.is_empty() && f.widgets.iter().any(|w| w.page == page && contains(&w.rect, pt, 2.0)))
            .map(|f| f.name.clone())
    }

    /// Long-press: draw ~3 inches around the finger at 6 px per point and look for a code there.
    fn read_spot(&mut self, ctx: &egui::Context, page: usize, pt: ViewPoint) {
        if self.v.scanner.spot_busy() {
            return;
        }
        let half = 110.0;
        let rect = [pt[0] - half, pt[1] - half, pt[0] + half, pt[1] + half];
        let scale = 6.0;
        let Some(doc) = self.doc.as_mut() else { return };
        match doc.render_region(page, rect, scale) {
            Ok(img) => {
                let origin = [rect[0].max(0.0), rect[1].max(0.0)];
                self.v.scanner.submit_spot(ctx, page, pt, origin, 1.0 / scale, img.width, img.height, img.rgba);
                self.say("Looking for a barcode…");
            }
            Err(e) => log::warn!("barcode spot: {e}"),
        }
    }

    /// Viewer: a tap on a link. Page links jump; web links are only shown (no browser here).
    fn follow_link(&mut self, page: usize, pt: ViewPoint) -> bool {
        let Some(link) = self.cache.links.iter().find(|l| l.page == page && contains(&l.rect, pt, 2.0)) else { return false };
        match link.target.clone() {
            LinkTarget::Page(p) => {
                self.v.jump_to = Some((p, 0.0));
                self.v.current_page = p;
            }
            LinkTarget::Uri(u) => crate::links::show_link(&u),
            LinkTarget::Other(what) => self.say(format!("This link opens {what} — not supported")),
        }
        true
    }

    /// Edit mode tap at a page point.
    pub(super) fn on_tap(&mut self, page: usize, pt: ViewPoint) {
        let at_box = |w: f32, h: f32| [pt[0], pt[1], pt[0] + w, pt[1] + h];
        let entry = |kind: EntryKind, rect: ViewRect, text: String| Some(Entry { kind, page, rect, text, focus: true });
        match self.e.tool {
            TouchTool::Note => self.e.entry = entry(EntryKind::Note, at_box(20.0, 20.0), String::new()),
            TouchTool::TextBox => self.e.entry = entry(EntryKind::TextBox, at_box(180.0, 40.0), String::new()),
            TouchTool::FillText => self.e.entry = entry(EntryKind::FillText, at_box(120.0, 16.0), String::new()),
            TouchTool::AddText => self.e.entry = entry(EntryKind::AddText, at_box(200.0, 18.0), String::new()),
            TouchTool::Mark(m) => {
                let size = self.e.mark_size;
                self.edit(|d| d.add_fill_mark(page, pt, size, m));
            }
            TouchTool::Date => {
                let Some(doc) = self.doc.as_ref() else { return };
                let (y, m, d) = doc.today();
                let text = date_text(y, m, d);
                let size = self.e.text_size;
                self.edit(|doc| doc.add_typed_text(page, pt, &text, size, [0.0, 0.0, 0.0]));
            }
            TouchTool::Sign => self.place_signature(page, pt),
            TouchTool::EditText => {
                let hit = self.lines(page).iter().find(|l| contains(&l.rect, pt, 2.0)).map(|l| (l.index, l.rect, l.text.clone(), l.reusable_font));
                match hit {
                    Some((index, rect, text, reusable)) => {
                        if !reusable {
                            self.say("This line's font can't be reused; changed words use a standard font");
                        }
                        self.e.entry = entry(EntryKind::EditLine(index), rect, text);
                    }
                    None => self.say("Tap a line of text (outlined) to change it"),
                }
            }
            TouchTool::None => self.select_at(page, pt),
            // Drag tools: a plain tap does nothing.
            _ => {}
        }
    }

    /// Edit mode, no tool: select what's under the finger (depends on the tab).
    pub(super) fn select_at(&mut self, page: usize, pt: ViewPoint) {
        self.e.selected = None;
        match self.e.tab {
            Tab::Annotate => {
                if let Some(c) = self.cache.comments.iter().rev().find(|c| c.page == page && !c.is_reply && !c.fill_sign && contains(&c.rect, pt, 4.0)) {
                    self.e.selected = Some(Sel::Comment(c.clone()));
                }
            }
            Tab::FillSign => {
                let field = self.cache.fields.iter().find_map(|f| {
                    let w = f.widgets.iter().find(|w| w.page == page && contains(&w.rect, pt, 2.0))?;
                    Some((f.clone(), w.clone()))
                });
                if let Some((f, w)) = field {
                    self.tap_field(f, w);
                } else if let Some(c) = self.cache.comments.iter().rev().find(|c| c.page == page && c.fill_sign && contains(&c.rect, pt, 4.0)) {
                    self.e.selected = Some(Sel::Comment(c.clone()));
                }
            }
            Tab::Edit => {
                if let Some(a) = self.cache.added.iter().rev().find(|a| a.page == page && contains(&a.rect, pt, 3.0)) {
                    self.e.selected = Some(Sel::Added(a.clone()));
                } else if let Some(l) = self.cache.links.iter().find(|l| l.page == page && contains(&l.rect, pt, 2.0)) {
                    self.e.selected = Some(Sel::Link(l.clone()));
                }
            }
            Tab::Pages | Tab::File => {}
        }
    }

    /// Fill & Sign: a tap on a form field.
    fn tap_field(&mut self, f: slate_pdf::FormField, w: slate_pdf::FieldWidget) {
        if f.read_only {
            self.say("This field can't be changed");
            return;
        }
        let name = f.name.clone();
        match f.kind {
            FieldKind::Text => {
                let text = f.value.first().cloned().unwrap_or_default();
                self.e.entry = Some(Entry { kind: EntryKind::Field(name), page: w.page, rect: w.rect, text, focus: true });
            }
            FieldKind::CheckBox => {
                let on = !w.is_on;
                self.edit(|d| d.set_field_checked(&name, on));
            }
            FieldKind::Radio => {
                let state = w.on_state.clone();
                self.edit(|d| d.set_field_radio(&name, state.as_deref()));
            }
            FieldKind::ComboBox | FieldKind::ListBox => self.sheet = super::sheets::Sheet::Choice(f),
            FieldKind::Signature => {
                if self.e.sig_choice.is_some() {
                    let c = egui::pos2((w.rect[0] + w.rect[2]) / 2.0, (w.rect[1] + w.rect[3]) / 2.0);
                    self.place_signature(w.page, [c.x, c.y]);
                } else {
                    self.say("Pick a signature in Sign first, then tap the field");
                }
            }
            FieldKind::Button => self.say("Buttons run scripts, which Page Slate never does"),
        }
    }

    /// The frame around the selected item; drag it to move, drag the corner knob to resize
    /// (added text and pictures).
    fn selection_overlay(&mut self, ui: &mut egui::Ui, page_rects: &[Rect], scale: f32, accent: Color32) {
        let Some(sel) = self.e.selected.clone() else { return };
        let (page, r, movable, resizable) = match &sel {
            Sel::Comment(c) => (c.page, c.rect, !c.locked, false),
            Sel::Added(a) => (a.page, a.rect, true, true),
            Sel::Link(l) => (l.page, l.rect, false, false),
        };
        let Some(pr) = page_rects.get(page) else { return };
        let off = self.v.obj_drag.unwrap_or([0.0; 4]);
        let local = to_rect(&r);
        let shown = Rect::from_min_size(pr.min + (local.min.to_vec2() + egui::vec2(off[0], off[1])) * scale, (local.size() + egui::vec2(off[2], off[3])).max(egui::vec2(4.0, 4.0)) * scale);
        let painter = ui.painter();
        painter.rect_stroke(shown.expand(3.0), 2.0, Stroke::new(2.0, accent), egui::StrokeKind::Middle);
        if movable {
            let body = ui.interact(shown.expand(3.0), ui.scope_id().with("pdf-obj-body"), Sense::drag());
            if body.dragged() {
                let d = body.drag_delta() / scale;
                let o = self.v.obj_drag.get_or_insert([0.0; 4]);
                o[0] += d.x;
                o[1] += d.y;
            }
            if body.drag_stopped() {
                self.commit_obj_drag(&sel);
            }
        }
        if resizable {
            let knob = shown.right_bottom() + egui::vec2(3.0, 3.0);
            ui.painter().circle_filled(knob, 10.0, accent);
            ui.painter().circle_stroke(knob, 10.0, Stroke::new(2.0, Color32::WHITE));
            let k = ui.interact(Rect::from_center_size(knob, egui::vec2(40.0, 40.0)), ui.scope_id().with("pdf-obj-knob"), Sense::drag());
            if k.dragged() {
                let d = k.drag_delta() / scale;
                let o = self.v.obj_drag.get_or_insert([0.0; 4]);
                let picture = matches!(&sel, Sel::Added(a) if a.text.is_none());
                if picture && local.width() > 0.0 {
                    // Pictures keep their proportions.
                    let aspect = local.height() / local.width();
                    o[2] += d.x;
                    o[3] = o[2] * aspect;
                } else {
                    o[2] += d.x;
                    o[3] += d.y;
                }
            }
            if k.drag_stopped() {
                self.commit_obj_drag(&sel);
            }
        }
    }

    fn commit_obj_drag(&mut self, sel: &Sel) {
        let Some(o) = self.v.obj_drag.take() else { return };
        if o.iter().all(|v| v.abs() < 0.5) {
            return;
        }
        match sel {
            Sel::Comment(c) => {
                let (p, i) = (c.page, c.index);
                if self.edit(|d| d.move_comment(p, i, o[0], o[1])) {
                    if let Some(n) = self.cache.comments.iter().find(|x| x.page == p && x.index == i) {
                        self.e.selected = Some(Sel::Comment(n.clone()));
                    }
                }
            }
            Sel::Added(a) => {
                let r = to_rect(&a.rect);
                let rect = [r.min.x + o[0], r.min.y + o[1], r.max.x + o[0] + o[2], r.max.y + o[1] + o[3]];
                let rect = [rect[0], rect[1], rect[2].max(rect[0] + 6.0), rect[3].max(rect[1] + 6.0)];
                let (p, i) = (a.page, a.index);
                if self.edit(|d| d.set_added_rect(p, i, rect)) {
                    // Keep it selected for the next nudge.
                    if let Some(n) = self.cache.added.iter().find(|x| x.page == p && x.index == i) {
                        self.e.selected = Some(Sel::Added(n.clone()));
                    }
                }
            }
            Sel::Link(_) => {}
        }
    }
}
