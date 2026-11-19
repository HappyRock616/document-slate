//! Touch support: what is under a finger, where the selection handles are, and drag gestures.
//!
//! All positions are slide points (the app converts screen pixels: `(px - slide_left) / scale`).
//! `tol` is how close (in points) counts as a hit; for a finger use about 24 dp worth, i.e.
//! `24 * density / scale`.
//!
//! Two ways to move/resize things:
//! 1. Simple: hit-test, then call `move_by` / `set_bounds` / `rotate_to` (in `edit`) when the
//!    finger lifts. One undo step per call.
//! 2. Live: feed the finger to the engine with [`DeckDoc::pointer`] (down, drag…, up). The engine
//!    then does exactly what a mouse does on desktop (move, resize by handle, rotate, snapping,
//!    smart guides) and the whole gesture is one undo step.

use deckcraft_engine::tools::{Hit, Mods, PointerEvent};
pub use deckcraft_engine::tools::PointerKind;
use deckcraft_geom::Point;
use deckcraft_model::ShapeKind;
use serde_json::Value;

use crate::{DeckDoc, DeckResult, ShapeId};

/// What kind of object a shape is (decides which contextual tools to show).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// A shape or text box (may hold text).
    Shape,
    Picture,
    Table,
    Chart,
    Group,
    Connector,
    Media,
    Ink,
    Model3d,
    /// Something kept from the file but not editable (OLE object, unknown content).
    Other,
}

/// One object on the slide.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectInfo {
    pub id: ShapeId,
    pub name: String,
    pub kind: ObjectKind,
    /// Box before rotation, in points: x, y, w, h.
    pub rect: (f64, f64, f64, f64),
    /// Clockwise degrees.
    pub rotation: f64,
    /// A layout placeholder (title, body…).
    pub placeholder: bool,
    pub has_text: bool,
    pub locked: bool,
    pub hidden: bool,
}

/// What a touch landed on.
#[derive(Clone, Debug, PartialEq)]
pub enum TouchHit {
    /// The body of an object (`text`: on its text, so a tap could start typing).
    Object { id: ShapeId, text: bool },
    /// Resize handle 0..8 of a selected object: 0 NW, 1 N, 2 NE, 3 E, 4 SE, 5 S, 6 SW, 7 W.
    Resize { id: ShapeId, handle: u8 },
    /// The rotate handle of a selected object.
    Rotate { id: ShapeId },
    /// A yellow shape-adjust handle (e.g. corner roundness).
    Adjust { id: ShapeId, index: usize },
    /// One end of a selected line.
    LineEnd { id: ShapeId, end: u8 },
    /// A table's column or row border (drag to resize).
    TableBorder { id: ShapeId },
    /// Anything else the engine distinguishes (point editing, WordArt handles, text frame).
    Other { id: ShapeId },
}

/// Handles of a selected object, for drawing them.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionHandles {
    pub id: ShapeId,
    /// The 8 resize handles (NW, N, NE, E, SE, S, SW, W) in slide points, rotation applied.
    pub resize: [(f64, f64); 8],
    /// The rotate handle.
    pub rotate: (f64, f64),
    /// A line has only its two ends (then `resize` and `rotate` should be ignored).
    pub line_ends: Option<[(f64, f64); 2]>,
}

fn kind_of(k: &ShapeKind) -> ObjectKind {
    match k {
        ShapeKind::Shape => ObjectKind::Shape,
        ShapeKind::Picture { .. } => ObjectKind::Picture,
        ShapeKind::Group { .. } => ObjectKind::Group,
        ShapeKind::Connector { .. } => ObjectKind::Connector,
        ShapeKind::Table(_) => ObjectKind::Table,
        ShapeKind::Chart(_) => ObjectKind::Chart,
        ShapeKind::Media(_) => ObjectKind::Media,
        ShapeKind::Ink { .. } => ObjectKind::Ink,
        ShapeKind::Model3d { .. } => ObjectKind::Model3d,
        ShapeKind::Opaque { .. } => ObjectKind::Other,
    }
}

impl DeckDoc {
    /// The top-level objects of the current slide, back to front.
    pub fn objects(&self) -> Vec<ObjectInfo> {
        let Ok(st) = self.state() else { return vec![] };
        st.shapes()
            .iter()
            .map(|sh| {
                let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
                ObjectInfo {
                    id: sh.id,
                    name: sh.name.clone(),
                    kind: kind_of(&sh.kind),
                    rect: (x.x, x.y, x.w, x.h),
                    rotation: x.rot,
                    placeholder: sh.ph.is_some(),
                    has_text: sh.text.as_ref().is_some_and(|t| !t.text().is_empty()),
                    locked: sh.locked,
                    hidden: sh.hidden,
                }
            })
            .collect()
    }

    /// What is at (x, y) on the current slide: handles of selected objects first, then objects
    /// from the top down. `None` = empty slide area.
    pub fn hit_test(&self, x: f64, y: f64, tol: f64) -> Option<TouchHit> {
        let hit = self.session.hit_test(Point::new(x, y), tol)?;
        Some(match hit {
            Hit::Shape { id, text } => TouchHit::Object { id, text },
            Hit::Resize { id, handle } => TouchHit::Resize { id, handle },
            Hit::Rotate { id } => TouchHit::Rotate { id },
            Hit::Adjust { id, index } => TouchHit::Adjust { id, index },
            Hit::LineEnd { id, end } => TouchHit::LineEnd { id, end },
            Hit::TableColumn { id, .. } | Hit::TableRow { id, .. } => TouchHit::TableBorder { id },
            Hit::Handle { id, .. } | Hit::Vertex { id, .. } | Hit::Warp { id, .. } | Hit::Frame { id } => TouchHit::Other { id },
        })
    }

    /// Handles to draw around the selected objects.
    pub fn selection_handles(&self) -> Vec<SelectionHandles> {
        let Ok(st) = self.state() else { return vec![] };
        st.selection
            .shapes
            .iter()
            .filter_map(|id| st.shape(*id))
            .map(|sh| {
                let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
                let (pts, rot) = deckcraft_engine::tools::box_handles(&x);
                let line_ends = sh.is_line().then(|| {
                    let a = x.affine();
                    let (p0, p1) = (a * Point::new(0.0, 0.0), a * Point::new(x.w, x.h));
                    [(p0.x, p0.y), (p1.x, p1.y)]
                });
                SelectionHandles { id: sh.id, resize: pts.map(|p| (p.x, p.y)), rotate: (rot.x, rot.y), line_ends }
            })
            .collect()
    }

    /// Feed one finger event to the engine's pointer tool (select/move/resize/rotate, and a
    /// double tap starts typing). Send `Down`, then `Drag` for each move, then `Up`.
    /// `DoubleClick` for a double tap. Returns the engine's description of what happened.
    pub fn pointer(&mut self, kind: PointerKind, x: f64, y: f64, tol: f64) -> DeckResult<Value> {
        let ev = PointerEvent { kind, x, y, mods: Mods::default(), tol, pressure: None };
        self.session.pointer(ev).map_err(|e| e.to_string())
    }
}
