//! # slate-deck: the presentations engine for Deck Slate
//!
//! A small wrapper around the DeckCraft engine (from ArtCraft's open-source `deckcraft` repo,
//! cloned in `DeckSlate/`). The Android app will call this; it knows nothing about egui,
//! windows or Android. Everything here is plain Rust: bytes in, pixels and bytes out.
//!
//! How the engine works, in one paragraph: a [`deckcraft_engine::Session`] holds the open
//! presentation. Every change ("new slide", "make bold", "move shape") is a *command* with a
//! text id (`slide.new`, `format.bold`, `shape.move`) and JSON parameters. Running a command
//! through the session makes it undoable and crash-safe (a command that panics is rolled back).
//! [`DeckDoc`] keeps one session with one presentation and gives the app friendly methods for
//! the commands Deck Slate needs. [`DeckDoc::run`] is the escape hatch for any other command.
//!
//! Main pieces:
//! - open / new / save / PDF: this file
//! - rendering slides to pixels: [`render`]
//! - editing helpers (slides, text, insert, design, objects): [`edit`]
//! - touch support (what's under a finger, selection handles, drag gestures): [`touch`]
//! - "will saving lose anything?" checks: [`fidelity`]
//! - hyperlinks and click actions under a finger (read-only): [`links`]
//! - fonts (Android keeps its fonts in /system/fonts, which the engine doesn't look in): [`fonts`]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod edit;
pub mod fidelity;
pub mod fonts;
pub mod links;
pub mod render;
pub mod shapes;
pub mod touch;

use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_engine::cmd::file as engine_file;
use serde_json::{Value, json};

pub use deckcraft_model::{Presentation, ShapeId, SlideId};
pub use fidelity::{FidelityIssue, FidelityReport};
pub use render::{RenderOptions, RenderedSlide};

/// All our functions report errors as plain text the app can show (or log).
pub type DeckResult<T> = Result<T, String>;

/// What kind of file a presentation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    /// PowerPoint 2007+ (`.pptx`, also `.potx` templates, `.ppsx` shows and `.pptm`).
    Pptx,
    /// DeckCraft's own zip+JSON format (`.deckcraft`). Lossless for everything the engine knows.
    Native,
    /// A text outline (`.txt`, `.md`, `.rtf`) turned into slides. Saving can't go back to it.
    Outline,
    /// Made in the app (File > New or the sample).
    New,
}

/// Formats [`DeckDoc::save_bytes`] can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    Pptx,
    /// PowerPoint template.
    Potx,
    /// PowerPoint show (opens straight into the slideshow).
    Ppsx,
    /// DeckCraft's own format: never loses anything the engine understands.
    Native,
    /// Slide titles and text only, as a tab-indented outline. An export, not a save.
    OutlineText,
}

impl SaveFormat {
    /// The engine's name for the format.
    fn engine_name(self) -> &'static str {
        match self {
            SaveFormat::Pptx => "pptx",
            SaveFormat::Potx => "potx",
            SaveFormat::Ppsx => "ppsx",
            SaveFormat::Native => "deckcraft",
            SaveFormat::OutlineText => "outline",
        }
    }
    /// The usual file extension (without the dot).
    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Pptx => "pptx",
            SaveFormat::Potx => "potx",
            SaveFormat::Ppsx => "ppsx",
            SaveFormat::Native => "deckcraft",
            SaveFormat::OutlineText => "txt",
        }
    }
    /// A full save that keeps the presentation (the open document then counts as saved).
    /// Outline text is only an export.
    pub fn is_full_save(self) -> bool {
        !matches!(self, SaveFormat::OutlineText)
    }
    /// The format for a file name's extension (`None` = the engine can't write it, e.g. `.odp`).
    pub fn from_file_name(name: &str) -> Option<SaveFormat> {
        let lower = name.to_ascii_lowercase();
        let ext = lower.rsplit_once('.').map(|(_, e)| e)?;
        Some(match ext {
            "pptx" => SaveFormat::Pptx,
            "potx" => SaveFormat::Potx,
            "ppsx" => SaveFormat::Ppsx,
            "deckcraft" => SaveFormat::Native,
            "txt" => SaveFormat::OutlineText,
            _ => return None,
        })
    }
}

/// The result of a save: the file's bytes plus anything the user should be told.
#[derive(Clone, Debug, Default)]
pub struct SaveOutput {
    pub bytes: Vec<u8>,
    /// Plain-language warnings, e.g. "1 equation would be lost". Empty = nothing known lost.
    /// The project rule is full fidelity, so the app should show these and offer to save a copy
    /// under a new name instead of overwriting the original.
    pub warnings: Vec<String>,
}

/// How the PDF (and printing) lays slides out on paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PdfLayout {
    /// One slide per page, page = slide size.
    Slides,
    /// Notes pages: the slide on top, speaker notes under it.
    Notes,
    /// Handouts: several slides per page. `per_page` must be 1, 2, 3, 4, 6 or 9.
    Handouts { per_page: u8 },
}

/// Options for [`DeckDoc::pdf_bytes`] / [`DeckDoc::export_pdf`].
#[derive(Clone, Debug)]
pub struct PdfOptions {
    pub layout: PdfLayout,
    /// Paper for notes and handouts: "letter", "a4", "legal" or "a3". (Slides use the slide size.)
    pub paper: String,
    pub landscape: bool,
    /// Also print slides marked hidden.
    pub include_hidden: bool,
    /// Picture resolution of the slide artwork (the engine draws each slide as an image and adds
    /// an invisible text layer so the PDF stays searchable). 150-200 is good for printing.
    pub dpi: f64,
    /// Only these slides (0-based); `None` = all.
    pub slides: Option<Vec<usize>>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions { layout: PdfLayout::Slides, paper: "a4".into(), landscape: false, include_hidden: false, dpi: 200.0, slides: None }
    }
}

/// What the app needs to know about one slide (for the slide strip, viewer and slideshow).
#[derive(Clone, Debug, PartialEq)]
pub struct SlideInfo {
    pub index: usize,
    pub id: SlideId,
    /// Text of the slide's title placeholder ("" when it has none).
    pub title: String,
    /// Hidden slides are skipped in the slideshow (and in PDF unless asked).
    pub hidden: bool,
    /// Name of the slide's layout ("Title and Content"…).
    pub layout: String,
    pub has_notes: bool,
    pub comment_count: usize,
}

/// One open presentation.
pub struct DeckDoc {
    session: Session,
    source: SourceFormat,
    /// The file's original bytes (pptx only), kept to check what a save would lose.
    original: Option<Arc<Vec<u8>>>,
    /// The presentation exactly as it was opened, before any edit (for the fidelity check).
    opened: Arc<Presentation>,
    /// Cached result of the fidelity check (it costs one export, so done once, lazily).
    fidelity: Option<FidelityReport>,
    /// Problems found while opening (damaged parts, missing fonts). Shown once by the app.
    open_warnings: Vec<String>,
}

impl DeckDoc {
    /// Open a presentation from its bytes. `name` is the file name (its extension helps tell
    /// outline files apart). Reads `.pptx` / `.potx` / `.ppsx` / `.pptm`, `.deckcraft`, and
    /// `.txt` / `.md` / `.rtf` outlines. `.odp` and old binary `.ppt` are NOT supported by the
    /// engine yet; they return a clear error.
    pub fn open_bytes(name: &str, bytes: &[u8]) -> DeckResult<DeckDoc> {
        if let Some(why) = unsupported_format(name, bytes) {
            return Err(why);
        }
        let (doc, damage) = engine_file::open_checked(name, bytes).map_err(|e| e.to_string())?;
        let source = if deckcraft_pptx::sniff(bytes) {
            SourceFormat::Pptx
        } else if bytes.starts_with(b"PK") {
            SourceFormat::Native
        } else {
            SourceFormat::Outline
        };
        let mut open_warnings = Vec::new();
        if !damage.slides.is_empty() {
            open_warnings.push(format!(
                "{} slide(s) in this file couldn't be read and are missing. Save under a new name to keep the original.",
                damage.slides.len()
            ));
        }
        if !damage.media.is_empty() {
            open_warnings.push(format!("{} picture(s) or media file(s) couldn't be read.", damage.media.len()));
        }
        let mut session = Session::new();
        engine_file::add_opened(&mut session, name, None, doc);
        let st = session.doc().map_err(|e| e.to_string())?;
        if !st.missing_fonts.is_empty() {
            open_warnings.push(format!("Fonts not on this device (a similar font is used): {}", st.missing_fonts.join(", ")));
        }
        let opened = st.doc.clone();
        let original = (source == SourceFormat::Pptx).then(|| Arc::new(bytes.to_vec()));
        let mut d = DeckDoc { session, source, original, opened, fidelity: None, open_warnings };
        d.drain_ui_requests();
        Ok(d)
    }

    /// A new, empty presentation (one title slide, 16:9, the default theme).
    pub fn new_blank() -> DeckResult<DeckDoc> {
        Self::new_with(&json!({}))
    }

    /// A new presentation with one of the engine's built-in themes (see [`DeckDoc::theme_names`]).
    pub fn new_with_theme(theme: &str) -> DeckResult<DeckDoc> {
        Self::new_with(&json!({"theme": theme}))
    }

    /// The engine's own sample deck (handy for testing the viewer).
    pub fn sample() -> DeckResult<DeckDoc> {
        let mut session = Session::new();
        deckcraft_engine::sample::open_sample(&mut session).map_err(|e| e.to_string())?;
        let opened = session.doc().map_err(|e| e.to_string())?.doc.clone();
        Ok(DeckDoc { session, source: SourceFormat::New, original: None, opened, fidelity: None, open_warnings: vec![] })
    }

    fn new_with(params: &Value) -> DeckResult<DeckDoc> {
        let mut session = Session::new();
        session.execute("file.new", params).map_err(|e| e.to_string())?;
        // The engine's default theme and slide master are named after the upstream app (a
        // trademark we must not ship), which would be written into every new .pptx. Rename every
        // string that is exactly "DeckCraft" in each master (theme, color/format schemes, master
        // name) to "Default", via JSON so no field is missed. New decks only.
        if let Ok(st) = session.doc_mut() {
            let doc = std::sync::Arc::make_mut(&mut st.doc);
            for m in &mut doc.masters {
                let Ok(mut v) = serde_json::to_value(&**m) else { continue };
                if rename_strings(&mut v, "DeckCraft", "Default") {
                    if let Ok(renamed) = serde_json::from_value(v) {
                        *m = std::sync::Arc::new(renamed);
                    }
                }
            }
        }
        let opened = session.doc().map_err(|e| e.to_string())?.doc.clone();
        Ok(DeckDoc { session, source: SourceFormat::New, original: None, opened, fidelity: None, open_warnings: vec![] })
    }

    // ----- general -------------------------------------------------------------------------

    /// Run any engine command by id with JSON parameters (see `_cleanup/deck-commands.txt`,
    /// made by `cargo test --test dump_commands -- --ignored`). Undo, crash guard and journal
    /// work as for the typed helpers. Always pass the parameters: some commands open a desktop
    /// dialog when called with none, which here just does nothing.
    pub fn run(&mut self, command: &str, params: Value) -> DeckResult<Value> {
        let r = self.session.execute(command, &params).map_err(|e| e.to_string());
        self.drain_ui_requests();
        r
    }

    /// Messages the engine wants shown (e.g. "the file was repaired"), plus open warnings.
    /// Each message is returned once.
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.open_warnings)
    }

    /// The engine asks a desktop UI for dialogs/panes through "UI requests". We keep only the
    /// plain messages (as warnings for the app) and drop the rest: the app has its own menus.
    fn drain_ui_requests(&mut self) {
        for r in std::mem::take(&mut self.session.ui_requests) {
            if let deckcraft_engine::UiRequest::Message { text } = r {
                self.open_warnings.push(text);
            }
        }
    }

    /// Direct access to the engine session (for things this wrapper doesn't cover yet).
    pub fn session(&self) -> &Session {
        &self.session
    }
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub(crate) fn state(&self) -> DeckResult<&deckcraft_engine::DocState> {
        self.session.doc().map_err(|e| e.to_string())
    }

    /// The presentation as it is now. Cheap (shared pointer): hand it to a background thread to
    /// draw slides with [`render::render_slide`] while the UI keeps going.
    pub fn snapshot(&self) -> DeckResult<Arc<Presentation>> {
        Ok(self.state()?.doc.clone())
    }

    /// Goes up by one with every change (edits, undo, selection). The UI can compare it to know
    /// when to redraw thumbnails and the slide.
    pub fn revision(&self) -> u64 {
        self.state().map(|s| s.revision).unwrap_or(0)
    }

    pub fn source_format(&self) -> SourceFormat {
        self.source
    }

    /// Unsaved changes?
    pub fn is_dirty(&self) -> bool {
        self.state().map(|s| s.is_dirty()).unwrap_or(false)
    }

    /// The author name put on new comments (default "Presenter").
    pub fn set_author(&mut self, name: &str) {
        self.session.prefs.author = name.chars().take(256).collect();
    }

    /// Folder for crash-recovery copies of unsaved work (app-private storage). Optional.
    pub fn set_recovery_dir(&mut self, dir: std::path::PathBuf) {
        self.session.recovery_dir = Some(dir);
    }

    // ----- slides --------------------------------------------------------------------------

    pub fn slide_count(&self) -> usize {
        self.state().map(|s| s.doc.slides.len()).unwrap_or(0)
    }

    /// Slide size in points (1 pt = 1/72 inch). 16:9 is 960 x 540.
    pub fn slide_size(&self) -> (f64, f64) {
        self.state().map(|s| (s.doc.slide_size.width, s.doc.slide_size.height)).unwrap_or((960.0, 540.0))
    }

    /// Info for every slide, in order.
    pub fn slides(&self) -> Vec<SlideInfo> {
        let Ok(st) = self.state() else { return vec![] };
        st.doc
            .slides
            .iter()
            .enumerate()
            .map(|(index, s)| SlideInfo {
                index,
                id: s.id,
                title: s.title(),
                hidden: s.hidden,
                layout: st.doc.layout(s.layout).map(|(_, l)| l.name.clone()).unwrap_or_default(),
                has_notes: !s.notes.text().trim().is_empty(),
                comment_count: s.comments.len(),
            })
            .collect()
    }

    /// Speaker notes of slide `index` as plain text ("" when none or out of range).
    pub fn notes(&self, index: usize) -> String {
        self.state().ok().and_then(|s| s.doc.slides.get(index).map(|s| s.notes_text())).unwrap_or_default()
    }

    /// All the text on a slide (shapes, tables), one line per paragraph, for search/accessibility.
    pub fn slide_text(&self, index: usize) -> String {
        fn walk(shapes: &[deckcraft_model::Shape], out: &mut Vec<String>, depth: usize) {
            if depth > 64 {
                return;
            }
            for sh in shapes {
                if let Some(t) = &sh.text {
                    out.push(t.text());
                }
                if let deckcraft_model::ShapeKind::Table(tb) = &sh.kind {
                    for row in &tb.rows {
                        for c in &row.cells {
                            out.push(c.text.text());
                        }
                    }
                }
                walk(sh.children(), out, depth + 1);
            }
        }
        let Ok(st) = self.state() else { return String::new() };
        let Some(slide) = st.doc.slides.get(index) else { return String::new() };
        let mut out = vec![];
        walk(&slide.shapes, &mut out, 0);
        out.retain(|t| !t.trim().is_empty());
        out.join("\n")
    }

    /// The slides a slideshow plays, in order: every slide that isn't hidden.
    pub fn show_order(&self) -> Vec<usize> {
        self.slides().into_iter().filter(|s| !s.hidden).map(|s| s.index).collect()
    }

    /// The slide being edited (0-based).
    pub fn current_slide(&self) -> usize {
        self.state().map(|s| s.selection.slide).unwrap_or(0)
    }

    /// Make slide `index` the one being edited (clears the object selection).
    pub fn go_to_slide(&mut self, index: usize) -> DeckResult<()> {
        self.run("slide.go", json!({"index": index})).map(|_| ())
    }

    // ----- undo ----------------------------------------------------------------------------

    pub fn undo(&mut self) -> DeckResult<()> {
        self.run("edit.undo", json!({})).map(|_| ())
    }
    pub fn redo(&mut self) -> DeckResult<()> {
        self.run("edit.redo", json!({})).map(|_| ())
    }
    pub fn can_undo(&self) -> bool {
        self.state().map(|s| !s.history.undo.is_empty()).unwrap_or(false)
    }
    pub fn can_redo(&self) -> bool {
        self.state().map(|s| !s.history.redo.is_empty()).unwrap_or(false)
    }

    // ----- saving --------------------------------------------------------------------------

    /// The file's bytes in `format`. Full saves (pptx/potx/ppsx/native) mark the document as
    /// saved; the outline is only an export. `warnings` lists anything known to get lost, from
    /// [`DeckDoc::fidelity_report`] (pptx-family saves) or because the format can't hold it.
    pub fn save_bytes(&mut self, format: SaveFormat) -> DeckResult<SaveOutput> {
        let doc = self.snapshot()?;
        let bytes = engine_file::save_bytes(&doc, format.engine_name()).map_err(|e| e.to_string())?;
        let mut warnings = vec![];
        match format {
            SaveFormat::Pptx | SaveFormat::Potx | SaveFormat::Ppsx => {
                warnings.extend(self.fidelity_report().issues.iter().map(|i| i.message.clone()));
            }
            SaveFormat::Native => {
                // Native keeps everything the engine read, but things the engine dropped while
                // reading the pptx are already gone; same list.
                warnings.extend(self.fidelity_report().issues.iter().map(|i| i.message.clone()));
            }
            SaveFormat::OutlineText => warnings.push("Only the text is exported: pictures, shapes and layout are left out.".into()),
        }
        if format.is_full_save() {
            engine_file::mark_saved(&mut self.session, None).map_err(|e| e.to_string())?;
        }
        Ok(SaveOutput { bytes, warnings })
    }

    /// Like [`DeckDoc::save_bytes`] but never marks the document saved (for writing a copy that
    /// may not be kept, e.g. before a fidelity check). Call [`DeckDoc::mark_saved`] once kept.
    pub fn export_bytes(&mut self, format: SaveFormat) -> DeckResult<SaveOutput> {
        let doc = self.snapshot()?;
        let bytes = engine_file::save_bytes(&doc, format.engine_name()).map_err(|e| e.to_string())?;
        let warnings = match format {
            SaveFormat::OutlineText => vec!["Only the text is exported: pictures, shapes and layout are left out.".into()],
            _ => self.fidelity_report().issues.iter().map(|i| i.message.clone()).collect(),
        };
        Ok(SaveOutput { bytes, warnings })
    }

    /// The current state is what's on disk now (clears the changed mark).
    pub fn mark_saved(&mut self) -> DeckResult<()> {
        engine_file::mark_saved(&mut self.session, None).map_err(|e| e.to_string())
    }

    /// Save to a file path (written to a temporary file first and renamed, so a failed save never
    /// damages the old file). Android apps then copy it to the user's chosen place.
    pub fn save_to_path(&mut self, path: &std::path::Path, format: SaveFormat) -> DeckResult<SaveOutput> {
        let out = self.save_bytes(format)?;
        deckcraft_engine::io::write_file(&path.to_string_lossy(), &out.bytes)?;
        Ok(out)
    }

    /// What saving this presentation would lose compared with the file it came from. Worked out
    /// once (it saves the unedited presentation in memory and compares the two packages), then
    /// cached. Empty for anything that didn't come from a pptx.
    pub fn fidelity_report(&mut self) -> &FidelityReport {
        let (original, opened) = (&self.original, &self.opened);
        self.fidelity.get_or_insert_with(|| match original {
            Some(orig) => fidelity::probe(orig, opened),
            None => FidelityReport::default(),
        })
    }

    /// The pieces the fidelity check needs, for running it on a background thread with
    /// [`fidelity::probe`] (it can take a moment on a big deck). `None` when not from a pptx.
    pub fn fidelity_inputs(&self) -> Option<(Arc<Vec<u8>>, Arc<Presentation>)> {
        self.original.clone().map(|o| (o, self.opened.clone()))
    }

    /// Store a report worked out on a background thread (see [`DeckDoc::fidelity_inputs`]).
    pub fn set_fidelity_report(&mut self, report: FidelityReport) {
        self.fidelity = Some(report);
    }

    // ----- PDF / printing ------------------------------------------------------------------

    /// The presentation as PDF bytes (for printing through Android's PrintManager, or sharing).
    pub fn pdf_bytes(&self, opts: &PdfOptions) -> DeckResult<Vec<u8>> {
        let doc = self.snapshot()?;
        pdf_for(&doc, opts)
    }

    /// Write the PDF to `path` (atomic, like [`DeckDoc::save_to_path`]).
    pub fn export_pdf(&self, path: &std::path::Path, opts: &PdfOptions) -> DeckResult<()> {
        let bytes = self.pdf_bytes(opts)?;
        deckcraft_engine::io::write_file(&path.to_string_lossy(), &bytes)
    }
}

/// PDF bytes for a presentation snapshot (usable from a background thread).
pub fn pdf_for(doc: &Presentation, opts: &PdfOptions) -> DeckResult<Vec<u8>> {
    let (layout, per_page) = match opts.layout {
        PdfLayout::Slides => ("slides", 6),
        PdfLayout::Notes => ("notes", 6),
        PdfLayout::Handouts { per_page } => ("handouts", per_page),
    };
    let mut p = json!({
        "layout": layout,
        "perPage": per_page,
        "paper": opts.paper,
        "orientation": if opts.landscape { "landscape" } else { "portrait" },
        "includeHidden": opts.include_hidden,
        "dpi": opts.dpi,
    });
    if let Some(s) = &opts.slides {
        p["slides"] = json!(s);
    }
    engine_file::pdf_bytes(doc, &p).map_err(|e| e.to_string())
}

/// Formats we recognise but the engine can't open, with a message for the user.
fn unsupported_format(name: &str, bytes: &[u8]) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    // OpenDocument: a zip whose first entry "mimetype" names the type (stored right after the
    // 30-byte zip header and the 8-byte name, so it's readable without unzipping).
    let odp = bytes.starts_with(b"PK") && bytes.get(30..38) == Some(b"mimetype") && {
        let head = &bytes[..bytes.len().min(120)];
        String::from_utf8_lossy(head).contains("opendocument.presentation")
    };
    if odp || lower.ends_with(".odp") || lower.ends_with(".otp") {
        return Some("OpenDocument presentations (.odp) can't be opened yet: the presentation engine only reads PowerPoint (.pptx) files.".into());
    }
    // Old binary PowerPoint 97-2003: a "compound file" (starts D0 CF 11 E0). Encrypted .pptx files
    // use the same container; the engine gives its own "password-protected" message for those.
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) && !deckcraft_pptx::is_encrypted(bytes) {
        return Some("Old PowerPoint 97-2003 files (.ppt) can't be opened yet. Save it as .pptx on a computer first.".into());
    }
    None
}

#[cfg(test)]
mod tests;

/// Replace every JSON string equal to `from` with `to`; true if anything changed.
fn rename_strings(v: &mut Value, from: &str, to: &str) -> bool {
    match v {
        Value::String(s) if s == from => {
            *s = to.into();
            true
        }
        Value::Array(a) => a.iter_mut().fold(false, |c, x| rename_strings(x, from, to) | c),
        Value::Object(o) => o.values_mut().fold(false, |c, x| rename_strings(x, from, to) | c),
        _ => false,
    }
}
