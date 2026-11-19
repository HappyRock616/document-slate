//! Size probe: a tiny program that uses the whole back-end (open, render, edit, save, PDF), so
//! building it for Android shows roughly how much the engine adds to the app.
//!   cargo ndk -t arm64-v8a --platform 26 build --profile release-small --example size_probe
//! Also handy on the desktop: `cargo run --example size_probe -- deck.pptx out-dir`.
use slate_deck::{DeckDoc, PdfOptions, SaveFormat};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    slate_deck::fonts::load_android_system_fonts();
    let mut doc = match args.get(1) {
        Some(path) => DeckDoc::open_bytes(path, &std::fs::read(path).map_err(|e| e.to_string())?)?,
        None => DeckDoc::sample()?,
    };
    let out = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_else(|| ".".into()));
    for (i, s) in doc.slides().iter().enumerate() {
        println!("slide {}: {} {}", i + 1, s.title, if s.hidden { "(hidden)" } else { "" });
    }
    let first = doc.render_slide(0, 1.0)?;
    std::fs::write(out.join("slide1.png"), first.to_png()).map_err(|e| e.to_string())?;
    doc.new_slide(Some("titleAndContent"))?;
    for issue in &doc.fidelity_report().issues {
        println!("fidelity: {}", issue.message);
    }
    doc.save_to_path(&out.join("probe.pptx"), SaveFormat::Pptx)?;
    doc.export_pdf(&out.join("probe.pdf"), &PdfOptions::default())?;
    Ok(())
}
