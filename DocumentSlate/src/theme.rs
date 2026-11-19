//! Document Slate look: dark by default (two-tone grey, from the app icon's slate and chalk),
//! light mode (slate grey on a very light grey-white), and one accent per app for the heading bar.
//! Colours are our own picks — not copied from Microsoft/Adobe/WPS palettes.

use eframe::egui::{self, Color32};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Dark,
    Light,
}

/// Which tool is showing; decides the heading-bar accent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Word,
    Grid,
    Deck,
    Page,
}

/// Font family for the app names ("Word Slate"…): Audiowide (OFL, assets/fonts/).
pub fn brand_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("audiowide".into()))
}

/// Register Audiowide alongside egui's default fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "audiowide".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Audiowide-Regular.ttf"))),
    );
    fonts.families.insert(egui::FontFamily::Name("audiowide".into()), vec!["audiowide".to_owned()]);
    // Inter SemiBold (OFL, from upstream WordCraft's assets): the "semibold" family upstream's icons use.
    fonts.font_data.insert(
        "inter-semibold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))),
    );
    fonts.families.insert(egui::FontFamily::Name("semibold".into()), vec!["inter-semibold".to_owned()]);
    ctx.set_fonts(fonts);
}

/// Upstream helper used by src/icons.rs: Inter SemiBold at `size`.
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

/// Charcoal grey for the app names, outlined in black (the start-screen design).
pub const BRAND_TEXT: Color32 = Color32::from_rgb(0x36, 0x3B, 0x40);

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Word, Tool::Grid, Tool::Deck, Tool::Page];

    /// "Word Slate", "Grid Slate"…
    pub fn name(self) -> &'static str {
        match self {
            Tool::Word => "Word Slate",
            Tool::Grid => "Grid Slate",
            Tool::Deck => "Deck Slate",
            Tool::Page => "Page Slate",
        }
    }

    /// What it opens, shown under the name.
    pub fn blurb(self) -> &'static str {
        match self {
            Tool::Word => "Documents · DOCX, ODT, RTF, TXT",
            Tool::Grid => "Spreadsheets · XLSX, ODS, CSV",
            Tool::Deck => "Presentations · PPTX, PPSX",
            Tool::Page => "PDF files",
        }
    }

    /// Ready to use yet? (Grid/Deck/Page are still being built.)
    pub fn available(self) -> bool {
        crate::doc_screen::built_in(self)
    }

    /// Heading-bar colour: Word blue, Grid green, Deck orange, Page (PDF) red.
    pub fn accent(self) -> Color32 {
        match self {
            Tool::Word => Color32::from_rgb(0x2F, 0x6D, 0xB5),
            Tool::Grid => Color32::from_rgb(0x2E, 0x8B, 0x57),
            Tool::Deck => Color32::from_rgb(0xD9, 0x73, 0x2B),
            Tool::Page => Color32::from_rgb(0xC2, 0x42, 0x3A),
        }
    }
}

/// Text/icons drawn on top of an accent bar.
pub const ON_ACCENT: Color32 = Color32::from_rgb(0xF7, 0xF8, 0xF9);

pub struct Palette {
    /// Behind the document pages.
    pub backdrop: Color32,
    /// Bars, panels, menus.
    pub surface: Color32,
    /// Buttons and text fields at rest.
    pub control: Color32,
    pub text: Color32,
}

impl Mode {
    pub fn palette(self) -> Palette {
        match self {
            // Two-tone grey: icon slate for surfaces, a deeper grey behind the pages, chalk text.
            Mode::Dark => Palette {
                backdrop: Color32::from_rgb(0x1E, 0x21, 0x24),
                surface: Color32::from_rgb(0x2B, 0x2F, 0x33),
                control: Color32::from_rgb(0x3A, 0x3F, 0x44),
                text: Color32::from_rgb(0xD9, 0xDD, 0xE0),
            },
            // Slate grey on a very light grey-white.
            Mode::Light => Palette {
                backdrop: Color32::from_rgb(0xE4, 0xE7, 0xEA),
                surface: Color32::from_rgb(0xF5, 0xF6, 0xF7),
                control: Color32::from_rgb(0xDD, 0xE1, 0xE5),
                text: Color32::from_rgb(0x3D, 0x48, 0x52),
            },
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Mode::Dark => Mode::Light,
            Mode::Light => Mode::Dark,
        }
    }
}

/// Apply the palette to egui's built-in widgets.
pub fn apply(ctx: &egui::Context, mode: Mode, tool: Tool) {
    let p = mode.palette();
    let mut v = match mode {
        Mode::Dark => egui::Visuals::dark(),
        Mode::Light => egui::Visuals::light(),
    };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.surface;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.backdrop;
    v.faint_bg_color = p.control;
    v.widgets.inactive.weak_bg_fill = p.control;
    v.widgets.inactive.bg_fill = p.control;
    v.selection.bg_fill = tool.accent();
    v.hyperlink_color = tool.accent();
    ctx.set_visuals(v);
}
