//! Deck Slate screen tests: shapes (insert every gallery kind, fill / outline colour / width /
//! dash / arrowheads), finger gestures (tap, drag to move, handle to resize, rotate) through the
//! real egui screen, typing in a shape, slides, PDF, search, and saving to .pptx and reopening
//! with every shape and style intact. Files go to `_cleanup/deck-ui/`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::tools::{self, GALLERY, LINE_BOTH};
use super::*;
use slate_deck::shapes::Paint;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../_cleanup/deck-ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn blank_slide_screen() -> DeckScreen {
    let mut s = DeckScreen::new_blank();
    s.current = s.doc.new_slide(Some("blank")).unwrap();
    s.set_editing(true);
    s
}

fn ctx() -> ScreenCtx {
    ScreenCtx { accent: egui::Color32::from_rgb(0xD9, 0x73, 0x2B), dark: true, scratch: std::env::temp_dir(), top_inset: 0.0, bottom_inset: 0.0 }
}

/// The one selected shape.
fn sel(s: &DeckScreen) -> ShapeId {
    let v = s.doc.selected();
    assert_eq!(v.len(), 1, "exactly one object should be selected");
    v[0]
}

#[test]
fn gallery_shapes_all_insert_and_preview() {
    let mut s = blank_slide_screen();
    let mut count = s.doc.objects().len();
    for (_, names) in GALLERY {
        for &name in names {
            let base = if name == LINE_BOTH { "line" } else { name };
            let outline = slate_deck::shapes::preset_outline(base, 40.0, 30.0).unwrap_or_else(|| panic!("no outline for {name}"));
            assert!(!outline.is_empty() && outline.iter().any(|p| p.points.len() > 1), "{name} preview is empty");
            tools::insert_gallery_shape(&mut s, name);
            count += 1;
            assert_eq!(s.doc.objects().len(), count, "{name} wasn't inserted");
            let id = sel(&s);
            let look = s.doc.shape_look(id).unwrap();
            assert_eq!(look.preset.as_deref(), Some(base), "{name}");
            assert!(s.selected_object().is_some(), "{name} should show the object tools");
        }
    }
    // The "both ends" line really has two arrowheads; the arrow line one at its end.
    let objs = s.doc.objects();
    let looks: Vec<_> = objs.iter().filter_map(|o| s.doc.shape_look(o.id)).collect();
    assert!(looks.iter().any(|l| l.preset.as_deref() == Some("line") && l.head == "triangle" && l.tail == "triangle"));
    assert!(looks.iter().any(|l| l.preset.as_deref() == Some("straightConnector1") && l.tail != "none"));
    // The whole engine catalog draws as previews too.
    for (name, ..) in slate_deck::shapes::all_presets() {
        assert!(slate_deck::shapes::preset_outline(name, 40.0, 40.0).is_some(), "{name}");
    }
}

/// Style a shape set the way the tools do, check the look, save as .pptx, reopen, check again.
#[test]
fn shape_styles_survive_pptx_round_trip() {
    let mut s = blank_slide_screen();
    // A box: red fill 50 % see-through, blue 4.5 pt dashed outline, with text.
    tools::insert_gallery_shape(&mut s, "roundRect");
    let boxed = sel(&s);
    s.doc.fill_color(&[], Some("#FF0000")).unwrap();
    s.doc.fill_transparency(&[], 0.5).unwrap();
    s.doc.outline_color(&[], Some("#0070C0")).unwrap();
    s.doc.outline_width(&[], 4.5).unwrap();
    s.doc.outline_dash(&[], "dash").unwrap();
    s.doc.set_text(boxed, "Hello box").unwrap();
    // A speech bubble with no outline and a yellow fill.
    tools::insert_gallery_shape(&mut s, "wedgeRoundRectCallout");
    let bubble = sel(&s);
    s.doc.fill_color(&[], Some("#FFFF00")).unwrap();
    s.doc.outline_color(&[], None).unwrap();
    // An arrow shape with no fill and a thick green edge.
    tools::insert_gallery_shape(&mut s, "rightArrow");
    let arrow = sel(&s);
    s.doc.fill_color(&[], None).unwrap();
    s.doc.outline_color(&[], Some("#00B050")).unwrap();
    s.doc.outline_width(&[], 6.0).unwrap();
    // A line with arrowheads at both ends, open-arrow style, round dots.
    tools::insert_gallery_shape(&mut s, "line");
    let line = sel(&s);
    s.doc.arrowheads(&[], Some("arrow"), Some("arrow")).unwrap();
    s.doc.outline_width(&[], 2.25).unwrap();
    s.doc.outline_dash(&[], "sysDot").unwrap();

    let check = |s: &DeckScreen, ids: [ShapeId; 4], when: &str| {
        let [boxed, bubble, arrow, line] = ids;
        let b = s.doc.shape_look(boxed).unwrap();
        match b.fill {
            Paint::Solid([255, 0, 0, a]) => assert!((120..=135).contains(&a), "{when}: box alpha {a}"),
            f => panic!("{when}: box fill {f:?}"),
        }
        assert_eq!(b.line, Paint::Solid([0x00, 0x70, 0xC0, 255]), "{when}");
        assert!((b.line_width - 4.5).abs() < 0.01, "{when}: {}", b.line_width);
        assert_eq!(b.dash, "dash", "{when}");
        let u = s.doc.shape_look(bubble).unwrap();
        assert_eq!(u.fill, Paint::Solid([255, 255, 0, 255]), "{when}");
        assert_eq!(u.line, Paint::None, "{when}");
        assert_eq!(u.preset.as_deref(), Some("wedgeRoundRectCallout"));
        let a = s.doc.shape_look(arrow).unwrap();
        assert_eq!(a.fill, Paint::None, "{when}");
        assert_eq!(a.line, Paint::Solid([0x00, 0xB0, 0x50, 255]), "{when}");
        assert!((a.line_width - 6.0).abs() < 0.01, "{when}");
        let l = s.doc.shape_look(line).unwrap();
        assert!(l.is_line);
        assert_eq!((l.head.as_str(), l.tail.as_str()), ("arrow", "arrow"), "{when}");
        assert!((l.line_width - 2.25).abs() < 0.01, "{when}");
        assert_eq!(l.dash, "sysDot", "{when}");
    };
    check(&s, [boxed, bubble, arrow, line], "before save");

    let path = scratch("shapes-roundtrip.pptx");
    let lost = s.write_to(&path).unwrap();
    assert!(lost.is_empty(), "a new deck loses nothing: {lost:?}");
    assert!(s.dirty(), "write_to must not mark the document saved");
    s.mark_saved("shapes-roundtrip.pptx", &path);
    assert!(!s.dirty());

    let bytes = std::fs::read(&path).unwrap();
    let mut r = DeckScreen::open("shapes-roundtrip.pptx", &bytes).unwrap();
    assert!(r.can_save_in_place());
    r.doc.go_to_slide(1).unwrap();
    // Find the shapes again by their preset.
    let find = |r: &DeckScreen, preset: &str| r.doc.objects().into_iter().find(|o| r.doc.shape_look(o.id).and_then(|l| l.preset).as_deref() == Some(preset)).map(|o| o.id).unwrap_or_else(|| panic!("{preset} lost"));
    let ids = [find(&r, "roundRect"), find(&r, "wedgeRoundRectCallout"), find(&r, "rightArrow"), find(&r, "line")];
    check(&r, ids, "after reopening");
    assert_eq!(r.doc.text_of(ids[0]).unwrap(), "Hello box");
    // Existing shapes from an opened file are editable the same way.
    r.doc.select(&[ids[1]]).unwrap();
    r.doc.fill_color(&[], Some("#7030A0")).unwrap();
    r.doc.outline_width(&[], 3.0).unwrap();
    let u = r.doc.shape_look(ids[1]).unwrap();
    assert_eq!(u.fill, Paint::Solid([0x70, 0x30, 0xA0, 255]));
    assert!((u.line_width - 3.0).abs() < 0.01);
}

// ------------------------------------------------------------------ the real egui screen

struct Harness {
    ctx: egui::Context,
    s: DeckScreen,
    t: f64,
    /// The slide's rectangle on screen and its scale (screen points per slide point).
    slide: egui::Rect,
    scale: f32,
}

impl Harness {
    fn new(s: DeckScreen) -> Self {
        let mut h = Harness { ctx: egui::Context::default(), s, t: 0.0, slide: egui::Rect::NOTHING, scale: 1.0 };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.t += 1.0 / 30.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(393.0, 851.0))),
            time: Some(self.t),
            events,
            ..Default::default()
        };
        let cx = ctx();
        let s = &mut self.s;
        let mut area = egui::Rect::NOTHING;
        let mut out = self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                area = ui.available_rect_before_wrap();
                s.show(ui, &cx);
            });
        });
        out.textures_delta.clear();
        // Same fit as show_editor (below the strip, 16 pt margin).
        let (sw, sh) = self.s.doc.slide_size();
        let a = egui::Rect::from_min_max(egui::pos2(area.left(), area.top() + STRIP_H + self.strip_extra()), area.max);
        let fit = ((a.width() - 16.0) / sw as f32).min((a.height() - 16.0) / sh as f32);
        self.scale = fit * self.s.edit_zoom;
        self.slide = egui::Rect::from_center_size(a.center() + self.s.pan, egui::vec2(sw as f32 * self.scale, sh as f32 * self.scale));
    }

    /// The strip's horizontal layout adds item spacing under it.
    fn strip_extra(&self) -> f32 {
        self.ctx.global_style().spacing.item_spacing.y
    }

    fn screen(&self, p: (f64, f64)) -> egui::Pos2 {
        egui::pos2(self.slide.min.x + p.0 as f32 * self.scale, self.slide.min.y + p.1 as f32 * self.scale)
    }

    fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    fn tap(&mut self, p: egui::Pos2) {
        self.frame(vec![egui::Event::PointerMoved(p), Self::press(p, true)]);
        self.frame(vec![Self::press(p, false)]);
        self.frame(vec![]);
        // Far apart in time, so two taps aren't a double tap.
        self.t += 1.0;
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        self.frame(vec![egui::Event::PointerMoved(from), Self::press(from, true)]);
        for i in 1..=8 {
            let p = from + (to - from) * (i as f32 / 8.0);
            self.frame(vec![egui::Event::PointerMoved(p)]);
        }
        self.frame(vec![Self::press(to, false)]);
        self.frame(vec![]);
        self.t += 1.0;
    }
}

fn rect_of(s: &DeckScreen, id: ShapeId) -> (f64, f64, f64, f64) {
    s.doc.objects().into_iter().find(|o| o.id == id).unwrap().rect
}

#[test]
fn finger_select_move_resize_rotate_type() {
    let mut s = blank_slide_screen();
    let id = s.doc.insert_shape("rect", slate_deck::edit::Rect::new(100.0, 100.0, 200.0, 120.0), None).unwrap();
    s.doc.deselect().unwrap();
    let mut h = Harness::new(s);
    assert!(h.s.doc.selected().is_empty());

    // Tap selects.
    let centre = h.screen((200.0, 160.0));
    h.tap(centre);
    assert_eq!(h.s.doc.selected(), vec![id], "tap should select the box");
    let undo_before = h.s.doc.can_undo();

    // Drag moves it by about the finger's distance, as one undo step.
    let to = centre + egui::vec2(60.0, 40.0);
    h.drag(centre, to);
    let (x, y, w, hh) = rect_of(&h.s, id);
    let (dx, dy) = (60.0 / h.scale as f64, 40.0 / h.scale as f64);
    assert!((x - (100.0 + dx)).abs() < 8.0 && (y - (100.0 + dy)).abs() < 8.0, "moved to {x},{y} (expected ~{},{})", 100.0 + dx, 100.0 + dy);
    assert!((w - 200.0).abs() < 0.5 && (hh - 120.0).abs() < 0.5);
    assert!(h.s.doc.can_undo() || undo_before);
    h.s.undo();
    let (x0, y0, ..) = rect_of(&h.s, id);
    assert!((x0 - 100.0).abs() < 0.5 && (y0 - 100.0).abs() < 0.5, "one undo puts the whole move back: {x0},{y0}");
    h.s.redo();

    // The bottom-right handle resizes.
    h.s.doc.select(&[id]).unwrap();
    h.frame(vec![]);
    let handles = h.s.doc.selection_handles();
    let se = h.screen(handles[0].resize[4]);
    h.drag(se, se + egui::vec2(50.0, 30.0));
    let (_, _, w2, h2) = rect_of(&h.s, id);
    assert!(w2 > 200.0 + 20.0 / h.scale as f64 && h2 > 120.0 + 10.0 / h.scale as f64, "resized to {w2} x {h2}");

    // The rotate knob turns it.
    let handles = h.s.doc.selection_handles();
    let knob = rotate_spot(h.screen(handles[0].resize[1]), h.screen(handles[0].rotate), h.screen(handles[0].resize[5]));
    let c = rect_of(&h.s, id);
    let mid = h.screen((c.0 + c.2 / 2.0, c.1 + c.3 / 2.0));
    let r = (knob - mid).length();
    h.drag(knob, mid + egui::vec2(r, 0.0));
    let rot = h.s.doc.objects().into_iter().find(|o| o.id == id).unwrap().rotation;
    assert!((rot - 90.0).abs() < 10.0, "rotated to {rot}°");

    // Tapping the selected, empty box starts typing; typed keys go in; caret shows.
    h.s.doc.rotate_to(&[id], 0.0).unwrap();
    h.frame(vec![]);
    let c = rect_of(&h.s, id);
    let mid = h.screen((c.0 + c.2 / 2.0, c.1 + c.3 / 2.0));
    h.tap(mid);
    assert_eq!(h.s.doc.editing_text(), Some(id), "second tap on an empty shape types in it");
    assert!(h.s.keyboard_wanted);
    h.frame(vec![egui::Event::Text("Hi there".into())]);
    h.frame(vec![egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
    assert_eq!(h.s.doc.text_of(id).unwrap(), "Hi ther");
    assert!(h.s.doc.caret().is_some());

    // Tap on empty slide leaves the shape.
    let empty = h.screen((900.0, 500.0));
    h.tap(empty);
    assert!(h.s.doc.editing_text().is_none());
}

#[test]
fn viewer_counts_slides_and_slideshow_runs() {
    let s = DeckScreen::open("sample.pptx", &slate_deck::DeckDoc::sample().unwrap().save_bytes(SaveFormat::Pptx).unwrap().bytes).unwrap();
    let n = s.doc.slide_count();
    assert!(n >= 3);
    assert_eq!(s.position(), (0, n));
    let mut h = Harness::new(s);
    h.s.view_action("show");
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(h.s.show.is_some() && h.s.search_open(), "slideshow reports itself so Back ends it");
    // Tap on the right advances, on the left goes back.
    h.tap(egui::pos2(350.0, 400.0));
    assert_eq!(h.s.show.as_ref().unwrap().pos, 1);
    h.tap(egui::pos2(30.0, 400.0));
    assert_eq!(h.s.show.as_ref().unwrap().pos, 0);
    h.s.toggle_search(); // = Back
    assert!(h.s.show.is_none() && !h.s.search_open());
    h.frame(vec![]);
}

#[test]
fn slides_notes_search_pdf() {
    let mut s = blank_slide_screen();
    let n0 = s.doc.slide_count();
    s.doc.duplicate_slide(1).unwrap();
    assert_eq!(s.doc.slide_count(), n0 + 1);
    s.doc.set_slide_hidden(0, true).unwrap();
    assert_eq!(s.doc.show_order().len(), n0);
    s.doc.set_notes(1, "Remember the arrows").unwrap();
    assert_eq!(s.doc.notes(1), "Remember the arrows");
    s.doc.move_slide(2, 0).unwrap();
    s.doc.go_to_slide(2).unwrap();
    let id = s.doc.insert_text_box(None, "Find the needle here").unwrap();
    assert!(s.doc.text_of(id).unwrap().contains("needle"));
    // Search (viewer).
    s.set_editing(false);
    s.toggle_search();
    s.query = "needle".into();
    s.run_search();
    assert_eq!(s.hits, vec![2]);
    assert_eq!(s.current, 2);
    // PDF: slides and handouts.
    for layout in [PdfLayout::Slides, PdfLayout::Handouts { per_page: 6 }, PdfLayout::Notes] {
        s.pdf_layout = layout;
        let p = scratch("deck-test.pdf");
        s.export_pdf(&p).unwrap();
        assert!(std::fs::read(&p).unwrap().starts_with(b"%PDF"), "{layout:?}");
        assert_eq!(s.pdf_layout, PdfLayout::Slides, "layout resets after use");
    }
    assert!(s.info_line().contains("slides"));
}

/// The edit bar draws every tab and the object tools without panicking.
#[test]
fn edit_bar_draws_every_tab_and_object_tools() {
    let mut s = blank_slide_screen();
    let ctx = egui::Context::default();
    let cx = ctx_for_test();
    let draw = |s: &mut DeckScreen| {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(393.0, 851.0))), ..Default::default() }, |ui| {
            let _ = s.edit_bar(ui, &cx);
        });
        out.textures_delta.clear();
    };
    for tab in [tools::Tab::Slides, tools::Tab::Text, tools::Tab::Insert, tools::Tab::Design, tools::Tab::File] {
        s.tabs.tab = tab;
        draw(&mut s);
    }
    for name in ["rect", "line", "wedgeEllipseCallout"] {
        tools::insert_gallery_shape(&mut s, name);
        draw(&mut s);
    }
    s.doc.insert_table(2, 2, None).unwrap();
    draw(&mut s);
}

fn ctx_for_test() -> ScreenCtx {
    ctx()
}



/// A speech bubble's tail (yellow adjust handle) follows the finger, and the change survives
/// saving to .pptx.
#[test]
fn bubble_tail_drag_and_save() {
    let mut s = blank_slide_screen();
    let id = s.doc.insert_shape("wedgeRectCallout", slate_deck::edit::Rect::new(300.0, 150.0, 300.0, 160.0), None).unwrap();
    s.doc.select(&[id]).unwrap();
    let mut h = Harness::new(s);
    let before = h.s.doc.adjust_handles();
    assert!(!before.is_empty(), "a bubble has a tail handle");
    let tail = h.screen(before[0].2);
    h.drag(tail, tail + egui::vec2(-40.0, 25.0));
    let after = h.s.doc.adjust_handles();
    assert_eq!(h.s.doc.objects().into_iter().find(|o| o.id == id).unwrap().rect, (300.0, 150.0, 300.0, 160.0), "the box itself stays");
    let (b, a) = (before[0].2, after[0].2);
    assert!(a.0 < b.0 - 40.0 && a.1 > b.1 + 25.0, "tail moved from {b:?} to {a:?}");
    let path = scratch("bubble.pptx");
    h.s.write_to(&path).unwrap();
    let mut r = DeckScreen::open("bubble.pptx", &std::fs::read(&path).unwrap()).unwrap();
    r.doc.go_to_slide(1).unwrap();
    let rid = r.doc.objects()[0].id;
    r.doc.select(&[rid]).unwrap();
    let ra = r.doc.adjust_handles()[0].2;
    assert!((ra.0 - a.0).abs() < 1.0 && (ra.1 - a.1).abs() < 1.0, "tail kept after reopening: {ra:?} vs {a:?}");
}

/// Viewer taps on links: a linked shape is a web link (pop-up, no jump), a slide link jumps,
/// empty space isn't a link (the tap toggles the bars instead).
#[test]
fn viewer_link_taps() {
    use slate_deck::edit::Rect;
    let mut s = blank_slide_screen();
    s.doc.new_slide(Some("blank")).unwrap();
    s.doc.go_to_slide(1).unwrap();
    let shape = s.doc.insert_shape("rect", Rect::new(100.0, 100.0, 200.0, 100.0), None).unwrap();
    s.doc.select(&[shape]).unwrap();
    s.doc.insert_link("https://example.org/deck", None).unwrap();
    let star = s.doc.insert_shape("star5", Rect::new(600.0, 100.0, 150.0, 150.0), None).unwrap();
    s.doc.select(&[star]).unwrap();
    s.doc.insert_slide_link(2, None).unwrap();
    s.doc.deselect().unwrap();
    s.set_editing(false);
    let n = s.doc.slide_count();
    assert_eq!(s.link_tap(1, 200.0, 150.0, n), Some(LinkGo::Shown));
    assert_eq!(s.link_tap(1, 675.0, 175.0, n), Some(LinkGo::Slide(2)));
    assert_eq!(s.link_tap(1, 40.0, 480.0, n), None);
}
