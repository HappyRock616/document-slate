//! End-to-end tests of the Page Slate back-end: open → view → edit → save → reopen.
//! Output PDFs are left in `_cleanup/slate-pdf-tests/` so a person can open them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{ink_pixels, make_pdf, make_png, scratch};
use slate_pdf::*;

fn three_pages() -> Vec<u8> {
    make_pdf(&["Page one Hello Slate", "Page two Lorem", "Page three Slate again"], false)
}

fn save_and_reopen(doc: &PdfDoc, file: &str) -> (SaveOutput, PdfDoc) {
    let out = doc.save_bytes().expect("save");
    std::fs::write(scratch(file), &out.bytes).unwrap();
    let again = PdfDoc::open_bytes(out.bytes.clone(), None).expect("reopen");
    (out, again)
}

#[test]
fn snapshot_can_go_to_a_worker_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<PageSnapshot>();
    let doc = PdfDoc::open_bytes(three_pages(), None).unwrap();
    let mut snap = doc.snapshot();
    let img = std::thread::spawn(move || snap.render_page(1, 0.5).unwrap()).join().unwrap();
    assert_eq!(img.width, 306);
}

#[test]
fn open_render_text_search() {
    let mut doc = PdfDoc::open_bytes(three_pages(), None).unwrap();
    assert_eq!(doc.page_count(), 3);
    let p = doc.page_info(0).unwrap();
    assert_eq!((p.width, p.height, p.rotation), (612.0, 792.0, 0));

    let img = doc.render_page(0, 1.0).unwrap();
    assert_eq!((img.width, img.height), (612, 792));
    assert_eq!(img.rgba.len(), 612 * 792 * 4);
    assert!(ink_pixels(&img.rgba) > 500, "the text and the blue line should be drawn");

    assert!(doc.page_text(0).unwrap().contains("Hello Slate"));
    let hits = doc.search("slate", false, false, 100);
    assert_eq!(hits.iter().map(|h| h.page).collect::<Vec<_>>(), vec![0, 2]);
    let r = hits[0].rects[0];
    // Text drawn at y = 700 (PDF, y up) → about 92 points from the top in view space.
    assert!(r[1] > 60.0 && r[3] < 110.0 && r[0] > 72.0, "hit rect {r:?}");
    assert_eq!(doc.search("SLATE", true, false, 100).len(), 0, "case-sensitive");
    assert_eq!(doc.search("Sla", false, true, 100).len(), 0, "whole words only");
    assert!(doc.search("Sla", false, false, 1).len() == 1, "limit");
}

#[test]
fn annotate_save_reopen_keeps_original_bytes() {
    let original = three_pages();
    let mut doc = PdfDoc::open_named("three.pdf", original.clone(), None).unwrap();
    doc.set_author("Alex");
    let hit = doc.search("Hello", false, false, 1).remove(0);
    doc.add_text_markup(hit.page, MarkupKind::Highlight, &hit.rects, Look::default(), "important").unwrap();
    doc.add_text_markup(0, MarkupKind::Underline, &hit.rects, Look::default(), "").unwrap();
    doc.add_text_markup(0, MarkupKind::Strikeout, &hit.rects, Look::default(), "").unwrap();
    doc.add_note(0, [400.0, 100.0], "A sticky note").unwrap();
    doc.add_text_box(0, [100.0, 300.0, 300.0, 340.0], "Text box words", 14.0, Look::default()).unwrap();
    doc.add_ink(0, &[vec![[100.0, 400.0], [150.0, 420.0], [200.0, 400.0]]], Look { color: Some([0.0, 0.5, 0.0]), ..Look::default() }).unwrap();
    for (i, k) in [ShapeKind::Rectangle, ShapeKind::Circle, ShapeKind::Line, ShapeKind::Arrow].into_iter().enumerate() {
        let y = 450.0 + 60.0 * i as f32;
        doc.add_shape(0, k, [100.0, y], [250.0, y + 40.0], Look::default()).unwrap();
    }
    assert!(doc.is_dirty());
    let listed = doc.comments();
    assert_eq!(listed.len(), 10, "{listed:#?}");
    assert!(listed.iter().any(|c| c.kind == "Highlight" && c.text == "important" && c.author == "Alex"));

    // The highlight shows up on the rendered page (yellow pixels where the text is).
    let img = doc.render_page(0, 1.0).unwrap();
    let px = |x: usize, y: usize| &img.rgba[(y * 612 + x) * 4..(y * 612 + x) * 4 + 4];
    let [x0, y0, x1, y1] = hit.rects[0];
    let yellow = (x0 as usize..x1 as usize).flat_map(|x| (y0 as usize..y1 as usize).map(move |y| (x, y))).filter(|&(x, y)| {
        let p = px(x, y);
        p[0] > 200 && p[1] > 200 && p[2] < 120
    });
    assert!(yellow.count() > 50, "highlight should be visible");

    let (out, mut again) = save_and_reopen(&doc, "annotated.pdf");
    assert!(out.incremental, "save must append, not rewrite");
    assert!(out.bytes.starts_with(&original), "the original file is kept byte for byte");
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    let back = again.comments();
    assert_eq!(back.len(), 10);
    assert!(back.iter().any(|c| c.kind == "Text" && c.text == "A sticky note"));
    assert!(back.iter().any(|c| c.kind == "FreeText" && c.text == "Text box words"));

    // Comment list operations.
    let note = back.iter().find(|c| c.kind == "Text").unwrap().clone();
    again.set_comment_text(note.page, note.index, "Changed").unwrap();
    assert!(again.comments().iter().any(|c| c.text == "Changed"));
    again.delete_comment(note.page, note.index).unwrap();
    assert_eq!(again.comments().len(), 9);
    assert!(again.can_undo().is_some());
    again.undo().unwrap();
    assert_eq!(again.comments().len(), 10);
    again.redo().unwrap();
    assert_eq!(again.comments().len(), 9);
}

#[test]
fn page_operations() {
    let mut doc = PdfDoc::open_bytes(three_pages(), None).unwrap();
    let text = |d: &mut PdfDoc, p: usize| d.page_text(p).unwrap();

    doc.rotate_pages(&[0], 90).unwrap();
    let p = doc.page_info(0).unwrap();
    assert_eq!((p.rotation, p.width, p.height), (90, 792.0, 612.0));
    assert!(doc.rotate_pages(&[0], 45).is_err());

    doc.move_pages(&[2], 0).unwrap(); // "three" to the front
    assert!(text(&mut doc, 0).contains("three"));
    assert!(text(&mut doc, 1).contains("one"));

    doc.delete_pages(&[2]).unwrap(); // removes "two"
    assert_eq!(doc.page_count(), 2);
    assert!(doc.delete_pages(&[0, 1]).is_err(), "can't delete every page");

    doc.insert_blank_page(1, None).unwrap();
    assert_eq!(doc.page_count(), 3);
    assert_eq!(text(&mut doc, 1).trim(), "");

    let other = make_pdf(&["Inserted A", "Inserted B"], false);
    doc.insert_pages_from("other.pdf", other, Some(vec![1]), 3).unwrap();
    assert_eq!(doc.page_count(), 4);
    assert!(text(&mut doc, 3).contains("Inserted B"));

    doc.crop_pages(&[0], [50.0, 50.0, 400.0, 300.0]).unwrap();
    let c = doc.page_info(0).unwrap();
    assert!((c.width - 350.0).abs() < 1.0 && (c.height - 250.0).abs() < 1.0, "cropped to {c:?}");
    doc.reset_crop(&[0]).unwrap();
    assert_eq!(doc.page_info(0).unwrap().width, 612.0, "page 0 is now 'three', which isn't turned");

    let extracted = doc.extract_pages(&[0, 3]).unwrap();
    let mut ex = PdfDoc::open_bytes(extracted, None).unwrap();
    assert_eq!(ex.page_count(), 2);
    assert!(ex.page_text(1).unwrap().contains("Inserted B"));

    let (out, mut again) = save_and_reopen(&doc, "pages.pdf");
    // Deleting pages makes the engine rewrite the whole file (on unsigned files), so the deleted
    // page isn't left hidden in an older revision. Everything still in the document is kept.
    assert!(!out.incremental);
    assert_eq!(again.page_count(), 4);
    assert!(again.page_text(0).unwrap().contains("three"));
    // Order now: three, blank, one (turned 90°), Inserted B.
    assert!(again.page_text(2).unwrap().contains("one"));
    assert_eq!(again.page_info(2).unwrap().rotation, 90);
}

#[test]
fn merge_pdfs() {
    let a = make_pdf(&["First A", "First B"], false);
    let b = make_pdf(&["Second A"], false);
    let merged = PdfDoc::merge(&[("a.pdf".into(), a), ("b.pdf".into(), b)]).unwrap();
    std::fs::write(scratch("merged.pdf"), &merged).unwrap();
    let mut doc = PdfDoc::open_bytes(merged, None).unwrap();
    assert_eq!(doc.page_count(), 3);
    assert!(doc.page_text(2).unwrap().contains("Second A"));
}

#[test]
fn password_protect_and_remove() {
    let mut doc = PdfDoc::open_bytes(three_pages(), None).unwrap();
    doc.protect("secret1", None).unwrap();
    let out = doc.save_bytes().unwrap();
    std::fs::write(scratch("protected.pdf"), &out.bytes).unwrap();
    assert!(matches!(PdfDoc::open_bytes(out.bytes.clone(), None), Err(PdfError::NeedsPassword)));
    assert!(matches!(PdfDoc::open_bytes(out.bytes.clone(), Some("nope")), Err(PdfError::WrongPassword)));
    let mut locked = PdfDoc::open_bytes(out.bytes, Some("secret1")).unwrap();
    assert!(locked.properties().encrypted);
    assert!(locked.page_text(0).unwrap().contains("Hello"));
    assert!(locked.permissions().print);

    locked.remove_protection().unwrap();
    let (_, open) = save_and_reopen(&locked, "unprotected.pdf");
    assert!(!open.properties().encrypted);
}

#[test]
fn forms_fill_and_save() {
    let mut doc = PdfDoc::open_bytes(make_pdf(&["Form page"], true), None).unwrap();
    let fields = doc.form_fields();
    assert_eq!(fields.len(), 2, "{fields:#?}");
    let name = fields.iter().find(|f| f.name == "name").unwrap();
    assert_eq!(name.kind, FieldKind::Text);
    assert_eq!(name.widgets[0].page, 0);
    let w = name.widgets[0].rect;
    assert!((w[0] - 72.0).abs() < 0.5 && (w[1] - 268.0).abs() < 0.5, "field rect in view space: {w:?}");

    doc.set_field_text("name", "Alex Example").unwrap();
    doc.set_field_checked("agree", true).unwrap();
    let (_, mut again) = save_and_reopen(&doc, "form-filled.pdf");
    let f = again.form_fields();
    assert_eq!(f.iter().find(|f| f.name == "name").unwrap().value, vec!["Alex Example".to_string()]);
    let agree = f.iter().find(|f| f.name == "agree").unwrap();
    assert!(agree.widgets[0].is_on, "{agree:#?}");
    // Filled text is drawn (it has an appearance), so it prints too.
    assert!(again.page_text(0).unwrap().contains("Alex Example"));
}

#[test]
fn fill_and_sign_marks() {
    let mut doc = PdfDoc::open_bytes(make_pdf(&["Flat form: Name ____"], false), None).unwrap();
    doc.add_typed_text(0, [300.0, 80.0], "Alex", 12.0, [0.0, 0.0, 0.0]).unwrap();
    doc.add_fill_mark(0, [100.0, 200.0], 14.0, FillMark::Check).unwrap();
    doc.add_fill_mark(0, [130.0, 200.0], 14.0, FillMark::Cross).unwrap();
    doc.add_drawn_signature(0, &[vec![[100.0, 650.0], [130.0, 630.0], [160.0, 660.0], [190.0, 640.0]]], None).unwrap();
    doc.add_typed_signature(0, [300.0, 650.0], "Alex", 32.0).unwrap();
    doc.add_signature_image(0, [300.0, 720.0], &make_png(40, 12, [20, 30, 160, 255]), false).unwrap();
    let marks = doc.comments();
    assert_eq!(marks.len(), 6, "{marks:#?}");
    assert!(marks.iter().all(|m| m.fill_sign), "{marks:#?}");
    let (y, m, d) = doc.today();
    assert!(y > 2000 && (1..=12).contains(&m) && (1..=31).contains(&d));

    let (_, mut again) = save_and_reopen(&doc, "signed-marks.pdf");
    assert_eq!(again.comments().len(), 6);
    let img = again.render_page(0, 1.0).unwrap();
    assert!(ink_pixels(&img.rgba) > 1500);
    // Typed text is an annotation until flattened; then it is real page text and the marks are gone.
    assert!(!again.page_text(0).unwrap().contains("Alex"), "not page text before flattening");
    again.flatten_fill_sign().unwrap();
    assert!(again.comments().is_empty(), "{:#?}", again.comments());
    let (_, mut flat) = save_and_reopen(&again, "signed-marks-flat.pdf");
    assert!(flat.comments().is_empty());
    let text = flat.page_text(0).unwrap();
    // The engine's text extraction may break the flattened word across lines: compare without
    // whitespace.
    let joined: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(joined.contains("Alex"), "typed text is real text once flattened: {text:?}");
}

#[test]
fn edit_text_images_links() {
    let original = three_pages();
    let mut doc = PdfDoc::open_bytes(original.clone(), None).unwrap();
    let lines = doc.text_lines(0).unwrap();
    let hello = lines.iter().find(|l| l.text.contains("Hello")).unwrap().clone();
    assert!(hello.font.contains("Helvetica"), "{hello:?}");
    doc.edit_text_line(0, hello.index, "Page one Goodbye Slate").unwrap();
    assert!(doc.page_text(0).unwrap().contains("Goodbye"));
    assert!(!doc.page_text(0).unwrap().contains("Hello"));

    doc.add_text(0, [72.0, 150.0, 400.0, 170.0], "Added words here", TextStyle { size: 16.0, bold: true, ..TextStyle::default() }).unwrap();
    doc.add_image(0, Some([400.0, 400.0, 500.0, 450.0]), "logo.png", make_png(20, 10, [200, 30, 30, 255])).unwrap();
    let items = doc.added_items();
    assert_eq!(items.len(), 2, "{items:#?}");
    let pic = items.iter().find(|i| i.text.is_none()).unwrap().clone();
    assert!((pic.rect[0] - 400.0).abs() < 1.0 && (pic.rect[1] - 400.0).abs() < 1.0, "{pic:?}");
    doc.set_added_rect(0, pic.index, [300.0, 500.0, 360.0, 530.0]).unwrap();
    let txt = doc.added_items().into_iter().find(|i| i.text.is_some()).unwrap();
    doc.set_added_text(0, txt.index, "Retyped words").unwrap();

    doc.add_link_uri(0, [72.0, 680.0, 200.0, 700.0], "https://example.org").unwrap();
    doc.add_link_to_page(0, [250.0, 680.0, 350.0, 700.0], 2).unwrap();
    let links = doc.links(Some(0));
    assert!(links.iter().any(|l| l.target == LinkTarget::Uri("https://example.org".into())));
    assert!(links.iter().any(|l| l.target == LinkTarget::Page(2)));

    let (out, mut again) = save_and_reopen(&doc, "edited.pdf");
    assert!(out.bytes.starts_with(&original));
    let t = again.page_text(0).unwrap();
    assert!(t.contains("Goodbye") && t.contains("Retyped words"), "{t}");
    assert_eq!(again.added_items().len(), 2);
    assert_eq!(again.links(Some(0)).len(), 2);
    assert!((again.added_items().into_iter().find(|i| i.text.is_none()).unwrap().rect[0] - 300.0).abs() < 1.0);
}

#[test]
fn properties_print_blank() {
    let mut doc = PdfDoc::create_blank(595.28, 841.89, 2).unwrap();
    assert_eq!(doc.page_count(), 2);
    doc.set_property(PropertyKey::Title, "Slate test").unwrap();
    doc.set_property(PropertyKey::Author, "Alex").unwrap();
    doc.add_note(0, [50.0, 50.0], "note to print").unwrap();
    let (_, again) = save_and_reopen(&doc, "properties.pdf");
    let p = again.properties();
    assert_eq!(p.title.as_deref(), Some("Slate test"));
    assert_eq!(p.author.as_deref(), Some("Alex"));
    assert_eq!(p.page_count, 2);
    assert!(!p.has_javascript && !p.signed);

    let print = again.print_pdf((595.28, 841.89)).unwrap();
    std::fs::write(scratch("print-ready.pdf"), &print).unwrap();
    let printed = PdfDoc::open_bytes(print, None).unwrap();
    assert_eq!(printed.page_count(), 2);
}

#[test]
fn bad_input_is_an_error_not_a_crash() {
    assert!(matches!(PdfDoc::open_bytes(b"not a pdf".to_vec(), None), Err(PdfError::NotPdf(_)) | Err(PdfError::Other(_))));
    let mut doc = PdfDoc::open_bytes(three_pages(), None).unwrap();
    assert!(doc.render_page(9, 1.0).is_err());
    assert!(doc.render_page(0, f32::NAN).is_err());
    assert!(doc.add_note(9, [0.0, 0.0], "x").is_err());
    assert!(doc.add_note(0, [f32::INFINITY, 0.0], "x").is_err());
    assert!(doc.add_ink(0, &[], Look::default()).is_err());
    assert!(doc.move_pages(&[], 0).is_err());
    assert!(doc.undo().is_err(), "nothing to undo");
    assert!(doc.outline().is_empty());
}
