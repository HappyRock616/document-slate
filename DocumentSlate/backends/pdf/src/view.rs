//! Viewer features: page sizes, rendering, text, search, bookmarks and links.

use std::sync::Arc;

use pdfcraft_render::{PageRenderer, ReadingColors, ReadingParams, RenderConfig, RenderRequest, RenderWarning, RequestKind};

use crate::geom::{rect_to_view, norm};
use crate::{Bookmark, LinkTarget, PageInfo, PageLink, PdfDoc, PdfError, RenderedImage, SearchHit, ViewPoint, ViewRect};

/// The largest scale we accept (pixels per point). 8 = 576 dpi; bigger pages should be drawn in
/// tiles by the app (not needed on a phone).
const MAX_SCALE: f32 = 8.0;

/// A frozen copy of the document for drawing pages, which can be **sent to a background thread**
/// (like WordSlate's `page-render` worker). It keeps drawing the version it was made from; make
/// a new one when [`PdfDoc::generation`] changes.
pub struct PageSnapshot {
    bytes: Arc<Vec<u8>>,
    config: RenderConfig,
    pages: Vec<PageInfo>,
    renderer: Option<PageRenderer>,
}

impl PageSnapshot {
    fn new(bytes: Arc<Vec<u8>>, config: RenderConfig, pages: Vec<PageInfo>) -> Self {
        PageSnapshot { bytes, config, pages, renderer: None }
    }

    fn empty() -> Self {
        PageSnapshot::new(Arc::new(Vec::new()), RenderConfig::default(), Vec::new())
    }

    fn renderer(&mut self) -> &mut PageRenderer {
        let (bytes, config) = (&self.bytes, &self.config);
        self.renderer.get_or_insert_with(|| PageRenderer::new(bytes.clone(), config.clone()))
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn page_info(&self, page: usize) -> Option<&PageInfo> {
        self.pages.get(page)
    }

    /// Draw page `page` (0-based) at `scale` pixels per point (1.0 = 72 dpi; a phone at
    /// "fit width" is about `screen_width_px / page_width_pt`).
    pub fn render_page(&mut self, page: usize, scale: f32) -> Result<RenderedImage, PdfError> {
        if page >= self.pages.len() {
            return Err(PdfError::Invalid(format!("there is no page {}", page + 1)));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(PdfError::Invalid("the zoom must be a positive number".into()));
        }
        let req = RenderRequest { page, kind: RequestKind::Pixels, tile: None, scale: scale.min(MAX_SCALE), tag: 0, preview: false };
        let out = self.renderer().render(req);
        if let Some(e) = out.error {
            return Err(PdfError::Other(format!("page {} couldn't be drawn: {e}", page + 1)));
        }
        let warnings = out
            .warnings
            .iter()
            .map(|w| match w {
                RenderWarning::ContentTruncated => "some of this page's content was too large and was left out".to_string(),
                RenderWarning::ColourFallback => "some colors couldn't be computed and are approximate".to_string(),
                RenderWarning::MalformedIndexedImage => "a picture on this page is damaged and may look wrong".to_string(),
                RenderWarning::Type3WidthsMissing => "a font on this page is damaged; spacing may look wrong".to_string(),
            })
            .collect();
        Ok(RenderedImage { width: out.width, height: out.height, rgba: out.rgba.to_vec(), warnings })
    }

    /// Draw only `rect` (page view space) of page `page` at `scale` pixels per point — for
    /// reading a small barcode at high resolution. The picture's top-left is `rect`'s top-left.
    pub fn render_region(&mut self, page: usize, rect: ViewRect, scale: f32) -> Result<RenderedImage, PdfError> {
        let info = self.pages.get(page).ok_or_else(|| PdfError::Invalid(format!("there is no page {}", page + 1)))?;
        if !scale.is_finite() || scale <= 0.0 || rect.iter().any(|v| !v.is_finite()) {
            return Err(PdfError::Invalid("the area isn't valid".into()));
        }
        let scale = scale.min(MAX_SCALE);
        let x0 = rect[0].clamp(0.0, info.width);
        let y0 = rect[1].clamp(0.0, info.height);
        let x1 = rect[2].clamp(0.0, info.width);
        let y1 = rect[3].clamp(0.0, info.height);
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
            return Err(PdfError::Invalid("the area is outside the page".into()));
        }
        let tile = pdfcraft_render::Tile { x: (x0 * scale) as u32, y: (y0 * scale) as u32, w: ((x1 - x0) * scale).ceil() as u32, h: ((y1 - y0) * scale).ceil() as u32 };
        let req = RenderRequest { page, kind: RequestKind::Pixels, tile: Some(tile), scale, tag: 0, preview: false };
        let out = self.renderer().render(req);
        if let Some(e) = out.error {
            return Err(PdfError::Other(format!("page {} couldn't be drawn: {e}", page + 1)));
        }
        Ok(RenderedImage { width: out.width, height: out.height, rgba: out.rgba.to_vec(), warnings: Vec::new() })
    }

    /// The page's text layer (glyphs with positions), or `None` if it can't be read.
    fn text_layer(&mut self, page: usize) -> Option<Arc<pdfcraft_render::PageText>> {
        if page >= self.pages.len() {
            return None;
        }
        let req = RenderRequest { page, kind: RequestKind::Text, tile: None, scale: 1.0, tag: 0, preview: false };
        self.renderer().render(req).text
    }

    /// The page's text, lines separated by newlines (for copy / share / search).
    pub fn page_text(&mut self, page: usize) -> Result<String, PdfError> {
        self.text_layer(page).map(|t| t.plain_text()).ok_or_else(|| PdfError::Other(format!("the text of page {} couldn't be read", page + 1)))
    }

    /// Night reading colours: dark text and line art become `text`, the white page becomes
    /// `background` (sRGB). `None` = the page's own colours. Display only — the document, saves
    /// and printing are not affected. Pictures keep their colours.
    pub fn set_reading_colors(&mut self, colors: Option<([u8; 3], [u8; 3])>) {
        self.config.reading_colors = match colors {
            Some((text, background)) => ReadingColors { params: Some(ReadingParams { text, background, only_black: false, line_art: true }) },
            None => ReadingColors::default(),
        };
        self.renderer = None;
    }

    /// The text between two points on a page (e.g. where a finger drag started and where it is
    /// now), as one rectangle per line plus the text itself — for snapping highlights to words.
    /// `None` when the page has no text near `from` (within `reach` points).
    pub fn text_span(&mut self, page: usize, from: ViewPoint, to: ViewPoint, reach: f32) -> Option<(Vec<ViewRect>, String)> {
        let text = self.text_layer(page)?;
        let near = |p: ViewPoint| -> Option<usize> {
            let i = text.nearest(p[0], p[1])?;
            let r = norm(text.glyphs.get(i)?.rect);
            let dx = (r[0] - p[0]).max(p[0] - r[2]).max(0.0);
            let dy = (r[1] - p[1]).max(p[1] - r[3]).max(0.0);
            (dx <= reach && dy <= reach).then_some(i)
        };
        let a = near(from)?;
        let b = text.nearest(to[0], to[1]).unwrap_or(a);
        let (s, e) = (a.min(b), a.max(b) + 1);
        let rects: Vec<ViewRect> = text.line_rects(s..e).into_iter().map(norm).collect();
        if rects.is_empty() {
            return None;
        }
        Some((rects, text.text_of(s..e)))
    }

    /// Find `query` on every page. Case-insensitive unless `case_sensitive`; with `whole_words`
    /// only whole words match. Stops after `limit` hits in total.
    pub fn search(&mut self, query: &str, case_sensitive: bool, whole_words: bool, limit: usize) -> Vec<SearchHit> {
        let mut hits = Vec::new();
        if query.trim().is_empty() {
            return hits;
        }
        for page in 0..self.pages.len() {
            if hits.len() >= limit {
                break;
            }
            hits.extend(self.search_page(page, query, case_sensitive, whole_words, limit - hits.len()));
        }
        hits
    }

    /// Like [`PageSnapshot::search`] for one page (lets the app search page by page and show
    /// progress on long documents).
    pub fn search_page(&mut self, page: usize, query: &str, case_sensitive: bool, whole_words: bool, limit: usize) -> Vec<SearchHit> {
        let Some(text) = self.text_layer(page) else { return Vec::new() };
        text.find_opts_limited(query, case_sensitive, whole_words, limit)
            .into_iter()
            .map(|range| SearchHit { page, rects: text.line_rects(range.clone()).into_iter().map(norm).collect(), text: text.text_of(range) })
            .collect()
    }
}

impl PdfDoc {
    pub fn page_count(&self) -> usize {
        self.doc().map(|d| d.info.pages.len()).unwrap_or(0)
    }

    /// Size, label and rotation of one page.
    pub fn page_info(&self, page: usize) -> Result<PageInfo, PdfError> {
        let p = self.page_ref(page)?;
        Ok(PageInfo { width: p.width, height: p.height, label: p.label.clone(), rotation: p.rotation })
    }

    /// All pages' sizes (for laying out the scroll view).
    pub fn pages(&self) -> Vec<PageInfo> {
        self.doc()
            .map(|d| d.info.pages.iter().map(|p| PageInfo { width: p.width, height: p.height, label: p.label.clone(), rotation: p.rotation }).collect())
            .unwrap_or_default()
    }

    /// A copy of the current version for drawing on another thread. Cheap: the file bytes are
    /// shared, not copied.
    pub fn snapshot(&self) -> PageSnapshot {
        match self.doc() {
            // `display` = the working file plus on-screen-only extras (appearances for comments
            // that have none). No installed system fonts are used (`on_screen` is not applied),
            // so what you see is what prints.
            Ok(d) => PageSnapshot::new(d.display.clone(), self.session.render_config(self.id).unwrap_or_default(), self.pages()),
            Err(_) => PageSnapshot::empty(),
        }
    }

    /// The cached snapshot, rebuilt after any change.
    fn cached(&mut self) -> &mut PageSnapshot {
        let generation = self.generation();
        if self.snapshot.as_ref().is_none_or(|(g, _)| *g != generation) {
            let fresh = self.snapshot();
            self.snapshot = Some((generation, fresh));
        }
        // Always filled just above; the fallback only avoids a panic path.
        &mut self.snapshot.get_or_insert_with(|| (generation, PageSnapshot::empty())).1
    }

    /// Draw a page on this thread. For smooth scrolling, use [`PdfDoc::snapshot`] on a worker
    /// thread instead.
    pub fn render_page(&mut self, page: usize, scale: f32) -> Result<RenderedImage, PdfError> {
        self.cached().render_page(page, scale)
    }

    pub fn page_text(&mut self, page: usize) -> Result<String, PdfError> {
        self.cached().page_text(page)
    }

    /// See [`PageSnapshot::render_region`].
    pub fn render_region(&mut self, page: usize, rect: ViewRect, scale: f32) -> Result<RenderedImage, PdfError> {
        self.cached().render_region(page, rect, scale)
    }

    pub fn search(&mut self, query: &str, case_sensitive: bool, whole_words: bool, limit: usize) -> Vec<SearchHit> {
        self.cached().search(query, case_sensitive, whole_words, limit)
    }

    /// See [`PageSnapshot::text_span`].
    pub fn text_span(&mut self, page: usize, from: ViewPoint, to: ViewPoint, reach: f32) -> Option<(Vec<ViewRect>, String)> {
        self.cached().text_span(page, from, to, reach)
    }

    /// The bookmarks (outline). Very large outlines are cut at the engine's limit.
    pub fn outline(&self) -> Vec<Bookmark> {
        fn convert(items: &[pdfcraft_render::OutlineItem], depth: usize) -> Vec<Bookmark> {
            items
                .iter()
                .map(|i| Bookmark {
                    title: i.title.clone(),
                    page: i.page,
                    uri: match &i.action {
                        Some(pdfcraft_render::LinkTarget::Uri(u)) => Some(u.clone()),
                        _ => None,
                    },
                    // Bounded: a hostile file can't make us recurse forever.
                    children: if depth < 64 { convert(&i.children, depth + 1) } else { Vec::new() },
                })
                .collect()
        }
        self.doc().map(|d| convert(&d.info.outline, 0)).unwrap_or_default()
    }

    /// The links on one page, or on all pages with `None`.
    pub fn links(&self, page: Option<usize>) -> Vec<PageLink> {
        let Ok(doc) = self.doc() else { return Vec::new() };
        doc.info
            .links
            .iter()
            .filter(|l| page.is_none_or(|p| p == l.page))
            .filter_map(|l| {
                let info = doc.info.pages.get(l.page)?;
                let target = match &l.target {
                    pdfcraft_render::LinkTarget::Page(p, _) => LinkTarget::Page(*p),
                    pdfcraft_render::LinkTarget::Uri(u) => LinkTarget::Uri(u.clone()),
                    // Never run: PDF JavaScript is off in this app.
                    pdfcraft_render::LinkTarget::JavaScript(_) => LinkTarget::Other("a script (not run)".into()),
                    pdfcraft_render::LinkTarget::SetLayers { .. } => LinkTarget::Other("show/hide layers".into()),
                    pdfcraft_render::LinkTarget::RemoteGoTo { file, .. } => LinkTarget::Other(format!("another file: {file}")),
                    pdfcraft_render::LinkTarget::Other(s) => LinkTarget::Other(s.clone()),
                };
                Some(PageLink { page: l.page, index: l.index, rect: rect_to_view(info, l.rect), target })
            })
            .collect()
    }
}
