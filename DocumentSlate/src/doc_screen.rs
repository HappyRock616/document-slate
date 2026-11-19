//! The common shape of a document screen for Grid, Deck and Page Slate (Word Slate predates this
//! and is wired directly in lib.rs, but follows the same rules).
//!
//! The app shell (lib.rs) owns everything the four apps share, so they look and behave alike:
//! - viewer mode: the document fills the screen, the accent bar floats on top and auto-hides
//!   (Back · title · Search · Share · Print · Edit · ⋮), a "n / N" pill shows while scrolling;
//! - edit mode: fixed accent top bar (Done · title · Undo · Redo · Save) and a bottom bar the
//!   screen fills (tool rows + tab strip, `chrome::tool` / `chrome::tool_menu` icon buttons),
//!   lifted above the keyboard;
//! - Save (over the original, with the fidelity check), Save as / Export (Android "save to"),
//!   Share, Print (PDF -> Android print dialog), Back handling, toasts.
//!
//! A screen implements this trait and only draws its document area and its edit tools.

use std::path::Path;

use eframe::egui;

use crate::edit_bar::FileAction;
use crate::theme::Tool;

/// What the shell tells a screen each frame.
pub struct ScreenCtx {
    pub accent: egui::Color32,
    /// Dark or light app theme.
    pub dark: bool,
    /// Private app folder (temporary files go under it).
    pub scratch: std::path::PathBuf,
    /// Points covered at the top (status bar) and bottom (navigation bar / keyboard) of the screen.
    pub top_inset: f32,
    pub bottom_inset: f32,
}

pub trait DocScreen {
    /// File name shown in the bars ("Budget.xlsx").
    fn name(&self) -> &str;
    fn tool(&self) -> Tool;
    /// Changed since opened / last saved?
    fn dirty(&self) -> bool;

    // ---- viewer / editor ------------------------------------------------------------------
    fn editing(&self) -> bool;
    fn set_editing(&mut self, on: bool);
    /// Draw the document (viewer or editor) into the central area.
    fn show(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx);
    /// A plain tap on the document since last asked (the shell toggles the viewer bars).
    fn take_tapped(&mut self) -> bool;
    /// Scrolled/zoomed this frame (hides the bars, shows the counter pill).
    fn scrolled(&self) -> bool;
    /// (current, total) for the counter pill: page, slide or sheet; current is 0-based.
    fn position(&self) -> (usize, usize);

    /// Back pressed: return true if the screen handled it itself (e.g. ended a slideshow,
    /// closed a sheet); false lets the shell do the usual (close search, leave edit, close).
    fn back(&mut self) -> bool {
        false
    }

    // ---- search -----------------------------------------------------------------------------
    fn search_open(&self) -> bool;
    fn toggle_search(&mut self);
    /// The search row (text field, n of N, prev/next, close), drawn by the shell under the bar.
    fn search_bar(&mut self, ui: &mut egui::Ui);

    /// Extra entries for the viewer's ⋮ menu: (label, id). `view_action(id)` runs one.
    fn view_menu(&self) -> Vec<(String, &'static str)> {
        Vec::new()
    }
    fn view_action(&mut self, _id: &str) {}

    // ---- edit mode --------------------------------------------------------------------------
    fn undo(&mut self);
    fn redo(&mut self);
    /// The bottom edit bar: tool rows and the tab strip. Returns a file action a tool asked for.
    fn edit_bar(&mut self, ui: &mut egui::Ui, cx: &ScreenCtx) -> Option<FileAction>;
    /// Your name (app preference) for new comments / annotations. Empty = leave as is.
    fn set_author(&mut self, _name: &str) {}
    /// A picture chosen in Android's photo picker (after `FileAction::PickImage`), at `path`.
    fn picked_image(&mut self, _path: &str) {}
    /// A picture made by the app (a barcode) to insert about `width_pt` points wide.
    fn insert_picture(&mut self, path: &str, _width_pt: f32) {
        self.picked_image(path);
    }

    // ---- files ------------------------------------------------------------------------------
    /// Save was pressed: return true to handle it yourself (e.g. ask "Save or Save flattened?").
    fn intercept_save(&mut self) -> bool {
        false
    }
    /// Can the open file be written back in its own format? (.ppt, .xls… can't.)
    fn can_save_in_place(&self) -> bool;
    /// Format for a new file / a Save as copy ("pptx", "xlsx", "pdf").
    fn default_ext(&self) -> &'static str;
    /// Write the document to `path` in the format of its extension, WITHOUT marking it saved.
    /// Returns what that file lacks compared with the document (fidelity check; empty = all kept).
    fn write_to(&mut self, path: &Path) -> Result<Vec<String>, String>;
    /// The file at `path` (named `name`) is now the saved document: clear the changed mark,
    /// remember its bytes as the new baseline.
    fn mark_saved(&mut self, name: &str, path: &Path);
    /// The document as a PDF (Print, Export PDF).
    fn export_pdf(&mut self, path: &Path) -> Result<(), String>;
    /// One line for File › Info ("12 slides · 16:9").
    fn info_line(&mut self) -> String {
        self.name().to_owned()
    }
}

/// Open a non-Word file in its app's screen (None: not one of theirs, or that app is switched off).
#[allow(unused_variables)]
pub fn open(tool: Tool, name: &str, bytes: &[u8]) -> Option<Result<Box<dyn DocScreen>, String>> {
    match tool {
        #[cfg(feature = "deck")]
        Tool::Deck => Some(crate::deck_ui::DeckScreen::open(name, bytes).map(|s| Box::new(s) as Box<dyn DocScreen>)),
        #[cfg(feature = "grid")]
        Tool::Grid => Some(crate::grid_ui::GridScreen::open(name, bytes).map(|s| Box::new(s) as Box<dyn DocScreen>)),
        #[cfg(feature = "pdf")]
        Tool::Page => Some(crate::pdf_ui::PdfScreen::open(name, bytes).map(|s| Box::new(s) as Box<dyn DocScreen>)),
        _ => None,
    }
}

/// A new blank document in that app ("New Deck Slate").
pub fn new_blank(tool: Tool) -> Option<Box<dyn DocScreen>> {
    match tool {
        #[cfg(feature = "deck")]
        Tool::Deck => Some(Box::new(crate::deck_ui::DeckScreen::new_blank())),
        #[cfg(feature = "grid")]
        Tool::Grid => Some(Box::new(crate::grid_ui::GridScreen::new_blank())),
        #[cfg(feature = "pdf")]
        Tool::Page => Some(Box::new(crate::pdf_ui::PdfScreen::new_blank())),
        _ => None,
    }
}

/// Is that app built into this APK?
pub fn built_in(tool: Tool) -> bool {
    match tool {
        Tool::Word => true,
        Tool::Deck => cfg!(feature = "deck"),
        Tool::Grid => cfg!(feature = "grid"),
        Tool::Page => cfg!(feature = "pdf"),
    }
}
