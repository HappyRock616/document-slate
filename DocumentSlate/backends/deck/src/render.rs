//! Drawing slides to pixels (for the slide strip, the viewer and the slideshow).
//!
//! The engine draws on the CPU with `vello_cpu` (no GPU needed), so it works the same on the
//! phone, the emulator and in unit tests. The app turns the pixels into an egui texture.
//!
//! Drawing a big slide takes a moment, so the app should do it on a background thread:
//! take a [`crate::DeckDoc::snapshot`] (a cheap shared copy) and call [`render_slide`] there.

use deckcraft_model::Presentation;

use crate::{DeckDoc, DeckResult};

/// A drawn slide: `width` x `height` pixels, 4 bytes each (red, green, blue, alpha).
///
/// The colours are *premultiplied* by alpha (that's what the renderer makes). Slides are
/// almost always fully opaque, where that makes no difference. For egui use
/// `egui::ColorImage::from_rgba_premultiplied([w, h], &pixels)`.
#[derive(Clone, Debug, Default)]
pub struct RenderedSlide {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RenderedSlide {
    /// The pixels with ordinary ("straight", not premultiplied) alpha.
    pub fn to_straight(&self) -> Vec<u8> {
        deckcraft_render::Image { width: self.width, height: self.height, pixels: self.pixels.clone() }.to_straight()
    }
    /// The slide as a PNG file (for "export slide as image" or debugging).
    pub fn to_png(&self) -> Vec<u8> {
        deckcraft_render::Image { width: self.width, height: self.height, pixels: self.pixels.clone() }.to_png()
    }
    /// True when nothing was drawn (bad slide index or a failed render).
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.is_empty()
    }
}

/// How to draw a slide.
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    /// Pixels per point. A 16:9 slide is 960 x 540 pt, so scale 1.0 = 960 x 540 pixels,
    /// 2.0 = 1920 x 1080. Use [`fit_scale`] to fill a box.
    pub scale: f64,
    /// Editor look: shows the grey "Click to add title" prompts and dashed outlines of empty
    /// placeholders. `false` for the viewer, slideshow, thumbnails and printing.
    pub edit: bool,
    /// Extra worker threads for the renderer (0 = draw on the calling thread). Effects that need
    /// filters always draw on one thread.
    pub threads: u16,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { scale: 1.0, edit: false, threads: 0 }
    }
}

/// The scale that makes a slide of `slide_size` (points) fit inside `w` x `h` pixels.
pub fn fit_scale(slide_size: (f64, f64), w: u32, h: u32) -> f64 {
    let (sw, sh) = (slide_size.0.max(1.0), slide_size.1.max(1.0));
    let s = (f64::from(w) / sw).min(f64::from(h) / sh);
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// Draw slide `index` of a presentation snapshot. Safe to call from any thread. A slide that
/// doesn't exist gives an empty [`RenderedSlide`]. The engine keeps the scale within its limits
/// (at most 16 000 pixels on the long side).
pub fn render_slide(doc: &Presentation, index: usize, opts: &RenderOptions) -> RenderedSlide {
    if index >= doc.slides.len() {
        return RenderedSlide::default();
    }
    let scale = deckcraft_engine::cmd::file::image_scale(doc, opts.scale);
    let ro = deckcraft_render::RenderOpts { scale, edit: opts.edit, threads: opts.threads, ..Default::default() };
    let img = deckcraft_render::render_slide(doc, index, &ro);
    RenderedSlide { width: img.width, height: img.height, pixels: img.pixels }
}

impl DeckDoc {
    /// Draw slide `index` at `scale` pixels per point (viewer look: no placeholder prompts).
    pub fn render_slide(&self, index: usize, scale: f64) -> DeckResult<RenderedSlide> {
        self.render_slide_with(index, &RenderOptions { scale, ..Default::default() })
    }

    /// Draw slide `index` with full options (e.g. the editor look).
    pub fn render_slide_with(&self, index: usize, opts: &RenderOptions) -> DeckResult<RenderedSlide> {
        let doc = self.snapshot()?;
        if index >= doc.slides.len() {
            return Err(format!("there is no slide {} (the presentation has {})", index + 1, doc.slides.len()));
        }
        Ok(render_slide(&doc, index, opts))
    }

    /// Draw slide `index` as large as fits in `w` x `h` pixels (thumbnails, full-screen show).
    pub fn render_slide_fit(&self, index: usize, w: u32, h: u32) -> DeckResult<RenderedSlide> {
        let scale = fit_scale(self.slide_size(), w, h);
        self.render_slide(index, scale)
    }
}
