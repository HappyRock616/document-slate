//! Document Slate — Android office suite (combined app).
//! Current stage: start screen (Open + Recent files) and WordSlate viewer mode on top of the
//! upstream WordCraft engine.

use std::path::PathBuf;

use eframe::egui::{self, Color32};

#[cfg(target_os = "android")]
mod android_bridge;
mod barcode_maker;
mod barcode_scan;
#[cfg(target_os = "android")]
mod android_ime;
mod chrome;
#[cfg(feature = "deck")]
mod deck_ui;
#[cfg(feature = "pdf")]
mod digital_ids;
mod doc_screen;
mod edit_bar;
#[cfg(feature = "grid")]
mod grid_ui;
#[cfg(feature = "pdf")]
mod pdf_ui;
mod home;
mod links;
mod paper;
mod rebrand;
#[allow(dead_code)] // upstream file, used partly
mod icons;
mod prefs;
mod settings;
#[allow(dead_code)] // kept for print-path tests; real printing exports the document
mod test_pdf;
mod theme;
mod word_viewer;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

pub struct SlateApp {
    /// The open document; None = start screen.
    viewer: Option<word_viewer::WordViewer>,
    /// An open Grid / Deck / Page document (Word uses `viewer`). At most one of the two is set.
    other: Option<Box<dyn doc_screen::DocScreen>>,
    /// Where the open document lives (content:// on Android, a path on desktop) — Save writes here.
    current_uri: Option<String>,
    /// Viewer bars: show/hide state.
    bars: chrome::AutoHide,
    /// Outline sheet open: the headings shown (paragraph, level, text).
    outline: Option<Vec<(usize, u64, String)>>,
    /// Time (egui seconds) the document last scrolled, for the page counter.
    last_scroll: f64,
    /// Time the current status message appeared (viewer toasts fade after a few seconds).
    status_since: f64,
    last_status: String,
    /// Edit mode: which bottom tab is showing.
    edit_tab: edit_bar::Tab,
    /// Insert tab: which input row is open and what's typed in it.
    insert_state: edit_bar::InsertState,
    /// Leaving a changed document asks once ("press Back again to discard").
    discard_armed: bool,
    /// A Save as / Export waiting for Android's "save to" screen: (temporary file, adopt it as
    /// the open document afterwards? — yes for DOCX/ODT, no for text-only TXT and PDF).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))] // desktop writes directly
    pending_save: Option<(PathBuf, bool)>,
    /// A file that arrived ("Open with") while the open document has unsaved changes:
    /// (name, address, bytes). It waits here until the user chooses (see `incoming_prompt`).
    incoming: Option<(String, String, Vec<u8>)>,
    /// One-line message shown in the bar (errors, export results).
    status: String,
    /// Private app folder: settings.json, sample .docx, PDFs for printing.
    scratch: PathBuf,
    settings: settings::Settings,
    /// Startup work (open command-line file / decide start screen) done?
    started: bool,
    /// Desktop: a file given on the command line.
    pending_open: Option<PathBuf>,
    mode: theme::Mode,
    /// The theme last applied to egui (None = not yet).
    applied: Option<(theme::Mode, theme::Tool)>,
    tool: theme::Tool,
    /// An app's own screen (Word Slate…) is showing when no document is open; None = suite start.
    app_home: Option<theme::Tool>,
    /// The viewer bar's ⋮ menu is open (keeps the bar from auto-hiding under it).
    bar_menu_open: bool,
    /// Preferences drawer (start screen, behind the app icon).
    prefs: prefs::Prefs,
    /// Preferences › Digital IDs.
    #[cfg(feature = "pdf")]
    ids: Option<digital_ids::IdManager>,
    /// Last "keep screen on" state sent to Android.
    #[cfg(target_os = "android")]
    awake_sent: Option<bool>,
    #[cfg(target_os = "android")]
    android: Option<AndroidApp>,
    #[cfg(target_os = "android")]
    ime: Option<android_ime::ImeBridge>,
    #[cfg(target_os = "android")]
    top_inset_px: f32,
    #[cfg(target_os = "android")]
    inset_checked_at: f64,
    #[cfg(target_os = "android")]
    open_checked_at: f64,
    /// Launched by "Open with": show a blank screen (not the start screen) until the file arrives.
    #[cfg(target_os = "android")]
    awaiting_launch_file: bool,
    /// Status-bar icon colour last sent to Java (Some(true) = dark icons).
    #[cfg(target_os = "android")]
    dark_icons_sent: Option<bool>,
    /// Whether Java currently routes back/back-gesture to us (only while a document is open).
    #[cfg(target_os = "android")]
    back_handled_sent: Option<bool>,
    /// Keyboard / navigation-bar height at the bottom, in pixels (edit toolbar sits above it).
    #[cfg(target_os = "android")]
    bottom_inset_px: f32,
    #[cfg(target_os = "android")]
    bottom_checked_at: f64,
}

impl SlateApp {
    fn new(scratch: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&scratch);
        let settings = settings::Settings::load(&scratch);
        let mode = if settings.dark { theme::Mode::Dark } else { theme::Mode::Light };
        Self {
            viewer: None,
            current_uri: None,
            bars: chrome::AutoHide::default(),
            outline: None,
            last_scroll: -10.0,
            status_since: 0.0,
            last_status: String::new(),
            edit_tab: edit_bar::Tab::Text,
            insert_state: edit_bar::InsertState::default(),
            discard_armed: false,
            pending_save: None,
            incoming: None,
            status: String::new(),
            scratch,
            settings,
            started: false,
            pending_open: None,
            mode,
            applied: None,
            tool: theme::Tool::Word,
            #[cfg(target_os = "android")]
            android: None,
            #[cfg(target_os = "android")]
            ime: None,
            #[cfg(target_os = "android")]
            top_inset_px: 0.0,
            #[cfg(target_os = "android")]
            inset_checked_at: -1.0,
            #[cfg(target_os = "android")]
            open_checked_at: -1.0,
            #[cfg(target_os = "android")]
            awaiting_launch_file: false,
            #[cfg(target_os = "android")]
            dark_icons_sent: None,
            other: None,
            app_home: None,
            bar_menu_open: false,
            prefs: prefs::Prefs::default(),
            #[cfg(feature = "pdf")]
            ids: None,
            #[cfg(target_os = "android")]
            awake_sent: None,
            #[cfg(target_os = "android")]
            back_handled_sent: None,
            #[cfg(target_os = "android")]
            bottom_inset_px: 0.0,
            #[cfg(target_os = "android")]
            bottom_checked_at: -1.0,
        }
    }

    /// First frame: open a command-line file (desktop), or note an "Open with" launch (Android).
    /// Otherwise the start screen shows.
    fn startup(&mut self) {
        self.started = true;
        if let Some(path) = self.pending_open.take() {
            self.open_path(&path);
        }
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            self.awaiting_launch_file = android_bridge::launched_with_file(app).unwrap_or(false);
        }
    }

    /// Desktop: open a file by path (command line, Recent).
    fn open_path(&mut self, path: &std::path::Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read(path) {
            Ok(bytes) => self.open_bytes(&name, &path.to_string_lossy(), &bytes),
            Err(e) => self.status = format!("Couldn't read {name}: {e}"),
        }
    }

    /// Show a document that arrived as bytes, and put it at the top of Recent.
    fn open_bytes(&mut self, name: &str, uri: &str, bytes: &[u8]) {
        let t = std::time::Instant::now();
        let tool = home::tool_for_file(name, bytes);
        if let Some(opened) = doc_screen::open(tool, name, bytes) {
            match opened {
                Ok(mut s) => {
                    log::info!("opened {name} ({} KB) in {} ms", bytes.len() / 1024, t.elapsed().as_millis());
                    s.set_author(&self.settings.author);
                    if self.settings.open_in_edit {
                        s.set_editing(true);
                    }
                    self.tool = tool;
                    self.app_home = Some(tool);
                    self.viewer = None;
                    self.other = Some(s);
                    self.current_uri = (!uri.is_empty()).then(|| uri.to_owned());
                    self.discard_armed = false;
                    self.bars = chrome::AutoHide::default(); // fresh viewer-bar timer for the new document
                    self.status.clear();
                    self.settings.touch_recent(name, uri);
                }
                Err(e) => self.status = format!("Couldn't open {name}: {e}"),
            }
            return;
        }
        match word_viewer::WordViewer::open_bytes(name, bytes) {
            Ok(mut v) => {
                log::info!("opened {} ({} pages, {} KB) in {} ms", v.name, v.page_count(), bytes.len() / 1024, t.elapsed().as_millis());
                v.apply_prefs(&self.settings.spell_lang, &self.settings.author, false);
                if self.settings.open_in_edit {
                    v.set_editing(true);
                }
                self.tool = home::tool_for(name);
                // Closing the document returns to its app's screen.
                if self.tool.available() {
                    self.app_home = Some(self.tool);
                }
                self.viewer = Some(v);
                self.other = None;
                self.current_uri = (!uri.is_empty()).then(|| uri.to_owned());
                self.discard_armed = false;
                self.bars = chrome::AutoHide::default(); // fresh viewer-bar timer for the new document
                self.status.clear();
                self.settings.touch_recent(name, uri);
            }
            Err(e) => self.status = format!("Couldn't open {name}: {e}"),
        }
    }

    /// "New Word Slate": a blank document, straight into edit mode. It has no file yet, so the
    /// first Save asks where to put it (Save as .docx).
    fn new_document(&mut self) {
        let tool = self.app_home.unwrap_or(theme::Tool::Word);
        if let Some(mut s) = doc_screen::new_blank(tool) {
            s.set_author(&self.settings.author);
            s.set_editing(true);
            self.tool = tool;
            self.viewer = None;
            self.other = Some(s);
            self.current_uri = None;
            self.discard_armed = false;
            self.bars = chrome::AutoHide::default(); // fresh viewer-bar timer for the new document
            self.status.clear();
            return;
        }
        let mut v = word_viewer::WordViewer::new_blank("Untitled.docx");
        v.apply_prefs(&self.settings.spell_lang, &self.settings.author, true);
        v.set_editing(true);
        self.tool = theme::Tool::Word;
        self.app_home = Some(theme::Tool::Word);
        self.other = None;
        self.viewer = Some(v);
        self.current_uri = None;
        self.discard_armed = false;
        self.bars = chrome::AutoHide::default(); // fresh viewer-bar timer for the new document
        self.status.clear();
    }

    /// The file picker starting in Downloads or Documents.
    fn open_picker_at(&mut self, folder: home::Folder) {
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            let doc = match folder {
                home::Folder::Downloads => "primary%3ADownload",
                home::Folder::Documents => "primary%3ADocuments",
            };
            let uri = format!("content://com.android.externalstorage.documents/document/{doc}");
            if let Err(e) = android_bridge::open_document_picker_at(app, &uri) {
                self.status = format!("File picker failed: {e}");
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = folder;
            self.open_picker();
        }
    }

    /// Open button: Android's file picker. (Desktop: pass a path on the command line.)
    fn open_picker(&mut self) {
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            if let Err(e) = android_bridge::open_document_picker(app) {
                self.status = format!("File picker failed: {e}");
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.status = "Desktop: run document-slate-desktop <file>".into();
        }
    }

    fn open_recent(&mut self, uri: &str) {
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            if let Err(e) = android_bridge::open_uri(app, uri) {
                self.status = format!("Couldn't reopen: {e}");
            }
        }
        #[cfg(not(target_os = "android"))]
        self.open_path(std::path::Path::new(uri));
    }

    fn toggle_theme(&mut self) {
        self.mode = self.mode.toggled();
        self.settings.dark = self.mode == theme::Mode::Dark;
        self.settings.save();
    }

    /// Back to the start screen. A changed document asks once first: the first press only warns,
    /// a second press discards the changes.
    fn close_document(&mut self) {
        self.outline = None;
        if self.doc_dirty() && !self.discard_armed {
            self.discard_armed = true;
            self.status = "Unsaved changes: tap Save, or go back again to discard them".into();
            return;
        }
        self.viewer = None;
        self.other = None;
        self.current_uri = None;
        self.discard_armed = false;
        self.status.clear();
    }

    // ---- helpers over Word (`viewer`) and the other apps (`other`) ----------------------------
    fn has_doc(&self) -> bool {
        self.viewer.is_some() || self.other.is_some()
    }
    fn doc_dirty(&self) -> bool {
        self.viewer.as_ref().is_some_and(|v| v.dirty) || self.other.as_ref().is_some_and(|o| o.dirty())
    }
    fn doc_editing(&self) -> bool {
        self.viewer.as_ref().is_some_and(|v| v.editing) || self.other.as_ref().is_some_and(|o| o.editing())
    }
    fn doc_name(&self) -> String {
        self.viewer.as_ref().map(|v| v.name.clone()).or_else(|| self.other.as_ref().map(|o| o.name().to_owned())).unwrap_or_default()
    }
    fn screen_ctx(&self) -> doc_screen::ScreenCtx {
        doc_screen::ScreenCtx { accent: self.tool.accent(), dark: self.mode == theme::Mode::Dark, scratch: self.scratch.clone(), top_inset: 0.0, bottom_inset: 0.0 }
    }
    fn tmp_path(&self, dir: &str, name: &str) -> PathBuf {
        let d = self.scratch.join(dir);
        let _ = std::fs::create_dir_all(&d);
        d.join(word_viewer::safe_file_name(name))
    }

    /// Save for Grid / Deck / Page: same rules as Word (fidelity check, never overwrite the
    /// original with a file that lacks something; new files go to Save as).
    fn save_other(&mut self) {
        let Some(o) = self.other.as_ref() else { return };
        let (name, ext, in_place) = (o.name().to_owned(), o.default_ext(), o.can_save_in_place());
        let Some(uri) = self.current_uri.clone().filter(|_| in_place) else {
            self.save_as(ext);
            return;
        };
        let tmp = self.tmp_path("save", &name);
        let Some(o) = self.other.as_mut() else { return };
        match o.write_to(&tmp) {
            Err(e) => {
                self.status = format!("Save failed: {e}");
                return;
            }
            Ok(lost) if !lost.is_empty() => {
                self.status = format!("Saving over the original would drop {}. Saving a copy instead.", lost.join(", "));
                self.save_as(ext);
                return;
            }
            Ok(_) => {}
        }
        #[cfg(target_os = "android")]
        let result = match self.android.as_ref() {
            Some(app) => android_bridge::write_uri(app, &uri, &tmp),
            None => Err("no Android context".into()),
        };
        #[cfg(not(target_os = "android"))]
        let result = std::fs::copy(&tmp, &uri).map(|_| ()).map_err(|e| e.to_string());
        match result {
            Ok(()) => {
                if let Some(o) = self.other.as_mut() {
                    o.mark_saved(&name, &tmp);
                }
                self.discard_armed = false;
                self.status = format!("Saved {name}");
            }
            Err(e) => self.status = format!("Couldn't save to the original file: {e}"),
        }
    }

    /// Save: the engine writes the file in its own format to a temporary file, which is then
    /// copied over the original (Android: through the content:// address, needs write access).
    fn save(&mut self) {
        if let Some(o) = self.other.as_mut() {
            if o.intercept_save() {
                return;
            }
            self.save_other();
            return;
        }
        let Some(v) = self.viewer.as_mut() else { return };
        if !v.can_save_in_place() {
            self.status = format!("{} can't be saved in its own format yet (Save as .docx comes with the File tab)", v.name);
            return;
        }
        let Some(uri) = self.current_uri.clone() else {
            // A new document (or the sample): ask where to save it.
            self.save_as("docx");
            return;
        };
        let tmp_dir = self.scratch.join("save");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let tmp = tmp_dir.join(word_viewer::safe_file_name(&v.name));
        match v.save_checked(&tmp) {
            Err(e) => {
                self.status = format!("Save failed: {e}");
                return;
            }
            Ok(lost) if !lost.is_empty() => {
                // Full fidelity: never overwrite the original with a file that lacks something.
                // Keep the original untouched and save the edits as a new .docx copy instead.
                self.status = format!("Saving over the original would drop {}. Saving a copy instead.", lost.join(", "));
                log::warn!("save blocked, would lose: {lost:?}");
                self.save_as("docx");
                return;
            }
            Ok(_) => {}
        }
        #[cfg(target_os = "android")]
        let result = match self.android.as_ref() {
            Some(app) => android_bridge::write_uri(app, &uri, &tmp),
            None => Err("no Android context".into()),
        };
        #[cfg(not(target_os = "android"))]
        let result = std::fs::copy(&tmp, &uri).map(|_| ()).map_err(|e| e.to_string());
        match result {
            Ok(()) => {
                self.discard_armed = false;
                self.status = format!("Saved {}", v.name);
            }
            Err(e) => {
                v.dirty = true; // not really saved
                self.status = format!("Couldn't save to the original file: {e}");
            }
        }
    }

    /// Save as DOCX / ODT / TXT: the engine writes a temporary copy, then Android's "save to"
    /// screen lets the user choose folder and name (Java copies it there).
    fn save_as(&mut self, ext: &'static str) {
        self.save_copy(ext, "", true);
    }

    /// Save as with a name suffix (" (signed)"); `adopt_ok`: the copy may become the open document.
    fn save_copy(&mut self, ext: &'static str, suffix: &str, adopt_ok: bool) {
        if self.other.is_some() {
            let base = self.doc_name().rsplit_once('.').map(|(b, _)| b.to_owned()).unwrap_or_else(|| self.doc_name());
            let name = format!("{base}{suffix}.{ext}");
            let tmp = self.tmp_path("save", &name);
            let Some(o) = self.other.as_mut() else { return };
            // A PDF document saved "as PDF" is a real copy of the document (write_to); other apps'
            // documents become a PDF export.
            let res = if ext == "pdf" && o.default_ext() != "pdf" { o.export_pdf(&tmp).map(|_| Vec::new()) } else { o.write_to(&tmp) };
            match res {
                Err(e) => self.status = format!("Save as failed: {e}"),
                Ok(lost) => {
                    if !lost.is_empty() {
                        self.status = format!("This .{ext} copy can't hold {}; the open document keeps them.", lost.join(", "));
                    }
                    let adopt = adopt_ok && lost.is_empty() && ext == o.default_ext();
                    self.ask_where_to_save(&name, &tmp, adopt);
                }
            }
            return;
        }
        let Some(v) = self.viewer.as_mut() else { return };
        let name = format!("{}.{ext}", v.base_name());
        let tmp_dir = self.scratch.join("save");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let tmp = tmp_dir.join(word_viewer::safe_file_name(&name));
        if let Err(e) = v.write_as(&tmp) {
            self.status = format!("Save as failed: {e}");
            return;
        }
        // TXT keeps only the words, so the open document stays the formatted one. A DOCX/ODT copy
        // that can't hold everything (fidelity guard) is written, but not adopted either: the
        // open document stays the complete one, and the user is told what the copy lacks.
        let lost = if ext == "txt" { Vec::new() } else { v.save_losses(&tmp) };
        if !lost.is_empty() {
            self.status = format!("This .{ext} copy can't hold {}; the open document keeps them.", lost.join(", "));
        }
        let adopt = ext != "txt" && lost.is_empty();
        self.ask_where_to_save(&name, &tmp, adopt);
    }

    /// Export PDF through the same "save to" screen.
    fn export_pdf_file(&mut self) {
        if self.other.is_some() {
            self.save_as("pdf");
            return;
        }
        let Some(v) = self.viewer.as_mut() else { return };
        let name = format!("{}.pdf", v.base_name());
        let tmp_dir = self.scratch.join("save");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let tmp = tmp_dir.join(word_viewer::safe_file_name(&name));
        if let Err(e) = v.export_pdf(&tmp) {
            self.status = format!("PDF export failed: {e}");
            return;
        }
        self.ask_where_to_save(&name, &tmp, false);
    }

    fn ask_where_to_save(&mut self, name: &str, tmp: &std::path::Path, adopt: bool) {
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            match android_bridge::save_as_picker(app, name, word_viewer::mime_for(name), tmp) {
                Ok(()) => self.pending_save = Some((tmp.to_path_buf(), adopt)),
                Err(e) => self.status = format!("Save as failed: {e}"),
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = adopt;
            self.status = format!("Desktop: wrote {} ({name})", tmp.display());
        }
    }

    /// Collect the outcome of Android's "save to" screen.
    #[cfg(target_os = "android")]
    fn poll_save_result(&mut self, app: &AndroidApp) {
        if self.pending_save.is_none() {
            return;
        }
        match android_bridge::take_save_result(app) {
            Ok(Some(android_bridge::SaveResult::Saved { uri, name })) => {
                let (tmp, adopt) = self.pending_save.take().unwrap_or_default();
                if adopt {
                    if let Some(v) = self.viewer.as_mut() {
                        v.adopt_saved(&name, &tmp);
                    }
                    if let Some(o) = self.other.as_mut() {
                        o.mark_saved(&name, &tmp);
                    }
                    self.current_uri = Some(uri.clone());
                    self.tool = home::tool_for(&name);
                    self.settings.touch_recent(&name, &uri);
                    self.discard_armed = false;
                }
                self.status = format!("Saved {name}");
            }
            Ok(Some(android_bridge::SaveResult::Cancelled)) => self.pending_save = None,
            Ok(Some(android_bridge::SaveResult::Failed(e))) => {
                self.pending_save = None;
                self.status = format!("Save as failed: {e}");
            }
            Ok(None) => {}
            Err(e) => log::warn!("save result: {e}"),
        }
    }

    fn file_action(&mut self, a: edit_bar::FileAction) {
        match a {
            edit_bar::FileAction::Save => self.save(),
            edit_bar::FileAction::SaveAs(ext) => self.save_as(ext),
            edit_bar::FileAction::InsertBarcode => barcode_maker::open(&self.scratch),
            edit_bar::FileAction::SaveCopy { suffix, adopt } => {
                let ext = self.other.as_ref().map_or("docx", |o| o.default_ext());
                self.save_copy(ext, suffix, adopt);
            }
            edit_bar::FileAction::ExportPdf => self.export_pdf_file(),
            edit_bar::FileAction::Share => self.share(),
            edit_bar::FileAction::Print => self.print(),
            edit_bar::FileAction::Info => {
                if let Some(v) = self.viewer.as_mut() {
                    self.status = v.info_line();
                }
                if let Some(o) = self.other.as_mut() {
                    self.status = o.info_line();
                }
            }
            edit_bar::FileAction::Status(s) => self.status = s,
            edit_bar::FileAction::PickImage => {
                #[cfg(target_os = "android")]
                if let Some(app) = self.android.as_ref() {
                    if let Err(e) = android_bridge::pick_image(app) {
                        self.status = format!("Picture picker failed: {e}");
                    }
                }
                #[cfg(not(target_os = "android"))]
                {
                    self.status = "Desktop: pictures can be inserted on Android".into();
                }
            }
        }
    }

    /// Height covered at the bottom (keyboard or navigation bar), in points.
    fn bottom_inset(&mut self, _ctx: &egui::Context) -> f32 {
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            let now = _ctx.input(|i| i.time);
            // The keyboard slides in and out, so check often while editing.
            if now - self.bottom_checked_at > 0.15 || self.bottom_checked_at < 0.0 {
                self.bottom_checked_at = now;
                match android_bridge::bottom_inset_px(app) {
                    Ok(px) => self.bottom_inset_px = px.max(0) as f32,
                    Err(e) => log::warn!("bottom inset: {e}"),
                }
                _ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            return self.bottom_inset_px / _ctx.pixels_per_point();
        }
        0.0
    }

    /// Collect a file Java finished reading (picker, Recent, "Open with"). Polled 4× a second.
    #[cfg(target_os = "android")]
    fn poll_android_open(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let now = ctx.input(|i| i.time);
        if now - self.open_checked_at < 0.25 {
            return;
        }
        self.open_checked_at = now;
        let Some(app) = self.android.clone() else { return };
        self.poll_save_result(&app);
        // Insert › Picture: the picked image, copied into files/insert/ by Java.
        if self.doc_editing() {
            match android_bridge::take_pending_image(&app) {
                Ok(Some(path)) => {
                    if let Some(o) = self.other.as_mut() {
                        o.picked_image(&path);
                    }
                    if let Some(v) = self.viewer.as_mut() {
                        // 3 inches wide by default (height keeps the shape); a phone photo at
                        // full size would be many pages tall.
                        if v.run("insert.picture", serde_json::json!({ "path": path, "width": 216.0 })).is_none() {
                            self.status = "Couldn't insert that picture".into();
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("picture: {e}"),
            }
        }
        match android_bridge::take_pending_file(&app) {
            Ok(Some(android_bridge::Pending::File { name, uri, bytes })) => {
                self.awaiting_launch_file = false;
                if self.doc_dirty() {
                    // Never replace unsaved work without asking.
                    self.incoming = Some((name, uri, bytes));
                } else {
                    self.open_bytes(&name, &uri, &bytes);
                }
            }
            Ok(Some(android_bridge::Pending::Error(e))) => {
                self.awaiting_launch_file = false;
                match e.strip_prefix("noaccess\n") {
                    Some(uri) => {
                        // Android no longer lets us read it (the file came from another app, or the
                        // grant ran out): say how to get it back, and drop the dead Recent entry.
                        self.settings.forget_recent(uri);
                        self.status = "Android no longer lets Document Slate open that file. Open it again with Open (or from Files) and it'll be back in Recent.".into();
                    }
                    None => self.status = format!("Couldn't read file: {e}"),
                }
            }
            Ok(None) => {}
            Err(e) => log::warn!("pending file: {e}"),
        }
    }

    /// Share: a copy of the open file goes to Android's share sheet (email, messages, Drive…).
    fn share(&mut self) {
        if self.other.is_some() {
            let mut name = self.doc_name();
            let Some(o) = self.other.as_mut() else { return };
            // A format the app only reads (.xls, .ods…) is shared in the app's own format.
            if !o.can_save_in_place() && o.default_ext() != "pdf" {
                let base = name.rsplit_once('.').map_or(name.as_str(), |(b, _)| b).to_owned();
                name = format!("{base}.{}", o.default_ext());
            }
            let path = self.tmp_path("share", &name);
            let Some(o) = self.other.as_mut() else { return };
            if let Err(e) = o.write_to(&path) {
                self.status = format!("Share failed: {e}");
                return;
            }
            let mime = word_viewer::mime_for(&name);
            #[cfg(target_os = "android")]
            if let Some(app) = self.android.as_ref() {
                if let Err(e) = android_bridge::share_file(app, &path, mime) {
                    self.status = format!("Share failed: {e}");
                }
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = mime;
                self.status = format!("Copied to {}", path.display());
            }
            return;
        }
        let Some(viewer) = self.viewer.as_mut() else { return };
        let (path, mime) = match viewer.write_copy(&self.scratch.join("share")) {
            Ok(x) => x,
            Err(e) => {
                self.status = format!("Share failed: {e}");
                return;
            }
        };
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            if let Err(e) = android_bridge::share_file(app, &path, mime) {
                self.status = format!("Share failed: {e}");
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = mime;
            self.status = format!("Copied to {} (desktop has no share sheet)", path.display());
        }
    }

    /// Back button / gesture: close search if open, else return to the start screen.
    #[cfg(target_os = "android")]
    fn handle_back(&mut self) {
        #[cfg(feature = "pdf")]
        let ids_open = self.ids.as_ref().is_some_and(|m| m.open);
        #[cfg(not(feature = "pdf"))]
        let ids_open = false;
        let want = self.has_doc() || self.app_home.is_some() || self.prefs.open || ids_open || links::is_open() || barcode_maker::is_open();
        let Some(app) = self.android.clone() else { return };
        if self.back_handled_sent != Some(want) {
            match android_bridge::set_back_handled(&app, want) {
                Ok(()) => self.back_handled_sent = Some(want),
                Err(e) => log::warn!("back: {e}"),
            }
        }
        if want && android_bridge::take_back_pressed(&app).unwrap_or(false) {
            if self.incoming.take().is_some() {
                return; // Back on the "open another file?" card = keep editing
            }
            if links::back() || barcode_maker::back() {
                return;
            }
            #[cfg(feature = "pdf")]
            if let Some(m) = self.ids.as_mut() {
                if m.back() {
                    return;
                }
            }
            if let Some(o) = self.other.as_mut() {
                if o.back() {
                    return;
                }
                if o.search_open() {
                    o.toggle_search();
                } else if o.editing() {
                    o.set_editing(false);
                } else {
                    self.close_document();
                }
                return;
            }
            match self.viewer.as_mut() {
                Some(v) if v.search_open => v.toggle_search(),
                Some(v) if v.editing => v.set_editing(false),
                Some(_) => self.close_document(),
                None if self.prefs.back() => {}
                None => self.app_home = None,
            }
        }
    }

    fn print(&mut self) {
        if self.other.is_some() {
            let pdf = self.scratch.join("print.pdf");
            let name = self.doc_name();
            let Some(o) = self.other.as_mut() else { return };
            if let Err(e) = o.export_pdf(&pdf) {
                self.status = format!("PDF export failed: {e}");
                return;
            }
            #[cfg(target_os = "android")]
            if let Some(app) = self.android.as_ref() {
                self.status = match android_bridge::print_pdf(app, &pdf, &name) {
                    Ok(()) => String::new(),
                    Err(e) => format!("Print failed: {e}"),
                };
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = name;
                self.status = format!("Exported {}", pdf.display());
            }
            return;
        }
        let Some(viewer) = self.viewer.as_mut() else { return };
        let pdf = self.scratch.join("print.pdf");
        if let Err(e) = viewer.export_pdf(&pdf) {
            self.status = format!("PDF export failed: {e}");
            return;
        }

        #[cfg(target_os = "android")]
        {
            let Some(app) = self.android.as_ref() else { return };
            self.status = match android_bridge::print_pdf(app, &pdf, &viewer.name) {
                Ok(()) => String::new(),
                Err(e) => format!("Print failed: {e}"),
            };
        }

        #[cfg(not(target_os = "android"))]
        {
            self.status = format!("Exported {} (desktop printing not wired up)", pdf.display());
        }
    }

    /// Height of the system status bar overlapping the window, in points (0 on desktop).
    /// Asked from Java about once a second (it changes on rotation), cached in between.
    fn top_inset(&mut self, _ctx: &egui::Context) -> f32 {
        #[cfg(target_os = "android")]
        if let Some(app) = self.android.as_ref() {
            let now = _ctx.input(|i| i.time);
            if now - self.inset_checked_at > 1.0 || self.inset_checked_at < 0.0 {
                self.inset_checked_at = now;
                match android_bridge::top_inset_px(app) {
                    Ok(px) => self.top_inset_px = px.max(0) as f32,
                    Err(e) => log::warn!("top inset: {e}"),
                }
            }
            return self.top_inset_px / _ctx.pixels_per_point();
        }
        0.0
    }

    /// Viewer mode bar: floats over the full-screen document in the app's accent colour,
    /// auto-hides (see chrome::AutoHide). Back · title · Search · Share · Print · Edit.
    /// `vis` 0…1 is how far it's shown (it slides up and fades).
    fn viewer_bar(&mut self, ctx: &egui::Context, inset: f32, vis: f32) {
        if vis <= 0.01 {
            return;
        }
        let screen = ctx.content_rect();
        let accent = self.tool.accent();
        // Tint behind the status bar so its icons stay readable over a white page.
        egui::Area::new(egui::Id::unique("viewer-status-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).interactable(false).show(ctx, |ui| {
            let r = egui::Rect::from_min_size(screen.min, egui::vec2(screen.width(), inset));
            ui.painter().rect_filled(r, 0.0, accent.gamma_multiply(vis));
        });
        let width = (screen.width() - 16.0).min(720.0);
        let slide = (1.0 - vis) * 70.0;
        let pos = egui::pos2(screen.center().x - width / 2.0, inset + 8.0 - slide);
        let mut act: Option<&str> = None;
        let title = self.doc_name();
        let (dark_on, read_on) = self.viewer.as_ref().map_or((false, false), |v| (v.dark_page, v.read_mode));
        let other_menu = self.other.as_ref().map(|o| o.view_menu());
        let mut other_act: Option<&'static str> = None;
        let mut menu_open = false;
        egui::Area::new(egui::Id::unique("viewer-top-bar")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            ui.set_opacity(vis);
            let frame = egui::Frame::new()
                .fill(accent)
                .corner_radius(16)
                .shadow(egui::Shadow { offset: [0, 3], blur: 10, spread: 0, color: Color32::from_black_alpha(70) })
                .inner_margin(egui::Margin::symmetric(4, 4));
            frame.show(ui, |ui| {
                ui.set_width(width - 8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let fg = theme::ON_ACCENT;
                    if chrome::bar_button(ui, "chevronLeft", "Back", fg).clicked() {
                        act = Some("back");
                    }
                    // Title next to Back, taking whatever room the four icons leave.
                    let title_w = (ui.available_width() - 5.0 * chrome::TOUCH - 8.0).max(40.0);
                    ui.allocate_ui_with_layout(egui::vec2(title_w, chrome::TOUCH), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_width(title_w);
                        ui.add(egui::Label::new(egui::RichText::new(&title).color(fg).size(16.0).strong()).truncate());
                    });
                    for (icon, hint, id) in [("search", "Search", "search"), ("share", "Share", "share"), ("print", "Print", "print"), ("draw", "Edit", "edit")] {
                        if chrome::bar_button(ui, icon, hint, fg).clicked() {
                            act = Some(id);
                        }
                    }
                    // ⋮ More: view options.
                    let more = chrome::bar_button(ui, "more", "More", fg);
                    menu_open = egui::Popup::menu(&more).show(|ui| {
                        ui.set_min_width(200.0);
                        let dark = if dark_on { "• Dark page" } else { "Dark page" };
                        let read = if read_on { "• Read mode (reflow)" } else { "Read mode (reflow)" };
                        if let Some(items) = &other_menu {
                            for (label, id) in items {
                                if chrome::menu_item(ui, label) {
                                    other_act = Some(id);
                                    ui.close();
                                }
                            }
                            if items.is_empty() {
                                ui.label("No view options");
                            }
                        } else {
                            for (label, id) in [("Outline", "outline"), (dark, "dark"), (read, "read")] {
                                if chrome::menu_item(ui, label) {
                                    act = Some(id);
                                    ui.close();
                                }
                            }
                        }
                    }).is_some();
                });
            });
        });
        self.bar_menu_open = menu_open;
        // Any bar action changes what's on screen: draw the next frame now (egui otherwise
        // waits for the next input event, so e.g. Edit looked like it did nothing).
        if act.is_some() || other_act.is_some() {
            ctx.request_repaint();
        }
        if let (Some(id), Some(o)) = (other_act, self.other.as_mut()) {
            o.view_action(id);
        }
        if let Some(o) = self.other.as_mut() {
            match act {
                Some("search") => o.toggle_search(),
                Some("edit") => o.set_editing(true),
                _ => {}
            }
        }
        match act {
            Some("back") => self.close_document(),
            Some("print") => self.print(),
            Some("share") => self.share(),
            Some("search") => {
                if let Some(v) = self.viewer.as_mut() {
                    v.toggle_search();
                }
            }
            Some("edit") => {
                if let Some(v) = self.viewer.as_mut() {
                    if v.read_mode {
                        v.set_read_mode(false); // editing works on the real page layout
                    }
                    v.set_editing(true);
                }
            }
            Some("dark") => {
                if let Some(v) = self.viewer.as_mut() {
                    v.set_dark_page(!v.dark_page);
                }
            }
            Some("read") => {
                if let Some(v) = self.viewer.as_mut() {
                    v.set_read_mode(!v.read_mode);
                }
            }
            Some("outline") => {
                self.outline = self.viewer.as_mut().map(|v| v.headings());
            }
            _ => {}
        }
    }

    /// Outline sheet: the document's headings; tap one to jump there.
    fn outline_sheet(&mut self, ctx: &egui::Context, inset: f32) {
        let Some(list) = self.outline.clone() else { return };
        let screen = ctx.content_rect();
        let width = (screen.width() - 16.0).min(560.0);
        let height = (screen.height() - inset - 96.0).max(200.0);
        let pos = egui::pos2(screen.center().x - width / 2.0, inset + 72.0);
        let p = self.mode.palette();
        let mut go = None;
        let mut close = false;
        egui::Area::new(egui::Id::unique("outline-sheet")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::new().fill(p.surface).corner_radius(16).inner_margin(12).show(ui, |ui| {
                ui.set_width(width - 24.0);
                ui.set_max_height(height);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Outline").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if chrome::bar_button(ui, "close", "Close", p.text).clicked() {
                            close = true;
                        }
                    });
                });
                if list.is_empty() {
                    ui.label(egui::RichText::new("This document has no headings.").weak());
                }
                egui::ScrollArea::vertical().max_height(height - 60.0).show(ui, |ui| {
                    for (block, level, text) in &list {
                        let indent = (level.saturating_sub(1).min(5) as f32) * 16.0;
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let size = if *level <= 1 { 16.0 } else { 15.0 };
                            let b = egui::Button::new(egui::RichText::new(text).size(size)).frame(false).min_size(egui::vec2(width - 60.0 - indent, 40.0));
                            if ui.add(b).clicked() {
                                go = Some(*block);
                            }
                        });
                    }
                });
            });
        });
        if let Some(b) = go {
            if let Some(v) = self.viewer.as_mut() {
                v.go_to_block(b);
            }
            close = true;
        }
        if close {
            self.outline = None;
        }
    }

    /// Edit-mode heading bar (fixed, accent): Done · title (* when changed) · Undo · Redo · Save.
    fn edit_top_bar(&mut self, ui: &mut egui::Ui, inset: f32) {
        let frame = egui::Frame::new()
            .fill(self.tool.accent())
            .inner_margin(egui::Margin { left: 4, right: 4, top: (inset + 2.0).min(120.0) as i8, bottom: 2 });
        egui::Panel::top("edit_top_bar").frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let fg = theme::ON_ACCENT;
                if chrome::bar_button(ui, "check", "Done", fg).clicked() {
                    if let Some(v) = self.viewer.as_mut() {
                        v.set_editing(false);
                    }
                    if let Some(o) = self.other.as_mut() {
                        o.set_editing(false);
                    }
                    return;
                }
                let title = format!("{}{}", if self.doc_dirty() { "* " } else { "" }, self.doc_name());
                let title_w = (ui.available_width() - 3.0 * chrome::TOUCH - 4.0).max(40.0);
                ui.allocate_ui_with_layout(egui::vec2(title_w, chrome::TOUCH), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_width(title_w);
                    ui.add(egui::Label::new(egui::RichText::new(title).color(fg).size(16.0).strong()).truncate());
                });
                if let Some(v) = self.viewer.as_mut() {
                    if chrome::bar_button(ui, "undo", "Undo", fg).clicked() {
                        v.run("edit.undo", serde_json::json!({}));
                    }
                    if chrome::bar_button(ui, "redo", "Redo", fg).clicked() {
                        v.run("edit.redo", serde_json::json!({}));
                    }
                }
                if let Some(o) = self.other.as_mut() {
                    if chrome::bar_button(ui, "undo", "Undo", fg).clicked() {
                        o.undo();
                    }
                    if chrome::bar_button(ui, "redo", "Redo", fg).clicked() {
                        o.redo();
                    }
                }
                if chrome::bar_button(ui, "save", "Save", fg).clicked() {
                    self.save();
                }
            });
        });
    }

    /// Edit-mode bottom bar (tool row + tabs), lifted above the keyboard / navigation bar.
    /// The keyboard's space is reserved by an empty panel of exactly its height (egui margins
    /// stop at 127 points; a phone keyboard is often taller).
    fn edit_bottom_bar(&mut self, ui: &mut egui::Ui, bottom: f32) {
        let accent = self.tool.accent();
        let surface = self.mode.palette().surface;
        if bottom > 0.5 {
            egui::Panel::bottom("keyboard_space")
                .frame(egui::Frame::new().fill(surface))
                .exact_size(bottom)
                .resizable(false)
                .show_separator_line(false)
                .show(ui, |_| {});
        }
        let frame = egui::Frame::new().fill(surface).inner_margin(egui::Margin::symmetric(8, 4));
        let tab = &mut self.edit_tab;
        let ins = &mut self.insert_state;
        let mut action = None;
        if let Some(v) = self.viewer.as_mut() {
            egui::Panel::bottom("edit_bottom_bar")
                .frame(frame)
                .resizable(false)
                .show(ui, |ui| action = edit_bar::show(ui, v, tab, ins, accent));
        }
        let cx = doc_screen::ScreenCtx { accent, dark: self.mode == theme::Mode::Dark, scratch: self.scratch.clone(), top_inset: 0.0, bottom_inset: bottom };
        if let Some(o) = self.other.as_mut() {
            egui::Panel::bottom("edit_bottom_bar").frame(frame).resizable(false).show(ui, |ui| action = o.edit_bar(ui, &cx));
        }
        if let Some(a) = action {
            self.file_action(a);
        }
    }

    /// A file arrived while the open document has unsaved changes: ask before replacing it.
    fn incoming_prompt(&mut self, ctx: &egui::Context) {
        let Some((name, _, _)) = self.incoming.as_ref() else { return };
        if !self.doc_dirty() {
            // Saved (or closed) in the meantime: nothing to lose any more.
            if let Some((name, uri, bytes)) = self.incoming.take() {
                self.open_bytes(&name, &uri, &bytes);
            }
            return;
        }
        let p = self.mode.palette();
        let screen = ctx.content_rect();
        let width = (screen.width() - 32.0).min(420.0);
        let text = format!("Open {name}?
{} has unsaved changes. Save it first, or they will be lost.", self.doc_name());
        let (mut keep, mut open) = (false, false);
        egui::Area::new(egui::Id::unique("incoming-file")).order(egui::Order::Tooltip).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .corner_radius(16)
                .inner_margin(16)
                .shadow(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui.label(egui::RichText::new(text).size(15.0));
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        keep = ui.add(egui::Button::new("Keep editing").min_size(egui::vec2(0.0, 44.0))).clicked();
                        open = ui.add(egui::Button::new("Discard changes and open").min_size(egui::vec2(0.0, 44.0))).clicked();
                    });
                });
        });
        if keep {
            self.incoming = None;
        } else if open {
            if let Some((name, uri, bytes)) = self.incoming.take() {
                self.open_bytes(&name, &uri, &bytes);
            }
        }
    }

    fn status_strip(&self, ui: &mut egui::Ui) {
        if self.status.is_empty() {
            return;
        }
        let frame = egui::Frame::new().fill(self.mode.palette().surface).inner_margin(10);
        egui::Panel::bottom("status_strip").frame(frame).show(ui, |ui| {
            ui.label(crate::rebrand::rename(&self.status));
        });
    }
}

impl eframe::App for SlateApp {
    // Runs before egui processes input each frame: feed in on-screen keyboard typing.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw_input: &mut egui::RawInput) {
        #[cfg(target_os = "android")]
        if let Some(ime) = self.ime.as_mut() {
            ime.pump(_ctx, _raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.started {
            self.startup();
        }
        if self.applied != Some((self.mode, self.tool)) {
            theme::apply(ui.ctx(), self.mode, self.tool);
            self.applied = Some((self.mode, self.tool));
        }
        #[cfg(target_os = "android")]
        {
            self.poll_android_open(ui.ctx());
            self.handle_back();
            // Dark status-bar icons only on the light start screen (the viewer bar is coloured).
            let awake = self.settings.keep_awake && self.has_doc();
            if self.awake_sent != Some(awake) {
                if let Some(app) = self.android.as_ref() {
                    match android_bridge::set_keep_screen_on(app, awake) {
                        Ok(()) => self.awake_sent = Some(awake),
                        Err(e) => log::warn!("keep screen on: {e}"),
                    }
                }
            }
            let dark_icons = !self.has_doc() && self.app_home.is_none() && self.mode == theme::Mode::Light;
            if self.dark_icons_sent != Some(dark_icons) {
                if let Some(app) = self.android.as_ref() {
                    match android_bridge::set_dark_status_icons(app, dark_icons) {
                        Ok(()) => self.dark_icons_sent = Some(dark_icons),
                        Err(e) => log::warn!("status icons: {e}"),
                    }
                }
            }
        }
        let inset = self.top_inset(ui.ctx());
        // Note when a new message appears (toasts fade out after a few seconds).
        if self.status != self.last_status {
            self.last_status = self.status.clone();
            self.status_since = ui.ctx().input(|i| i.time);
        }

        // A "Saved …" note is out of date as soon as the document changes again.
        if self.doc_dirty() && self.status.starts_with("Saved ") {
            self.status.clear();
        }
        let mut cx = self.screen_ctx();
        cx.top_inset = inset;
        if self.doc_editing() {
            // Edit mode: fixed bars (you need them while editing).
            self.edit_top_bar(ui, inset);
            let bottom = self.bottom_inset(ui.ctx());
            self.edit_bottom_bar(ui, bottom);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(self.mode.palette().backdrop))
                .show(ui, |ui| {
                    if let Some(v) = self.viewer.as_mut() {
                        v.show(ui);
                    }
                    if let Some(o) = self.other.as_mut() {
                        o.show(ui, &cx);
                    }
                });
            chrome::toast(ui.ctx(), &self.status, bottom + 130.0);
            // Messages fade after a few seconds in edit mode too.
            let now = ui.ctx().input(|i| i.time);
            if !self.status.is_empty() && now - self.status_since > 4.0 {
                self.status.clear();
            } else if !self.status.is_empty() {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
        } else if self.has_doc() {
            // Viewer mode: the document fills the screen; bars float on top and auto-hide.
            let ctx = ui.ctx().clone();
            let now = ctx.input(|i| i.time);
            // Grid/Deck/PDF keep their content (sheet tabs…) above the gesture/navigation bar.
            let nav = if self.other.is_some() { self.bottom_inset(&ctx).min(120.0) } else { 0.0 };
            cx.bottom_inset = nav;
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(self.mode.palette().backdrop).inner_margin(egui::Margin { top: inset as i8, bottom: nav as i8, ..Default::default() }))
                .show(ui, |ui| {
                    if let Some(v) = self.viewer.as_mut() {
                        v.show(ui);
                    }
                    if let Some(o) = self.other.as_mut() {
                        o.show(ui, &cx);
                    }
                });
            let (tapped, scrolled, page, pages, searching) = match (self.viewer.as_mut(), self.other.as_mut()) {
                (Some(v), _) => {
                    let t = std::mem::take(&mut v.tapped);
                    (t, v.scrolled, v.current_page, v.page_count(), v.search_open)
                }
                (None, Some(o)) => {
                    let (cur, total) = o.position();
                    (o.take_tapped(), o.scrolled(), cur, total, o.search_open())
                }
                _ => (false, false, 0, 0, false),
            };
            if tapped {
                self.bars.toggle(now);
            }
            self.bars.pinned = searching || !self.status.is_empty() || self.outline.is_some() || self.bar_menu_open;
            let vis = self.bars.update(&ctx, scrolled);
            self.viewer_bar(&ctx, inset, vis);
            self.outline_sheet(&ctx, inset);
            // Search row floats under the bar while search is open.
            if searching {
                let screen = ctx.content_rect();
                let width = (screen.width() - 16.0).min(720.0);
                let pos = egui::pos2(screen.center().x - width / 2.0, inset + 72.0);
                let surface = self.mode.palette().surface;
                if let Some(v) = self.viewer.as_mut() {
                    egui::Area::new(egui::Id::unique("viewer-search")).order(egui::Order::Foreground).fixed_pos(pos).show(&ctx, |ui| {
                        egui::Frame::new().fill(surface).corner_radius(14).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                            ui.set_width(width - 20.0);
                            v.search_bar(ui);
                        });
                    });
                }
                if let Some(o) = self.other.as_mut() {
                    egui::Area::new(egui::Id::unique("viewer-search")).order(egui::Order::Foreground).fixed_pos(pos).show(&ctx, |ui| {
                        egui::Frame::new().fill(surface).corner_radius(14).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                            ui.set_width(width - 20.0);
                            o.search_bar(ui);
                        });
                    });
                }
            }
            // Page counter while scrolling (and a moment after).
            if scrolled {
                self.last_scroll = now;
            }
            let pill = ctx.animate_bool_with_time(egui::Id::unique("page-pill"), now - self.last_scroll < 1.2, 0.25);
            if pill > 0.0 {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            chrome::page_pill(&ctx, &format!("{} / {}", page + 1, pages.max(1)), pill, 0.0);
            chrome::toast(&ctx, &self.status, 0.0);
            if !self.status.is_empty() && now - self.status_since > 4.0 {
                self.status.clear();
            }
        } else {
            #[cfg(target_os = "android")]
            if self.awaiting_launch_file {
                // "Open with" launch: plain backdrop for the moment until the file arrives.
                egui::CentralPanel::default().frame(egui::Frame::new().fill(self.mode.palette().backdrop)).show(ui, |_| {});
                return;
            }
            self.status_strip(ui);
            let now = ui.ctx().input(|i| i.time);
            if !self.status.is_empty() && now - self.status_since > 6.0 {
                self.status.clear();
            } else if !self.status.is_empty() {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
            let action = match self.app_home {
                Some(tool) => home::app_home(ui, tool, &self.settings, self.mode, inset),
                None => home::show(ui, &self.settings, self.mode, inset),
            };
            match action {
                Some(home::HomeAction::OpenApp(t)) => self.app_home = Some(t),
                Some(home::HomeAction::NewDocument) => self.new_document(),
                Some(home::HomeAction::OpenPickerAt(f)) => self.open_picker_at(f),
                Some(home::HomeAction::BackToSuite) => self.app_home = None,
                Some(home::HomeAction::OpenPicker) => self.open_picker(),
                Some(home::HomeAction::ComingSoon(t)) => self.status = format!("{} is coming soon — Word Slate comes first", t.name()),
                Some(home::HomeAction::OpenRecent(uri)) => self.open_recent(&uri),
                Some(home::HomeAction::Forget(uri)) => self.settings.forget_recent(&uri),
                Some(home::HomeAction::ToggleTheme) => self.toggle_theme(),
                Some(home::HomeAction::OpenPrefs) => self.prefs.open = true,
                None => {}
            }
            if self.app_home.is_none() || self.prefs.open {
                let bottom = self.bottom_inset(ui.ctx());
                self.prefs.show(ui.ctx(), &mut self.settings, self.mode, inset, bottom);
                #[cfg(feature = "pdf")]
                {
                    if std::mem::take(&mut self.prefs.open_ids) {
                        let author = self.settings.author.clone();
                        self.ids.get_or_insert_with(|| digital_ids::IdManager::new(&self.scratch)).show_for(digital_ids::Purpose::Manage, &author);
                    }
                    if let Some(m) = self.ids.as_mut() {
                        m.show(ui.ctx(), inset, bottom);
                    }
                }
                // Dark switch in the drawer.
                if self.settings.dark != (self.mode == theme::Mode::Dark) {
                    self.mode = self.mode.toggled();
                }
            }
        }

        // Insert › Barcode card, then the link / barcode pop-up: above everything, in every app.
        let bottom_now = self.bottom_inset(ui.ctx());
        barcode_maker::draw(ui.ctx(), inset, bottom_now);
        if let Some((path, width)) = barcode_maker::take_picture() {
            if let Some(o) = self.other.as_mut() {
                o.insert_picture(&path, width);
            }
            if let Some(v) = self.viewer.as_mut() {
                if v.run("insert.picture", serde_json::json!({ "path": path, "width": width })).is_none() {
                    self.status = "Couldn't insert the barcode".into();
                }
            }
        }
        links::draw(ui.ctx(), inset, bottom_now);
        self.incoming_prompt(ui.ctx());
        if let Some(m) = links::take_message() {
            self.status = m;
        }
        #[cfg(target_os = "android")]
        if let Some(e) = self.android.as_ref().and_then(android_bridge::take_link_error) {
            self.status = e;
        }

        // One more frame after every finger lift: a tap's effect (mode switch, menu action) may
        // only show on the next frame, and egui otherwise waits for the next input event.
        if ui.ctx().input(|i| i.pointer.any_released()) {
            ui.ctx().request_repaint();
        }
        // The keyboard bridge is polled, so keep frames coming while a text box has focus.
        if ui.ctx().memory(|m| m.focused().is_some()) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
}


fn run(options: eframe::NativeOptions, app: SlateApp) -> eframe::Result {
    eframe::run_native(
        "Document Slate",
        options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

/// Desktop entry point (used by src/main.rs). An optional file path opens that document.
pub fn run_desktop(open: Option<PathBuf>) -> eframe::Result {
    let mut app = SlateApp::new(std::env::temp_dir().join("document-slate"));
    app.pending_open = open;
    // Phone-sized window (a typical phone is ~393 x 851 dp): the app is designed for phones.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([393.0, 851.0]).with_title(std::env::var("DS_TITLE").unwrap_or_else(|_| "Document Slate (desktop test)".into())),
        ..Default::default()
    };
    run(options, app)
}

/// Let the engines use Android's installed fonts (Roboto, Noto Serif, Noto CJK, emoji…), both for
/// documents that name them and as fallback for characters the bundled fonts lack.
///
/// The WordCraft font catalogue scans desktop folders only (`/usr/share/fonts`, `$HOME/.fonts`,
/// `$HOME/.local/share/fonts`). Without editing upstream code: HOME becomes an app-private folder
/// whose `.fonts` links to `/system/fonts` and `.local/share/fonts` to `/product/fonts` (extra
/// fonts some phones ship). The catalogue only reads font *names* up front; a family's data is
/// loaded the first time a document needs it, so this costs no memory for unused fonts.
/// Must run before any thread starts (it sets an environment variable) and before any font lookup.
#[cfg(target_os = "android")]
fn android_system_fonts(scratch: &std::path::Path) {
    let home = scratch.join("home");
    let links = [(home.join(".fonts"), "/system/fonts"), (home.join(".local/share/fonts"), "/product/fonts")];
    for (link, target) in &links {
        if let Some(parent) = link.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::symlink_metadata(link).is_err() && std::path::Path::new(target).is_dir() {
            if let Err(e) = std::os::unix::fs::symlink(target, link) {
                log::warn!("font link {} -> {target}: {e}", link.display());
            }
        }
    }
    // SAFETY: called at the very start of android_main, before this process starts any thread
    // of ours (GameActivity's own threads don't read HOME).
    unsafe { std::env::set_var("HOME", &home) };
    wordcraft_fonts::FontDb::global().scan_in_background();
}

/// Running in the Android emulator? (System properties set only by the emulator's QEMU images.)
#[cfg(target_os = "android")]
fn is_emulator() -> bool {
    unsafe extern "C" {
        fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> std::ffi::c_int;
    }
    let prop = |name: &std::ffi::CStr| -> String {
        let mut buf = [0 as std::ffi::c_char; 92]; // PROP_VALUE_MAX
        // SAFETY: bionic's documented API; writes at most PROP_VALUE_MAX bytes incl. the NUL.
        let n = unsafe { __system_property_get(name.as_ptr(), buf.as_mut_ptr()) };
        if n <= 0 {
            return String::new();
        }
        let bytes: Vec<u8> = buf[..n as usize].iter().map(|c| *c as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let hw = prop(c"ro.hardware");
    let emu = prop(c"ro.kernel.qemu") == "1" || prop(c"ro.boot.qemu") == "1" || hw == "ranchu" || hw == "goldfish";
    log::info!("renderer: {} (ro.hardware={hw})", if emu { "glow (emulator)" } else { "wgpu" });
    emu
}

/// Android entry point: GameActivity loads libdocument_slate.so and calls this.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("DocumentSlate"),
    );
    let scratch = app.internal_data_path().unwrap_or_else(std::env::temp_dir);
    android_system_fonts(&scratch);
    android_bridge::set_app(&app);
    let mut slate = SlateApp::new(scratch);
    slate.android = Some(app.clone());
    slate.ime = Some(android_ime::ImeBridge::new(app.clone()));
    let options = eframe::NativeOptions {
        android_app: Some(app),
        // wgpu (Vulkan) on real devices — verified on a real phone (Adreno 620). The Android
        // emulator's wgpu surface panics with "Invalid surface" (eframe 0.37, egui PR #8850),
        // so emulators use OpenGL ES (glow).
        renderer: if is_emulator() { eframe::Renderer::Glow } else { eframe::Renderer::Wgpu },
        ..Default::default()
    };
    if let Err(e) = run(options, slate) {
        log::error!("eframe exited with error: {e}");
    }
}
