//! # slate-pdf — Page Slate's PDF back-end
//!
//! A small wrapper around the PdfCraft engine (`PageSlate/crates/engine`, from ArtCraft's open
//! source PdfCraft). It knows nothing about egui, windows or Android: the app calls these plain
//! functions and draws the results itself.
//!
//! ## How it works (in plain words)
//! * [`PdfDoc`] holds one open PDF. Inside is the engine's `Session`, which keeps the file, an
//!   undo history and the list of edits.
//! * Every change (a highlight, a rotated page, a filled field…) is one engine "edit". The engine
//!   applies it to a copy-on-write object graph, so undo is cheap and nothing it does not
//!   understand is thrown away.
//! * [`PdfDoc::save_bytes`] writes an **incremental update**: the original file's bytes, untouched,
//!   followed by the changes. That is the "full fidelity" rule: whatever was in the file stays.
//!
//! ## Coordinates used everywhere in this API
//! **"Page view space"**: points (1/72 inch), origin at the **top-left** of the page *as it is
//! shown* (after the page's own rotation), x to the right, y **down**. A page rendered at
//! `scale` has pixel `(x * scale, y * scale)`. So the app can take a finger position on the
//! rendered picture, divide by the scale, and pass it straight in. Conversion to the PDF's own
//! coordinates (origin bottom-left, y up, unrotated) happens in here.
//!
//! ## Deliberately left out
//! * PDF JavaScript is switched **off** when a document is opened (project rule) — forms with
//!   calculation scripts won't recalculate.
//! * No AI / automation bridge, no network, no OCR calls.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;

use pdfcraft_engine::{DocId, Edit, Session};

mod annotate;
mod certs;
mod content;
mod file;
mod fill_sign;
mod geom;
mod pages;
mod types;
mod view;

pub use certs::*;
pub use types::*;
pub use view::PageSnapshot;

/// One open PDF document.
pub struct PdfDoc {
    session: Session,
    id: DocId,
    /// Name shown to the user (e.g. "report.pdf"). Only used for messages.
    name: String,
    /// A cached single-thread renderer and the edit generation it was made for.
    snapshot: Option<(u64, PageSnapshot)>,
    /// The name written as the author of new comments (empty until the app sets one).
    author: String,
}

impl PdfDoc {
    // ------------------------------------------------------------------ opening

    /// Open a PDF from its bytes. `password` is needed for files locked with an open password;
    /// call once without, and if you get [`PdfError::NeedsPassword`] ask the user and try again.
    pub fn open_bytes(bytes: Vec<u8>, password: Option<&str>) -> Result<PdfDoc, PdfError> {
        Self::open_named("document.pdf", bytes, password)
    }

    /// Like [`PdfDoc::open_bytes`], with the file's name (used in messages and as the default
    /// name of extracted files).
    pub fn open_named(name: &str, bytes: Vec<u8>, password: Option<&str>) -> Result<PdfDoc, PdfError> {
        let mut session = new_session();
        let id = session.open(name, None, Arc::new(bytes), password).map_err(PdfError::from_open)?;
        Ok(PdfDoc { session, id, name: name.to_string(), snapshot: None, author: String::new() })
    }

    /// A new, empty PDF with `pages` blank pages of `width` x `height` points
    /// (A4 = 595.28 x 841.89, US Letter = 612 x 792).
    pub fn create_blank(width: f64, height: f64, pages: usize) -> Result<PdfDoc, PdfError> {
        let mut session = new_session();
        let bytes = session.create_blank(width, height, pages.max(1)).map_err(PdfError::from_edit)?;
        let id = session.open_new("Untitled.pdf", bytes).map_err(PdfError::from_open)?;
        Ok(PdfDoc { session, id, name: "Untitled.pdf".into(), snapshot: None, author: String::new() })
    }

    /// Merge several PDFs (in order) into one new PDF file. Each entry is (file name, bytes).
    /// Form fields with clashing names are renamed so they stay separate.
    pub fn merge(sources: &[(String, Vec<u8>)]) -> Result<Vec<u8>, PdfError> {
        let session = new_session();
        let list: Vec<(String, Arc<Vec<u8>>)> = sources.iter().map(|(n, b)| (n.clone(), Arc::new(b.clone()))).collect();
        session.combine(&list).map(|b| b.as_ref().clone()).map_err(PdfError::from_edit)
    }

    // ------------------------------------------------------------------ basic state

    /// The engine's view of this document (read only). Escape hatch for features this wrapper
    /// doesn't cover yet; prefer the methods below.
    pub fn engine_document(&self) -> Option<&pdfcraft_engine::Document> {
        self.session.get(self.id)
    }

    pub(crate) fn doc(&self) -> Result<&pdfcraft_engine::Document, PdfError> {
        self.session.get(self.id).ok_or_else(|| PdfError::Other("the document is no longer open".into()))
    }

    /// Run one engine edit. Every change in this crate goes through here, so undo and the
    /// "unsaved changes" flag always work.
    pub(crate) fn apply(&mut self, edit: Edit) -> Result<(), PdfError> {
        self.session.apply(self.id, edit).map_err(PdfError::from_edit)
    }

    /// Set the name written as the author of new comments and signatures.
    pub fn set_author(&mut self, author: &str) {
        self.author = author.to_string();
    }

    /// The file name given when opening.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `true` when there are changes that haven't been saved.
    pub fn is_dirty(&self) -> bool {
        self.doc().map(|d| d.dirty).unwrap_or(false)
    }

    /// Why the document can't be edited (for example a permissions password), or `None` when
    /// it can.
    pub fn read_only_reason(&self) -> Option<String> {
        let d = self.doc().ok()?;
        if d.editable() { None } else { Some(d.read_only_reason.clone().unwrap_or_else(|| "this document can't be edited".into())) }
    }

    /// A number that changes after every edit, undo, redo or save. Use it to know when cached
    /// page pictures are out of date.
    pub fn generation(&self) -> u64 {
        self.doc().map(|d| d.edit_generation()).unwrap_or(0)
    }

    /// Messages the last edit wants the user to see (e.g. "a character couldn't be drawn").
    pub fn last_edit_notices(&self) -> Vec<String> {
        self.doc().map(|d| d.edit_notices.iter().map(ToString::to_string).collect()).unwrap_or_default()
    }

    // ------------------------------------------------------------------ undo / redo

    /// The name of the step Undo would take back ("Rotate pages"…), if any.
    pub fn can_undo(&self) -> Option<String> {
        self.doc().ok()?.can_undo().map(str::to_owned)
    }

    pub fn can_redo(&self) -> Option<String> {
        self.doc().ok()?.can_redo().map(str::to_owned)
    }

    /// Undo the last edit; returns its name.
    pub fn undo(&mut self) -> Result<String, PdfError> {
        self.session.undo(self.id).map(|l| l.to_string()).map_err(PdfError::from_edit)
    }

    /// Redo the last undone edit; returns its name.
    pub fn redo(&mut self) -> Result<String, PdfError> {
        self.session.redo(self.id).map(|l| l.to_string()).map_err(PdfError::from_edit)
    }

    // ------------------------------------------------------------------ saving

    /// The bytes to write for **Save**. This is an incremental update: the file as opened (or
    /// last saved), byte for byte, plus the changes appended — so nothing in the original is
    /// lost, and existing digital signatures stay valid. The bytes are checked by reopening
    /// them before they're returned. After writing them to disk successfully, call
    /// [`PdfDoc::mark_saved`].
    ///
    /// `notes` lists anything the user should know about the saved file (repairs the engine
    /// made to a damaged file, parts of an XFA form it couldn't keep…). An empty list means
    /// nothing was changed besides the user's own edits.
    pub fn save_bytes(&self) -> Result<SaveOutput, PdfError> {
        let bytes = self.session.save_bytes(self.id).map_err(PdfError::from_edit)?;
        self.session.check_save(self.id, &bytes).map_err(PdfError::from_edit)?;
        let doc = self.doc()?;
        let mut notes: Vec<String> = Vec::new();
        notes.extend(doc.repair_log().into_iter().map(|r| format!("Repaired: {r}")));
        notes.extend(doc.unreadable_objects().into_iter().map(|o| format!("Couldn't read (kept as is): {o}")));
        notes.extend(doc.xfa_warnings.iter().cloned());
        // Signed files, and changes such as a new password, are written differently: say so.
        let incremental = bytes.len() >= doc.saved_bytes().len() && bytes.starts_with(doc.saved_bytes().as_slice());
        Ok(SaveOutput { bytes: bytes.as_ref().clone(), incremental, notes })
    }

    /// A compact full rewrite (drops old revisions and unused objects). Smaller, but breaks
    /// existing digital signatures, so it is refused for signed files. Use for "Save as… (reduced)".
    pub fn save_full_bytes(&self) -> Result<Vec<u8>, PdfError> {
        self.session.save_full_bytes(self.id).map(|b| b.as_ref().clone()).map_err(PdfError::from_edit)
    }

    /// Tell the engine the bytes from [`PdfDoc::save_bytes`] were written. The next save then
    /// appends only newer edits. Undo history is kept.
    pub fn mark_saved(&mut self, saved: &[u8]) -> Result<(), PdfError> {
        self.session.mark_saved(self.id, Arc::new(saved.to_vec()), None).map_err(PdfError::from_edit)?;
        self.snapshot = None;
        Ok(())
    }

    /// The working file right now: what Save would write, minus the save date. Handy for
    /// Share or for a quick copy; prefer [`PdfDoc::save_bytes`] for real saving.
    pub fn current_bytes(&self) -> Vec<u8> {
        self.doc().map(|d| d.bytes.as_ref().clone()).unwrap_or_default()
    }
}

/// A fresh engine session with PDF JavaScript switched off.
fn new_session() -> Session {
    let mut s = Session::new();
    // Project rule: PDF JavaScript stays off (no scripts run on open, on click, or on save).
    s.set_javascript(false);
    s
}
