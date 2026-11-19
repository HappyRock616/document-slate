//! Unit tests: make a deck, render it, edit it, save it, reopen it, export PDF.
//! Files the tests write go to `_cleanup/deck-tests/`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::edit::{HAlign, HeaderFooter, Rect, SlideSize};
use super::touch::{ObjectKind, PointerKind, TouchHit};
use super::*;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../_cleanup/deck-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A small PNG made in code (a red/blue checker), for picture tests.
fn test_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(64, 48, |x, y| if (x / 8 + y / 8) % 2 == 0 { image::Rgba([200, 30, 30, 255]) } else { image::Rgba([30, 60, 200, 255]) });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// How many pixels differ from the slide's top-left pixel (a blank slide gives ~0).
fn varied_pixels(r: &RenderedSlide) -> usize {
    let first = &r.pixels[..4];
    r.pixels.as_chunks::<4>().0.iter().filter(|p| p.as_slice() != first).count()
}

/// The ids of the current slide's placeholders, in order.
fn placeholders(d: &DeckDoc) -> Vec<ShapeId> {
    d.objects().into_iter().filter(|o| o.placeholder).map(|o| o.id).collect()
}

/// Build a deck with every kind of v1 content, the way the app would.
fn build_deck() -> DeckDoc {
    let mut d = DeckDoc::new_blank().unwrap();
    assert_eq!(d.slide_count(), 1);
    let ph = placeholders(&d);
    d.set_text(ph[0], "Deck Slate test").unwrap();
    d.set_text(ph[1], "Made by the unit tests").unwrap();
    d.set_notes(0, "Speaker notes for slide one").unwrap();

    let i = d.new_slide(Some("titleAndContent")).unwrap();
    assert_eq!(i, 1);
    let ph = placeholders(&d);
    d.set_text(ph[0], "Second slide").unwrap();
    d.set_text(ph[1], "First point\nSecond point").unwrap();
    // Format the body: select it, bold, bigger, centred, numbered.
    d.select(&[ph[1]]).unwrap();
    d.bold(Some(true)).unwrap();
    d.font_size(28.0).unwrap();
    d.align(HAlign::Center).unwrap();
    d.numbering(Some(true)).unwrap();
    d.font_color("#C00000").unwrap();

    d.new_slide(Some("blank")).unwrap();
    d.insert_text_box(Some(Rect::new(40.0, 40.0, 400.0, 60.0)), "A text box").unwrap();
    d.insert_shape("star5", Rect::new(500.0, 60.0, 200.0, 200.0), Some("Star")).unwrap();
    let t = d.insert_table(2, 3, Some(Rect::new(40.0, 300.0, 400.0, 120.0))).unwrap();
    d.table_insert_row(t, 1, false).unwrap();
    assert_eq!(d.table_size(t), Some((3, 3)));
    d.insert_picture(&test_png(), "checker.png", Some(Rect::new(600.0, 320.0, 160.0, 120.0))).unwrap();
    d.set_header_footer(&HeaderFooter { slide_number: true, footer: Some("Footer text".into()), ..Default::default() }).unwrap();
    d.set_slide_hidden(2, true).unwrap();
    d
}

#[test]
fn new_deck_renders_non_blank() {
    let d = build_deck();
    for i in 0..d.slide_count() {
        let r = d.render_slide(i, 0.5).unwrap();
        assert_eq!((r.width, r.height), (480, 270), "16:9 at half size");
        assert_eq!(r.pixels.len(), 480 * 270 * 4);
        assert!(varied_pixels(&r) > 500, "slide {i} looks blank");
    }
    std::fs::write(scratch("slide3.png"), d.render_slide(2, 1.0).unwrap().to_png()).unwrap();
    // Fit into a thumbnail box.
    let t = d.render_slide_fit(0, 160, 160).unwrap();
    assert_eq!(t.width, 160);
    assert_eq!(t.height, 90);
    // Bad index is an error, not a crash.
    assert!(d.render_slide(99, 1.0).is_err());
}

#[test]
fn edit_save_reopen_keeps_everything() {
    let mut d = build_deck();
    assert!(d.is_dirty());
    let out = d.save_bytes(SaveFormat::Pptx).unwrap();
    assert!(out.warnings.is_empty(), "a new deck has nothing to lose: {:?}", out.warnings);
    assert!(!d.is_dirty(), "a full save marks the document saved");
    std::fs::write(scratch("roundtrip.pptx"), &out.bytes).unwrap();

    let mut r = DeckDoc::open_bytes("roundtrip.pptx", &out.bytes).unwrap();
    assert_eq!(r.source_format(), SourceFormat::Pptx);
    let slides = r.slides();
    assert_eq!(slides.len(), 3);
    assert_eq!(slides[0].title, "Deck Slate test");
    assert_eq!(slides[1].title, "Second slide");
    assert!(slides[2].hidden);
    assert_eq!(r.show_order(), vec![0, 1]);
    assert_eq!(r.notes(0), "Speaker notes for slide one");
    assert!(r.slide_text(1).contains("Second point"));
    r.go_to_slide(2).unwrap();
    let kinds: Vec<ObjectKind> = r.objects().iter().map(|o| o.kind).collect();
    for k in [ObjectKind::Shape, ObjectKind::Table, ObjectKind::Picture] {
        assert!(kinds.contains(&k), "{k:?} missing after reopen: {kinds:?}");
    }
    assert!(r.slide_text(2).contains("A text box"));
    // The re-opened engine file must also survive another save untouched.
    assert!(r.fidelity_report().is_lossless(), "{:?}", r.fidelity_report());

    // Edit the reopened deck and save again: the edit is there.
    r.go_to_slide(0).unwrap();
    let ph = placeholders(&r);
    r.set_text(ph[0], "Edited title").unwrap();
    let again = r.save_bytes(SaveFormat::Pptx).unwrap();
    let r2 = DeckDoc::open_bytes("again.pptx", &again.bytes).unwrap();
    assert_eq!(r2.slides()[0].title, "Edited title");

    // Native format round trip too.
    let native = r.save_bytes(SaveFormat::Native).unwrap();
    let r3 = DeckDoc::open_bytes("x.deckcraft", &native.bytes).unwrap();
    assert_eq!(r3.source_format(), SourceFormat::Native);
    assert_eq!(r3.slide_count(), 3);
    // Outline is an export: warns, doesn't count as a save.
    r.set_notes(0, "changed").unwrap();
    let txt = r.save_bytes(SaveFormat::OutlineText).unwrap();
    assert!(String::from_utf8_lossy(&txt.bytes).contains("Edited title"));
    assert!(!txt.warnings.is_empty());
    assert!(r.is_dirty());
}

#[test]
fn sample_deck_round_trips_without_loss() {
    // The engine's sample has transitions, gradients, charts and notes.
    let mut d = DeckDoc::sample().unwrap();
    assert!(d.slide_count() >= 3);
    let bytes = d.save_bytes(SaveFormat::Pptx).unwrap().bytes;
    std::fs::write(scratch("sample.pptx"), &bytes).unwrap();
    let mut r = DeckDoc::open_bytes("sample.pptx", &bytes).unwrap();
    assert_eq!(r.slide_count(), d.slide_count());
    let report = r.fidelity_report().clone();
    assert!(report.is_lossless(), "engine-written sample loses on re-save: {report:?}");
}

#[test]
fn slide_operations_and_undo() {
    let mut d = build_deck();
    let titles = |d: &DeckDoc| d.slides().into_iter().map(|s| s.title).collect::<Vec<_>>();
    d.duplicate_slide(0).unwrap();
    assert_eq!(d.slide_count(), 4);
    assert_eq!(titles(&d)[1], "Deck Slate test");
    d.move_slide(0, 2).unwrap();
    assert_eq!(titles(&d)[2], "Deck Slate test");
    assert_eq!(titles(&d)[0], "Deck Slate test", "the duplicate is now first");
    d.delete_slide(0).unwrap();
    assert_eq!(d.slide_count(), 3);
    assert!(d.can_undo());
    d.undo().unwrap();
    assert_eq!(d.slide_count(), 4);
    assert!(d.can_redo());
    d.redo().unwrap();
    assert_eq!(d.slide_count(), 3);

    assert!(d.layout_names().len() >= 5, "{:?}", d.layout_names());
    d.set_slide_layout(0, "titleOnly").unwrap();
    assert_eq!(d.slides()[0].layout, "Title Only");
    d.reset_slide(0).unwrap();
    d.set_slide_hidden(0, true).unwrap();
    assert!(d.slides()[0].hidden);
}

#[test]
fn design_theme_background_and_size() {
    let mut d = build_deck();
    let themes = DeckDoc::theme_names();
    assert!(themes.len() > 1);
    let before = d.render_slide(0, 0.25).unwrap();
    d.apply_theme(&themes[1]).unwrap();
    d.background_color("#103050", true).unwrap();
    let after = d.render_slide(0, 0.25).unwrap();
    assert_ne!(before.pixels, after.pixels, "theme + background change the picture");
    // The background colour shows in a corner (slide 1 has nothing in its corner).
    let px = &after.to_straight()[..4];
    assert_eq!(px, &[0x10, 0x30, 0x50, 255]);
    d.background_picture(&test_png(), false).unwrap();
    d.reset_background(true).unwrap();
    assert!(d.design_galleries().unwrap().is_object());
    d.set_slide_size(SlideSize::Standard).unwrap();
    assert_eq!(d.slide_size(), (720.0, 540.0));
}

#[test]
fn touch_select_move_and_handles() {
    let mut d = DeckDoc::new_blank().unwrap();
    d.new_slide(Some("blank")).unwrap();
    let id = d.insert_shape("rect", Rect::new(100.0, 100.0, 200.0, 100.0), None).unwrap();
    d.deselect().unwrap();
    // A finger on the rectangle's middle finds it; empty space finds nothing.
    assert!(matches!(d.hit_test(200.0, 150.0, 8.0), Some(TouchHit::Object { id: hit, .. }) if hit == id));
    assert_eq!(d.hit_test(800.0, 500.0, 8.0), None);
    // Drag it 50 pt right with the engine's pointer tool: one undo step.
    d.pointer(PointerKind::Down, 200.0, 150.0, 8.0).unwrap();
    d.pointer(PointerKind::Drag, 225.0, 150.0, 8.0).unwrap();
    d.pointer(PointerKind::Drag, 250.0, 150.0, 8.0).unwrap();
    d.pointer(PointerKind::Up, 250.0, 150.0, 8.0).unwrap();
    assert_eq!(d.selected(), vec![id]);
    let o = d.objects().into_iter().find(|o| o.id == id).unwrap();
    assert!((o.rect.0 - 150.0).abs() < 7.0, "moved by about 50 pt (snapping allowed): {:?}", o.rect);
    // Handles: SE corner at the box's bottom-right; touching it is a resize hit.
    let h = d.selection_handles();
    assert_eq!(h.len(), 1);
    let (sx, sy) = h[0].resize[4];
    assert!((sx - (o.rect.0 + o.rect.2)).abs() < 0.01 && (sy - (o.rect.1 + o.rect.3)).abs() < 0.01);
    assert!(matches!(d.hit_test(sx, sy, 8.0), Some(TouchHit::Resize { handle: 4, .. })));
    // Simple helpers.
    d.set_bounds(&[id], Rect::new(10.0, 20.0, 30.0, 40.0)).unwrap();
    d.rotate_to(&[id], 45.0).unwrap();
    d.fill_color(&[id], Some("accent2")).unwrap();
    d.outline(&[id], Some("#000000"), Some(3.0)).unwrap();
    let o = d.objects().into_iter().find(|o| o.id == id).unwrap();
    assert_eq!(o.rect, (10.0, 20.0, 30.0, 40.0));
    assert_eq!(o.rotation, 45.0);
    d.duplicate_objects(&[id]).unwrap();
    assert_eq!(d.objects().len(), 2);
    d.send_to_back(&[id]).unwrap();
    assert_eq!(d.objects()[0].id, id);
    d.delete_objects(&[id]).unwrap();
    assert_eq!(d.objects().len(), 1);
}

#[test]
fn typing_text() {
    let mut d = DeckDoc::new_blank().unwrap();
    let ph = placeholders(&d);
    d.edit_text(ph[0], None).unwrap();
    assert_eq!(d.editing_text(), Some(ph[0]));
    d.type_text("Hellp").unwrap();
    d.backspace().unwrap();
    d.type_text("o").unwrap();
    d.select_all_text().unwrap();
    d.italic(None).unwrap();
    assert_eq!(d.format_state().unwrap()["italic"], serde_json::json!(true));
    d.exit_text().unwrap();
    assert_eq!(d.text_of(ph[0]).unwrap(), "Hello");
    assert_eq!(d.slides()[0].title, "Hello");
}

#[test]
fn pdf_export_slides_notes_handouts() {
    let d = build_deck();
    let slides = d.pdf_bytes(&PdfOptions { dpi: 72.0, ..Default::default() }).unwrap();
    assert!(slides.starts_with(b"%PDF"));
    let handouts = d.pdf_bytes(&PdfOptions { layout: PdfLayout::Handouts { per_page: 6 }, dpi: 72.0, ..Default::default() }).unwrap();
    assert!(handouts.starts_with(b"%PDF"));
    let notes = d.pdf_bytes(&PdfOptions { layout: PdfLayout::Notes, include_hidden: true, dpi: 72.0, ..Default::default() }).unwrap();
    assert!(notes.starts_with(b"%PDF"));
    d.export_pdf(&scratch("handouts.pdf"), &PdfOptions { layout: PdfLayout::Handouts { per_page: 3 }, ..Default::default() }).unwrap();
    assert!(std::fs::metadata(scratch("handouts.pdf")).unwrap().len() > 1000);
    // Bad per-page count is an error.
    assert!(d.pdf_bytes(&PdfOptions { layout: PdfLayout::Handouts { per_page: 5 }, ..Default::default() }).is_err());
}

/// Rebuild a zip, adding extra entries (name, bytes).
fn zip_with_extra(bytes: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let mut data = vec![];
        f.read_to_end(&mut data).unwrap();
        out.start_file(f.name().to_string(), opts).unwrap();
        out.write_all(&data).unwrap();
    }
    for (name, data) in extra {
        out.start_file(*name, opts).unwrap();
        out.write_all(data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn fidelity_check_reports_what_a_save_would_drop() {
    let mut d = build_deck();
    let clean = d.save_bytes(SaveFormat::Pptx).unwrap().bytes;
    // A macro project and an embedded file the engine has no place for (nothing refers to them,
    // so it can't keep them): the check must notice they'd be gone.
    let tampered = zip_with_extra(&clean, &[("ppt/vbaProject.bin", b"fake macros"), ("ppt/embeddings/oleObject9.bin", &[7u8; 3000])]);
    let mut t = DeckDoc::open_bytes("macro.pptm", &tampered).unwrap();
    let report = t.fidelity_report().clone();
    let kinds: Vec<&str> = report.issues.iter().map(|i| i.kind.as_str()).collect();
    assert!(kinds.contains(&"macros (VBA)"), "{report:?}");
    assert!(kinds.contains(&"embedded objects or chart data"), "{report:?}");
    // Saving passes the warnings on.
    let out = t.save_bytes(SaveFormat::Pptx).unwrap();
    assert!(out.warnings.iter().any(|w| w.contains("macros")), "{:?}", out.warnings);

    // compare() directly: the tampered package has more than the clean one, never the reverse.
    let a = fidelity::compare(&tampered, &clean);
    assert!(!a.is_lossless());
    let b = fidelity::compare(&clean, &clean);
    assert!(b.is_lossless(), "{b:?}");
}

#[test]
fn unsupported_formats_say_so() {
    let mut odp = b"PK\x03\x04".to_vec();
    odp.resize(30, 0);
    odp.extend_from_slice(b"mimetypeapplication/vnd.oasis.opendocument.presentation");
    let e = DeckDoc::open_bytes("talk.odp", &odp).err().unwrap();
    assert!(e.contains(".odp"), "{e}");
    let mut ppt = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ppt.resize(4096, 0);
    let e = DeckDoc::open_bytes("old.ppt", &ppt).err().unwrap();
    assert!(e.contains(".ppt"), "{e}");
    assert!(DeckDoc::open_bytes("junk.pptx", b"not a deck").is_err());
    assert_eq!(SaveFormat::from_file_name("x.odp"), None);
    assert_eq!(SaveFormat::from_file_name("X.PPTX"), Some(SaveFormat::Pptx));
}

#[test]
fn outline_text_opens_as_slides() {
    let d = DeckDoc::open_bytes("notes.md", b"# Hello\n- one\n- two\n# World\n").unwrap();
    assert_eq!(d.source_format(), SourceFormat::Outline);
    assert_eq!(d.slide_count(), 2);
    assert_eq!(d.slides()[1].title, "World");
}

#[test]
fn insert_link_and_comment() {
    let mut d = build_deck();
    d.go_to_slide(1).unwrap();
    d.deselect().unwrap();
    d.insert_link("https://example.org", Some("Example")).unwrap();
    d.add_comment("Check this slide").unwrap();
    assert_eq!(d.slides()[1].comment_count, 1);
    let bytes = d.save_bytes(SaveFormat::Pptx).unwrap().bytes;
    let r = DeckDoc::open_bytes("c.pptx", &bytes).unwrap();
    assert_eq!(r.slides()[1].comment_count, 1);
    assert!(r.slide_text(1).contains("Example"));
}

#[test]
fn copy_paste_objects() {
    let mut d = DeckDoc::new_blank().unwrap();
    d.new_slide(Some("blank")).unwrap();
    let id = d.insert_shape("ellipse", Rect::new(50.0, 50.0, 100.0, 100.0), Some("Copy me")).unwrap();
    d.select(&[id]).unwrap();
    d.copy().unwrap();
    d.paste().unwrap();
    assert_eq!(d.objects().len(), 2);
    d.cut().unwrap();
    assert_eq!(d.objects().len(), 1);
    d.paste_text("plain text").unwrap();
    assert!(d.objects().len() >= 2);
}

/// New decks must not carry the upstream app name (a trademark) as their theme or master name.
/// (docProps/app.xml still says Application=DeckCraft: hard-coded in the upstream pptx writer.)
#[test]
fn new_deck_has_no_upstream_name() {
    let mut d = DeckDoc::new_blank().unwrap();
    let out = d.save_bytes(SaveFormat::Pptx).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(out.bytes.clone())).unwrap();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        if !(f.name().starts_with("ppt/theme/") || f.name().starts_with("ppt/slideMasters/")) {
            continue;
        }
        let mut s = String::new();
        if std::io::Read::read_to_string(&mut f, &mut s).is_ok() {
            let at = s.find("DeckCraft").map(|i| s[i.saturating_sub(80)..(i + 40).min(s.len())].to_string());
            assert!(at.is_none(), "{} names DeckCraft: {:?}", f.name(), at);
        }
    }
}

/// Links under a point: a linked shape gives its address, a linked text box gives the slide it
/// leads to, empty slide space gives nothing; still so after a .pptx save + reopen.
#[test]
fn links_under_a_point() {
    let mut d = DeckDoc::new_blank().unwrap();
    d.new_slide(Some("blank")).unwrap();
    d.new_slide(Some("blank")).unwrap();
    d.go_to_slide(1).unwrap();
    let shape = d.insert_shape("rect", Rect::new(100.0, 100.0, 200.0, 100.0), None).unwrap();
    d.select(&[shape]).unwrap();
    d.insert_link("https://example.org/deck", None).unwrap();
    let tb = d.insert_text_box(Some(Rect::new(400.0, 300.0, 300.0, 60.0)), "Go to slide three").unwrap();
    d.select(&[tb]).unwrap();
    d.insert_slide_link(2, None).unwrap();
    d.deselect().unwrap();

    let check = |d: &DeckDoc| {
        assert_eq!(d.link_at(1, 200.0, 150.0), Some(links::DeckLink::Url("https://example.org/deck".into())));
        // The text box's text sits near its top-left corner.
        let found = (0..12).find_map(|i| d.link_at(1, 420.0 + i as f64 * 10.0, 320.0));
        assert_eq!(found, Some(links::DeckLink::Slide(2)));
        assert_eq!(d.link_at(1, 40.0, 480.0), None, "empty space has no link");
        assert_eq!(d.link_at(0, 200.0, 150.0), None, "other slide");
    };
    check(&d);
    let bytes = d.save_bytes(SaveFormat::Pptx).unwrap().bytes;
    let r = DeckDoc::open_bytes("links.pptx", &bytes).unwrap();
    check(&r);
}
