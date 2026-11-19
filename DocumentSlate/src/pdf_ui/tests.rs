//! Page Slate screen tests: generated PDFs only (no files from outside).

use std::path::PathBuf;

use slate_pdf::{FillMark, MarkupKind, PdfDoc};

use super::sheets::Sheet;
use super::sig::SavedSig;
use super::tools::{EntryKind, Tab, TouchTool};
use super::view::Drag;
use super::PdfScreen;
use crate::doc_screen::DocScreen;

/// A Letter-size PDF: one page per entry (a 24 pt heading and two 12 pt lines of body text),
/// an outline with one bookmark per page, a link on page 1 to the last page, and on page 1 a
/// text field "name", a check box "agree" and a combo box "colour".
pub(super) fn make_pdf(pages: &[&str]) -> Vec<u8> {
    let n = pages.len();
    // 1 catalog, 2 pages, 3 font, 4 outline root, 5.. bookmarks (n), then page + content per
    // page, then: text field, check box, its two appearances, combo box, link.
    let first_bm = 5;
    let first_page = first_bm + n;
    let page_obj = |i: usize| first_page + 2 * i;
    let field = first_page + 2 * n;
    let check = field + 1;
    let (ap_yes, ap_off) = (check + 1, check + 2);
    let combo = check + 3;
    let link = combo + 1;
    let mut o: Vec<String> = Vec::new();
    o.push(format!(
        "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /AcroForm << /Fields [{field} 0 R {check} 0 R {combo} 0 R] /DR << /Font << /Helv 3 0 R >> >> /DA (/Helv 0 Tf 0 g) >> >>"
    ));
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
    o.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    o.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into());
    o.push(format!("<< /Type /Outlines /First {first_bm} 0 R /Last {} 0 R /Count {n} >>", first_bm + n - 1));
    for (i, title) in pages.iter().enumerate() {
        let prev = if i > 0 { format!(" /Prev {} 0 R", first_bm + i - 1) } else { String::new() };
        let next = if i + 1 < n { format!(" /Next {} 0 R", first_bm + i + 1) } else { String::new() };
        o.push(format!("<< /Title ({title}) /Parent 4 0 R{prev}{next} /Dest [{} 0 R /XYZ 0 792 0] >>", page_obj(i)));
    }
    for (i, title) in pages.iter().enumerate() {
        let annots = if i == 0 { format!(" /Annots [{field} 0 R {check} 0 R {combo} 0 R {link} 0 R]") } else { String::new() };
        o.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R{annots} >>", page_obj(i) + 1));
        let stream = format!(
            "BT /F1 24 Tf 72 720 Td ({title}) Tj ET\n\
             BT /F1 12 Tf 72 680 Td (Lorem ipsum dolor sit amet, consectetur adipiscing elit.) Tj 0 -16 Td (Page Slate test document, page {}.) Tj ET\n\
             0.2 0.4 0.7 RG 3 w 72 640 m 540 640 l S\n",
            i + 1
        );
        o.push(format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()));
    }
    let p1 = page_obj(0);
    o.push(format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V () /Rect [72 500 300 524] /P {p1} 0 R /F 4 /DA (/Helv 12 Tf 0 g) >>"));
    o.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /V /Off /AS /Off /Rect [72 450 90 468] /P {p1} 0 R /F 4 /MK << /CA (4) >> /DA (/ZaDb 0 Tf 0 g) /AP << /N << /Yes {ap_yes} 0 R /Off {ap_off} 0 R >> >> >>"
    ));
    for ap in ["0 g 2 2 14 14 re f\n", "0 G 1 w 0.5 0.5 17 17 re S\n"] {
        o.push(format!("<< /Type /XObject /Subtype /Form /BBox [0 0 18 18] /Length {} >>\nstream\n{ap}endstream", ap.len()));
    }
    o.push(format!(
        "<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (colour) /V (Red) /Opt [(Red) (Green) (Blue)] /Rect [72 400 200 420] /P {p1} 0 R /F 4 /DA (/Helv 12 Tf 0 g) >>"
    ));
    o.push(format!("<< /Type /Annot /Subtype /Link /Rect [72 600 300 620] /Border [0 0 0] /Dest [{} 0 R /Fit] >>", page_obj(n - 1)));
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in o.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{obj}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", o.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", o.len() + 1).as_bytes());
    out
}

fn sample() -> Vec<u8> {
    make_pdf(&["Introduction", "Methods", "Results"])
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("document-slate-pdf-ui-tests");
    let _ = std::fs::create_dir_all(&d);
    d.join(name)
}

fn open() -> PdfScreen {
    let s = PdfScreen::open("test.pdf", &sample()).expect("opens");
    assert!(s.locked.is_none());
    s
}

/// Desktop visual check helper: `DS_PDF_SAMPLE_OUT=<path> cargo test … write_sample` writes the
/// sample PDF there (does nothing otherwise).
#[test]
fn write_sample_pdf_when_asked() {
    if let Ok(p) = std::env::var("DS_PDF_SAMPLE_OUT") {
        std::fs::write(&p, make_pdf(&["Introduction", "Methods", "Results", "Discussion"])).expect("write sample");
        let locked = {
            let mut d = PdfDoc::open_bytes(sample(), None).expect("open");
            d.protect("slate", None).expect("protect");
            d.save_bytes().expect("save").bytes
        };
        std::fs::write(format!("{p}.locked.pdf"), locked).expect("write locked sample");
    }
}

#[test]
fn opens_renders_and_searches() {
    let mut s = open();
    assert_eq!(s.page_count(), 3);
    assert_eq!(s.position(), (0, 3));
    assert!(!s.dirty());
    let img = s.doc.as_mut().expect("doc").render_page(0, 1.0).expect("renders");
    assert_eq!((img.width, img.height), (612, 792));
    let ink = img.rgba.chunks(4).filter(|p| p[0] < 128 && p[1] < 128).count();
    assert!(ink > 200, "page has visible text ({ink} dark pixels)");
    // Search through the screen (what the search row runs).
    s.v.query = "lorem".into();
    s.run_search();
    assert_eq!(s.v.hits.len(), 3, "one hit per page");
    assert_eq!(s.v.hits[1].page, 1);
    assert!(!s.v.hits[0].rects.is_empty());
    s.v.query = "Results".into();
    s.run_search();
    assert_eq!(s.v.hits.len(), 1);
    assert_eq!(s.v.hits[0].page, 2);
    // Outline (bookmarks) and the link.
    s.open_outline();
    match &s.sheet {
        Sheet::Outline(list) => {
            assert_eq!(list.len(), 3);
            assert_eq!(list[2].1, "Results");
            assert_eq!(list[2].2, Some(2));
        }
        _ => panic!("outline sheet"),
    }
    assert!(s.cache.links.iter().any(|l| l.target == slate_pdf::LinkTarget::Page(2)));
    // Night mode is a view menu toggle.
    s.view_action("night");
    assert!(s.v.night);
}

#[test]
fn highlight_note_signature_save_reopen() {
    let original = sample();
    let mut s = open();
    s.set_editing(true);
    // Highlight: a finger drag across the 24 pt heading (baseline at y = 72 in view space).
    s.e.tool = TouchTool::Markup(MarkupKind::Highlight);
    s.finish_drag(Drag { page: 0, points: vec![[74.0, 62.0], [200.0, 64.0]], preview: Vec::new() });
    assert_eq!(s.cache.comments.len(), 1, "highlight added");
    assert_eq!(s.cache.comments[0].kind, "Highlight");
    // Underline over the body text.
    s.e.tool = TouchTool::Markup(MarkupKind::Underline);
    s.finish_drag(Drag { page: 0, points: vec![[74.0, 108.0], [300.0, 108.0]], preview: Vec::new() });
    assert_eq!(s.cache.comments.len(), 2);
    // Sticky note: tap, type, OK.
    s.e.tool = TouchTool::Note;
    s.on_tap(0, [400.0, 300.0]);
    let mut en = s.e.entry.take().expect("note entry opens");
    assert_eq!(en.kind, EntryKind::Note);
    en.text = "Check this".into();
    s.commit_entry(en);
    assert!(s.cache.comments.iter().any(|c| c.kind == "Text" && c.text == "Check this"));
    // Pen stroke.
    s.e.tool = TouchTool::Ink;
    s.finish_drag(Drag { page: 1, points: (0..20).map(|i| [100.0 + i as f32 * 5.0, 400.0 + (i % 3) as f32]).collect(), preview: Vec::new() });
    // Saved drawn signature, placed with a tap.
    let sig = SavedSig::drawn(&[vec![[0.0, 20.0], [30.0, 0.0], [60.0, 25.0], [90.0, 5.0]]]).expect("drawn");
    let i = s.sigs.add(sig);
    s.e.sig_choice = Some(i);
    s.e.tool = TouchTool::Sign;
    s.on_tap(0, [300.0, 650.0]);
    // Tick and date (Fill & Sign).
    s.e.tool = TouchTool::Mark(FillMark::Check);
    s.on_tap(0, [80.0, 700.0]);
    s.e.tool = TouchTool::Date;
    s.on_tap(0, [100.0, 700.0]);
    let n = s.cache.comments.len();
    assert!(n >= 7, "all marks added ({n})");
    assert!(s.dirty());
    // Save: incremental, the original bytes stay as the prefix.
    let path = scratch("annotated.pdf");
    let lost = s.write_to(&path).expect("saves");
    assert!(lost.is_empty(), "{lost:?}");
    let saved = std::fs::read(&path).expect("read back");
    assert!(saved.starts_with(&original), "original bytes kept");
    assert!(saved.len() > original.len());
    s.mark_saved("annotated.pdf", &path);
    assert!(!s.dirty());
    assert_eq!(s.name(), "annotated.pdf");
    // Reopen: everything is still there.
    let again = PdfScreen::open("annotated.pdf", &saved).expect("reopens");
    assert_eq!(again.cache.comments.len(), n);
    // Undo after save still works (and makes it dirty again).
    s.undo();
    assert!(s.dirty());
}

#[test]
fn comment_select_reply_move_delete() {
    let mut s = open();
    s.set_editing(true);
    s.edit(|d| d.add_note(0, [400.0, 300.0], "first"));
    s.e.tab = Tab::Annotate;
    s.select_at(0, [405.0, 305.0]);
    let Some(super::tools::Sel::Comment(c)) = s.e.selected.clone() else { panic!("note selected") };
    assert_eq!(c.text, "first");
    s.edit(|d| d.reply_to_comment(c.page, c.index, "a reply"));
    assert!(s.cache.comments.iter().any(|x| x.is_reply && x.text == "a reply"));
    s.edit(|d| d.move_comment(c.page, c.index, 20.0, 10.0));
    let moved = s.cache.comments.iter().find(|x| x.text == "first").expect("still there").rect;
    assert!((moved[0] - c.rect[0] - 20.0).abs() < 1.0, "moved right: {moved:?} vs {:?}", c.rect);
    s.edit(|d| d.delete_comment(c.page, c.index));
    assert!(s.cache.comments.iter().all(|x| x.text != "first"));
}

#[test]
fn page_operations_and_undo() {
    let mut s = open();
    s.set_editing(true);
    s.edit(|d| d.duplicate_pages(&[0]));
    assert_eq!(s.page_count(), 4);
    s.edit(|d| d.rotate_pages(&[1], 90));
    assert_eq!(s.cache.pages[1].rotation, 90);
    assert!(s.cache.pages[1].width > s.cache.pages[1].height, "turned to landscape");
    s.edit(|d| d.delete_pages(&[0]));
    assert_eq!(s.page_count(), 3);
    s.edit(|d| d.insert_blank_page(3, None));
    assert_eq!(s.page_count(), 4);
    s.edit(|d| d.move_pages(&[3], 0));
    assert_eq!(s.page_count(), 4);
    s.undo();
    s.undo();
    s.undo();
    assert_eq!(s.page_count(), 4);
    assert_eq!(s.cache.pages[0].rotation, 0);
    // Deleting the only page left is refused, not a crash.
    let mut one = PdfScreen::open("one.pdf", &make_pdf(&["Only"])).expect("opens");
    assert!(!one.edit(|d| d.delete_pages(&[0])));
    assert_eq!(one.page_count(), 1);
}

#[test]
fn password_prompt_then_open() {
    let locked = {
        let mut d = PdfDoc::open_bytes(sample(), None).expect("open");
        d.protect("s3cret", None).expect("protect");
        d.save_bytes().expect("save").bytes
    };
    let mut s = PdfScreen::open("locked.pdf", &locked).expect("a locked file opens to the password card");
    assert!(s.locked.is_some() && s.doc.is_none());
    assert_eq!(s.position(), (0, 1));
    s.set_editing(true);
    assert!(!s.editing(), "no editing before the password");
    s.locked.as_mut().expect("locked").password = "wrong".into();
    assert!(!s.try_password());
    assert!(s.locked.as_ref().expect("still locked").error.is_some());
    s.locked.as_mut().expect("locked").password = "s3cret".into();
    assert!(s.try_password());
    assert!(s.locked.is_none());
    assert_eq!(s.page_count(), 3);
    assert!(s.cache.props.encrypted);
}

#[test]
fn fill_form_fields_and_typed_signature() {
    let mut s = open();
    s.set_editing(true);
    s.e.tab = Tab::FillSign;
    // Text field "name" at user [72 500 300 524] → view y 268..292.
    s.select_at(0, [100.0, 280.0]);
    let mut en = s.e.entry.take().expect("field entry");
    assert_eq!(en.kind, EntryKind::Field("name".into()));
    en.text = "Alex".into();
    s.commit_entry(en);
    // Check box "agree" at view y 324..342.
    s.select_at(0, [80.0, 333.0]);
    // Combo box opens the choice sheet.
    s.select_at(0, [100.0, 382.0]);
    assert!(matches!(s.sheet, Sheet::Choice(_)));
    s.sheet = Sheet::None;
    s.edit(|d| d.set_field_choice("colour", &["Blue".into()]));
    let f = |n: &str| s.cache.fields.iter().find(|f| f.name == n).cloned().expect("field");
    assert_eq!(f("name").value, vec!["Alex".to_string()]);
    assert!(!f("agree").value.is_empty(), "ticked");
    assert_eq!(f("colour").value, vec!["Blue".to_string()]);
    // Typed signature.
    let i = s.sigs.add(SavedSig::Typed { name: "D. Slate".into(), initials: false });
    s.e.sig_choice = Some(i);
    s.e.tool = TouchTool::Sign;
    s.on_tap(0, [300.0, 600.0]);
    assert!(s.cache.comments.iter().any(|c| c.fill_sign), "signature added");
    let out = s.doc.as_ref().expect("doc").save_bytes().expect("saves");
    let back = PdfDoc::open_bytes(out.bytes, None).expect("reopen");
    assert!(back.form_fields().iter().any(|f| f.name == "name" && f.value == vec!["Alex".to_string()]));
}

#[test]
fn edit_existing_text_and_added_items() {
    let mut s = open();
    s.set_editing(true);
    s.e.tab = Tab::Edit;
    s.e.tool = TouchTool::EditText;
    let lines = s.lines(0).to_vec();
    assert!(lines.iter().any(|l| l.text.contains("Introduction")), "{lines:?}");
    let heading = lines.iter().find(|l| l.text.contains("Introduction")).expect("heading").clone();
    s.on_tap(0, [(heading.rect[0] + heading.rect[2]) / 2.0, (heading.rect[1] + heading.rect[3]) / 2.0]);
    let mut en = s.e.entry.take().expect("line entry");
    assert!(matches!(en.kind, EntryKind::EditLine(_)));
    en.text = "Overview".into();
    s.commit_entry(en);
    s.v.query = "Overview".into();
    s.run_search();
    assert_eq!(s.v.hits.len(), 1, "edited text is real text");
    // Add text, then move and resize it.
    s.e.tool = TouchTool::AddText;
    s.on_tap(0, [100.0, 500.0]);
    let mut en = s.e.entry.take().expect("add text entry");
    en.text = "Added words".into();
    s.commit_entry(en);
    assert_eq!(s.cache.added.len(), 1);
    let a = s.cache.added[0].clone();
    s.edit(|d| d.set_added_rect(a.page, a.index, [a.rect[0] + 50.0, a.rect[1], a.rect[2] + 80.0, a.rect[3]]));
    assert!((s.cache.added[0].rect[0] - a.rect[0] - 50.0).abs() < 1.0);
    // A link.
    s.edit(|d| d.add_link_uri(0, [72.0, 720.0, 200.0, 740.0], "https://example.org"));
    assert!(s.cache.links.iter().any(|l| l.target == slate_pdf::LinkTarget::Uri("https://example.org".into())));
}

#[test]
fn print_save_as_and_extract_files() {
    let mut s = open();
    s.set_editing(true);
    s.edit(|d| d.add_note(0, [300.0, 300.0], "n"));
    let dir = scratch("x").parent().expect("dir").to_path_buf();
    // Print copy.
    let print = dir.join("print.pdf");
    s.export_pdf(&print).expect("print copy");
    let p = PdfDoc::open_bytes(std::fs::read(&print).expect("read"), None).expect("valid PDF");
    assert_eq!(p.page_count(), 3);
    // Save as (…/save/): the document itself, adoptable.
    let save_dir = dir.join("save");
    std::fs::create_dir_all(&save_dir).expect("mkdir");
    let copy = save_dir.join("copy.pdf");
    s.export_pdf(&copy).expect("save as copy");
    s.mark_saved("copy.pdf", &copy);
    assert!(!s.dirty());
    assert_eq!(s.name(), "copy.pdf");
    assert!(s.can_save_in_place());
    // Extract page 2: a one-page file; the open document stays, Save asks where next time.
    s.extract_pending = Some(vec![1]);
    let ex = save_dir.join("copy-extract.pdf");
    assert!(s.write_to(&ex).expect("extract").is_empty());
    assert!(s.extract_pending.is_none(), "the extract request must not outlive the save");
    let e = PdfDoc::open_bytes(std::fs::read(&ex).expect("read"), None).expect("valid PDF");
    assert_eq!(e.page_count(), 1);
    s.mark_saved("copy.pdf", &ex);
    assert_eq!(s.page_count(), 3);
    assert!(!s.can_save_in_place());
    assert!(s.info_line().contains("3 pages"));
}

#[test]
fn saved_signatures_persist() {
    let dir = scratch("sigstore");
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = super::sig::Store::default();
    store.load(&dir);
    assert!(store.list.is_empty());
    store.add(SavedSig::Typed { name: "AB".into(), initials: true });
    let mut again = super::sig::Store::default();
    again.load(&dir);
    assert_eq!(again.list.len(), 1);
    assert_eq!(again.list[0].label(0), "Initials: AB");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_blank_document() {
    let s = PdfScreen::new_blank();
    assert_eq!(s.page_count(), 1);
    assert!(!s.can_save_in_place(), "a new file goes to Save as");
    assert_eq!(s.default_ext(), "pdf");
}
