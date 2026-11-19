//! Pictures, shapes and charts in WordSlate edit mode (WPS-Lite-level object editing).
//!
//! The engine selects an object by selecting its one placeholder character (`OBJ`) in the text,
//! so selecting is: hit-test the tap (`DocLayout::object_at`) -> select that character. Everything
//! else is engine commands on the selection: `arrange.bounds` (move/resize), `arrange.rotate`,
//! `arrange.wrap`, `shape.fill`, `chart.type`, `chart.editData`…
//!
//! On screen: a frame around the selected object, a round handle at the bottom-right corner to
//! resize it, and (for objects that float rather than sit in a line of text) drag anywhere inside
//! to move it. While dragging, an outline previews the new place; the engine command runs when
//! the finger lifts, so the whole move or resize is one undo step.

use super::*;
use wordcraft_engine::doc::Pos;
use wordcraft_engine::doc::para::{InlineObject, OBJ, Wrap};

/// What kind of object is selected (decides the Object tab's tools).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    Picture,
    Shape,
    Chart,
    /// SmartArt: shown and saved, editable only as a whole (move, size, wrap).
    Diagram,
}

#[derive(Clone, Debug)]
pub struct SelectedObject {
    pub kind: ObjKind,
    pub pos: Pos,
    pub page: usize,
    /// Frame on its page, in page points.
    pub rect: egui::Rect,
    /// In line with text: moves with the text, so it can be resized but not dragged.
    pub inline: bool,
}

/// A move or resize in progress.
#[derive(Clone, Copy, Debug)]
pub(super) struct ObjDrag {
    resize: bool,
    /// The preview frame, page points.
    rect: egui::Rect,
}

/// A chart's data as the editing grid shows it: categories (rows) × series (columns).
#[derive(Clone, Debug, Default)]
pub struct ChartTable {
    pub categories: Vec<String>,
    pub series: Vec<(String, Vec<String>)>,
}

impl ObjKind {
    /// Can it be moved, resized, wrapped, rotated and reordered? The engine doesn't do this for
    /// charts and SmartArt yet (they are kept and saved, and charts' data/type/labels edit).
    pub fn arrangeable(self) -> bool {
        matches!(self, ObjKind::Picture | ObjKind::Shape)
    }
}

impl WordViewer {
    /// Select the picture/shape/chart under a tap, if there is one. Returns true when it did.
    pub(super) fn try_select_object(&mut self, page: usize, x: f32, y: f32) -> bool {
        let Some(hit) = self.layout.object_at(page, x, y, 6.0) else { return false };
        let pos = Pos { story: hit.story, path: hit.path.clone(), off: hit.off };
        let is_drawing = self.session.doc.para_at(&pos).and_then(|p| p.object_at(pos.off)).is_some_and(InlineObject::is_drawing);
        if !is_drawing {
            return false;
        }
        let end = Pos { off: pos.off + OBJ.len_utf8(), ..pos.clone() };
        self.session.sel = Selection { anchor: pos, focus: end };
        self.keyboard_wanted = false;
        self.after_change();
        true
    }

    /// Work out the selected object from the selection (called after every change).
    pub(super) fn refresh_selected_object(&mut self) {
        self.selected = None;
        let (a, b) = (&self.session.sel.anchor, &self.session.sel.focus);
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        if start.story != end.story || start.path != end.path || end.off != start.off + OBJ.len_utf8() {
            return;
        }
        let Some(obj) = self.session.doc.para_at(start).and_then(|p| p.object_at(start.off)) else { return };
        let kind = match obj {
            InlineObject::Image { .. } => ObjKind::Picture,
            InlineObject::Shape { .. } | InlineObject::Group { .. } => ObjKind::Shape,
            InlineObject::Graphic { graphic, .. } => {
                if graphic.chart.is_some() { ObjKind::Chart } else { ObjKind::Diagram }
            }
            _ => return,
        };
        let inline = obj.frame().is_none_or(|(_, _, f)| f.wrap == Wrap::Inline);
        let Some(hit) = self.layout.object(start, self.current_page) else { return };
        let r = hit.rect;
        self.selected = Some(SelectedObject {
            kind,
            pos: start.clone(),
            page: hit.page,
            rect: egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.w, r.h)),
            inline,
        });
    }

    pub fn selected_object(&self) -> Option<&SelectedObject> {
        self.selected.as_ref()
    }

    /// The selected chart's data, for the editing grid.
    pub fn chart_table(&self) -> Option<ChartTable> {
        let sel = self.selected.as_ref()?;
        let InlineObject::Graphic { graphic, .. } = self.session.doc.para_at(&sel.pos)?.object_at(sel.pos.off)? else { return None };
        let spec = graphic.chart.as_ref()?;
        Some(ChartTable {
            categories: spec.categories.clone(),
            series: spec
                .series
                .iter()
                .map(|s| (s.name.clone(), s.values.iter().map(|v| v.map(|x| format!("{x}")).unwrap_or_default()).collect()))
                .collect(),
        })
    }

    /// Write an edited grid back to the selected chart (`chart.editData`, one undo step).
    pub fn apply_chart_table(&mut self, t: &ChartTable) {
        let series: Vec<serde_json::Value> = t
            .series
            .iter()
            .map(|(name, vals)| {
                let values: Vec<serde_json::Value> = vals
                    .iter()
                    .map(|v| v.trim().replace(',', ".").parse::<f64>().map(serde_json::Value::from).unwrap_or(serde_json::Value::Null))
                    .collect();
                serde_json::json!({ "name": name, "values": values })
            })
            .collect();
        self.run("chart.editData", serde_json::json!({ "data": { "categories": t.categories, "series": series } }));
    }

    /// Frame and handles over the selected object; handles move/resize drags.
    pub(super) fn object_overlay(&mut self, ui: &mut egui::Ui, page_rects: &[egui::Rect], scale: f32, accent: egui::Color32) {
        let Some(sel) = self.selected.clone() else {
            self.obj_drag = None;
            return;
        };
        let Some(page_r) = page_rects.get(sel.page).copied() else { return };
        let to_screen = |r: egui::Rect| egui::Rect::from_min_size(page_r.min + r.min.to_vec2() * scale, r.size() * scale);
        let frame = to_screen(sel.rect);
        const KNOB: f32 = 13.0;
        if !sel.kind.arrangeable() {
            self.obj_drag = None;
            ui.painter().rect_stroke(frame, 0.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Outside);
            return;
        }

        // Body: drag to move (floating objects only).
        let body = ui.interact(frame, ui.scope_id().with("obj-body"), if sel.inline { egui::Sense::hover() } else { egui::Sense::drag() });
        // Corner: drag to resize.
        let corner = egui::Rect::from_center_size(frame.right_bottom(), egui::Vec2::splat(KNOB * 4.0));
        let knob = ui.interact(corner, ui.scope_id().with("obj-resize"), egui::Sense::drag());

        for (resp, resize) in [(&knob, true), (&body, false)] {
            if resp.drag_started() {
                self.obj_drag = Some(ObjDrag { resize, rect: sel.rect });
            }
            if resp.dragged() {
                if let Some(d) = self.obj_drag.as_mut().filter(|d| d.resize == resize) {
                    let delta = resp.drag_delta() / scale;
                    if resize {
                        let mut size = d.rect.size() + delta;
                        if sel.kind == ObjKind::Picture {
                            // Pictures keep their shape: follow the larger of the two changes.
                            let ratio = (sel.rect.width() / sel.rect.height().max(1.0)).max(0.01);
                            size.y = size.x / ratio;
                        }
                        d.rect = egui::Rect::from_min_size(d.rect.min, size.max(egui::vec2(12.0, 12.0)));
                    } else {
                        d.rect = d.rect.translate(delta);
                    }
                }
            }
            if resp.drag_stopped() {
                if let Some(d) = self.obj_drag.take().filter(|d| d.resize == resize) {
                    let params = if resize {
                        serde_json::json!({ "width": d.rect.width(), "height": d.rect.height() })
                    } else {
                        serde_json::json!({ "x": d.rect.min.x, "y": d.rect.min.y, "page": sel.page })
                    };
                    self.run("arrange.bounds", params);
                }
            }
        }

        let painter = ui.painter();
        painter.rect_stroke(frame, 0.0, egui::Stroke::new(2.0, accent), egui::StrokeKind::Outside);
        if let Some(d) = &self.obj_drag {
            let ghost = to_screen(d.rect);
            painter.rect_filled(ghost, 0.0, accent.gamma_multiply(0.12));
            painter.rect_stroke(ghost, 0.0, egui::Stroke::new(1.5, accent), egui::StrokeKind::Outside);
        }
        painter.circle_filled(frame.right_bottom(), KNOB, accent);
        painter.circle_stroke(frame.right_bottom(), KNOB, egui::Stroke::new(2.0, egui::Color32::WHITE));
    }
}
