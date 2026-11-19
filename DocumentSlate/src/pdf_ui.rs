//! Page Slate — the PDF screen (viewer + editor) on top of the `slate-pdf` back-end.
//!
//! Same look and behaviour as Word Slate (approved list: docs/PageSlate-features.md):
//! - Viewer: pages stacked vertically, fit to width, pinch zoom; pages are drawn on a background
//!   thread (`pdf-render`, its own copy of the document), so scrolling never waits. ⋮ menu:
//!   Outline, Page thumbnails, Night mode (the engine's reading colours).
//! - Password-protected files open to a password card inside this screen.
//! - Edit mode: icon tool rows + tab strip (Annotate · Edit · Fill & Sign · Pages · File), built
//!   from the shared `chrome` buttons. Touch tools (highlight drag, pen, shapes…) act on the page.
//! - Saving is the engine's incremental update: the original bytes, untouched, plus the changes.
//!
//! Module layout: `view` (page column, worker, search, touch), `tools` (bottom edit bar),
//! `sheets` (outline, thumbnails, comments, signature pad, properties, password), `sig` (saved
//! signatures).

use std::collections::HashMap;
use std::path::Path;

use eframe::egui;
use slate_pdf::{AddedItem, CommentInfo, FormField, PageInfo, PageLink, PdfDoc, PdfError, Properties, TextLineInfo};

use crate::doc_screen::{DocScreen, ScreenCtx};
use crate::edit_bar::FileAction;
use crate::theme::Tool;

mod cert;
mod sheets;
mod sig;
mod tools;
mod view;

#[cfg(test)]
mod tests;

/// A file locked with an open password: the bytes wait here until the right password arrives.
struct Locked {
    bytes: Vec<u8>,
    password: String,
    error: Option<String>,
    focus: bool,
}

/// Lists read from the document, refreshed whenever its edit generation changes.
#[derive(Default)]
struct Cache {
    generation: Option<u64>,
    pages: Vec<PageInfo>,
    comments: Vec<CommentInfo>,
    added: Vec<AddedItem>,
    fields: Vec<FormField>,
    links: Vec<PageLink>,
    /// Existing text lines per page (Edit › Edit text), read when first needed.
    lines: HashMap<usize, Vec<TextLineInfo>>,
    props: Properties,
}

pub struct PdfScreen {
    name: String,
    /// Your name (app preference) for new comments and signatures.
    author: String,
    /// Paper to print on (File › Paper), portrait points; None = each page's own size.
    pub(crate) print_paper: Option<(f32, f32)>,
    doc: Option<PdfDoc>,
    locked: Option<Locked>,
    editing: bool,
    cache: Cache,
    v: view::View,
    e: tools::EditState,
    sheet: sheets::Sheet,
    sigs: sig::Store,
    /// Short message shown as a pill near the bottom: (text, since).
    toast: Option<(String, f64)>,
    toast_new: Option<String>,
    /// The bytes `write_to` last wrote (Save marks exactly these as the saved file).
    last_written: Option<Vec<u8>>,
    /// Pages › Extract: the next Save-as copy holds only these pages.
    extract_pending: Option<Vec<usize>>,
    /// The app's "current file" may no longer be this document (after an extract copy was
    /// adopted): Save then asks where, instead of overwriting.
    uri_stale: bool,
    /// A new document that has never been saved.
    is_new: bool,
    /// Certificate signing, flatten-and-save, and the digital ID manager.
    cert: cert::CertState,
}

impl PdfScreen {
    fn blank_state(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            author: String::new(),
            print_paper: None,
            doc: None,
            locked: None,
            editing: false,
            cache: Cache::default(),
            v: view::View::default(),
            e: tools::EditState::default(),
            sheet: sheets::Sheet::None,
            sigs: sig::Store::default(),
            toast: None,
            toast_new: None,
            last_written: None,
            extract_pending: None,
            uri_stale: false,
            is_new: false,
            cert: cert::CertState::default(),
        }
    }

    /// Open a PDF. A password-protected file doesn't fail: the screen shows a password card.
    pub fn open(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let mut s = Self::blank_state(name);
        match PdfDoc::open_named(name, bytes.to_vec(), None) {
            Ok(doc) => s.set_doc(doc),
            Err(PdfError::NeedsPassword) => {
                s.locked = Some(Locked { bytes: bytes.to_vec(), password: String::new(), error: None, focus: true });
            }
            Err(e) => return Err(e.to_string()),
        }
        #[cfg(debug_assertions)]
        s.demo_setup();
        Ok(s)
    }

    /// Desktop test builds only: `DS_PDF_DEMO=edit:fill,sign,marks` opens straight into a state
    /// for screenshots (edit:<tab>, thumbs, outline, comments, sign, props, password, night,
    /// marks = a few sample annotations, tool:<pen|highlight|edittext>).
    #[cfg(debug_assertions)]
    fn demo_setup(&mut self) {
        let Ok(spec) = std::env::var("DS_PDF_DEMO") else { return };
        for item in spec.split(',').map(str::trim) {
            match item {
                "thumbs" => self.sheet = sheets::Sheet::Thumbs,
                "outline" => self.open_outline(),
                "comments" => self.sheet = sheets::Sheet::comments(),
                "signatures" => self.cert.stage = cert::Stage::List,
                "sign" => self.sheet = sheets::Sheet::sign_pad(false),
                "props" => self.sheet = self.properties_sheet(),
                "password" => self.sheet = sheets::Sheet::Password { pw: String::new(), pw2: String::new(), owner: String::new() },
                "night" => self.v.night = true,
                "marks" => {
                    self.e.tool = tools::TouchTool::Markup(slate_pdf::MarkupKind::Highlight);
                    self.finish_drag(view::Drag { page: 0, points: vec![[74.0, 62.0], [300.0, 64.0]], preview: Vec::new() });
                    self.e.tool = tools::TouchTool::Ink;
                    let pts: Vec<[f32; 2]> = (0..40).map(|i| [300.0 + i as f32 * 4.0, 300.0 + (i as f32 * 0.4).sin() * 20.0]).collect();
                    self.finish_drag(view::Drag { page: 0, points: pts, preview: Vec::new() });
                    self.e.tool = tools::TouchTool::None;
                    self.edit(|d| d.add_note(0, [520.0, 120.0], "Check this paragraph"));
                }
                _ => {
                    if let Some(tab) = item.strip_prefix("edit:") {
                        self.set_editing(true);
                        self.e.tab = match tab {
                            "edit" => tools::Tab::Edit,
                            "fill" => tools::Tab::FillSign,
                            "pages" => tools::Tab::Pages,
                            "file" => tools::Tab::File,
                            _ => tools::Tab::Annotate,
                        };
                    }
                    if let Some(t) = item.strip_prefix("tool:") {
                        self.e.tool = match t {
                            "pen" => tools::TouchTool::Ink,
                            "highlight" => tools::TouchTool::Markup(slate_pdf::MarkupKind::Highlight),
                            "edittext" => tools::TouchTool::EditText,
                            _ => tools::TouchTool::None,
                        };
                    }
                }
            }
        }
    }

    /// A new document: one blank A4 page.
    pub fn new_blank() -> Self {
        let mut s = Self::blank_state("Untitled.pdf");
        match PdfDoc::create_blank(595.28, 841.89, 1) {
            Ok(doc) => s.set_doc(doc),
            Err(e) => s.say(format!("Couldn't create a blank PDF: {e}")),
        }
        s.is_new = true;
        s
    }

    fn set_doc(&mut self, mut doc: PdfDoc) {
        if !self.author.is_empty() {
            doc.set_author(&self.author);
        }
        self.doc = Some(doc);
        self.cache = Cache::default();
        self.v.reset_pages();
        self.refresh();
        if let Some(r) = self.doc.as_ref().and_then(|d| d.read_only_reason()) {
            self.say(format!("Read only: {r}"));
        }
    }

    /// Try the password typed on the lock card.
    fn try_password(&mut self) -> bool {
        let Some(l) = self.locked.as_mut() else { return false };
        match PdfDoc::open_named(&self.name, l.bytes.clone(), Some(&l.password)) {
            Ok(doc) => {
                self.locked = None;
                self.set_doc(doc);
                true
            }
            Err(PdfError::WrongPassword) | Err(PdfError::NeedsPassword) => {
                l.error = Some("That password is incorrect".into());
                l.password.clear();
                l.focus = true;
                false
            }
            Err(e) => {
                l.error = Some(e.to_string());
                false
            }
        }
    }

    /// Show a short message.
    fn say(&mut self, text: impl Into<String>) {
        self.toast_new = Some(text.into());
    }

    /// Re-read the document's lists if it changed.
    fn refresh(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        let generation = doc.generation();
        if self.cache.generation == Some(generation) {
            return;
        }
        let pages = doc.pages();
        if pages != self.cache.pages {
            self.v.pages_changed(pages.len());
        }
        self.cache = Cache {
            generation: Some(generation),
            pages,
            comments: doc.comments(),
            added: doc.added_items(),
            fields: doc.form_fields(),
            links: doc.links(None),
            lines: HashMap::new(),
            props: doc.properties(),
        };
        let n = self.cache.pages.len();
        if self.v.current_page >= n {
            self.v.current_page = n.saturating_sub(1);
        }
    }

    /// Run one change on the document; errors become a message. Returns success.
    fn edit(&mut self, f: impl FnOnce(&mut PdfDoc) -> Result<(), PdfError>) -> bool {
        let Some(doc) = self.doc.as_mut() else { return false };
        let res = f(doc);
        let notices = doc.last_edit_notices();
        let ok = match res {
            Ok(()) => {
                if let Some(n) = notices.first() {
                    self.say(n.clone());
                }
                true
            }
            Err(e) => {
                self.say(capitalise(&e.to_string()));
                false
            }
        };
        self.e.selected = None;
        self.refresh();
        ok
    }

    fn page_count(&self) -> usize {
        self.cache.pages.len()
    }

    /// Text lines of a page (cached until the next change).
    fn lines(&mut self, page: usize) -> &[TextLineInfo] {
        if !self.cache.lines.contains_key(&page) {
            let lines = self.doc.as_ref().and_then(|d| d.text_lines(page).ok()).unwrap_or_default();
            self.cache.lines.insert(page, lines);
        }
        self.cache.lines.get(&page).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Paper for the print copy: the first page's size (portrait), so a uniform document prints 1:1.
    fn paper(&self) -> (f64, f64) {
        if let Some((w, h)) = self.print_paper {
            return (f64::from(w), f64::from(h));
        }
        match self.cache.pages.first() {
            Some(p) if p.width > 72.0 && p.height > 72.0 => {
                let (w, h) = (f64::from(p.width), f64::from(p.height));
                (w.min(h), w.max(h))
            }
            _ => (595.28, 841.89),
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(t) = self.toast_new.take() {
            self.toast = Some((t, now));
        }
        let Some((text, since)) = self.toast.clone() else { return };
        if now - since > 3.5 {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let screen = ctx.content_rect();
        let max_w = (screen.width() - 48.0).max(120.0);
        let font = egui::FontId::proportional(14.0);
        let galley = ctx.fonts_mut(|f| f.layout(text, font, egui::Color32::WHITE, max_w - 28.0));
        let size = galley.size() + egui::vec2(28.0, 18.0);
        let bottom = if self.editing { 150.0 } else { 90.0 };
        let pos = egui::pos2(screen.center().x - size.x / 2.0, screen.bottom() - bottom - size.y);
        egui::Area::new(egui::Id::unique("pdf-toast")).order(egui::Order::Tooltip).fixed_pos(pos).interactable(false).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(r, 12.0, egui::Color32::from_rgba_unmultiplied(30, 33, 36, 235));
            ui.painter().galley(r.center() - galley.size() / 2.0, galley, egui::Color32::WHITE);
        });
    }

    /// The lock card for a password-protected file.
    fn lock_card(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) {
        let mut go = false;
        let Some(l) = self.locked.as_mut() else { return };
        let avail = ui.available_rect_before_wrap();
        let w = (avail.width() - 32.0).min(420.0);
        let card = egui::Rect::from_center_size(egui::pos2(avail.center().x, avail.top() + avail.height() * 0.38), egui::vec2(w, 250.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
            egui::Frame::new().fill(ui.visuals().panel_fill).corner_radius(16).inner_margin(18).show(ui, |ui| {
                ui.set_width(w - 36.0);
                ui.vertical_centered(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                    crate::chrome::icon(ui.painter(), r, "protect", ui.visuals().text_color(), cx.accent);
                    ui.label(egui::RichText::new("This PDF is protected").size(18.0).strong());
                    ui.label(egui::RichText::new("Enter its password to open it.").weak());
                    ui.add_space(8.0);
                    let resp = ui.add(egui::TextEdit::singleline(&mut l.password).password(true).hint_text("Password").desired_width(w - 60.0));
                    if l.focus {
                        resp.request_focus();
                        l.focus = false;
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        go = true;
                    }
                    if let Some(e) = &l.error {
                        ui.label(egui::RichText::new(e).color(egui::Color32::from_rgb(0xD0, 0x40, 0x40)));
                    }
                    ui.add_space(6.0);
                    if ui.add(egui::Button::new(egui::RichText::new("Open").strong()).fill(cx.accent.gamma_multiply(0.35)).min_size(egui::vec2(140.0, 44.0))).clicked() {
                        go = true;
                    }
                });
            });
        });
        if go {
            self.try_password();
        }
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Is `path` inside a folder called `dir`?
impl PdfScreen {
    /// Remember what was just written to `path`, for `mark_saved`. The upstream-name scrub
    /// (src/rebrand.rs) runs here, not only when the file leaves the app: the bytes recorded are
    /// then exactly the ones that reach the user's file, and the later scrub finds nothing to do.
    pub(crate) fn record_written(&mut self, path: &Path, written: Vec<u8>) {
        crate::rebrand::scrub_file(path);
        self.last_written = Some(std::fs::read(path).unwrap_or(written));
    }
}

fn in_dir(path: &Path, dir: &str) -> bool {
    path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == dir)
}

impl DocScreen for PdfScreen {
    fn set_author(&mut self, name: &str) {
        self.author = name.trim().to_owned();
        if let Some(d) = self.doc.as_mut() {
            if !self.author.is_empty() {
                d.set_author(&self.author);
            }
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn tool(&self) -> Tool {
        Tool::Page
    }
    fn dirty(&self) -> bool {
        self.doc.as_ref().is_some_and(PdfDoc::is_dirty)
    }

    fn editing(&self) -> bool {
        self.editing
    }
    fn set_editing(&mut self, on: bool) {
        if self.doc.is_none() {
            self.editing = false;
            return;
        }
        if on {
            if let Some(r) = self.doc.as_ref().and_then(|d| d.read_only_reason()) {
                self.say(format!("Read only: {r} (File › Password can unlock it)"));
            }
            self.v.search_open = false;
            self.v.hits.clear();
        }
        self.editing = on;
        self.e.tool = tools::TouchTool::None;
        self.e.selected = None;
        self.e.entry = None;
        self.v.drag = None;
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) {
        if self.locked.is_some() {
            self.lock_card(ui, cx);
            self.draw_toast(ui.ctx());
            return;
        }
        self.refresh();
        let area = ui.max_rect();
        self.show_pages(ui, cx);
        let ctx = ui.ctx().clone();
        self.sheets(&ctx, area, cx);
        self.cert_cards(&ctx, area, &cx.scratch, cx.top_inset, cx.bottom_inset);
        // Thumbnails only live while their sheet is open (a long PDF would hold a lot of them).
        if !matches!(self.sheet, sheets::Sheet::Thumbs) {
            self.v.thumbs.iter_mut().for_each(|t| *t = None);
        }
        self.draw_toast(&ctx);
    }

    fn take_tapped(&mut self) -> bool {
        std::mem::take(&mut self.v.tapped)
    }
    fn scrolled(&self) -> bool {
        self.v.scrolled
    }
    fn position(&self) -> (usize, usize) {
        (self.v.current_page, self.page_count().max(1))
    }

    fn search_open(&self) -> bool {
        self.v.search_open
    }
    fn toggle_search(&mut self) {
        if self.doc.is_none() {
            return;
        }
        self.v.search_open = !self.v.search_open;
        self.v.focus_search = self.v.search_open;
        if !self.v.search_open {
            self.v.hits.clear();
            self.v.searched.clear();
        }
    }
    fn search_bar(&mut self, ui: &mut egui::Ui) {
        self.search_row(ui);
    }

    fn view_menu(&self) -> Vec<(String, &'static str)> {
        if self.doc.is_none() {
            return Vec::new();
        }
        vec![
            ("Outline (bookmarks)".into(), "outline"),
            ("Page thumbnails".into(), "thumbs"),
            (if self.v.night { "• Night mode".into() } else { "Night mode".into() }, "night"),
            ("Comments".into(), "comments"),
            ("Signatures".into(), "signatures"),
        ]
    }
    fn view_action(&mut self, id: &str) {
        match id {
            "outline" => self.open_outline(),
            "thumbs" => self.sheet = sheets::Sheet::Thumbs,
            "night" => self.v.night = !self.v.night,
            "comments" => self.sheet = sheets::Sheet::comments(),
            "signatures" => self.cert.stage = cert::Stage::List,
            _ => {}
        }
    }

    fn undo(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        match doc.undo() {
            Ok(label) => self.say(format!("Undid: {label}")),
            Err(_) => self.say("Nothing to undo"),
        }
        self.e.selected = None;
        self.v.clear_textures = true;
        self.refresh();
    }
    fn redo(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        match doc.redo() {
            Ok(label) => self.say(format!("Redid: {label}")),
            Err(_) => self.say("Nothing to redo"),
        }
        self.e.selected = None;
        self.v.clear_textures = true;
        self.refresh();
    }

    fn edit_bar(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) -> Option<FileAction> {
        if self.doc.is_none() {
            return None;
        }
        self.sigs.load(&cx.scratch);
        let a = self.tool_bar(ui, cx);
        self.cert.action.take().or(a)
    }

    fn back(&mut self) -> bool {
        self.cert_back()
    }

    fn intercept_save(&mut self) -> bool {
        self.intercept_save_choice()
    }

    fn picked_image(&mut self, path: &str) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.say(format!("Couldn't read the picture: {e}"));
                return;
            }
        };
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
        let page = self.v.current_page;
        if self.edit(|d| d.add_image(page, None, &name, bytes)) {
            self.say("Picture added — tap it to move or resize");
        }
    }

    fn insert_picture(&mut self, path: &str, width_pt: f32) {
        let Ok(bytes) = std::fs::read(path) else { return };
        let page = self.v.current_page;
        let Some(info) = self.cache.pages.get(page) else { return };
        // Keep the picture's shape: read its size from the PNG header.
        let (iw, ih) = png_size(&bytes).unwrap_or((1, 1));
        let w = width_pt.min(info.width * 0.8);
        let h = w * ih as f32 / iw.max(1) as f32;
        let (x, y) = ((info.width - w) / 2.0, (info.height - h) / 2.0);
        if self.edit(|d| d.add_image(page, Some([x, y, x + w, y + h]), "barcode.png", bytes)) {
            self.say("Barcode added — tap it to move or resize");
        }
    }

    fn can_save_in_place(&self) -> bool {
        let Some(doc) = self.doc.as_ref() else { return false };
        !self.is_new && !self.uri_stale && doc.read_only_reason().is_none() && !self.cache.props.signed
    }
    fn default_ext(&self) -> &'static str {
        "pdf"
    }

    fn write_to(&mut self, path: &Path) -> Result<Vec<String>, String> {
        // Pages › Extract: a file with just those pages. It is never "the saved document"
        // (nothing is recorded in `last_written`), and the request is used up here so it can't
        // leak into a later Print.
        if let Some(pages) = self.extract_pending.take() {
            let doc = self.doc.as_ref().ok_or("the document is still locked")?;
            let bytes = doc.extract_pages(&pages).map_err(|e| e.to_string())?;
            std::fs::write(path, bytes).map_err(|e| e.to_string())?;
            return Ok(Vec::new());
        }
        if let Some(r) = self.cert_write(path) {
            return r;
        }
        let doc = self.doc.as_ref().ok_or("the document is still locked")?;
        let out = doc.save_bytes().map_err(|e| e.to_string())?;
        std::fs::write(path, &out.bytes).map_err(|e| e.to_string())?;
        self.record_written(path, out.bytes);
        // The notes say the saved file differs from the original in more than the user's edits
        // (a damaged file repaired, unreadable objects, XFA parts): never overwrite the original
        // with that — the app saves a copy instead.
        Ok(out.notes)
    }

    fn mark_saved(&mut self, name: &str, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else { return };
        if self.last_written.as_deref() == Some(bytes.as_slice()) {
            if let Some(doc) = self.doc.as_mut() {
                let r = if std::mem::take(&mut self.cert.wrote_signed) { doc.mark_signed(&bytes) } else { doc.mark_saved(&bytes) };
                if let Err(e) = r {
                    log::warn!("mark_saved: {e}");
                }
            }
            self.v.clear_textures = true;
            self.name = name.to_owned();
            self.is_new = false;
            self.uri_stale = false;
            self.cache.generation = None;
            self.refresh();
        } else {
            // Not this document (an extracted-pages copy): keep the open document as it is, and
            // make the next Save ask where to go rather than overwrite that copy.
            self.uri_stale = true;
        }
    }

    fn export_pdf(&mut self, path: &Path) -> Result<(), String> {
        self.extract_pending = None; // only `write_to` makes the extract (see there)
        let doc = self.doc.as_ref().ok_or("the document is still locked")?;
        // The app's Save as (…/save/) for a PDF is a copy of the document itself, edits and all,
        // with nothing flattened. Print (and anything else) gets the print-ready copy.
        if in_dir(path, "save") {
            let out = doc.save_bytes().map_err(|e| e.to_string())?;
            std::fs::write(path, &out.bytes).map_err(|e| e.to_string())?;
            self.record_written(path, out.bytes);
            return Ok(());
        }
        let bytes = doc.print_pdf(self.paper()).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    fn info_line(&mut self) -> String {
        let p = &self.cache.props;
        let mut parts = vec![format!("{} page{}", p.page_count, if p.page_count == 1 { "" } else { "s" })];
        if !p.pdf_version.is_empty() {
            parts.push(format!("PDF {}", p.pdf_version));
        }
        parts.push(format!("{} KB", p.file_size.div_ceil(1024)));
        if p.encrypted {
            parts.push("password protected".into());
        }
        if p.signed {
            parts.push("digitally signed".into());
        }
        if p.has_form {
            parts.push("form".into());
        }
        let c = self.cache.comments.iter().filter(|c| !c.is_reply).count();
        if c > 0 {
            parts.push(format!("{c} comment{}", if c == 1 { "" } else { "s" }));
        }
        format!("{} · {}", self.name, parts.join(" · "))
    }
}

/// Width and height from a PNG header.
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    Some((w, h))
}
