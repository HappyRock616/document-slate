//! WordSlate document view: viewer mode by default, edit mode on demand.
//!
//! The upstream WordCraft engine (`Session`) owns the document: every change is an engine
//! command (`text.insert`, `format.bold`, `para.alignCenter`, `edit.undo`, …), and
//! `Session::layout()` re-lays-out only what changed. Pages are drawn by `wordcraft-render` on a
//! background thread, only when they scroll into view; after an edit the visible pages are
//! redrawn while the old picture stays on screen, so nothing flickers.
//!
//! Edit mode: tap places the caret (double-tap selects a word), the on-screen keyboard types
//! through the IME bridge (src/android_ime.rs → egui Text/Key events), and the bottom tab bar
//! (src/lib.rs) runs formatting commands.
//!
//! Search uses the engine's `edit.find` and the layout's selection rectangles to highlight
//! every match on the pages; Next/Previous scroll to each one.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;

use eframe::egui;
use wordcraft_engine::doc::{Document, StoryRef};
use wordcraft_engine::layout::DocLayout;
use wordcraft_engine::{Selection, Session};

mod fidelity;
mod links;
mod objects;
pub use objects::{ChartTable, ObjKind};

/// Gap between pages, in screen points.
const PAGE_GAP: f32 = 16.0;
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 4.0;
/// Highlight at most this many search matches (keeps a search for "e" in a long book fast).
const MAX_HITS: usize = 500;

/// A page picture request for the background thread: one page of one document version.
struct Job {
    page: usize,
    /// Pixels per page point.
    scale: f32,
    px: u32,
    generation: u64,
    doc: Arc<Document>,
    layout: Arc<DocLayout>,
    /// Dark page: light text on a dark sheet (the renderer's own dark mode).
    dark: bool,
}

/// A finished page picture.
struct Done {
    page: usize,
    px: u32,
    generation: u64,
    img: wordcraft_render::Rendered,
}

struct Worker {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
}

impl Worker {
    /// Start the drawing thread; it stops when the viewer is dropped (the job channel closes).
    fn start(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel::<Done>();
        let spawned = std::thread::Builder::new().name("page-render".into()).spawn(move || {
            // Same as the engine's "Export Page as PNG": the page as printed, headers not dimmed.
            let mut opts = wordcraft_render::RenderOptions::default();
            opts.display.dim_header = false;
            // Pending requests, at most one per page (the newest). Every page asked for is drawn
            // eventually; while typing, older versions of a page are skipped.
            let mut queue: Vec<Job> = Vec::new();
            loop {
                if queue.is_empty() {
                    match job_rx.recv() {
                        Ok(j) => queue.push(j),
                        Err(_) => break,
                    }
                }
                while let Ok(j) = job_rx.try_recv() {
                    match queue.iter_mut().find(|q| q.page == j.page) {
                        Some(q) if j.generation >= q.generation => *q = j,
                        Some(_) => {}
                        None => queue.push(j),
                    }
                }
                // Draw the newest-version job first (the page just edited), else the oldest request.
                let newest = queue.iter().map(|q| q.generation).max().unwrap_or(0);
                let idx = queue.iter().position(|q| q.generation == newest).unwrap_or(0);
                let job = queue.remove(idx);
                let Some(page) = job.layout.pages.get(job.page) else { continue };
                opts.dark = job.dark;
                let img = wordcraft_render::render_page(&job.doc, page, job.scale, &opts);
                if done_tx.send(Done { page: job.page, px: job.px, generation: job.generation, img }).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        if let Err(e) = spawned {
            log::error!("page-render thread: {e}");
        }
        Self { jobs, done }
    }
}

/// A drawn page: texture, the pixel width and document version it was drawn for.
struct PageTex {
    tex: egui::TextureHandle,
    px: u32,
    generation: u64,
}

pub struct WordViewer {
    /// What the file held when opened / last saved (the full-fidelity guard compares against it).
    census: fidelity::Census,
    pub name: String,
    /// The file as last opened or saved (Share sends this).
    bytes: Vec<u8>,
    session: Session,
    layout: Arc<DocLayout>,
    textures: Vec<Option<PageTex>>,
    /// (pixel width, version) requested from the worker and not yet back, per page.
    in_flight: Vec<Option<(u32, u64)>>,
    worker: Option<Worker>,
    /// Bumped on every change; pages drawn for an older version get redrawn.
    generation: u64,
    /// The document copy the worker draws from, for `generation`.
    snapshot: Option<(u64, Arc<Document>)>,
    /// 1.0 = page width fits the screen width.
    zoom: f32,
    // ---- edit mode ----
    pub editing: bool,
    /// Changed since opened / last saved.
    pub dirty: bool,
    /// The keyboard opens only after the user taps the page (not as soon as Edit is pressed).
    keyboard_wanted: bool,
    /// Time (egui seconds) the caret last moved; it stays solid for a moment, then blinks.
    blink_from: f64,
    scroll_to_caret: bool,
    /// Set by `show` each frame for the app's bars: tapped the page (viewer mode), scrolled,
    /// and the page under the middle of the screen (0-based).
    pub tapped: bool,
    /// Barcodes found on the drawn pages (viewer: tap one to read it).
    pub scanner: crate::barcode_scan::Scanner,
    pub scrolled: bool,
    pub current_page: usize,
    /// Scroll position and visible height of the page area last frame (for keeping the caret visible).
    last_offset: f32,
    last_view_h: f32,
    /// Formatting at the caret, for highlighting toolbar buttons; refreshed after each change.
    fmt: serde_json::Value,
    /// Spelling problem at the caret (word + suggestions), shown in the Review tab.
    pub issue: Option<Issue>,
    // ---- Review tab: find & replace row ----
    pub replace_open: bool,
    pub replace_find: String,
    pub replace_with: String,
    // ---- search ----
    pub search_open: bool,
    query: String,
    searched: String,
    /// Per match: its highlight rectangles (page index, rectangle in page points).
    hits: Vec<Vec<(usize, egui::Rect)>>,
    current: usize,
    /// Scroll to the current match on the next frame.
    jump: bool,
    focus_search: bool,
    // ---- viewer extras ----
    /// Dark page (night reading): pages drawn light-on-dark.
    pub dark_page: bool,
    /// Read mode: text reflows to the screen width (no page layout), like an e-reader.
    pub read_mode: bool,
    /// Width (page points) read mode was last laid out for.
    read_w_applied: f32,
    /// Scroll to this page + y (page points) on the next frame (outline jumps).
    jump_to: Option<(usize, f32)>,
    // ---- objects (src/word_viewer/objects.rs) ----
    selected: Option<objects::SelectedObject>,
    obj_drag: Option<objects::ObjDrag>,
    /// Chart data grid being edited (Object tab › Data), if open.
    pub chart_edit: Option<ChartTable>,
}

/// A spelling mistake the engine's offline checker found.
#[derive(Clone, Debug)]
pub struct Issue {
    pub word: String,
    pub suggestions: Vec<String>,
}

impl Issue {
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        let word = v.get("text")?.as_str()?.to_owned();
        let suggestions = v
            .get("suggestions")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).take(5).collect())
            .unwrap_or_default();
        Some(Self { word, suggestions })
    }
}

/// Commands that only look or move (never change the document): they don't mark it unsaved.
fn changes_nothing(id: &str) -> bool {
    id.starts_with("caret.")
        || id.starts_with("select.")
        || matches!(
            id,
            "format.state"
                | "review.proofing"
                | "review.suggestions"
                | "review.spelling"
                | "review.wordCount"
                | "review.nextComment"
                | "review.previousComment"
                | "review.nextChange"
                | "review.previousChange"
                | "review.ignoreAll"
                | "review.addToDictionary"
                | "link.targets"
        )
}

impl WordViewer {
    /// Open from raw file bytes (how Android's file picker hands files over).
    pub fn open_bytes(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let doc = match wordcraft_engine::io::open_bytes(name, bytes) {
            Ok(doc) => doc,
            // A text file the engine reads some other way and fails on (".json" = the engine's
            // own document format, ".xml" = Word XML): open it as plain text instead of refusing.
            Err(e) if looks_like_text(bytes) => wordcraft_engine::io::open_bytes("text.txt", bytes).map_err(|_| e)?,
            Err(e) => return Err(e),
        };
        Ok(Self::from_session(name, bytes.to_vec(), Session::new(doc)))
    }

    /// A new blank document (the engine's Normal template on the region's paper size).
    pub fn new_blank(name: &str) -> Self {
        let probe = Session::new(wordcraft_engine::doc::Document::new());
        let doc = probe.blank_document();
        Self::from_session(name, Vec::new(), Session::new(doc))
    }

    /// Apply the app preferences: your name on new comments / tracked changes, and the spelling
    /// language. An opened file keeps its own language tags (only the default changes); a brand-new
    /// document (`new_doc`) is written in that language. Doesn't mark the document as changed.
    pub fn apply_prefs(&mut self, lang: &str, author: &str, new_doc: bool) {
        // Blank = "Author": the engine's own default ("WordCraft User") names the upstream app.
        let author = if author.trim().is_empty() { "Author" } else { author.trim() };
        if let Err(e) = self.session.run("file.setAuthor", &serde_json::json!({ "name": author })) {
            log::warn!("file.setAuthor: {e}");
        }
        if !lang.is_empty() {
            let _ = self.session.run("review.language.preferred", &serde_json::json!({ "lang": lang }));
            if new_doc {
                let r = self.session.run("review.language", &serde_json::json!({ "lang": lang, "default": true, "scope": "document" }));
                if let Err(e) = r {
                    log::warn!("review.language: {e}");
                }
                self.after_change();
            }
        }
    }

    /// The engine's built-in sample document, saved as a real .docx and read back in,
    /// so the DOCX reader and writer both get exercised.
    #[cfg(test)]
    pub fn sample_via_docx(scratch_dir: &Path) -> Result<Self, String> {
        let path = scratch_dir.join("sample.docx");
        let mut s = Session::new(wordcraft_engine::sample::sample_document());
        s.run("file.saveAs", &serde_json::json!({ "path": path.to_string_lossy() }))
            .map_err(|e| format!("save sample: {e}"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("read sample: {e}"))?;
        Self::open_bytes("sample.docx", &bytes)
    }

    fn from_session(name: &str, bytes: Vec<u8>, mut session: Session) -> Self {
        // Viewer mode: no spelling underlines (they switch on with Edit).
        let _ = session.run("review.proofing", &serde_json::json!({ "value": false }));
        let layout = session.layout();
        let pages = layout.pages.len();
        let mut v = Self {
            census: fidelity::Census::default(),
            name: name.to_owned(),
            bytes,
            session,
            layout,
            textures: (0..pages).map(|_| None).collect(),
            in_flight: vec![None; pages],
            worker: None,
            generation: 0,
            snapshot: None,
            zoom: 1.0,
            editing: false,
            dirty: false,
            keyboard_wanted: false,
            blink_from: 0.0,
            tapped: false,
            scanner: crate::barcode_scan::Scanner::default(),
            scrolled: false,
            current_page: 0,
            scroll_to_caret: false,
            last_offset: 0.0,
            last_view_h: 0.0,
            fmt: serde_json::Value::Null,
            issue: None,
            replace_open: false,
            replace_find: String::new(),
            replace_with: String::new(),
            search_open: false,
            query: String::new(),
            searched: String::new(),
            hits: Vec::new(),
            current: 0,
            jump: false,
            focus_search: false,
            dark_page: false,
            read_mode: false,
            read_w_applied: 0.0,
            jump_to: None,
            selected: None,
            obj_drag: None,
            chart_edit: None,
        };
        v.refresh_fmt();
        v.take_census();
        v
    }

    /// The listed paper (Letter, Legal, A4, 11×17) the current page is, if any.
    pub fn paper_name(&self) -> Option<&'static str> {
        let p = self.layout.pages.get(self.current_page).or(self.layout.pages.first())?;
        crate::paper::name_for(p.w, p.h)
    }

    pub fn page_count(&self) -> usize {
        self.layout.pages.len()
    }

    // ---------------------------------------------------------------- engine commands

    /// Run an engine command, then re-lay-out and redraw. Looking/moving commands don't mark the
    /// document as changed. Returns the command's result (e.g. the spelling issue found).
    pub fn run(&mut self, id: &str, params: serde_json::Value) -> Option<serde_json::Value> {
        match self.session.run(id, &params) {
            Ok(r) => {
                if !changes_nothing(id) {
                    self.dirty = true;
                }
                self.after_change();
                Some(r)
            }
            Err(e) => {
                log::warn!("{id}: {e}");
                None
            }
        }
    }

    fn after_change(&mut self) {
        self.layout = self.session.layout();
        self.generation += 1;
        let pages = self.layout.pages.len();
        self.textures.resize_with(pages, || None);
        self.in_flight.resize(pages, None);
        self.scroll_to_caret = true;
        if !self.hits.is_empty() {
            self.searched.clear(); // positions moved: search again next frame
        }
        self.refresh_fmt();
        self.refresh_selected_object();
    }

    fn refresh_fmt(&mut self) {
        self.fmt = self.session.run("format.state", &serde_json::json!({})).unwrap_or(serde_json::Value::Null);
        // Spelling suggestions for the word at the caret (edit mode only: it costs a lookup).
        self.issue = if self.editing {
            self.session.run("review.suggestions", &serde_json::json!({})).ok().as_ref().and_then(Issue::from_json)
        } else {
            None
        };
    }

    /// Replace the misspelled word at the caret with `word` (a suggestion).
    pub fn apply_suggestion(&mut self, word: &str) {
        self.run("review.applySuggestion", serde_json::json!({ "text": word }));
    }

    /// Jump to the next spelling mistake (selects it; its suggestions show in the Review tab).
    /// Returns false when there are none.
    pub fn next_mistake(&mut self) -> bool {
        let found = self.run("review.spelling", serde_json::json!({})).as_ref().and_then(Issue::from_json);
        let any = found.is_some();
        if any {
            self.issue = found;
        }
        any
    }

    /// Track Changes on/off (stored in the document).
    pub fn tracking(&self) -> bool {
        self.session.doc.settings.track_changes
    }

    /// "412 words · 2,310 characters · 2 pages" — the selection's count if text is selected.
    pub fn word_count_line(&mut self) -> String {
        let selected = self.session.sel.anchor != self.session.sel.focus;
        match self.session.run("review.wordCount", &serde_json::json!({})) {
            Ok(v) => {
                let n = |k: &str| v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
                let what = if selected { "Selection" } else { "Document" };
                format!("{what}: {} words · {} characters · {}", n("words"), n("charactersWithSpaces"), plural(n("pages"), "page"))
            }
            Err(e) => format!("Word count unavailable: {e}"),
        }
    }

    /// Replace every match of the Review tab's find text. Returns how many were replaced.
    pub fn replace_all(&mut self) -> String {
        if self.replace_find.is_empty() {
            return "Type something to find first".into();
        }
        let params = serde_json::json!({ "text": self.replace_find, "with": self.replace_with });
        match self.run("edit.replaceAll", params) {
            Some(r) => {
                let n = r.get("replaced").or_else(|| r.get("count")).and_then(serde_json::Value::as_u64).unwrap_or(0);
                format!("Replaced {n}")
            }
            None => "Replace failed".into(),
        }
    }

    /// Is a toggle (e.g. "bold", "underline") on at the caret / for the whole selection?
    /// The engine reports some as true/false and some as a kind ("single" underline).
    pub fn fmt_on(&self, key: &str) -> bool {
        match self.fmt.get(key) {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(s)) => !s.is_empty() && s != "none" && s != "mixed",
            _ => false,
        }
    }

    /// Paragraph style name at the caret, e.g. "Heading 1".
    pub fn style_name(&self) -> String {
        self.fmt.get("styleName").and_then(serde_json::Value::as_str).unwrap_or("Normal").to_owned()
    }

    /// Font name at the caret, e.g. "Calibri" (empty if mixed).
    /// Font size at the caret in points (None when the selection mixes sizes).
    pub fn font_size(&self) -> Option<f64> {
        self.fmt.get("size").and_then(serde_json::Value::as_f64)
    }

    /// Is the caret inside a table cell? (Paths in a table are [block, row, cell, block...].)
    pub fn in_table(&self) -> bool {
        self.session.sel.focus.path.0.len() >= 4
    }

    pub fn font_name(&self) -> String {
        self.fmt.get("font").and_then(serde_json::Value::as_str).unwrap_or("").to_owned()
    }

    /// Export the whole document as PDF (for printing / sharing).
    pub fn export_pdf(&mut self, path: &Path) -> Result<(), String> {
        self.session
            .run("file.exportPdf", &serde_json::json!({ "path": path.to_string_lossy() }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Save the document in its own format to `path` (a temporary file; the caller copies it to
    /// the real location). Clears the changed mark on success.
    #[cfg(test)]
    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        self.session
            .run("file.saveAs", &serde_json::json!({ "path": path.to_string_lossy() }))
            .map_err(|e| e.to_string())?;
        self.bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        self.dirty = false;
        self.take_census();
        Ok(())
    }

    /// Save for overwriting the original: write to `path`, then check nothing would be lost
    /// (`save_losses`). Safe: marks the document saved and returns an empty list. Otherwise the
    /// document stays unsaved and the losses are returned, so the caller can keep the original
    /// and offer a copy instead.
    pub fn save_checked(&mut self, path: &Path) -> Result<Vec<String>, String> {
        self.write_as(path)?;
        let lost = self.save_losses(path);
        if lost.is_empty() {
            self.bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            self.dirty = false;
            self.take_census();
        }
        Ok(lost)
    }

    /// Write the document in the format of `path`'s extension (.docx, .odt, .txt…) without
    /// clearing the changed mark: Save as only counts once the copy really landed somewhere.
    pub fn write_as(&mut self, path: &Path) -> Result<(), String> {
        let was_dirty = self.dirty;
        self.session
            .run("file.saveAs", &serde_json::json!({ "path": path.to_string_lossy() }))
            .map_err(|e| e.to_string())?;
        self.dirty = was_dirty;
        Ok(())
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // used after Android's "save to" screen
    /// After a successful Save as: this view now shows the new file (`name`, contents at `path`).
    pub fn adopt_saved(&mut self, name: &str, path: &Path) {
        self.name = name.to_owned();
        if let Ok(b) = std::fs::read(path) {
            self.bytes = b;
        }
        self.dirty = false;
        self.take_census();
    }

    /// "2 pages · 412 words · 45 paragraphs" from the engine's File › Info.
    pub fn info_line(&mut self) -> String {
        match self.session.run("file.info", &serde_json::json!({})) {
            Ok(v) => {
                let n = |k: &str| v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
                format!("{} · {} words · {} paragraphs", plural(n("pages"), "page"), n("words"), n("paragraphs"))
            }
            Err(e) => format!("Info unavailable: {e}"),
        }
    }

    /// The name without its extension ("Budget.docx" -> "Budget"), for Save as suggestions.
    pub fn base_name(&self) -> String {
        match self.name.rsplit_once('.') {
            Some((b, _)) if !b.is_empty() => b.to_owned(),
            _ => self.name.clone(),
        }
    }

    /// Can the engine write this file type back? (.doc and other read-only formats can't.)
    pub fn can_save_in_place(&self) -> bool {
        matches!(ext(&self.name).as_str(), "docx" | "odt" | "rtf" | "txt" | "md" | "html" | "htm")
    }

    /// Write the document as it is now into `dir` (for the share sheet) and return its path and
    /// type. Unchanged since it was opened or saved: the file's own bytes. With unsaved edits (or
    /// a new document): the engine writes the current state, as .docx when the file's own format
    /// can't be written.
    pub fn write_copy(&mut self, dir: &Path) -> Result<(std::path::PathBuf, &'static str), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        if !self.dirty && !self.bytes.is_empty() {
            let path = dir.join(safe_file_name(&self.name));
            std::fs::write(&path, &self.bytes).map_err(|e| e.to_string())?;
            return Ok((path, mime_for(&self.name)));
        }
        let name = if self.can_save_in_place() { self.name.clone() } else { format!("{}.docx", self.base_name()) };
        let path = dir.join(safe_file_name(&name));
        self.write_as(&path)?;
        let mime = mime_for(&name);
        Ok((path, mime))
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Dark page on/off (redraws every page; the document itself is unchanged).
    pub fn set_dark_page(&mut self, on: bool) {
        self.dark_page = on;
        self.generation += 1;
    }

    /// Read mode on/off: the engine lays the text out as one continuous column as wide as the
    /// screen (set in `show`), so it reads like an e-book instead of tiny pages on a phone.
    pub fn set_read_mode(&mut self, on: bool) {
        self.read_mode = on;
        self.session.view.read_mode = on;
        // Screen-high pages until `show` sets the real size (0 would give the engine's 2-inch minimum).
        self.session.view.read_height = self.session.view.read_height.max(600.0);
        self.read_w_applied = 0.0;
        self.zoom = 1.0;
        self.after_change();
    }

    /// The document's headings for the outline: (paragraph index, level 1…9, text).
    pub fn headings(&mut self) -> Vec<(usize, u64, String)> {
        let Ok(v) = self.session.run("link.targets", &serde_json::json!({})) else { return Vec::new() };
        v.get("headings")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|h| {
                        let block = h.get("block")?.as_u64()? as usize;
                        let level = h.get("level").and_then(serde_json::Value::as_u64).unwrap_or(1);
                        let text = h.get("text")?.as_str()?.to_owned();
                        Some((block, level, text))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Scroll to the paragraph `block` (an outline entry).
    pub fn go_to_block(&mut self, block: usize) {
        let pos = wordcraft_engine::doc::Pos::body(block, 0);
        if let Some(c) = self.layout.caret(&pos) {
            self.jump_to = Some((c.page, c.top));
        }
        if self.editing {
            self.session.sel = Selection::caret(pos);
            self.after_change();
        }
    }

    pub fn set_editing(&mut self, on: bool) {
        self.editing = on;
        self.keyboard_wanted = false;
        if on {
            self.search_open = false;
            self.hits.clear();
            self.scroll_to_caret = true;
        }
        // Red spelling underlines only while editing; the viewer stays clean (like WPS).
        let _ = self.session.run("review.proofing", &serde_json::json!({ "value": on }));
        self.after_change();
    }

    // ---------------------------------------------------------------- search

    pub fn toggle_search(&mut self) {
        self.search_open = !self.search_open;
        self.focus_search = self.search_open;
        if !self.search_open {
            self.hits.clear();
            self.searched.clear();
        }
    }

    fn run_search(&mut self) {
        self.searched = self.query.clone();
        self.hits.clear();
        self.current = 0;
        let q = self.query.trim().to_owned();
        if q.is_empty() {
            return;
        }
        // The engine searches the part the caret is in; search the main text, not a header,
        // footer or text box the caret happens to be in. (Find moves the selection anyway.)
        if self.session.sel.focus.story != StoryRef::Body {
            self.session.sel = Selection::caret(wordcraft_engine::doc::Pos::body(0, 0));
        }
        if let Err(e) = self.session.run("edit.find", &serde_json::json!({ "text": q })) {
            log::warn!("find: {e}");
            return;
        }
        let results = self.session.find.results.clone();
        for (a, b) in results.iter().take(MAX_HITS) {
            let rects: Vec<(usize, egui::Rect)> = self
                .layout
                .selection_rects(&self.session.doc, a, b, 0)
                .into_iter()
                .map(|(page, r)| (page, egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.w, r.h))))
                .collect();
            if !rects.is_empty() {
                self.hits.push(rects);
            }
        }
        self.jump = !self.hits.is_empty();
    }

    fn step_match(&mut self, delta: isize) {
        let n = self.hits.len();
        if n == 0 {
            return;
        }
        self.current = (self.current as isize + delta).rem_euclid(n as isize) as usize;
        self.jump = true;
    }

    /// The search row under the heading bar (only while search is open).
    pub fn search_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(44.0);
            let edit = egui::TextEdit::singleline(&mut self.query)
                .hint_text("Find in document")
                .desired_width((ui.available_width() - 260.0).max(120.0));
            let resp = ui.add(edit);
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            // Search as you type; Enter jumps to the next match.
            if self.query != self.searched {
                self.run_search();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.step_match(1);
                resp.request_focus();
            }
            let big = |t: &str| egui::Button::new(t).min_size(egui::vec2(56.0, 36.0));
            if ui.add(big("Prev")).clicked() {
                self.step_match(-1);
            }
            if ui.add(big("Next")).clicked() {
                self.step_match(1);
            }
            let count = match (self.hits.len(), self.query.trim().is_empty()) {
                (_, true) => String::new(),
                (0, false) => "No matches".into(),
                (n, false) => format!("{} of {}{}", self.current + 1, n, if n >= MAX_HITS { "+" } else { "" }),
            };
            ui.label(count);
        });
    }

    // ---------------------------------------------------------------- typing

    /// Turn this frame's keyboard input into engine commands (edit mode, no text box focused).
    fn handle_typing(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Text(t) if !t.is_empty() => {
                    self.run("text.insert", serde_json::json!({ "text": t }));
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let id = match key {
                        egui::Key::Backspace => "text.backspace",
                        egui::Key::Delete => "text.delete",
                        egui::Key::Enter if modifiers.shift => "text.lineBreak",
                        egui::Key::Enter => "text.newParagraph",
                        egui::Key::Tab => "text.tab",
                        egui::Key::ArrowLeft => "caret.left",
                        egui::Key::ArrowRight => "caret.right",
                        egui::Key::ArrowUp => "caret.up",
                        egui::Key::ArrowDown => "caret.down",
                        egui::Key::Home => "caret.home",
                        egui::Key::End => "caret.end",
                        egui::Key::Z if modifiers.command => "edit.undo",
                        egui::Key::Y if modifiers.command => "edit.redo",
                        _ => continue,
                    };
                    self.run(id, serde_json::json!({}));
                }
                _ => {}
            }
        }
    }

    /// Put the caret where the user tapped (page-local point in page points).
    fn place_caret(&mut self, page: usize, x: f32, y: f32) {
        if self.try_select_object(page, x, y) {
            return;
        }
        if let Some(pos) = self.layout.hit(page, x, y, StoryRef::Body) {
            self.session.sel = Selection { anchor: pos.clone(), focus: pos };
            self.keyboard_wanted = true;
            self.after_change();
        }
    }

    // ---------------------------------------------------------------- drawing

    /// Draw the page column. Call inside a panel.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.worker.is_none() {
            self.worker = Some(Worker::start(ctx.clone()));
        }
        self.collect_finished(&ctx);
        match self.scanner.poll() {
            Some(crate::barcode_scan::SpotResult::Found(h)) => crate::barcode_scan::show(&h.found),
            Some(crate::barcode_scan::SpotResult::Nothing) => crate::links::note("No barcode found there"),
            None => {}
        }
        if self.search_open && self.query != self.searched {
            self.run_search();
        }
        let typing_focus = ctx.memory(|m| m.focused().is_none());
        if self.editing && typing_focus {
            self.handle_typing(&ctx);
        }

        // Pinch-to-zoom (two fingers) or Ctrl+wheel on desktop.
        let pinch = ctx.input(|i| i.zoom_delta());
        if pinch != 1.0 {
            self.zoom_by(pinch);
        }

        // Read mode: lay the text out as wide as the screen (page points = screen points at zoom 1).
        if self.read_mode {
            let want = (ui.available_width() - 2.0 * PAGE_GAP).max(200.0);
            if (want - self.read_w_applied).abs() > 4.0 {
                self.read_w_applied = want;
                self.session.view.read_width = want;
                // One "page" per screenful, as upstream does (the engine cuts the column there).
                self.session.view.read_height = ui.available_height().max(300.0);
                self.layout = self.session.layout();
                self.generation += 1;
                let pages = self.layout.pages.len();
                self.textures.resize_with(pages, || None);
                self.in_flight.resize(pages, None);
            }
        }
        let ppp = ctx.pixels_per_point();
        let view_w = ui.available_width();
        let view_h = ui.available_height();
        // Width of the widest page in points; every page is scaled by the same factor.
        let widest = self.layout.pages.iter().map(|p| p.w).fold(1.0_f32, f32::max);
        let base = ((view_w - 2.0 * PAGE_GAP) / widest).max(0.05);
        let scale = base * self.zoom; // screen points per page point

        // Top of each page in the scrolled column.
        let mut tops = Vec::with_capacity(self.layout.pages.len());
        let mut y = PAGE_GAP;
        for p in &self.layout.pages {
            tops.push(y);
            y += p.h * scale + PAGE_GAP;
        }
        let total_h = y;
        let total_w = (widest * scale + 2.0 * PAGE_GAP).max(view_w);

        let caret = if self.editing { self.layout.caret(&self.session.sel.focus) } else { None };
        // Something changed (typing, tap, command): restart the blink with the caret showing.
        if self.scroll_to_caret {
            self.blink_from = ctx.input(|i| i.time);
        }

        // The keyboard or a taller toolbar shrank (or grew) the page area: keep the caret in view.
        if self.editing && self.last_view_h > 0.0 && (view_h - self.last_view_h).abs() > 24.0 {
            self.scroll_to_caret = true;
        }
        let mut area = egui::ScrollArea::both().auto_shrink(false);
        if let Some((page, y)) = self.jump_to.take() {
            let target = tops.get(page).copied().unwrap_or(0.0) + y * scale - 16.0;
            area = area.vertical_scroll_offset(target.max(0.0));
        } else if self.jump {
            self.jump = false;
            if let Some((page, r)) = self.hits.get(self.current).and_then(|h| h.first()) {
                let target = tops.get(*page).copied().unwrap_or(0.0) + r.min.y * scale - view_h / 3.0;
                area = area.vertical_scroll_offset(target.max(0.0));
            }
        } else if self.scroll_to_caret {
            self.scroll_to_caret = false;
            if let Some(c) = &caret {
                let top = tops.get(c.page).copied().unwrap_or(0.0) + c.top * scale;
                let bottom = top + c.height * scale;
                // Use this frame's height: the keyboard may have just appeared.
                let (lo, hi) = (self.last_offset, self.last_offset + view_h);
                if view_h > 0.0 && (top < lo + 8.0 || bottom > hi - 8.0) {
                    area = area.vertical_scroll_offset((top - view_h / 3.0).max(0.0));
                }
            }
        }

        let accent = ui.visuals().selection.bg_fill;
        let out = area.show_viewport(ui, |ui, viewport| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(total_w, total_h), egui::Sense::click());
            // Viewer mode: a tap on a link follows it; any other tap on the page shows/hides the
            // bars (the app reads `tapped`).
            let viewer_tap = !self.editing && resp.clicked();
            let mut link_tap: Option<(usize, egui::Pos2)> = None;
            let mut tap: Option<(usize, egui::Pos2, bool)> = None;
            // Each page's rectangle on screen (for turning a handle drag into a page point).
            let mut page_rects: Vec<egui::Rect> = Vec::with_capacity(self.layout.pages.len());
            // Long-press (touch) selects the word, like double-tap.
            let select_word = resp.double_clicked() || resp.long_touched();

            for i in 0..self.layout.pages.len() {
                let (pw, ph) = (self.layout.pages[i].w * scale, self.layout.pages[i].h * scale);
                let x = (total_w - pw) / 2.0;
                let local = egui::Rect::from_min_size(egui::pos2(x, tops[i]), egui::vec2(pw, ph));
                let screen = local.translate(rect.min.to_vec2());
                page_rects.push(screen);

                if viewer_tap {
                    if let Some(p) = resp.interact_pointer_pos().filter(|p| screen.contains(*p)) {
                        link_tap = Some((i, ((p - screen.min) / scale).to_pos2()));
                    }
                }
                if self.editing && (resp.clicked() || select_word) {
                    if let Some(p) = resp.interact_pointer_pos().filter(|p| screen.contains(*p)) {
                        tap = Some((i, ((p - screen.min) / scale).to_pos2(), select_word));
                    }
                }

                // Off-screen (with a one-screen margin): drop the texture to save memory.
                if !local.intersects(viewport.expand(viewport.height())) {
                    self.textures[i] = None;
                    continue;
                }
                self.request_page(i, local.intersects(viewport), pw * ppp, scale * ppp);

                // Page shadow + paper, then the page picture (stretched while a sharper one is drawn).
                let painter = ui.painter();
                painter.rect_filled(screen.translate(egui::vec2(2.0, 3.0)), 2.0, egui::Color32::from_black_alpha(60));
                painter.rect_filled(screen, 0.0, egui::Color32::WHITE);
                if let Some(t) = &self.textures[i] {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(t.tex.id(), screen, uv, egui::Color32::WHITE);
                }
                let to_screen = |r: &egui::Rect| egui::Rect::from_min_size(screen.min + r.min.to_vec2() * scale, r.size() * scale);

                // Selection (edit mode).
                if self.editing && self.session.sel.anchor != self.session.sel.focus {
                    let sel = &self.session.sel;
                    for (page, r) in self.layout.selection_rects(&self.session.doc, &sel.anchor, &sel.focus, i) {
                        if page == i {
                            let r = egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.w, r.h));
                            painter.rect_filled(to_screen(&r), 0.0, accent.gamma_multiply(0.35));
                        }
                    }
                }
                // Caret: blinks (on 0.5 s, off 0.5 s), solid for a moment after each change.
                if let Some(c) = caret.as_ref().filter(|c| c.page == i) {
                    let r = egui::Rect::from_min_size(egui::pos2(c.x, c.top), egui::vec2(0.0, c.height));
                    let s = to_screen(&r);
                    let since = ui.input(|inp| inp.time) - self.blink_from;
                    let shown = self.session.sel.anchor == self.session.sel.focus && (since < 0.5 || (since * 2.0) as i64 % 2 == 0);
                    if shown {
                        painter.line_segment([s.left_top(), s.left_bottom()], egui::Stroke::new(2.0, accent));
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                    // Tell egui (and so Android) where typing happens: keeps the keyboard open.
                    let caret_rect = egui::Rect::from_min_size(s.min, egui::vec2(2.0, s.height()));
                    let ime = egui::output::IMEOutput::new(
                        ui.scope_id().with("word-editor"),
                        1,
                        egui::IMEPurpose::Normal,
                        screen,
                        caret_rect,
                    );
                    if typing_focus && self.keyboard_wanted {
                        ui.ctx().output_mut(|o| o.ime = Some(ime));
                    }
                }
                // Search highlights on this page: all matches yellow, the current one orange.
                for (m, rects) in self.hits.iter().enumerate() {
                    let color = if m == self.current {
                        egui::Color32::from_rgba_unmultiplied(255, 140, 0, 110)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(255, 220, 0, 90)
                    };
                    for (page, r) in rects {
                        if *page == i {
                            painter.rect_filled(to_screen(r), 1.0, color);
                        }
                    }
                }
            }
            if self.editing {
                if self.selected.is_some() {
                    self.object_overlay(ui, &page_rects, scale, accent);
                } else {
                    self.selection_handles(ui, &page_rects, scale, accent);
                }
            }
            if viewer_tap && !link_tap.is_some_and(|(page, p)| self.follow_link_at(page, p.x, p.y) || self.read_barcode_at(page, p)) {
                self.tapped = true;
            }
            // Viewer: long-press reads a barcode too small to find at screen resolution.
            if !self.editing && resp.long_touched() {
                if let Some((i, r)) = resp.interact_pointer_pos().and_then(|p| page_rects.iter().enumerate().find(|(_, r)| r.contains(p)).map(|(i, r)| (i, (p - r.min) / scale))) {
                    self.read_spot(ui.ctx(), i, [r.x, r.y]);
                }
            }
            if !self.editing {
                for (i, r) in page_rects.iter().enumerate() {
                    if r.intersects(viewport.translate(rect.min.to_vec2())) {
                        self.scanner.draw_badges(ui.painter(), i, *r, scale, accent);
                    }
                }
            }
            if let Some((page, p, word)) = tap {
                self.place_caret(page, p.x, p.y);
                if word && self.selected.is_none() {
                    self.run("select.word", serde_json::json!({}));
                }
            }
            viewport
        });
        // Scrolled this frame? (drives auto-hide and the page counter)
        self.scrolled = (out.inner.min.y - self.last_offset).abs() > 0.5 && self.last_view_h > 0.0;
        // Current page = the one under the middle of the screen.
        let mid = out.inner.center().y;
        self.current_page = tops.iter().rposition(|t| *t <= mid).unwrap_or(0);
        self.last_offset = out.inner.min.y;
        self.last_view_h = out.inner.height();
    }

    /// Two round handles under the ends of a selection; dragging one moves that end of the
    /// selection (the page scrolls only when dragging elsewhere).
    fn selection_handles(&mut self, ui: &mut egui::Ui, page_rects: &[egui::Rect], scale: f32, accent: egui::Color32) {
        if self.session.sel.anchor == self.session.sel.focus {
            return;
        }
        let (start, end) = if self.session.sel.anchor <= self.session.sel.focus {
            (self.session.sel.anchor.clone(), self.session.sel.focus.clone())
        } else {
            (self.session.sel.focus.clone(), self.session.sel.anchor.clone())
        };
        const R: f32 = 11.0;
        let ends = [start.clone(), end.clone()];
        for (which, pos) in [(0usize, start), (1usize, end)] {
            let Some(c) = self.layout.caret(&pos) else { continue };
            let Some(page) = page_rects.get(c.page) else { continue };
            // Stem from the bottom of the line, knob below it.
            let foot = page.min + egui::vec2(c.x, c.top + c.height) * scale;
            let knob = foot + egui::vec2(0.0, R + 2.0);
            let hit = egui::Rect::from_center_size(knob, egui::vec2(R * 4.0, R * 4.0));
            let resp = ui.interact(hit, ui.scope_id().with(("sel-handle", which)), egui::Sense::drag());
            let painter = ui.painter();
            painter.line_segment([foot - egui::vec2(0.0, c.height * scale), foot], egui::Stroke::new(2.0, accent));
            painter.circle_filled(knob, R, accent);
            if resp.dragged() {
                if let Some(p) = resp.interact_pointer_pos() {
                    // The point the user means is just above the knob (the line they're aiming at).
                    let p = p - egui::vec2(0.0, R * 2.0 + 4.0);
                    if let Some((i, r)) = page_rects.iter().enumerate().min_by(|a, b| {
                        a.1.distance_to_pos(p).total_cmp(&b.1.distance_to_pos(p))
                    }) {
                        let local = ((p - r.min) / scale).to_pos2();
                        if let Some(hit_pos) = self.layout.hit(i, local.x, local.y, StoryRef::Body) {
                            // The other end stays put; this end follows the finger.
                            let other = ends[1 - which].clone();
                            self.session.sel = Selection { anchor: other, focus: hit_pos };
                            self.refresh_fmt();
                        }
                    }
                }
            }
        }
    }

    /// Ask the worker for page `i` if what's on screen is missing, blurry or out of date.
    fn request_page(&mut self, i: usize, visible: bool, want_px: f32, pixels_per_point: f32) {
        let want_px = want_px.round().max(1.0) as u32;
        let off_by = |px: u32| (px as f32 - want_px as f32).abs() / want_px as f32;
        let cur_gen = self.generation;
        let stale = self.textures[i].as_ref().is_none_or(|t| off_by(t.px) > 0.25 || t.generation < cur_gen);
        let asked = self.in_flight[i].is_some_and(|(px, g)| off_by(px) <= 0.25 && g == cur_gen);
        if !stale || asked || !visible {
            return;
        }
        let doc = match &self.snapshot {
            Some((g, d)) if *g == cur_gen => Arc::clone(d),
            _ => {
                let d = Arc::new(self.session.doc.clone());
                self.snapshot = Some((cur_gen, Arc::clone(&d)));
                d
            }
        };
        if let Some(w) = &self.worker {
            let job = Job { page: i, scale: pixels_per_point, px: want_px, generation: cur_gen, doc, layout: Arc::clone(&self.layout), dark: self.dark_page };
            if w.jobs.send(job).is_ok() {
                self.in_flight[i] = Some((want_px, cur_gen));
            }
        }
    }

    /// Viewer tap on a found barcode: show what it says.
    fn read_barcode_at(&self, page: usize, p: egui::Pos2) -> bool {
        match self.scanner.hit_at(page, [p.x, p.y], 6.0) {
            Some(h) => {
                crate::barcode_scan::show(&h.found);
                true
            }
            None => false,
        }
    }

    /// Long-press: draw the page at 4 px per point on the scan thread and read the code there.
    fn read_spot(&mut self, ctx: &egui::Context, page: usize, pt: [f32; 2]) {
        let (Some(doc), Some(layout)) = (self.snapshot.as_ref().map(|(_, d)| Arc::clone(d)), Some(Arc::clone(&self.layout))) else {
            return;
        };
        let dark = self.dark_page;
        let half = 110.0;
        let rect = [pt[0] - half, pt[1] - half, pt[0] + half, pt[1] + half];
        self.scanner.submit_spot_render(
            ctx,
            page,
            pt,
            Box::new(move || {
                let p = layout.pages.get(page)?;
                let mut opts = wordcraft_render::RenderOptions::default();
                opts.display.dim_header = false;
                opts.dark = dark;
                let scale = 4.0;
                let img = wordcraft_render::render_page(&doc, p, scale, &opts);
                crate::barcode_scan::Scanner::crop(img.width, img.height, &img.pixels, scale, rect)
            }),
        );
        crate::links::note("Looking for a barcode…");
    }

    /// Turn finished page pictures into textures (ignoring ones older than what's shown).
    fn collect_finished(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.worker else { return };
        while let Ok(d) = w.done.try_recv() {
            if self.in_flight.get(d.page).copied().flatten().is_some_and(|(px, g)| px == d.px && g == d.generation) {
                self.in_flight[d.page] = None;
            }
            let Some(slot) = self.textures.get_mut(d.page) else { continue };
            if slot.as_ref().is_some_and(|t| t.generation > d.generation) {
                continue;
            }
            if let Some(p) = self.layout.pages.get(d.page) {
                let unit = p.w / d.img.width.max(1) as f32;
                self.scanner.submit_page(ctx, d.page, d.generation, unit, d.img.width, d.img.height, &d.img.pixels);
            }
            let color = egui::ColorImage::from_rgba_premultiplied([d.img.width as usize, d.img.height as usize], &d.img.pixels);
            let tex = ctx.load_texture(format!("word-page-{}", d.page), color, egui::TextureOptions::LINEAR);
            *slot = Some(PageTex { tex, px: d.px, generation: d.generation });
        }
    }
}

fn ext(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

/// Keep only the file-name part, so a strange name can't write outside the target folder.
pub fn safe_file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    if s.is_empty() { "document".into() } else { s }
}

/// MIME type for the share sheet, by file extension.
pub fn mime_for(name: &str) -> &'static str {
    match ext(name).as_str() {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "doc" => "application/msword",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "pdf" => "application/pdf",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "ppsx" => "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsm" => "application/vnd.ms-excel.sheet.macroEnabled.12",
        "csv" => "text/csv",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WordViewer {
        let dir = std::env::temp_dir().join("document-slate-test");
        std::fs::create_dir_all(&dir).unwrap();
        WordViewer::sample_via_docx(&dir).expect("open sample")
    }

    /// Sample → .docx → read back → lay out → draw page 1. Proves the engine path end to end.
    #[test]
    fn sample_round_trips_through_docx_and_renders() {
        let v = sample();
        assert!(v.page_count() >= 1, "no pages");
        let page = &v.layout.pages[0];
        let img = wordcraft_render::render_page(&v.session.doc, page, 1.0, &wordcraft_render::RenderOptions::default());
        assert!(img.width > 100 && img.height > 100);
        let inked = img.pixels.chunks(4).filter(|p| p[0] < 200).count();
        assert!(inked > 100, "page looks blank ({inked} dark pixels)");
    }

    /// Search finds words in the sample and gives each match a highlight rectangle.
    #[test]
    fn search_finds_and_places_matches() {
        let mut v = sample();
        v.query = "studio".into();
        v.run_search();
        assert!(v.hits.len() >= 2, "expected several matches, got {}", v.hits.len());
        for rects in &v.hits {
            for (page, r) in rects {
                assert!(*page < v.page_count());
                assert!(r.width() > 1.0 && r.height() > 1.0);
            }
        }
        v.query = "zzzz-not-there".into();
        v.run_search();
        assert!(v.hits.is_empty());
    }

    /// Tap → caret, type, bold, undo, save, reopen: the edit path end to end.
    #[test]
    fn tap_type_format_undo_save() {
        let mut v = sample();
        v.set_editing(true);
        let gen0 = v.generation;
        // Tap near the top-left of the body text on page 1.
        v.place_caret(0, 80.0, 120.0);
        assert!(v.layout.caret(&v.session.sel.focus).is_some(), "tap gave no caret");
        v.run("text.insert", serde_json::json!({ "text": "Hello Slate " }));
        assert!(v.dirty);
        assert!(v.generation > gen0);
        // Select the typed words and make them bold.
        v.run("caret.wordLeft", serde_json::json!({}));
        v.run("select.word", serde_json::json!({}));
        v.run("format.bold", serde_json::json!({}));
        assert!(v.fmt_on("bold"), "bold not on after format.bold: {}", v.fmt);
        v.run("edit.undo", serde_json::json!({}));
        assert!(!v.fmt_on("bold"), "undo didn't remove bold");
        // Save and reopen: the typed text survives.
        let dir = std::env::temp_dir().join("document-slate-test");
        let out = dir.join("edited.docx");
        v.save_to(&out).expect("save");
        assert!(!v.dirty);
        let bytes = std::fs::read(&out).unwrap();
        let mut v2 = WordViewer::open_bytes("edited.docx", &bytes).expect("reopen");
        v2.query = "Hello Slate".into();
        v2.run_search();
        assert_eq!(v2.hits.len(), 1, "typed text not found after save + reopen");
    }
    /// Review tab: find a misspelling, apply a suggestion, count words, replace all, track a change.
    #[test]
    fn review_spelling_count_replace_track() {
        let mut v = WordViewer::open_bytes("t.txt", b"Thsi sentence has a wrnog word.").expect("open");
        v.set_editing(true);
        // Next mistake: selects "Thsi" and offers "This".
        assert!(v.next_mistake(), "no mistake found");
        let issue = v.issue.clone().expect("issue");
        assert_eq!(issue.word, "Thsi");
        assert!(issue.suggestions.iter().any(|s| s == "This"), "suggestions: {:?}", issue.suggestions);
        v.apply_suggestion("This");
        assert!(v.dirty);
        // Word count of the whole document (collapse the selection first).
        v.run("caret.docStart", serde_json::json!({}));
        let wc = v.word_count_line();
        assert!(wc.starts_with("Document: 6 words"), "{wc}");
        // Replace all.
        v.replace_find = "wrnog".into();
        v.replace_with = "wrong".into();
        assert_eq!(v.replace_all(), "Replaced 1");
        // After both fixes there are no mistakes left.
        v.run("caret.docStart", serde_json::json!({}));
        assert!(!v.next_mistake(), "still a mistake: {:?}", v.issue);
        // Track changes: on, type, the change is there; accept all keeps the text.
        v.run("review.trackChanges", serde_json::json!({ "value": true }));
        assert!(v.tracking());
        v.run("caret.docEnd", serde_json::json!({}));
        v.run("text.insert", serde_json::json!({ "text": " Added." }));
        v.run("review.acceptAll", serde_json::json!({}));
        v.query = "Added".into();
        v.run_search();
        assert_eq!(v.hits.len(), 1);
    }
    /// Insert tab: table, link, page break, page numbers, header, footer, comment, symbol, picture.
    #[test]
    fn insert_everything() {
        let mut v = WordViewer::open_bytes("t.txt", b"First paragraph.").expect("open");
        v.set_editing(true);
        v.run("caret.docEnd", serde_json::json!({}));
        let pages0 = v.page_count();
        assert!(v.run("insert.table", serde_json::json!({ "rows": 2, "cols": 3 })).is_some(), "table");
        v.run("caret.docEnd", serde_json::json!({}));
        assert!(v.run("insert.link", serde_json::json!({ "url": "https://example.org", "text": "Example link" })).is_some(), "link");
        assert!(v.run("insert.symbol", serde_json::json!({ "char": "©" })).is_some(), "symbol");
        assert!(v.run("insert.pageBreak", serde_json::json!({})).is_some(), "page break");
        assert_eq!(v.page_count(), pages0 + 1, "page break didn't add a page");
        assert!(v.run("insert.pageNumber", serde_json::json!({ "position": "bottom", "align": "center" })).is_some(), "page numbers");
        assert!(v.run("insert.header", serde_json::json!({ "text": "My header" })).is_some(), "header");
        assert!(v.run("insert.footer", serde_json::json!({ "text": "My footer" })).is_some(), "footer");
        v.run("insert.closeHeader", serde_json::json!({}));
        // Comment on a selected word.
        v.run("caret.docStart", serde_json::json!({}));
        v.run("select.word", serde_json::json!({}));
        assert!(v.run("review.newComment", serde_json::json!({ "text": "Check this" })).is_some(), "comment");
        let comments = v.session.run("review.comments", &serde_json::json!({})).expect("comments");
        assert!(comments.to_string().contains("Check this"), "comment missing: {comments}");
        // Picture: a real PNG (page 1 rendered small).
        let dir = std::env::temp_dir().join("document-slate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("pic.png");
        let img = wordcraft_render::render_page(&v.session.doc, &v.layout.pages[0], 0.2, &wordcraft_render::RenderOptions::default());
        std::fs::write(&png, img.to_png()).unwrap();
        v.run("caret.docEnd", serde_json::json!({}));
        assert!(v.run("insert.picture", serde_json::json!({ "path": png.to_string_lossy() })).is_some(), "picture");
        // Text that went into the body is findable; the document saves and reopens.
        v.query = "Example link".into();
        v.run_search();
        assert_eq!(v.hits.len(), 1, "link text not found");
        let out = dir.join("inserted.docx");
        v.save_to(&out).expect("save");
        let v2 = WordViewer::open_bytes("inserted.docx", &std::fs::read(&out).unwrap()).expect("reopen");
        assert!(v2.page_count() >= 2);
    }
    /// Objects: insert a chart and a shape, select by tapping, change chart type and data, move,
    /// wrap, recolour; save to .docx and reopen: the chart keeps its edited data (full fidelity).
    #[test]
    fn object_editing() {
        let mut v = WordViewer::open_bytes("t.txt", b"Intro paragraph.").expect("open");
        v.set_editing(true);
        v.run("caret.docEnd", serde_json::json!({}));
        assert!(v.run("insert.chart", serde_json::json!({ "type": "column", "data": {
            "categories": ["Q1", "Q2"], "series": [{ "name": "Sales", "values": [3, 5] }] } })).is_some(), "chart");
        // Insert leaves the chart selected.
        assert_eq!(v.selected_object().map(|o| o.kind), Some(ObjKind::Chart), "chart not selected after insert");
        let sel = v.selected_object().unwrap().clone();
        // Deselect, then select it again by tapping its middle.
        v.run("caret.docStart", serde_json::json!({}));
        assert!(v.selected_object().is_none());
        let c = sel.rect.center();
        assert!(v.try_select_object(sel.page, c.x, c.y), "tap didn't select the chart");
        assert_eq!(v.selected_object().map(|o| o.kind), Some(ObjKind::Chart));
        // Type + data through the grid helpers.
        assert!(v.run("chart.type", serde_json::json!({ "type": "line" })).is_some(), "chart.type");
        let mut t = v.chart_table().expect("chart table");
        assert_eq!(t.categories, vec!["Q1", "Q2"]);
        t.categories.push("Q3".into());
        t.series[0].1.push("8".into());
        v.apply_chart_table(&t);
        assert_eq!(v.chart_table().unwrap().series[0].1, vec!["3", "5", "8"]);
        assert!(v.run("chart.legend", serde_json::json!({ "pos": "bottom" })).is_some(), "legend");
        assert!(v.run("chart.title", serde_json::json!({ "text": "Quarterly" })).is_some(), "title");
        // Float it and move it.
        // The engine can't move/resize/wrap charts yet: the Wrap tool is refused (and hidden in the UI).
        assert!(v.run("arrange.wrap", serde_json::json!({ "wrap": "square" })).is_none());
        // A shape: fill + outline + rotate.
        v.run("caret.docEnd", serde_json::json!({}));
        assert!(v.run("insert.shape", serde_json::json!({ "kind": "ellipse", "width": 100, "height": 50 })).is_some(), "shape");
        assert_eq!(v.selected_object().map(|o| o.kind), Some(ObjKind::Shape));
        assert!(v.run("shape.fill", serde_json::json!({ "color": "2F6DB5" })).is_some(), "fill");
        assert!(v.run("shape.outline", serde_json::json!({ "width": 2.0 })).is_some(), "outline");
        assert!(v.run("arrange.rotate", serde_json::json!({ "direction": "right" })).is_some(), "rotate");
        // Float it and move it.
        assert!(v.run("arrange.wrap", serde_json::json!({ "wrap": "square" })).is_some(), "wrap");
        assert!(!v.selected_object().unwrap().inline, "still inline after wrap square");
        assert!(v.run("arrange.bounds", serde_json::json!({ "x": 200.0, "y": 300.0, "page": 0 })).is_some(), "move");
        assert!(v.run("arrange.bounds", serde_json::json!({ "width": 150.0, "height": 75.0 })).is_some(), "resize");
        // Save and reopen: the chart is still a chart with the edited data.
        let dir = std::env::temp_dir().join("document-slate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("objects.docx");
        v.save_to(&out).expect("save");
        let mut v2 = WordViewer::open_bytes("objects.docx", &std::fs::read(&out).unwrap()).expect("reopen");
        v2.set_editing(true);
        let kinds: Vec<_> = {
            let mut k = vec![];
            for p in 0..v2.page_count() {
                for i in 0..40 { for j in 0..40 {
                    let (x, y) = (i as f32 * 15.0, j as f32 * 20.0);
                    if v2.try_select_object(p, x, y) { k.push(v2.selected_object().unwrap().kind); }
                }}
            }
            k
        };
        assert!(kinds.contains(&ObjKind::Chart), "chart lost on save: {kinds:?}");
        assert!(kinds.contains(&ObjKind::Shape), "shape lost on save: {kinds:?}");
        let chart_at = (0..v2.page_count()).find_map(|p| (0..1600).find_map(|n| {
            let (x, y) = ((n / 40) as f32 * 15.0, (n % 40) as f32 * 20.0);
            (v2.try_select_object(p, x, y) && v2.selected_object().unwrap().kind == ObjKind::Chart).then_some(())
        }));
        assert!(chart_at.is_some());
        assert_eq!(v2.chart_table().unwrap().series[0].1, vec!["3", "5", "8"], "chart data lost on save");
    }
    /// Full-fidelity guard: a DOCX save keeps everything (no losses, marked saved); a save that
    /// would drop objects (plain text) is reported and leaves the document unsaved.
    #[test]
    fn fidelity_guard() {
        let dir = std::env::temp_dir().join("document-slate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut v = sample();
        v.set_editing(true);
        v.run("caret.docEnd", serde_json::json!({}));
        v.run("insert.chart", serde_json::json!({ "type": "pie" }));
        v.run("caret.docEnd", serde_json::json!({}));
        v.run("insert.table", serde_json::json!({ "rows": 2, "cols": 2 }));
        let before = fidelity::Census::of(&v.session.doc);
        assert!(before.charts >= 1 && before.tables >= 1, "{before:?}");
        let ok = v.save_checked(&dir.join("fid.docx")).expect("save docx");
        assert!(ok.is_empty(), "docx round trip reported losses: {ok:?}");
        assert!(!v.dirty);
        // Saving the same document again (now the baseline is the saved file) is still clean.
        v.run("caret.docEnd", serde_json::json!({}));
        v.run("text.insert", serde_json::json!({ "text": "more" }));
        assert!(v.save_checked(&dir.join("fid2.docx")).unwrap().is_empty());
        // Plain text can't hold a chart or a table: reported, and still unsaved.
        v.run("text.insert", serde_json::json!({ "text": "!" }));
        let lost = v.save_checked(&dir.join("fid.txt")).expect("save txt");
        assert!(lost.iter().any(|l| l.contains("chart")), "txt losses: {lost:?}");
        assert!(v.dirty, "a lossy save must not mark the document saved");
    }
    /// Table row tools + Text tab size: caret in a table is detected; rows/columns are added and
    /// deleted; font size is set and read back.
    #[test]
    fn table_rows_columns_and_font_size() {
        let mut v = WordViewer::open_bytes("t.txt", b"Before table.").expect("open");
        v.set_editing(true);
        v.run("caret.docEnd", serde_json::json!({}));
        assert!(!v.in_table());
        v.run("insert.table", serde_json::json!({ "rows": 2, "cols": 2 }));
        assert!(v.in_table(), "caret should be in the new table: {:?}", v.session.sel.focus);
        let shape = |v: &WordViewer| {
            let c = fidelity::Census::of(&v.session.doc);
            assert_eq!(c.tables, 1);
            let t = v.session.doc.body.iter().find_map(|b| match &**b { wordcraft_engine::doc::Block::Table(t) => Some(t), _ => None }).expect("table");
            (t.rows.len(), t.rows[0].cells.len())
        };
        assert_eq!(shape(&v), (2, 2));
        assert!(v.run("table.insertRowBelow", serde_json::json!({})).is_some());
        assert!(v.run("table.insertRowAbove", serde_json::json!({})).is_some());
        assert!(v.run("table.insertColumnRight", serde_json::json!({})).is_some());
        assert!(v.run("table.insertColumnLeft", serde_json::json!({})).is_some());
        assert_eq!(shape(&v), (4, 4));
        assert!(v.run("table.deleteRow", serde_json::json!({})).is_some());
        assert!(v.run("table.deleteColumn", serde_json::json!({})).is_some());
        assert_eq!(shape(&v), (3, 3));
        v.run("edit.undo", serde_json::json!({}));
        assert_eq!(shape(&v), (3, 4), "undo brings the column back");
        // Font size.
        v.run("text.insert", serde_json::json!({ "text": "cell" }));
        v.run("select.word", serde_json::json!({}));
        assert!(v.run("format.size", serde_json::json!({ "size": 18.0 })).is_some());
        assert_eq!(v.font_size(), Some(18.0), "fmt: {}", v.fmt);
        assert!(v.run("table.deleteTable", serde_json::json!({})).is_some());
        assert!(!v.in_table());
    }
    /// Text tab extras: font, colour, highlight, superscript, change case, paragraph spacing.
    #[test]
    fn text_tab_extras() {
        let mut v = WordViewer::open_bytes("t.txt", b"hello world").expect("open");
        v.set_editing(true);
        v.run("select.all", serde_json::json!({}));
        assert!(v.run("format.font", serde_json::json!({ "name": "Source Serif 4" })).is_some());
        assert_eq!(v.font_name(), "Source Serif 4");
        assert!(v.run("format.color", serde_json::json!({ "color": "C00000" })).is_some());
        assert!(v.fmt.to_string().to_ascii_uppercase().contains("C00000"), "color not applied: {}", v.fmt);
        assert!(v.run("format.highlight", serde_json::json!({ "color": "yellow" })).is_some());
        assert!(v.fmt.to_string().contains("FFFF00"), "highlight not applied: {}", v.fmt);
        assert!(v.run("format.superscript", serde_json::json!({})).is_some());
        assert!(v.fmt_on("superscript"), "superscript not on: {}", v.fmt);
        assert!(v.run("format.changeCase", serde_json::json!({ "mode": "upper" })).is_some());
        v.query = "HELLO WORLD".into();
        v.run_search();
        assert_eq!(v.hits.len(), 1, "upper case not applied");
        assert!(v.run("para.spacing", serde_json::json!({ "before": 12.0, "after": 6.0 })).is_some());
    }
    /// Viewer: a tap on linked text finds its address (web link -> pop-up), a link to a bookmark
    /// scrolls there; a tap beside the link finds nothing. Survives .docx save + reopen.
    #[test]
    fn viewer_link_taps() {
        let mut v = WordViewer::new_blank("links.docx");
        v.set_editing(true);
        v.run("text.insert", serde_json::json!({ "text": "Target here" }));
        v.run("caret.home", serde_json::json!({}));
        assert!(v.run("insert.bookmark", serde_json::json!({ "name": "Spot" })).is_some(), "bookmark");
        v.run("caret.end", serde_json::json!({}));
        v.run("text.newParagraph", serde_json::json!({}));
        for _ in 0..40 {
            v.run("text.newParagraph", serde_json::json!({}));
        }
        v.run("text.insert", serde_json::json!({ "text": "See " }));
        assert!(v.run("insert.link", serde_json::json!({ "url": "https://example.org/a", "text": "Example link" })).is_some(), "link");
        v.run("text.insert", serde_json::json!({ "text": " and " }));
        assert!(v.run("insert.link", serde_json::json!({ "bookmark": "Spot", "text": "Go back" })).is_some(), "bookmark link");
        let dir = std::env::temp_dir().join("document-slate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("links.docx");
        v.save_to(&out).expect("save");
        let mut v = WordViewer::open_bytes("links.docx", &std::fs::read(&out).unwrap()).expect("reopen");

        let centre = |v: &mut WordViewer, text: &str| {
            v.query = text.into();
            v.run_search();
            let (page, r) = v.hits[0][0];
            (page, r.center())
        };
        let (page, c) = centre(&mut v, "Example link");
        assert_eq!(v.link_at(page, c.x, c.y).as_deref(), Some("https://example.org/a"));
        let (page, see) = centre(&mut v, "See");
        assert_eq!(v.link_at(page, see.x, see.y), None, "plain text isn't a link");
        assert_eq!(v.link_at(page, c.x, c.y + 40.0), None, "below the link isn't the link");

        let (page, back) = centre(&mut v, "Go back");
        let link = v.link_at(page, back.x, back.y).expect("bookmark link");
        assert_eq!(link, "#Spot");
        v.jump_to = None;
        let sel = v.session.sel.clone();
        assert!(v.follow_link_at(page, back.x, back.y));
        assert_eq!(v.jump_to.map(|(p, _)| p), Some(0), "didn't jump to the bookmark");
        assert_eq!(v.session.sel, sel, "viewer selection changed");
        assert!(!v.dirty, "following a link changed the document");
    }

    /// Viewer extras: outline headings, jump, read mode reflow, dark page render.
    #[test]
    fn viewer_extras() {
        let mut v = sample();
        let heads = v.headings();
        assert!(heads.iter().any(|(_, _, t)| t.contains("share a studio")), "headings: {heads:?}");
        let (block, _, _) = heads.last().cloned().expect("a heading");
        v.go_to_block(block);
        assert!(v.jump_to.is_some(), "no jump target");
        // Read mode: a 360 pt column (phone width) instead of the page width.
        let page_w = v.layout.pages[0].w;
        v.set_read_mode(true);
        v.session.view.read_width = 360.0;
        v.layout = v.session.layout();
        assert!(v.layout.pages[0].w < page_w, "read mode didn't reflow: {} vs {page_w}", v.layout.pages[0].w);
        v.set_read_mode(false);
        assert!((v.layout.pages[0].w - page_w).abs() < 1.0);
        // Dark page: the renderer's dark mode paints a dark sheet.
        let mut opts = wordcraft_render::RenderOptions::default();
        opts.dark = true;
        let img = wordcraft_render::render_page(&v.session.doc, &v.layout.pages[0], 0.3, &opts);
        assert!(img.pixels[0] < 128, "dark page corner isn't dark: {}", img.pixels[0]);
        assert!(!v.dirty, "viewing changed the document");
    }
}

/// "1 page", "3 pages".
fn plural(n: u64, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// Text, not binary: a UTF-16 byte-order mark, or no NUL byte in the first 64 KB.
fn looks_like_text(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) || !bytes[..bytes.len().min(65536)].contains(&0)
}

#[cfg(test)]
mod open_text_tests {
    use super::*;

    /// Any text file opens in Word Slate, whatever its extension.
    #[test]
    fn opens_any_text_file() {
        let files: [(&str, &[u8]); 7] = [
            ("data.json", br#"{"name": "Slate", "items": [1, 2, 3]}"#),
            ("notes.md", b"# Title\n\nSome *Markdown* text."),
            ("app.log", b"12:00 started\n12:01 done\n"),
            ("config.yaml", b"key: value\nlist:\n  - a\n"),
            ("feed.xml", b"<?xml version=\"1.0\"?><rss><item>Hello</item></rss>"),
            ("script.py", b"print('hello')\n"),
            ("noextension", b"just some words"),
        ];
        for (name, bytes) in files {
            let v = WordViewer::open_bytes(name, bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(v.page_count() >= 1, "{name}");
        }
        assert!(WordViewer::open_bytes("blob.bin", &[0u8, 1, 2, 0, 255]).is_err(), "binary junk is refused");
    }

    /// Files with an extension that says nothing go to the app their content belongs to.
    #[test]
    fn routes_by_content_when_extension_is_unknown() {
        use crate::home::tool_for_file;
        use crate::theme::Tool;
        assert_eq!(tool_for_file("report", b"%PDF-1.7 ..."), Tool::Page);
        assert_eq!(tool_for_file("x.download", b"PK\x03\x04....ppt/presentation.xml"), Tool::Deck);
        assert_eq!(tool_for_file("x.bin", b"PK\x03\x04....xl/workbook.xml"), Tool::Grid);
        assert_eq!(tool_for_file("letter.docx", b"PK\x03\x04....xl/workbook.xml"), Tool::Word);
        assert_eq!(tool_for_file("data.json", b"{}"), Tool::Word);
        assert_eq!(tool_for_file("book.xltm", b""), Tool::Grid);
    }
}
