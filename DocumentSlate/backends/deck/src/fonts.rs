//! Fonts for drawing slides.
//!
//! The engine looks for installed fonts in desktop folders (`/usr/share/fonts` on Linux-like
//! systems). Android keeps its fonts in `/system/fonts`, which the engine doesn't know, so on the
//! phone it would only have its one built-in last-resort font (Ubuntu Light) unless:
//! - the app was built with `CRAFT_FONTS_DIR` pointing at a checkout of storytold/craft-fonts
//!   (then the engine embeds its open-licence presentation fonts: Inter, Carlito, Liberation…), or
//! - the app adds fonts at startup with the functions here (Android's own Roboto/Noto Serif, or
//!   OFL fonts the app already ships, e.g. Inter from WordSlate's assets).
//!
//! Call these once, early (before opening a document), ideally on a background thread.

/// Add a font from its file bytes (TTF, OTF or TTC). Returns how many faces were added
/// (0 = not a font, or already there).
pub fn add_font_bytes(bytes: Vec<u8>) -> usize {
    deckcraft_fonts::FontDb::global().add_font(bytes)
}

/// Add a font file from disk. Files over 32 MB are skipped (huge CJK collections would cost
/// too much memory: the engine keeps loaded fonts in RAM).
pub fn add_font_file(path: &std::path::Path) -> usize {
    const MAX: u64 = 32 << 20;
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() <= MAX => std::fs::read(path).map(add_font_bytes).unwrap_or(0),
        _ => 0,
    }
}

/// Android system font files worth loading for slides: Roboto (sans), Noto Serif, monospace and
/// a few display faces. File names differ between Android versions, so we match by prefix.
const ANDROID_FONT_PREFIXES: &[&str] =
    &["Roboto", "NotoSerif-", "NotoSerif[", "DroidSans", "CutiveMono", "SourceSansPro", "CarroisGothicSC", "ComingSoon", "DancingScript"];

/// Load the useful Latin fonts from Android's `/system/fonts` (see the list above). Returns the
/// number of faces added. Does nothing (returns 0) where that folder doesn't exist.
pub fn load_android_system_fonts() -> usize {
    load_fonts_from_dir(std::path::Path::new("/system/fonts"), ANDROID_FONT_PREFIXES)
}

/// Load font files in `dir` whose names start with one of `prefixes` (all fonts when empty).
pub fn load_fonts_from_dir(dir: &std::path::Path, prefixes: &[&str]) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut added = 0;
    // At most 400 files: a safety limit, a phone has ~250 font files.
    for e in entries.flatten().take(400) {
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        let is_font = lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc");
        if is_font && (prefixes.is_empty() || prefixes.iter().any(|p| name.starts_with(p))) {
            added += add_font_file(&e.path());
        }
    }
    added
}

/// Font family names the engine can draw with right now (for the Text tab's font picker).
pub fn font_families() -> Vec<String> {
    deckcraft_fonts::FontDb::global().families()
}
