//! Drawing slides on a background thread (the engine draws on the CPU; a big slide takes a
//! moment). The UI asks for (slide, kind) at a pixel size each frame with [`Renderer::want`];
//! finished pictures become egui textures. While a newer version is drawn, the old picture stays
//! on screen (stretched if the size changed), so nothing flickers while typing or dragging.

use std::collections::HashMap;
use std::sync::{Arc, mpsc};

use eframe::egui;
use slate_deck::render::{RenderOptions, render_slide};
use slate_deck::{DeckDoc, Presentation, RenderedSlide};

/// Which look a picture is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// As shown/printed (viewer, slideshow).
    Plain,
    /// Editor look: "Click to add title" prompts and dashed empty placeholders.
    Edit,
    /// A small thumbnail for the slide strip.
    Thumb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub slide: usize,
    pub kind: Kind,
}

struct Job {
    key: Key,
    /// Pixels per slide point.
    scale: f64,
    px: u32,
    generation: u64,
    /// 0 = on screen now, 1 = soon (thumbnails, the next slide of a show).
    priority: u8,
    doc: Arc<Presentation>,
}

struct Done {
    key: Key,
    px: u32,
    generation: u64,
    img: RenderedSlide,
}

struct Tex {
    tex: egui::TextureHandle,
    px: u32,
    generation: u64,
}

/// Biggest picture side we ask for (phones' GPUs take at least 4096).
const MAX_PX: f32 = 4096.0;

#[derive(Default)]
pub struct Renderer {
    jobs: Option<mpsc::Sender<Job>>,
    done: Option<mpsc::Receiver<Done>>,
    textures: HashMap<Key, Tex>,
    in_flight: HashMap<Key, (u32, u64)>,
    /// Bumped whenever the presentation changes (a new snapshot).
    generation: u64,
    snapshot: Option<Arc<Presentation>>,
    slide_w: f32,
    /// Barcodes found on the drawn slides (viewer: tap one to read it).
    pub scanner: crate::barcode_scan::Scanner,
}

impl Renderer {
    /// Start the drawing thread if needed, and notice document changes.
    pub fn sync(&mut self, doc: &DeckDoc, ctx: &egui::Context) {
        if self.jobs.is_none() {
            self.start(ctx.clone());
        }
        let Ok(snap) = doc.snapshot() else { return };
        let changed = self.snapshot.as_ref().is_none_or(|s| !Arc::ptr_eq(s, &snap));
        if changed {
            self.generation += 1;
            self.snapshot = Some(snap);
        }
        self.slide_w = doc.slide_size().0.max(1.0) as f32;
    }

    fn start(&mut self, ctx: egui::Context) {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel::<Done>();
        let spawned = std::thread::Builder::new().name("slide-render".into()).spawn(move || {
            let mut queue: Vec<Job> = Vec::new();
            loop {
                if queue.is_empty() {
                    match job_rx.recv() {
                        Ok(j) => queue.push(j),
                        Err(_) => break,
                    }
                }
                while let Ok(j) = job_rx.try_recv() {
                    match queue.iter_mut().find(|q| q.key == j.key) {
                        Some(q) if j.generation >= q.generation => *q = j,
                        Some(_) => {}
                        None => queue.push(j),
                    }
                }
                // Newest version first (what was just edited), then what's on screen, then oldest.
                let newest = queue.iter().map(|q| q.generation).max().unwrap_or(0);
                let idx = queue
                    .iter()
                    .enumerate()
                    .min_by_key(|(i, q)| (q.generation != newest, q.priority, *i))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let job = queue.remove(idx);
                let opts = RenderOptions { scale: job.scale, edit: job.key.kind == Kind::Edit, threads: 0 };
                let img = render_slide(&job.doc, job.key.slide, &opts);
                if done_tx.send(Done { key: job.key, px: job.px, generation: job.generation, img }).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        match spawned {
            Ok(_) => {
                self.jobs = Some(jobs);
                self.done = Some(done);
            }
            Err(e) => log::error!("slide-render thread: {e}"),
        }
    }

    /// Ask for slide `key` drawn at `scale` screen pixels per slide point. `now` = on screen
    /// (drawn first); otherwise it's a prefetch.
    pub fn want(&mut self, key: Key, scale: f32, now: bool) {
        let scale = scale.min(MAX_PX / self.slide_w.max(1.0)).max(0.01);
        let px = (self.slide_w * scale).round().max(1.0) as u32;
        let off = |have: u32| (have as f32 - px as f32).abs() / px as f32;
        let generation = self.generation;
        let stale = self.textures.get(&key).is_none_or(|t| off(t.px) > 0.2 || t.generation < generation);
        let asked = self.in_flight.get(&key).is_some_and(|(p, g)| off(*p) <= 0.2 && *g == generation);
        if !stale || asked {
            return;
        }
        let (Some(tx), Some(doc)) = (&self.jobs, &self.snapshot) else { return };
        let job = Job { key, scale: f64::from(scale), px, generation, priority: if now { 0 } else { 1 }, doc: Arc::clone(doc) };
        if tx.send(job).is_ok() {
            self.in_flight.insert(key, (px, generation));
        }
    }

    pub fn texture(&self, key: Key) -> Option<egui::TextureId> {
        self.textures.get(&key).map(|t| t.tex.id())
    }

    /// Free a picture that's far off screen.
    pub fn drop_texture(&mut self, key: Key) {
        self.textures.remove(&key);
    }

    /// Forget pictures of slides from `n` on (after slides were deleted).
    pub fn forget_beyond(&mut self, n: usize) {
        self.textures.retain(|k, _| k.slide < n);
        self.in_flight.retain(|k, _| k.slide < n);
    }

    /// Long-press at slide point `pt`: draw the slide at 4 px per point on the scan thread and
    /// read the barcode there.
    pub fn read_spot(&mut self, ctx: &egui::Context, slide: usize, pt: [f32; 2]) {
        let Some(doc) = self.snapshot.clone() else { return };
        let half = 110.0;
        let rect = [pt[0] - half, pt[1] - half, pt[0] + half, pt[1] + half];
        self.scanner.submit_spot_render(
            ctx,
            slide,
            pt,
            Box::new(move || {
                let scale = 4.0;
                let img = render_slide(&doc, slide, &RenderOptions { scale, edit: false, threads: 0 });
                if img.is_empty() {
                    return None;
                }
                crate::barcode_scan::Scanner::crop(img.width, img.height, &img.pixels, scale as f32, rect)
            }),
        );
        crate::links::note("Looking for a barcode…");
    }

    /// Turn finished pictures into textures.
    pub fn collect(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.done else { return };
        while let Ok(d) = rx.try_recv() {
            if self.in_flight.get(&d.key).is_some_and(|(p, g)| *p == d.px && *g == d.generation) {
                self.in_flight.remove(&d.key);
            }
            if d.img.is_empty() || self.textures.get(&d.key).is_some_and(|t| t.generation > d.generation) {
                continue;
            }
            if d.key.kind == Kind::Plain {
                let unit = self.slide_w / d.img.width.max(1) as f32;
                self.scanner.submit_page(ctx, d.key.slide, d.generation, unit, d.img.width, d.img.height, &d.img.pixels);
            }
            let color = egui::ColorImage::from_rgba_premultiplied([d.img.width as usize, d.img.height as usize], &d.img.pixels);
            let name = format!("deck-{:?}-{}", d.key.kind, d.key.slide);
            let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
            self.textures.insert(d.key, Tex { tex, px: d.px, generation: d.generation });
        }
    }
}
