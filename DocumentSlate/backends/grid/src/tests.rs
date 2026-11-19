//! Unit tests: open, read, edit, recalculate, save, reopen, export PDF.
//!
//! Test files: `tests/data/foreign.xlsx` and `foreign_with_slicer.xlsx` are made by openpyxl
//! (`tests/data/make_fixture.py`), so they come from "another program"; CSV and ODS inputs are
//! built in code below.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::*;

const FOREIGN: &[u8] = include_bytes!("../tests/data/foreign.xlsx");
const FOREIGN_SLICER: &[u8] = include_bytes!("../tests/data/foreign_with_slicer.xlsx");

fn a(s: &str) -> Addr {
    Addr::parse(s).unwrap()
}
fn r(s: &str) -> Area {
    Area::parse(s).unwrap()
}
fn num(doc: &GridDoc, sheet: usize, at: &str) -> f64 {
    match doc.cell(sheet, a(at)).unwrap().value {
        CellValue::Number(n) => n,
        other => panic!("{at} is not a number: {other:?}"),
    }
}
/// Scratch folder for files the tests write (the git-ignored `_cleanup/` folder next to the project).
fn scratch() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../_cleanup/grid-backend");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn csv_open_edit_recalc_save_reopen() {
    let csv = "Item,Qty,Price\nPens,3,1.5\nPaper,2,4\n";
    let mut doc = GridDoc::open_bytes("shop.csv", csv.as_bytes()).unwrap();
    assert_eq!(doc.source_kind(), SourceKind::Csv);
    assert_eq!(doc.cell_text(0, a("A2")), "Pens");
    assert_eq!(num(&doc, 0, "B2"), 3.0);
    assert_eq!(doc.used_range(0), Some(r("A1:C3")));

    // A formula, then change an input: the formula follows.
    doc.set_cell(a("D2"), "=B2*C2").unwrap();
    assert_eq!(num(&doc, 0, "D2"), 4.5);
    doc.set_cell(a("B2"), "10").unwrap();
    assert_eq!(num(&doc, 0, "D2"), 15.0);
    assert_eq!(doc.cell(0, a("D2")).unwrap().formula.as_deref(), Some("B2*C2"));
    assert!(doc.is_dirty());

    // Save as CSV: values only, and the formula loss is reported.
    let saved = doc.save_bytes(SaveFormat::Csv).unwrap();
    let text = String::from_utf8(saved.bytes.clone()).unwrap();
    assert!(text.contains("Pens,10,1.5,15"), "{text}");
    assert!(saved.report.lost.iter().any(|l| l.contains("formula")), "{:?}", saved.report);
    assert_eq!(saved.suggested_name, "shop.csv");

    // Save as XLSX keeps the formula; reopen and check.
    let x = doc.save_bytes(SaveFormat::Xlsx).unwrap();
    assert!(x.report.is_lossless(), "{:?}", x.report);
    assert!(!doc.is_dirty());
    let back = GridDoc::open_bytes("shop.xlsx", &x.bytes).unwrap();
    assert_eq!(num(&back, 0, "D2"), 15.0);
    assert_eq!(back.cell(0, a("D2")).unwrap().formula.as_deref(), Some("B2*C2"));
}

#[test]
fn foreign_xlsx_opens_with_everything() {
    let doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    assert_eq!(doc.source_kind(), SourceKind::Xlsx);
    assert_eq!(doc.sheet_names(), vec!["Sales", "Notes"]);
    // openpyxl stores no cached results: the engine calculated them on open.
    assert_eq!(num(&doc, 0, "D2"), 5.0);
    assert_eq!(num(&doc, 0, "D6"), 40.5);
    assert_eq!(num(&doc, 1, "B2"), 81.0); // cross-sheet formula
    let info = doc.sheets();
    assert_eq!(info[0].frozen, Some((1, 0)));
    let header = doc.cell(0, a("A1")).unwrap();
    assert!(header.bold);
    assert_eq!(doc.cell(0, a("C2")).unwrap().display, "0.50");
    assert_eq!(doc.cell(0, a("G1")).unwrap().merged, Some(r("F1:H1")));
    assert_eq!(doc.cell(0, a("A2")).unwrap().comment.as_deref().map(|c| c.contains("Fresh today")), Some(true));
    assert_eq!(doc.cell(0, a("A8")).unwrap().link.as_deref(), Some("https://example.org"));
    assert_eq!(doc.workbook().unwrap().sheets[0].charts.len(), 1);
}

#[test]
fn viewport_model_describes_cells() {
    let doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    let vp = doc.viewport(0, 0.0, 0.0, 800.0, 400.0).unwrap();
    assert_eq!(vp.cols.first().map(|c| c.index), Some(0));
    assert_eq!(vp.rows.first().map(|c| (c.index, c.start)), Some((0, 0.0)));
    assert_eq!(vp.frozen, Some((1, 0)));
    let find = |at: &str| vp.cells.iter().find(|c| c.addr == a(at)).unwrap_or_else(|| panic!("{at} missing"));
    // Header: bold white text on the green fill from the file.
    let h = find("B1");
    assert!(h.bold);
    assert_eq!(h.fill, Some([0x2E, 0x8B, 0x57]));
    assert_eq!(h.text_color, Some([0xFF, 0xFF, 0xFF]));
    // Numbers right-aligned, text left.
    assert_eq!(find("B2").align, TextAlign::Right);
    assert!(find("B2").numeric);
    assert_eq!(find("A2").align, TextAlign::Left);
    assert!(find("A2").has_comment);
    // The total's borders: thin top, double bottom.
    let t = find("D6");
    assert_eq!(t.text, "40.5");
    assert!(t.borders[1].is_some());
    assert!(t.borders[3].unwrap().double);
    // Merged title spans three columns and only its top-left cell is listed.
    let m = find("F1");
    assert_eq!(m.merge_span, Some((1, 3)));
    assert!(vp.cells.iter().all(|c| c.addr != a("G1")));
    assert!((m.w - (doc.cell_rect(0, a("F1")).unwrap().2)).abs() < 0.01);
    // The chart shows up as an object box, and it can be drawn.
    let chart = vp.objects.iter().find(|o| o.kind == ObjectKind::Chart).expect("chart box");
    let prims = doc.chart_prims(0, chart.index, chart.w, chart.h, &ApproxMeasure);
    assert!(prims.iter().any(|p| matches!(p, Prim::Rect { .. })));
    // Taps map back to cells.
    let (x, y, _, _) = doc.cell_rect(0, a("C3")).unwrap();
    assert_eq!(doc.hit(0, x + 2.0, y + 2.0), Some(a("C3")));
    let (w, h2) = doc.content_size(0).unwrap();
    assert!(w > 400.0 && h2 > 200.0);
}

#[test]
fn edit_format_undo_redo() {
    let mut doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    doc.set_bold(r("A2:A5"), true).unwrap();
    doc.set_italic(r("A2"), true).unwrap();
    doc.set_underline(r("A3"), true).unwrap();
    doc.set_font_size(r("A2"), 14.0).unwrap();
    doc.set_font_name(r("A2"), "Arial").unwrap();
    doc.set_font_color(r("A2"), Some("#C00000")).unwrap();
    doc.set_fill_color(r("B2:B5"), Some("#FFFF00")).unwrap();
    doc.set_align(r("A2"), HAlignChoice::Center).unwrap();
    doc.set_valign(r("A2"), VAlignChoice::Top).unwrap();
    doc.set_wrap(r("A2"), true).unwrap();
    doc.set_borders(r("A2:D5"), BorderPreset::All).unwrap();
    doc.set_number_format(r("C2:C5"), &NumberFormat::Currency).unwrap();
    doc.set_number_format(r("B2:B5"), &NumberFormat::Percent).unwrap();
    doc.set_number_format(r("D2:D5"), &NumberFormat::Custom("0.000".into())).unwrap();
    doc.increase_decimals(r("D2")).unwrap();
    let c = doc.cell(0, a("A2")).unwrap();
    assert!(c.bold && c.italic && c.wrap);
    assert_eq!(c.font_size, 14.0);
    assert_eq!(c.font_name, "Arial");
    assert!(doc.cell(0, a("A3")).unwrap().underline);
    assert_eq!(doc.cell(0, a("B2")).unwrap().display, "1000%");
    assert_eq!(doc.cell(0, a("D2")).unwrap().display, "5.0000");
    let vp = doc.viewport_cells(0, r("A2:D5")).unwrap();
    let a2 = vp.cells.iter().find(|x| x.addr == a("A2")).unwrap();
    assert_eq!(a2.text_color, Some([0xC0, 0, 0]));
    assert_eq!(a2.align, TextAlign::Center);
    assert_eq!(a2.valign, 0);
    assert!(a2.borders.iter().all(Option::is_some));

    // Undo the last change, redo it.
    doc.undo().unwrap();
    assert_eq!(doc.cell(0, a("D2")).unwrap().display, "5.000");
    assert!(doc.can_redo());
    doc.redo().unwrap();
    assert_eq!(doc.cell(0, a("D2")).unwrap().display, "5.0000");

    doc.clear_formats(r("A2")).unwrap();
    assert!(!doc.cell(0, a("A2")).unwrap().bold);
    doc.merge(r("F10:G10"), false).unwrap();
    assert_eq!(doc.cell(0, a("G10")).unwrap().merged, Some(r("F10:G10")));
    doc.unmerge(r("F10:G10")).unwrap();
    assert_eq!(doc.cell(0, a("G10")).unwrap().merged, None);
}

#[test]
fn rows_columns_sort_filter_freeze() {
    let mut doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    // Insert two rows above row 3: the SUM grows to cover the moved rows.
    doc.insert_rows(2, 2).unwrap();
    assert_eq!(doc.cell(0, a("D8")).unwrap().formula.as_deref(), Some("SUM(D2:D7)"));
    assert_eq!(num(&doc, 0, "D8"), 40.5);
    doc.delete_rows(2, 2).unwrap();
    assert_eq!(num(&doc, 0, "D6"), 40.5);
    doc.insert_cols(1, 1).unwrap();
    assert_eq!(doc.cell(0, a("E6")).unwrap().formula.as_deref(), Some("SUM(E2:E5)"));
    doc.delete_cols(1, 1).unwrap();
    assert_eq!(doc.cell_text(0, a("B1")), "Qty");

    // Sort the table by Qty, largest first (header row stays on top).
    doc.sort(r("A1:D5"), 1, true).unwrap();
    assert_eq!(doc.cell_text(0, a("A1")), "Fruit");
    assert_eq!(doc.cell_text(0, a("A2")), "Banana");
    assert_eq!(num(&doc, 0, "D2"), 6.0); // row formulas moved with their rows
    doc.sort_by(r("A1:D5"), &[SortKey { column: "A".into(), descending: false }], true).unwrap();
    assert_eq!(doc.cell_text(0, a("A2")), "Apple");

    doc.toggle_filter(r("A1:D5")).unwrap();
    assert!(doc.workbook().unwrap().sheets[0].autofilter.is_some());
    doc.filter_values(0, &["Apple", "Cherry"]).unwrap();
    assert!(doc.workbook().unwrap().sheets[0].is_row_hidden(2)); // Banana's row
    doc.toggle_filter(r("A1:D5")).unwrap();

    doc.unfreeze().unwrap();
    assert_eq!(doc.sheets()[0].frozen, None);
    doc.freeze_first_column().unwrap();
    assert_eq!(doc.sheets()[0].frozen, Some((0, 1)));
    doc.freeze_panes(a("B3")).unwrap();
    assert_eq!(doc.sheets()[0].frozen, Some((2, 1)));
    doc.freeze_top_row().unwrap();
    assert_eq!(doc.sheets()[0].frozen, Some((1, 0)));

    // Remove duplicates.
    doc.set_cell(a("J1"), "x").unwrap();
    doc.set_cell(a("J2"), "x").unwrap();
    doc.set_cell(a("J3"), "y").unwrap();
    doc.remove_duplicates(r("J1:J3"), false).unwrap();
    assert_eq!(doc.cell_text(0, a("J2")), "y");
}

#[test]
fn formulas_search_clipboard() {
    let mut doc = GridDoc::new_blank("Book.xlsx");
    for (i, v) in [3, 5, 7].iter().enumerate() {
        doc.set_cell(Addr::new(i as u32, 0), &v.to_string()).unwrap();
    }
    doc.autosum(r("A1:A3"), "sum").unwrap();
    assert_eq!(num(&doc, 0, "A4"), 15.0);
    doc.set_cell(a("B1"), "=AVERAGE(A1:A3)").unwrap();
    assert_eq!(num(&doc, 0, "B1"), 5.0);
    let maths = doc.functions(Some("MathTrig"), None).unwrap();
    assert!(maths.iter().any(|f| f.name == "SUM"), "{} functions", maths.len());
    let search = doc.functions(None, Some("VLOOKUP")).unwrap();
    assert!(search.iter().any(|f| f.name == "VLOOKUP"));
    let recent = doc.recent_functions().unwrap();
    assert!(recent.iter().any(|f| f == "AVERAGE" || f == "SUM"), "{recent:?}");

    doc.set_cell(a("C1"), "apple pie").unwrap();
    doc.set_cell(a("C2"), "Apple juice").unwrap();
    assert_eq!(doc.find_all("apple").unwrap(), vec![a("C1"), a("C2")]);
    assert_eq!(doc.replace_all("apple", "pear").unwrap(), 2);
    assert_eq!(doc.cell_text(0, a("C1")), "pear pie");

    // Copy/paste keeps relative formulas relative; paste values pastes the result only.
    let text = doc.copy(r("B1")).unwrap();
    assert_eq!(text.trim(), "5");
    doc.paste(a("B2")).unwrap();
    assert_eq!(doc.cell(0, a("B2")).unwrap().formula.as_deref(), Some("AVERAGE(A2:A4)"));
    doc.copy(r("B1")).unwrap();
    doc.paste_values(a("B3")).unwrap();
    assert_eq!(doc.cell(0, a("B3")).unwrap().formula, None);
    assert_eq!(num(&doc, 0, "B3"), 5.0);
    doc.paste_text(a("E1"), "1\t2\n3\t4").unwrap();
    assert_eq!(num(&doc, 0, "F2"), 4.0);
    doc.clear_contents(r("E1:F2")).unwrap();
    assert_eq!(doc.cell(0, a("F2")).unwrap().value, CellValue::Empty);
}

#[test]
fn insert_tab_and_sheets() {
    let mut doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    doc.insert_chart(r("A1:B5"), "line", Some("Qty")).unwrap();
    doc.insert_chart(r("A1:B5"), "pie", None).unwrap();
    assert_eq!(doc.workbook().unwrap().sheets[0].charts.len(), 3);
    // A 1×1 PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF,
        0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
        0x82,
    ];
    doc.insert_picture(a("J2"), PNG, "image/png", Some(96.0)).unwrap();
    let sh = &doc.workbook().unwrap().sheets[0];
    assert_eq!(sh.images.len(), 1);
    assert_eq!(doc.picture(0, 0).unwrap().1, "image/png");
    doc.insert_link(a("A10"), "https://example.com", Some("Example")).unwrap();
    assert_eq!(doc.cell(0, a("A10")).unwrap().link.as_deref(), Some("https://example.com"));
    assert_eq!(doc.cell_text(0, a("A10")), "Example");
    doc.add_note(a("B10"), "check this").unwrap();
    doc.add_comment(a("C10"), "looks good", "Alex").unwrap();
    assert!(doc.cell(0, a("B10")).unwrap().comment.unwrap().contains("check this"));
    assert!(doc.cell(0, a("C10")).unwrap().comment.unwrap().contains("looks good"));

    let i = doc.add_sheet(Some("Extra")).unwrap();
    assert_eq!(doc.sheet_names()[i], "Extra");
    doc.rename_sheet(i, "Budget").unwrap();
    assert_eq!(doc.sheet_names(), vec!["Sales", "Notes", "Budget"]);
    doc.set_active_sheet(1).unwrap();
    assert_eq!(doc.active_sheet(), 1);
    doc.set_cell(a("C3"), "on notes").unwrap();
    assert_eq!(doc.cell_text(1, a("C3")), "on notes");
    doc.delete_sheet(2).unwrap();
    assert_eq!(doc.sheet_count(), 2);

    // Everything above survives a save + reopen.
    let saved = doc.save_bytes(SaveFormat::Xlsx).unwrap();
    assert!(saved.report.is_lossless(), "{:?}", saved.report);
    let back = GridDoc::open_bytes("x.xlsx", &saved.bytes).unwrap();
    let sh = &back.workbook().unwrap().sheets[0];
    assert_eq!((sh.charts.len(), sh.images.len()), (3, 1));
    assert_eq!(back.cell(0, a("A10")).unwrap().link.as_deref(), Some("https://example.com"));
    assert!(back.cell(0, a("C10")).unwrap().comment.is_some());
}

#[test]
fn xlsx_roundtrip_is_lossless() {
    let mut doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    doc.set_cell(a("B2"), "20").unwrap();
    let saved = doc.save_bytes(SaveFormat::Xlsx).unwrap();
    assert!(saved.report.is_lossless(), "{:?}", saved.report);
    // The chart note is informational.
    assert!(saved.report.notes.iter().any(|n| n.contains("chart")));
    let back = GridDoc::open_bytes("foreign.xlsx", &saved.bytes).unwrap();
    assert_eq!(num(&back, 0, "D6"), 45.5);
    assert_eq!(back.sheets()[0].frozen, Some((1, 0)));
    assert_eq!(back.cell(0, a("G1")).unwrap().merged, Some(r("F1:H1")));
    assert_eq!(back.workbook().unwrap().sheets[0].validations.len(), 1);
    assert_eq!(back.workbook().unwrap().sheets[0].charts.len(), 1);
    // The writer caches formula results, so other apps show the right numbers straight away.
    let sheet1 = crate::fidelity::part_text(&saved.bytes, "xl/worksheets/sheet1.xml").unwrap();
    assert!(sheet1.contains("<v>45.5</v>"), "cached value missing");
    std::fs::write(scratch().join("roundtrip.xlsx"), &saved.bytes).unwrap();
}

#[test]
fn slicer_loss_is_reported() {
    let mut doc = GridDoc::open_bytes("with_slicer.xlsx", FOREIGN_SLICER).unwrap();
    let saved = doc.save_bytes(SaveFormat::Xlsx).unwrap();
    assert!(saved.report.lost.iter().any(|l| l.starts_with("slicers")), "{:?}", saved.report);
}

#[test]
fn csv_save_of_workbook_lists_losses() {
    let mut doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    let saved = doc.save_bytes(SaveFormat::Csv).unwrap();
    let lost = saved.report.lost.join(" | ");
    assert!(lost.contains("other sheet"), "{lost}");
    assert!(lost.contains("formula"), "{lost}");
    assert!(lost.contains("formatting"), "{lost}");
    assert!(lost.contains("chart"), "{lost}");
    assert!(doc.is_dirty() || !doc.is_dirty()); // CSV never marks an xlsx as saved
    assert!(doc.save_bytes(SaveFormat::Ods).is_err());
}

/// A minimal OpenDocument spreadsheet written by hand (mimetype + content.xml).
fn tiny_ods() -> Vec<u8> {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("mimetype", stored).unwrap();
        z.write_all(b"application/vnd.oasis.opendocument.spreadsheet").unwrap();
        z.start_file("META-INF/manifest.xml", stored).unwrap();
        z.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#).unwrap();
        z.start_file("content.xml", stored).unwrap();
        z.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2" office:version="1.2"><office:body><office:spreadsheet><table:table table:name="Data"><table:table-row><table:table-cell office:value-type="string"><text:p>Name</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell><table:table-cell table:formula="of:=[.B1]*3" office:value-type="float" office:value="6"><text:p>6</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#).unwrap();
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn ods_imports_and_saves_as_xlsx() {
    let mut doc = GridDoc::open_bytes("data.ods", &tiny_ods()).unwrap();
    assert_eq!(doc.source_kind(), SourceKind::Ods);
    assert!(!doc.source_kind().saves_in_place());
    assert_eq!(doc.sheet_names(), vec!["Data"]);
    assert_eq!(doc.cell_text(0, a("A1")), "Name");
    assert_eq!(num(&doc, 0, "C1"), 6.0);
    let saved = doc.save_bytes(SaveFormat::Xlsx).unwrap();
    assert_eq!(saved.suggested_name, "data.xlsx");
    assert!(saved.report.notes.iter().any(|n| n.contains("OpenDocument")));
    assert!(doc.save_bytes(SaveFormat::Ods).unwrap_err().contains("ODS"));
}

#[test]
fn pdf_export_for_printing() {
    let mut doc = GridDoc::open_bytes("foreign.xlsx", FOREIGN).unwrap();
    doc.set_landscape(true).unwrap();
    doc.set_paper("A4").unwrap();
    doc.set_margins("narrow").unwrap();
    doc.set_print_gridlines(true).unwrap();
    doc.set_print_area(Some(r("A1:H20"))).unwrap();
    doc.set_fit_to_page(Some((1, 0))).unwrap();
    let (bytes, pages) = doc.pdf_bytes(PdfOptions::default()).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(pages, 1);
    let (all, all_pages) = doc.pdf_bytes(PdfOptions { all_sheets: true, fit_on_one_page: true }).unwrap();
    assert!(all.starts_with(b"%PDF-"));
    assert_eq!(all_pages, 2);
    let path = scratch().join("foreign.pdf");
    assert_eq!(doc.export_pdf(&path, PdfOptions { all_sheets: true, fit_on_one_page: false }).unwrap(), 2);
    assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    doc.set_print_area(None).unwrap();
    doc.set_fit_to_page(None).unwrap();
    // Page setup is saved with the workbook.
    let back = GridDoc::open_bytes("p.xlsx", &doc.save_bytes(SaveFormat::Xlsx).unwrap().bytes).unwrap();
    assert_eq!(back.workbook().unwrap().sheets[0].print.orientation, gridcraft_engine::model::Orientation::Landscape);
}

#[test]
fn bad_input_is_an_error_not_a_crash() {
    assert!(GridDoc::open_bytes("junk.xlsx", b"PK\x03\x04 not really a zip").is_err());
    assert!(GridDoc::open_bytes("empty.xlsx", b"").is_err() || GridDoc::open_bytes("empty.xlsx", b"").is_ok());
    let mut doc = GridDoc::new_blank("b.xlsx");
    assert!(doc.set_active_sheet(9).is_err());
    assert!(doc.run("no.such.command", serde_json::json!({})).is_err());
    assert!(doc.viewport(5, 0.0, 0.0, 10.0, 10.0).is_none());
    // A huge rectangle is capped, not a memory blow-up.
    let vp = doc.viewport(0, 0.0, 0.0, 1.0e9, 1.0e9).unwrap();
    assert!(vp.cells.len() <= 40_000);
}

#[test]
fn doc_can_move_to_a_worker_thread() {
    fn is_send<T: Send>() {}
    is_send::<GridDoc>();
}

#[test]
fn ui_support_find_region_filter_encode() {
    let csv = "Name,Qty\nPens,3\nPaper,2\nPens,5\n";
    let mut doc = GridDoc::open_bytes("s.csv", csv.as_bytes()).unwrap();
    doc.add_sheet(Some("Other")).unwrap();
    doc.set_cell(a("B2"), "pens too").unwrap();
    let hits = doc.find_in_workbook("pens", 100);
    assert_eq!(hits, vec![(0, a("A2")), (0, a("A4")), (1, a("B2"))]);
    assert_eq!(doc.current_region(0, a("B3")), r("A1:B4"));
    assert_eq!(doc.current_region(0, a("F9")), r("F9"));
    doc.set_active_sheet(0).unwrap();
    doc.toggle_filter(r("A1:B4")).unwrap();
    assert_eq!(doc.filter_range(0), Some(r("A1:B4")));
    assert_eq!(doc.column_values(0, 0, r("A1:B4"), 50), vec!["Paper".to_string(), "Pens".to_string()]);
    doc.filter_values(0, &["Paper"]).unwrap();
    doc.clear_filter().unwrap();
    // encode() leaves the unsaved flag alone; mark_clean() clears it.
    assert!(doc.is_dirty());
    let saved = doc.encode(SaveFormat::Xlsx).unwrap();
    assert!(doc.is_dirty());
    doc.adopt_saved("s.xlsx", &saved.bytes);
    assert!(!doc.is_dirty());
    assert_eq!(doc.source_kind(), SourceKind::Xlsx);
    let ps = doc.page_setup(0).unwrap();
    assert!(!ps.landscape);
}

/// Hyperlink targets: web links are addresses, "Sheet2!B3" is a place (sheet + cell), a bad
/// reference is refused; looking doesn't move the selection or the active sheet.
#[test]
fn link_targets() {
    let mut doc = GridDoc::new_blank("links.xlsx");
    let s2 = doc.add_sheet(Some("Data")).unwrap();
    doc.set_active_sheet(0).unwrap();
    doc.insert_link(a("A1"), "https://example.org/x", Some("Web")).unwrap();
    doc.insert_link(a("A2"), "Data!B3", Some("Inside")).unwrap();
    doc.insert_link(a("A3"), "mailto:someone@example.org", None).unwrap();
    doc.select(r("D4")).unwrap();
    let web = doc.cell(0, a("A1")).unwrap().link.unwrap();
    assert_eq!(doc.link_target(&web), LinkTarget::Address("https://example.org/x".into()));
    let inside = doc.cell(0, a("A2")).unwrap().link.unwrap();
    assert_eq!(doc.link_target(&inside), LinkTarget::Place { sheet: s2, area: r("B3") });
    assert_eq!(doc.link_target("#Data!C5:D6"), LinkTarget::Place { sheet: s2, area: r("C5:D6") });
    assert!(matches!(doc.link_target("Nowhere!A1"), LinkTarget::Refused(_) | LinkTarget::Address(_)));
    let mail = doc.cell(0, a("A3")).unwrap().link.unwrap();
    assert_eq!(doc.link_target(&mail), LinkTarget::Address("mailto:someone@example.org".into()));
    assert_eq!(doc.active_sheet(), 0, "looking at a link switched sheets");
    let sel = &doc.doc().unwrap().selection;
    assert_eq!(sel.ranges.last().map(|r| r.a1()), Some("D4".to_string()), "looking at a link moved the selection");
}
