use super::*;
use crate::doc_screen::DocScreen;

fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join("document-slate-test").join("rebrand");
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every file each app can write: what leaks before the scrub, and nothing after it.
#[test]
fn saved_files_carry_no_upstream_names() {
    let d = dir();
    let mut outputs: Vec<std::path::PathBuf> = vec![];
    // Word: docx, odt, pdf.
    let mut w = crate::word_viewer::WordViewer::new_blank("Untitled.docx");
    for ext in ["docx", "odt"] {
        let p = d.join(format!("word.{ext}"));
        w.write_as(&p).unwrap();
        outputs.push(p);
    }
    let p = d.join("word.pdf");
    w.export_pdf(&p).unwrap();
    outputs.push(p);
    // Grid, Deck, Page: own format + PDF.
    let mut screens: Vec<(&str, &str, Box<dyn DocScreen>)> = vec![];
    #[cfg(feature = "grid")]
    screens.push(("grid", "xlsx", Box::new(crate::grid_ui::GridScreen::new_blank())));
    #[cfg(feature = "deck")]
    screens.push(("deck", "pptx", Box::new(crate::deck_ui::DeckScreen::new_blank())));
    #[cfg(feature = "pdf")]
    screens.push(("page", "pdf", Box::new(crate::pdf_ui::PdfScreen::new_blank())));
    for (name, ext, mut s) in screens {
        let p = d.join(format!("{name}.{ext}"));
        s.write_to(&p).unwrap();
        outputs.push(p);
        let p = d.join(format!("{name}-export.pdf"));
        s.export_pdf(&p).unwrap();
        outputs.push(p);
    }
    let mut before = vec![];
    let mut after = vec![];
    for p in &outputs {
        let b = std::fs::read(p).unwrap();
        for l in leaks(&b) {
            before.push(format!("{}: {l}", p.file_name().unwrap().to_string_lossy()));
        }
        scrub_file(p);
        let b = std::fs::read(p).unwrap();
        for l in leaks(&b) {
            after.push(format!("{}: {l}", p.file_name().unwrap().to_string_lossy()));
        }
    }
    eprintln!("BEFORE scrub:\n  {}", before.join("\n  "));
    assert!(after.is_empty(), "still named after scrub:\n  {}", after.join("\n  "));
}
