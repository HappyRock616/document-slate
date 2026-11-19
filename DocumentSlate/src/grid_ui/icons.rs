//! Grid Slate's extra icons (spreadsheet-only: AutoSum, fx, filter, freeze, number formats...)
//! and copies of the shared tool buttons that can draw them.
//!
//! Provenance: the drawings are adapted from upstream GridCraft's icon set
//! (`GridSlate/crates/ui-egui/src/icons.rs`, MIT OR Apache-2.0), on the same 16×16 design grid,
//! with a fixed palette instead of upstream's theme tokens. Names starting with "g." are drawn
//! here; every other name goes to the shared set (`crate::chrome::icon`, upstream WordCraft's).

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use egui::epaint::{CubicBezierShape, PathShape, PathStroke};

use crate::chrome;

const BLUE: Color32 = Color32::from_rgb(0x2F, 0x6D, 0xB5);
const GREEN: Color32 = Color32::from_rgb(0x2E, 0x8B, 0x57);
const RED: Color32 = Color32::from_rgb(0xD1, 0x3B, 0x3B);

struct G<'a> {
    p: &'a egui::Painter,
    r: Rect,
    s: f32,
}

impl G<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x * self.s, self.r.top() + y * self.s)
    }
    fn line(&self, pts: &[(f32, f32)], c: Color32, w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(PathShape::line(v, PathStroke::new(w * self.s.clamp(1.0, 2.0), c)));
    }
    fn stroke(&self, pts: &[(f32, f32)], c: Color32) {
        self.line(pts, c, 1.0);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, fill: Option<Color32>, stroke: Option<Color32>) {
        let r = Rect::from_min_size(self.pt(x, y), vec2(w * self.s, h * self.s));
        if let Some(f) = fill {
            self.p.rect_filled(r, 1.0, f);
        }
        if let Some(c) = stroke {
            self.p.rect_stroke(r, 1.0, Stroke::new(1.25, c), egui::StrokeKind::Inside);
        }
    }
    fn poly(&self, pts: &[(f32, f32)], fill: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(PathShape::convex_polygon(v, fill, Stroke::NONE));
    }
    fn text(&self, x: f32, y: f32, t: &str, size: f32, c: Color32, italic_family: bool) {
        let font = if italic_family && super::has_family(self.p.ctx(), "gs-serif-i") {
            egui::FontId::new(size * self.s, egui::FontFamily::Name("gs-serif-i".into()))
        } else {
            egui::FontId::proportional(size * self.s)
        };
        self.p.text(self.pt(x, y), egui::Align2::CENTER_CENTER, t, font, c);
    }
}

/// Draw icon `name` centred in `r` (24 dp), like `chrome::icon`.
pub fn icon(p: &egui::Painter, r: Rect, name: &str, fg: Color32, accent: Color32) {
    let Some(g_name) = name.strip_prefix("g.") else {
        chrome::icon(p, r, name, fg, accent);
        return;
    };
    let side = chrome::ICON.min(r.width()).min(r.height());
    let r = Rect::from_center_size(r.center(), Vec2::splat(side));
    let g = G { p, r, s: side / 16.0 };
    let c = fg;
    let light = c.gamma_multiply(0.45);
    let wash = |col: Color32| col.gamma_multiply(0.3);
    match g_name {
        "sum" => g.text(8.0, 8.0, "Σ", 14.0, c, false),
        "fx" => g.text(8.0, 8.0, "fx", 12.0, c, true),
        "cell" => {
            g.rect(1.5, 2.5, 13.0, 11.0, None, Some(c));
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], light);
            g.stroke(&[(1.5, 6.2), (14.5, 6.2)], light);
            g.stroke(&[(1.5, 9.8), (14.5, 9.8)], light);
            g.rect(6.0, 6.2, 4.0, 3.6, Some(wash(accent)), Some(accent));
        }
        "fill" => {
            g.poly(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0)], Color32::WHITE);
            g.line(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0), (3.0, 7.0)], c, 1.1);
            g.poly(&[(12.5, 9.0), (14.5, 9.0), (13.5, 11.5)], accent);
            g.rect(2.0, 13.5, 12.0, 2.0, Some(accent), None);
        }
        "alignTop" | "alignMiddle" | "alignBottom" => {
            let ys: [f32; 3] = match g_name {
                "alignTop" => [4.0, 6.5, 9.0],
                "alignMiddle" => [5.5, 8.0, 10.5],
                _ => [7.0, 9.5, 12.0],
            };
            match g_name {
                "alignTop" => g.stroke(&[(2.0, 1.8), (14.0, 1.8)], c),
                "alignBottom" => g.stroke(&[(2.0, 14.2), (14.0, 14.2)], c),
                _ => {}
            }
            for (i, y) in ys.iter().enumerate() {
                let w = if i == 1 { 7.0 } else { 10.0 };
                g.stroke(&[(8.0 - w / 2.0, *y), (8.0 + w / 2.0, *y)], c);
            }
        }
        "wrap" => {
            g.stroke(&[(2.0, 4.0), (14.0, 4.0)], c);
            g.stroke(&[(2.0, 8.0), (12.0, 8.0)], c);
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(12.0, 8.0), g.pt(15.0, 8.0), g.pt(15.0, 12.0), g.pt(11.0, 12.0)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.25, accent),
            ));
            g.poly(&[(9.0, 12.0), (11.5, 10.5), (11.5, 13.5)], accent);
            g.stroke(&[(2.0, 12.0), (7.0, 12.0)], c);
        }
        "merge" => {
            g.rect(1.5, 3.5, 13.0, 9.0, None, Some(c));
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], accent);
            g.poly(&[(3.0, 8.0), (5.5, 6.5), (5.5, 9.5)], accent);
            g.poly(&[(13.0, 8.0), (10.5, 6.5), (10.5, 9.5)], accent);
        }
        "numfmt" => {
            g.text(5.0, 8.5, "$", 10.0, c, false);
            g.text(11.0, 8.5, "%", 9.0, accent, false);
        }
        "decInc" => {
            g.text(6.0, 4.5, ".0", 7.0, c, false);
            g.text(10.0, 11.0, ".00", 7.0, c, false);
            g.stroke(&[(1.5, 13.0), (5.0, 13.0)], accent);
            g.poly(&[(0.5, 13.0), (2.5, 11.5), (2.5, 14.5)], accent);
        }
        "decDec" => {
            g.text(7.0, 4.5, ".00", 7.0, c, false);
            g.text(11.0, 11.0, ".0", 7.0, c, false);
            g.stroke(&[(1.5, 13.0), (5.5, 13.0)], accent);
            g.poly(&[(7.0, 13.0), (5.0, 11.5), (5.0, 14.5)], accent);
        }
        "insert" | "delete" | "rows" | "cols" | "sheet" => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.stroke(&[(2.0, 6.5), (14.0, 6.5)], light);
            g.stroke(&[(2.0, 10.0), (14.0, 10.0)], light);
            g.stroke(&[(6.0, 3.0), (6.0, 13.0)], light);
            g.stroke(&[(10.0, 3.0), (10.0, 13.0)], light);
            match g_name {
                "insert" => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(wash(accent)), Some(accent));
                    g.line(&[(0.5, 8.2), (4.5, 8.2)], GREEN, 1.6);
                    g.line(&[(2.5, 6.2), (2.5, 10.2)], GREEN, 1.6);
                }
                "delete" => {
                    g.line(&[(8.0, 6.0), (13.0, 11.0)], RED, 1.6);
                    g.line(&[(13.0, 6.0), (8.0, 11.0)], RED, 1.6);
                }
                "rows" => g.rect(2.0, 6.5, 12.0, 3.5, Some(wash(accent)), Some(accent)),
                "cols" => g.rect(6.0, 3.0, 4.0, 10.0, Some(wash(accent)), Some(accent)),
                _ => g.rect(2.0, 3.0, 12.0, 3.5, Some(wash(accent)), None),
            }
        }
        "sortAsc" | "sortDesc" => {
            let (a, b) = if g_name == "sortAsc" { ("A", "Z") } else { ("Z", "A") };
            g.text(5.0, 4.5, a, 7.0, c, false);
            g.text(5.0, 11.5, b, 7.0, c, false);
            g.stroke(&[(11.0, 2.5), (11.0, 12.5)], accent);
            g.poly(&[(8.8, 11.0), (13.2, 11.0), (11.0, 14.5)], accent);
        }
        "sortCustom" => {
            for (i, y) in [4.0f32, 8.0, 12.0].iter().enumerate() {
                g.stroke(&[(2.0, *y), (9.0 - i as f32 * 2.0, *y)], c);
            }
            g.stroke(&[(12.0, 2.5), (12.0, 12.5)], accent);
            g.poly(&[(9.8, 11.0), (14.2, 11.0), (12.0, 14.5)], accent);
        }
        "filter" => g.poly(&[(2.0, 3.0), (14.0, 3.0), (9.5, 8.5), (9.5, 13.5), (6.5, 12.0), (6.5, 8.5)], c),
        "filterCol" => {
            g.poly(&[(1.0, 2.0), (10.0, 2.0), (6.5, 6.5), (6.5, 10.5), (4.5, 9.5), (4.5, 6.5)], c);
            g.line(&[(9.0, 11.0), (15.0, 11.0)], accent, 1.4);
            g.line(&[(9.0, 14.0), (13.0, 14.0)], accent, 1.4);
        }
        "duplicates" => {
            g.rect(2.0, 2.0, 9.0, 4.0, None, Some(c));
            g.rect(5.0, 7.0, 9.0, 4.0, None, Some(c));
            g.line(&[(9.0, 12.0), (14.0, 15.0)], RED, 1.4);
            g.line(&[(14.0, 12.0), (9.0, 15.0)], RED, 1.4);
        }
        "freeze" => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(wash(accent)), None);
            g.rect(2.0, 2.5, 3.5, 11.0, Some(wash(accent)), None);
            g.line(&[(2.0, 5.5), (14.0, 5.5)], accent, 1.2);
            g.line(&[(5.5, 2.5), (5.5, 13.5)], accent, 1.2);
        }
        "note" => {
            g.poly(&[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (2.0, 11.0)], Color32::from_rgb(0xFF, 0xF2, 0xA8).gamma_multiply(0.6));
            g.line(&[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (7.0, 11.0), (4.0, 14.0), (4.0, 11.0), (2.0, 11.0), (2.0, 3.0)], c, 1.1);
        }
        "fit" => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.line(&[(5.0, 8.0), (11.0, 8.0)], accent, 1.2);
            g.poly(&[(4.0, 8.0), (6.5, 6.3), (6.5, 9.7)], accent);
            g.poly(&[(12.0, 8.0), (9.5, 6.3), (9.5, 9.7)], accent);
        }
        "printArea" => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.rect(5.0, 4.0, 6.0, 6.0, Some(wash(accent)), Some(accent));
        }
        "gridlines" => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            for x in [6.0, 10.0] {
                g.stroke(&[(x, 2.5), (x, 13.5)], c);
            }
            for y in [6.2, 9.8] {
                g.stroke(&[(2.0, y), (14.0, y)], c);
            }
        }
        "recent" => {
            g.p.circle_stroke(g.pt(8.0, 8.0), 6.0 * g.s, Stroke::new(1.25, c));
            g.line(&[(8.0, 4.5), (8.0, 8.0), (10.5, 9.5)], accent, 1.3);
        }
        "cat" => {
            // A function-category book (Formula tab): spine in the accent colour.
            g.rect(3.0, 2.0, 10.0, 12.0, None, Some(c));
            g.rect(3.0, 2.0, 2.5, 12.0, Some(accent), None);
            g.stroke(&[(7.0, 5.5), (11.0, 5.5)], light);
            g.stroke(&[(7.0, 8.0), (11.0, 8.0)], light);
        }
        "size" => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.line(&[(5.0, 8.0), (11.0, 8.0)], accent, 1.2);
            g.poly(&[(3.5, 8.0), (6.0, 6.3), (6.0, 9.7)], accent);
            g.poly(&[(12.5, 8.0), (10.0, 6.3), (10.0, 9.7)], accent);
        }
        _ => {
            // Unknown: a framed first letter, like the shared set's fallback.
            p.rect_stroke(r.shrink(r.width() * 0.12), 3.0, Stroke::new(1.0, c), egui::StrokeKind::Middle);
            let ch = g_name.chars().next().map(|x| x.to_ascii_uppercase().to_string()).unwrap_or_default();
            p.text(r.center(), egui::Align2::CENTER_CENTER, ch, egui::FontId::proportional(r.height() * 0.45), accent);
        }
    }
    let _ = BLUE;
}

/// Copy of `chrome::tool` (icon with a label under it, 60×60) that also draws "g." icons.
pub fn tool(ui: &mut egui::Ui, name: &str, label: &str, on: bool, accent: Color32) -> egui::Response {
    let fg = ui.visuals().text_color();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(60.0, 60.0), egui::Sense::click());
    let p = ui.painter();
    let bg = if on {
        Some(accent.gamma_multiply(0.30))
    } else if resp.is_pointer_button_down_on() {
        Some(fg.gamma_multiply(0.12))
    } else {
        None
    };
    if let Some(bg) = bg {
        p.rect_filled(r.shrink(2.0), 10.0, bg);
    }
    let icon_r = Rect::from_center_size(egui::pos2(r.center().x, r.top() + 22.0), Vec2::splat(chrome::ICON));
    icon(p, icon_r, name, fg, accent);
    p.text(egui::pos2(r.center().x, r.bottom() - 11.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.0), fg.gamma_multiply(0.85));
    resp
}

/// Copy of `chrome::tool_menu` (a tool that opens an upward pop-up menu) using [`tool`].
pub fn tool_menu(ui: &mut egui::Ui, name: &str, label: &str, accent: Color32, menu: impl FnOnce(&mut egui::Ui)) {
    let resp = tool(ui, name, label, false, accent);
    let room = (resp.rect.top() - ui.ctx().content_rect().top() - 48.0).max(160.0);
    egui::Popup::menu(&resp)
        .align(egui::RectAlign::TOP_START)
        .align_alternatives(&[egui::RectAlign::TOP_END, egui::RectAlign::TOP])
        .show(|ui| {
            ui.set_min_width(180.0);
            ui.style_mut().spacing.button_padding = egui::vec2(12.0, 10.0);
            egui::ScrollArea::vertical().max_height(room).show(ui, menu);
        });
}
