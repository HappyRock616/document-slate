//! Shape helpers for the app's shape gallery and shape tools (Deck Slate's main editing job:
//! boxes, speech bubbles, arrows and lines with their colours, edge thickness and fill).
//!
//! - [`preset_outline`]: a preset's outline as plain polylines, so the app can draw gallery
//!   previews with its own painter (no engine rendering needed).
//! - [`DeckDoc::shape_look`]: what a shape looks like now (effective fill, outline colour, width,
//!   dash, arrowheads), resolved through the theme, to light up the matching tool buttons.
//! - Outline extras the basic `outline` helper lacks: dash style, arrowheads, transparency.
//! - The text caret and text selection of the text being typed, in slide points.

use serde_json::{Value, json};

use crate::{DeckDoc, DeckResult, ShapeId};

/// One piece of a preset's outline, in the box (0,0)-(w,h).
#[derive(Clone, Debug, PartialEq)]
pub struct OutlinePath {
    pub points: Vec<(f32, f32)>,
    pub closed: bool,
    /// Drawn with the outline (some pieces are fill-only, e.g. shading on a cube).
    pub stroke: bool,
    /// Filled (closed pieces of shapes; lines are never filled).
    pub filled: bool,
}

/// The outline of preset `name` drawn in a `w` x `h` box (`None` = unknown preset). Curves are
/// flattened to short straight segments.
pub fn preset_outline(name: &str, w: f64, h: f64) -> Option<Vec<OutlinePath>> {
    let g = deckcraft_geom::preset::build(name, w, h, &[])?;
    let mut out = Vec::new();
    for sp in &g.paths {
        let filled = sp.fill != deckcraft_geom::preset::FillMode::None;
        let mut pts: Vec<(f32, f32)> = Vec::new();
        let mut push = |pts: &mut Vec<(f32, f32)>, closed: bool| {
            if pts.len() > 1 {
                out.push(OutlinePath { points: std::mem::take(pts), closed, stroke: sp.stroke, filled: filled && closed });
            } else {
                pts.clear();
            }
        };
        deckcraft_geom::preset_flatten(&sp.path, &mut |el| match el {
            deckcraft_geom::PathEl::MoveTo(p) => {
                push(&mut pts, false);
                pts.push((p.x as f32, p.y as f32));
            }
            deckcraft_geom::PathEl::LineTo(p) => pts.push((p.x as f32, p.y as f32)),
            deckcraft_geom::PathEl::ClosePath => push(&mut pts, true),
            _ => {}
        });
        push(&mut pts, false);
    }
    Some(out)
}

/// Is `name` drawn as a line (no fill, two ends)?
pub fn preset_is_line(name: &str) -> bool {
    deckcraft_geom::preset::is_line_like(name)
}

/// Every preset the engine can draw: (id, label, category), in gallery order.
pub fn all_presets() -> Vec<(&'static str, &'static str, &'static str)> {
    deckcraft_geom::preset::CATALOG.iter().map(|p| (p.name, p.label, p.category.label())).collect()
}

/// The label of a preset ("Rectangle: Rounded Corners").
pub fn preset_label(name: &str) -> Option<&'static str> {
    deckcraft_geom::preset::info(name).map(|i| i.label)
}

/// A colour as the shape tools see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// No fill / no outline.
    None,
    /// A plain colour (r, g, b, alpha 0-255), resolved through the theme.
    Solid([u8; 4]),
    /// Gradient, picture, pattern…: something a swatch can't show.
    Other,
}

/// How a shape looks now: effective values (the shape's own, else its theme style's).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeLook {
    /// Preset name ("rect", "wedgeRectCallout"…), `None` for custom geometry.
    pub preset: Option<String>,
    pub is_line: bool,
    pub fill: Paint,
    pub line: Paint,
    /// Outline width in points.
    pub line_width: f64,
    /// Dash preset ("solid", "dash", "sysDot"…).
    pub dash: String,
    /// Arrowhead at the start (`head`) and end (`tail`) of a line: "none", "triangle", "arrow"…
    pub head: String,
    pub tail: String,
    /// Has a text body (can be typed in).
    pub has_text_body: bool,
}

fn paint_of(ctx: &deckcraft_model::resolve::Ctx, fill: Option<&deckcraft_model::Fill>, ph: Option<deckcraft_model::Rgba>) -> Paint {
    match fill {
        None | Some(deckcraft_model::Fill::None) => Paint::None,
        Some(deckcraft_model::Fill::Solid { color }) => {
            let c = ctx.color(color, ph);
            Paint::Solid([c.r, c.g, c.b, c.a])
        }
        Some(_) => Paint::Other,
    }
}

/// Arrowhead kinds the shape tools offer (engine names).
pub const ARROWHEADS: [&str; 6] = ["none", "triangle", "arrow", "stealth", "diamond", "oval"];

/// Dash styles the shape tools offer (engine names).
pub const DASHES: [&str; 7] = ["solid", "sysDot", "sysDash", "dash", "dashDot", "lgDash", "lgDashDot"];

impl DeckDoc {
    /// What shape `id` on the current slide looks like (`None` = no such shape).
    pub fn shape_look(&self, id: ShapeId) -> Option<ShapeLook> {
        let st = self.state().ok()?;
        let sh = st.shape(id)?;
        let slide = st.doc.slides.get(st.selection.slide)?;
        let ctx = deckcraft_model::resolve::Ctx::for_slide(&st.doc, slide)?;
        let (fill, fph) = deckcraft_model::resolve::fill(&ctx, sh);
        let (line, lph) = deckcraft_model::resolve::line(&ctx, sh);
        let is_line = sh.is_line();
        let preset = match &sh.geom {
            deckcraft_model::Geom::Preset { name, .. } => Some(name.clone()),
            _ => None,
        };
        let dash = match &line.dash {
            Some(deckcraft_model::style::Dash::Preset { name }) => name.clone(),
            Some(_) => "custom".into(),
            None => "solid".into(),
        };
        let end = |e: &Option<deckcraft_model::style::LineEnd>| e.as_ref().map(|e| e.kind.clone()).unwrap_or_else(|| "none".into());
        Some(ShapeLook {
            preset,
            is_line,
            fill: if is_line { Paint::None } else { paint_of(&ctx, fill.as_ref(), fph) },
            line: paint_of(&ctx, line.fill.as_ref(), lph),
            line_width: line.width.unwrap_or(0.75),
            dash,
            head: end(&line.head),
            tail: end(&line.tail),
            has_text_body: sh.text.is_some(),
        })
    }

    fn ids_param(ids: &[ShapeId], mut p: Value) -> Value {
        if !ids.is_empty() {
            p["ids"] = json!(ids.iter().map(|i| i.0).collect::<Vec<_>>());
        }
        p
    }

    /// Outline width only (keeps the colour). Points.
    pub fn outline_width(&mut self, ids: &[ShapeId], width: f64) -> DeckResult<()> {
        self.run("shape.line", Self::ids_param(ids, json!({"width": width}))).map(|_| ())
    }

    /// Outline colour only (keeps the width); `None` = no outline.
    pub fn outline_color(&mut self, ids: &[ShapeId], color: Option<&str>) -> DeckResult<()> {
        let p = match color {
            Some(c) => json!({"color": c}),
            None => json!({"none": true}),
        };
        self.run("shape.line", Self::ids_param(ids, p)).map(|_| ())
    }

    /// Dash style (see [`DASHES`]).
    pub fn outline_dash(&mut self, ids: &[ShapeId], dash: &str) -> DeckResult<()> {
        self.run("shape.line", Self::ids_param(ids, json!({"dash": dash}))).map(|_| ())
    }

    /// Arrowheads of a line: `head` at its start, `tail` at its end (see [`ARROWHEADS`]);
    /// `None` leaves that end alone.
    pub fn arrowheads(&mut self, ids: &[ShapeId], head: Option<&str>, tail: Option<&str>) -> DeckResult<()> {
        let mut p = json!({});
        if let Some(h) = head {
            p["head"] = json!(h);
        }
        if let Some(t) = tail {
            p["tail"] = json!(t);
        }
        self.run("shape.line", Self::ids_param(ids, p)).map(|_| ())
    }

    /// Fill transparency 0 (opaque) .. 1 (invisible), keeping the fill colour.
    pub fn fill_transparency(&mut self, ids: &[ShapeId], t: f64) -> DeckResult<()> {
        self.run("shape.fill", Self::ids_param(ids, json!({"transparency": t.clamp(0.0, 1.0)}))).map(|_| ())
    }

    /// Turn the selected shapes into another preset (keeps text, colours and box).
    pub fn change_shape(&mut self, ids: &[ShapeId], preset: &str) -> DeckResult<()> {
        self.run("shape.change", Self::ids_param(ids, json!({"preset": preset}))).map(|_| ())
    }

    /// Flip horizontally (`true`) or vertically.
    pub fn flip(&mut self, ids: &[ShapeId], horizontal: bool) -> DeckResult<()> {
        let cmd = if horizontal { "arrange.flipHorizontal" } else { "arrange.flipVertical" };
        self.run(cmd, Self::ids_param(ids, json!({}))).map(|_| ())
    }

    /// Turn 90° to the right.
    pub fn rotate_right(&mut self, ids: &[ShapeId]) -> DeckResult<()> {
        self.run("arrange.rotateRight", Self::ids_param(ids, json!({}))).map(|_| ())
    }

    // ----- text being typed --------------------------------------------------------------

    /// The text caret (top and bottom points of its line, slide points), while typing.
    pub fn caret(&self) -> Option<((f64, f64), (f64, f64))> {
        let st = self.state().ok()?;
        let ts = st.selection.text.as_ref()?;
        let (a, b) = deckcraft_engine::cmd::text::caret_line(st, ts)?;
        Some(((a.x, a.y), (b.x, b.y)))
    }

    /// Is a range of text selected (not just a caret)?
    pub fn text_range_selected(&self) -> bool {
        self.state().ok().and_then(|s| s.selection.text.as_ref().map(|t| t.is_range())).unwrap_or(false)
    }

    /// The selected text as quads (four corners each, slide points).
    pub fn text_selection_quads(&self) -> Vec<[(f64, f64); 4]> {
        let Ok(st) = self.state() else { return vec![] };
        let Some(ts) = st.selection.text.as_ref() else { return vec![] };
        if !ts.is_range() {
            return vec![];
        }
        let Some((l, m)) = deckcraft_engine::cmd::text::text_frame(st, ts) else { return vec![] };
        let (s0, s1) = ts.ordered();
        let pos = |p: (usize, usize)| deckcraft_text::Pos { para: p.0, ch: p.1 };
        l.selection_rects(pos(s0), pos(s1))
            .into_iter()
            .map(|r| {
                [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)].map(|(x, y)| {
                    let p = m * deckcraft_geom::Point::new(x, y);
                    (p.x, p.y)
                })
            })
            .collect()
    }

    /// The yellow shape-adjust handles of the selected shapes (a speech bubble's tail, a rounded
    /// corner's radius, an arrow's head size…): (shape, handle index, position in slide points).
    /// Drag one with [`DeckDoc::pointer`] from exactly that spot.
    pub fn adjust_handles(&self) -> Vec<(ShapeId, usize, (f64, f64))> {
        let Ok(st) = self.state() else { return vec![] };
        let mut out = vec![];
        for id in &st.selection.shapes {
            let Some(sh) = st.shape(*id) else { continue };
            if sh.locked || sh.is_line() {
                continue;
            }
            let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
            let geo = deckcraft_engine::render::shape_geometry(sh, x.w, x.h);
            for hd in &geo.handles {
                let p = x.affine() * hd.pos;
                out.push((*id, hd.index, (p.x, p.y)));
            }
        }
        out
    }

    /// The table cell being typed in: (table, row, column).
    pub fn current_cell(&self) -> Option<(ShapeId, usize, usize)> {
        let st = self.state().ok()?;
        let ts = st.selection.text.as_ref()?;
        ts.cell.map(|(r, c)| (ts.shape, r, c))
    }

    /// Move the caret ("left", "right", "up", "down", "lineStart", "lineEnd"…).
    pub fn move_caret(&mut self, to: &str) -> DeckResult<()> {
        self.run("text.move", json!({"to": to})).map(|_| ())
    }

    /// Delete forward (the Delete key).
    pub fn delete_forward(&mut self) -> DeckResult<()> {
        self.run("text.delete", json!({"dir": "forward"})).map(|_| ())
    }
}
