//! Size probe: a tiny shared library that calls most of the API, so a release build shows
//! roughly how much the PDF back-end adds to the app's .so. Not used by the app.
//! Build: cargo ndk -t arm64-v8a --platform 26 build --release --example size_probe

use slate_pdf::{Look, MarkupKind, PdfDoc, ShapeKind, TextStyle};

fn exercise(bytes: &[u8]) -> Option<usize> {
    let mut d = PdfDoc::open_bytes(bytes.to_vec(), None).ok()?;
    let img = d.render_page(0, 1.0).ok()?;
    let hits = d.search("a", false, false, 10);
    if let Some(h) = hits.first() {
        d.add_text_markup(h.page, MarkupKind::Highlight, &h.rects, Look::default(), "").ok()?;
    }
    d.add_note(0, [1.0, 1.0], "n").ok()?;
    d.add_shape(0, ShapeKind::Arrow, [1.0, 1.0], [9.0, 9.0], Look::default()).ok()?;
    d.add_ink(0, &[vec![[1.0, 1.0], [2.0, 2.0]]], Look::default()).ok()?;
    d.add_text_box(0, [1.0, 1.0, 50.0, 20.0], "t", 12.0, Look::default()).ok()?;
    let _ = d.set_field_text("a", "b");
    let _ = d.add_typed_signature(0, [1.0, 1.0], "x", 20.0);
    let _ = d.add_signature_image(0, [1.0, 1.0], bytes, false);
    let _ = d.add_drawn_signature(0, &[vec![[1.0, 1.0], [2.0, 2.0]]], None);
    let _ = d.add_typed_text(0, [1.0, 1.0], "t", 10.0, [0.0; 3]);
    let _ = d.rotate_pages(&[0], 90);
    let _ = d.insert_pages_from("x.pdf", bytes.to_vec(), None, 0);
    let _ = d.crop_pages(&[0], [0.0, 0.0, 100.0, 100.0]);
    let _ = d.edit_text_line(0, 0, "x");
    let _ = d.add_text(0, [1.0, 1.0, 50.0, 20.0], "t", TextStyle::default());
    let _ = d.add_image(0, None, "a.png", bytes.to_vec());
    let _ = d.add_link_uri(0, [1.0, 1.0, 50.0, 20.0], "https://x");
    let _ = d.protect("pw", None);
    let _ = d.print_pdf((595.0, 842.0));
    let _ = d.extract_pages(&[0]);
    let _ = PdfDoc::merge(&[("a".into(), bytes.to_vec())]);
    let _ = d.undo();
    let n = d.comments().len() + d.form_fields().len() + d.outline().len() + d.links(None).len() + d.text_lines(0).ok()?.len();
    let out = d.save_bytes().ok()?;
    Some(out.bytes.len() + img.rgba.len() + n)
}

/// # Safety
/// `ptr` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slate_pdf_size_probe(ptr: *const u8, len: usize) -> usize {
    if ptr.is_null() {
        return 0;
    }
    // SAFETY: the caller promises `ptr` points to `len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    exercise(bytes).unwrap_or(0)
}
