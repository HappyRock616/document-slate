//! Grid Slate screen tests: open, paint, select + type, format, rows, sort, search, save + reopen.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn a(s: &str) -> Addr {
    Addr::parse(s).unwrap()
}

fn shop() -> GridScreen {
    let csv = "Item,Qty,Price\nPens,3,1.5\nPaper,2,4\nInk,7,12\n";
    GridScreen::open("shop.csv", csv.as_bytes()).unwrap()
}

fn cx() -> ScreenCtx {
    ScreenCtx { accent: Color32::from_rgb(0x2E, 0x8B, 0x57), dark: false, scratch: std::env::temp_dir().join("document-slate-test"), top_inset: 0.0, bottom_inset: 0.0 }
}

/// Run frames of the screen in a phone-sized window; returns every text drawn in the last one.
fn frame_texts(s: &mut GridScreen, frames: usize) -> Vec<String> {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(393.0, 851.0))), ..Default::default() };
    let mut texts = Vec::new();
    for _ in 0..frames {
        let out = ctx.run_ui(input(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| s.show(ui, &cx()));
        });
        texts.clear();
        for cs in &out.shapes {
            collect_text(&cs.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
    }
    texts
}

fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect_text(s, out)),
        _ => {}
    }
}

#[test]
fn opens_csv_and_paints_visible_cells() {
    let mut s = shop();
    assert_eq!(s.position(), (0, 1));
    let texts = frame_texts(&mut s, 3);
    for want in ["Item", "Pens", "Paper", "12", "1.5", "A", "C", "1", "4"] {
        assert!(texts.iter().any(|t| t == want), "{want:?} not painted; got {texts:?}");
    }
    // Sheet tab.
    assert!(texts.iter().any(|t| t == "shop"), "sheet tab missing: {texts:?}");
    // Viewport cache: the same block is reused while nothing changes.
    let area = Area::new(a("A1"), a("P64"));
    let e1 = s.cache.get(&s.doc, 0, area).unwrap();
    let e2 = s.cache.get(&s.doc, 0, area).unwrap();
    assert!(std::rc::Rc::ptr_eq(&e1, &e2));
    assert!(e1.occupied.contains(&(0, 0)));
}

#[test]
fn select_type_formula_and_enter_moves_down() {
    let mut s = shop();
    s.set_editing(true);
    // Tap D2 once = select, twice = edit.
    s.tap_cell(a("D2"));
    assert_eq!(s.sel, Area::cell(a("D2")));
    assert!(s.editing_cell.is_none());
    s.tap_cell(a("D2"));
    assert_eq!(s.editing_cell, Some(a("D2")));
    // Typing a formula: tapping cells inserts references.
    s.input = "=".into();
    s.tap_cell(a("B2"));
    s.input.push('*');
    s.tap_cell(a("C2"));
    assert_eq!(s.input, "=B2*C2");
    assert_eq!(s.sel, Area::cell(a("D2")), "reference taps must not move the selection");
    let ctx = egui::Context::default();
    s.commit_and_next(&ctx);
    assert_eq!(s.doc.cell_text(0, a("D2")), "4.5");
    assert_eq!(s.sel, Area::cell(a("D3")));
    assert_eq!(s.editing_cell, Some(a("D3")));
    // Plain value, then cancel leaves the cell alone.
    s.input = "hello".into();
    s.cancel_edit();
    assert_eq!(s.doc.cell_text(0, a("D3")), "");
    assert!(s.dirty());
    // Function insert starts a formula in the selected cell.
    s.insert_function("SUM");
    assert_eq!(s.input, "=SUM(");
    s.tap_cell(a("B2"));
    s.input.push_str(":B4)");
    assert!(s.commit());
    assert_eq!(s.doc.cell_text(0, a("D3")), "12");
    // Undo / redo through the screen.
    s.undo();
    assert_eq!(s.doc.cell_text(0, a("D3")), "");
    s.redo();
    assert_eq!(s.doc.cell_text(0, a("D3")), "12");
}

#[test]
fn format_rows_sort_and_search() {
    let mut s = shop();
    s.set_editing(true);
    s.select(Area::new(a("A1"), a("C1")));
    let t = s.target();
    s.doc.set_bold(t, true).unwrap();
    assert!(s.doc.cell(0, a("B1")).unwrap().bold);
    // Whole-column selection is cut down to the used range for formatting.
    s.select(Area::new(Addr::new(0, 1), Addr::new(LAST_ROW, 1)));
    assert_eq!(s.target(), Area::new(a("B1"), a("B4")));
    // Insert a row above row 2 via the long-press action, then delete it again.
    s.select(Area::cell(a("A2")));
    let ctx = egui::Context::default();
    s.context_action(&ctx, "rowAbove");
    assert_eq!(s.doc.cell_text(0, a("A3")), "Pens");
    s.context_action(&ctx, "delRows");
    assert_eq!(s.doc.cell_text(0, a("A2")), "Pens");
    // Sort by Qty descending using the block around one selected cell.
    s.select(Area::cell(a("B3")));
    assert_eq!(s.data_area(), Area::new(a("A1"), a("C4")));
    let area = s.data_area();
    s.doc.sort(area, 1, true).unwrap();
    assert_eq!(s.doc.cell_text(0, a("A2")), "Ink");
    assert_eq!(s.doc.cell_text(0, a("A1")), "Item", "header row stays on top");
    // Search the workbook (a second sheet too) and jump to a hit.
    s.doc.add_sheet(Some("More")).unwrap();
    s.doc.set_cell(a("C5"), "ink refill").unwrap();
    s.arrive_on_sheet();
    s.switch_sheet(0);
    s.toggle_search();
    s.query = "ink".into();
    s.run_search();
    assert_eq!(s.hits, vec![(0, a("A2")), (1, a("C5"))]);
    s.step_match(1);
    s.go_to_hit();
    assert_eq!(s.sheet(), 1);
    assert_eq!(s.sel, Area::cell(a("C5")));
    assert_eq!(s.position(), (1, 2));
}

#[test]
fn save_xlsx_and_reopen() {
    let mut s = shop();
    s.set_editing(true);
    s.select(Area::cell(a("D2")));
    s.begin_edit(Some("=B2*C2".into()));
    let dir = std::env::temp_dir().join("document-slate-test");
    std::fs::create_dir_all(&dir).unwrap();
    // CSV can't hold the formula: write_to reports it, and the screen stays "changed".
    let csv = dir.join("grid-shop.csv");
    let lost = s.write_to(&csv).unwrap();
    assert!(lost.iter().any(|l| l.contains("formula")), "{lost:?}");
    assert!(s.dirty());
    assert!(s.can_save_in_place());
    // XLSX keeps everything; adopting it clears the changed mark.
    let x = dir.join("grid-shop.xlsx");
    let lost = s.write_to(&x).unwrap();
    assert!(lost.is_empty(), "{lost:?}");
    assert!(s.dirty(), "write_to must not mark the document saved");
    s.mark_saved("grid-shop.xlsx", &x);
    assert!(!s.dirty());
    assert_eq!(s.name(), "grid-shop.xlsx");
    let bytes = std::fs::read(&x).unwrap();
    let back = GridScreen::open("grid-shop.xlsx", &bytes).unwrap();
    assert_eq!(back.doc.cell(0, a("D2")).unwrap().formula.as_deref(), Some("B2*C2"));
    assert_eq!(back.doc.cell_text(0, a("D2")), "4.5");
    // PDF for printing.
    let pdf = dir.join("grid-shop.pdf");
    s.export_pdf(&pdf).unwrap();
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
}

#[test]
fn chart_and_edit_bar_frame() {
    let mut s = shop();
    s.set_editing(true);
    s.select(Area::cell(a("B2")));
    let area = s.data_area();
    s.doc.insert_chart(area, "column", None).unwrap();
    // Paint a frame with the chart plus the edit bar for every tab (no panics, texts drawn).
    let texts = frame_texts(&mut s, 3);
    assert!(texts.iter().any(|t| t == "Pens"), "{texts:?}");
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    for tab in [Tab::Cell, Tab::Formula, Tab::Data, Tab::Insert, Tab::File] {
        s.tab = tab;
        for _ in 0..2 {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let _ = s.edit_bar(ui, &cx());
            })
            .drop_without_applying_deltas();
        }
    }
    let info = s.info_line();
    assert!(info.contains("1 sheet"), "{info}");
}

#[test]
fn picture_insert_decode_and_filter() {
    let mut s = shop();
    s.set_editing(true);
    // A 1×1 PNG from Android's picker.
    let png = slate_grid::engine::io::base64_decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=").unwrap();
    let dir = std::env::temp_dir().join("document-slate-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("grid-pic.png");
    std::fs::write(&path, &png).unwrap();
    s.select(Area::cell(a("E2")));
    s.picked_image(path.to_str().unwrap());
    assert_eq!(s.doc.workbook().unwrap().sheet(0).unwrap().images.len(), 1);
    let (w, h, px) = s.doc.picture_rgba(0, 0, 1024).unwrap();
    assert_eq!((w, h, px.len()), (1, 1, 4));
    let _ = frame_texts(&mut s, 2);
    // Filter: on for the table around B3, show only "Paper".
    s.select(Area::cell(a("B3")));
    let area = s.data_area();
    s.doc.toggle_filter(area).unwrap();
    assert_eq!(s.doc.filter_range(0), Some(Area::new(a("A1"), a("C4"))));
    s.doc.filter_values(0, &["Paper"]).unwrap();
    let sh = s.doc.workbook().unwrap().sheet(0).unwrap();
    assert!(sh.is_row_filtered(1) && !sh.is_row_filtered(2) && sh.is_row_filtered(3));
    let texts = frame_texts(&mut s, 2);
    assert!(texts.iter().any(|t| t == "Paper") && !texts.iter().any(|t| t == "Pens"), "{texts:?}");
}

/// Not a real test: with GRID_FIXTURE=<in.xlsx>;<pic.png>;<out.xlsx> it adds a picture and a
/// filter to a workbook for the desktop visual check. Does nothing otherwise.
#[test]
fn make_visual_fixture() {
    let Ok(spec) = std::env::var("GRID_FIXTURE") else { return };
    let parts: Vec<&str> = spec.split(';').collect();
    let mut s = GridScreen::open("budget.xlsx", &std::fs::read(parts[0]).unwrap()).unwrap();
    s.select(Area::cell(a("K3")));
    s.picked_image(parts[1]);
    s.doc.toggle_filter(Area::new(a("A3"), a("I15"))).unwrap();
    s.write_to(Path::new(parts[2])).unwrap();
}

#[test]
fn pinch_zoom_keeps_point_and_big_sheet_is_quick() {
    let mut csv = String::new();
    for r in 0..3000 {
        let row: Vec<String> = (0..30).map(|c| format!("{}", r * 30 + c)).collect();
        csv.push_str(&row.join(","));
        csv.push('\n');
    }
    let mut s = GridScreen::open("big.csv", csv.as_bytes()).unwrap();
    s.dark_sheet = true;
    let _ = frame_texts(&mut s, 2);
    // Ctrl+wheel / pinch over the grid: zoom 2× about the pointer.
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(393.0, 851.0));
    let p = pos2(250.0, 400.0);
    let mut before = None;
    for i in 0..4 {
        let mut input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        input.events.push(egui::Event::PointerMoved(p));
        if i == 2 {
            before = s.last_geo.map(|g| g.sheet_point(p));
            input.events.push(egui::Event::Zoom(2.0));
        }
        let t = std::time::Instant::now();
        ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| s.show(ui, &cx()));
        })
        .drop_without_applying_deltas();
        assert!(t.elapsed().as_secs_f32() < 3.0, "frame took {:?}", t.elapsed());
    }
    assert!((s.zoom - 2.0).abs() < 0.01, "zoom {}", s.zoom);
    let after = s.last_geo.map(|g| g.sheet_point(p)).unwrap();
    let before = before.unwrap();
    assert!((after.0 - before.0).abs() < 2.0 && (after.1 - before.1).abs() < 2.0, "{before:?} -> {after:?}");
}

/// Links: a "Data!B3" link switches to that sheet and selects B3; a web link is only shown
/// (pop-up), never followed; a cell without a link isn't a link.
#[test]
fn cell_links_follow() {
    let mut s = shop();
    let data = s.doc.add_sheet(Some("Data")).unwrap();
    s.switch_sheet(0);
    s.doc.insert_link(a("E1"), "Data!B3", Some("To data")).unwrap();
    s.doc.insert_link(a("E2"), "https://example.org/grid", Some("Web")).unwrap();
    assert!(!s.follow_link(a("A1")), "A1 has no link");
    assert!(s.follow_link(a("E2")));
    assert_eq!(s.sheet(), 0, "a web link moved the view");
    assert!(s.follow_link(a("E1")));
    assert_eq!(s.sheet(), data);
    assert_eq!(s.sel, Area::cell(a("B3")));
    assert_eq!(s.want_visible, Some(a("B3")));
}
