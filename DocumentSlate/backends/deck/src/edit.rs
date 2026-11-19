//! Editing helpers: one small method per edit-mode button in DeckSlate-features.md (v1).
//!
//! Each method just runs an engine command (see [`DeckDoc::run`]), so every edit is undoable
//! and crash-guarded. Positions and sizes are in points (1/72 inch) on the slide, origin at the
//! top-left corner. Colors are `"#RRGGBB"` or a theme color name (`"accent1"`…`"accent6"`,
//! `"tx1"`, `"bg1"`), which follows the theme when it changes.

use serde_json::{Value, json};

use crate::{DeckDoc, DeckResult, ShapeId};

/// A rectangle on the slide, in points: x, y (top-left corner), width, height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    fn json(self) -> Value {
        json!([self.x, self.y, self.w, self.h])
    }
}

/// Horizontal text alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
    Justify,
}

/// Vertical text alignment inside its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// Slide sizes for the Design tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideSize {
    /// 16:9 (960 x 540 pt).
    Widescreen,
    /// 4:3 (720 x 540 pt).
    Standard,
}

/// Header & footer switches (Insert > slide number / date / footer).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeaderFooter {
    pub slide_number: bool,
    pub date: bool,
    /// Fixed date text; `None` = today's date, updated automatically.
    pub date_text: Option<String>,
    /// Footer text; `None` = no footer.
    pub footer: Option<String>,
    /// Don't show them on title slides.
    pub hide_on_title: bool,
}

/// One entry of the shape gallery.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapePreset {
    /// Id to pass to [`DeckDoc::insert_shape`] ("rect", "ellipse", "rightArrow"…).
    pub name: String,
    pub label: String,
    pub category: String,
}

/// Read a shape id from a command's answer (`{"id": 12}`).
fn id_from(v: &Value) -> DeckResult<ShapeId> {
    v.get("id")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .map(ShapeId)
        .ok_or_else(|| "the engine didn't say which object it made".to_string())
}

fn ids_json(ids: &[ShapeId]) -> Value {
    json!(ids.iter().map(|i| i.0).collect::<Vec<_>>())
}

/// `{"on": x}` when `on` is given, else `{}` (= toggle).
fn on_json(on: Option<bool>) -> Value {
    match on {
        Some(b) => json!({"on": b}),
        None => json!({}),
    }
}

impl DeckDoc {
    fn ok(&mut self, cmd: &str, p: Value) -> DeckResult<()> {
        self.run(cmd, p).map(|_| ())
    }

    // ===== Slide tab =========================================================================

    /// Layout names available for new slides / Change layout (from the current slide's master).
    pub fn layout_names(&self) -> Vec<String> {
        let Ok(st) = self.state() else { return vec![] };
        let master = st.current_slide().and_then(|s| st.doc.layout(s.layout)).map(|(m, _)| m.id);
        st.doc
            .masters
            .iter()
            .find(|m| Some(m.id) == master)
            .or_else(|| st.doc.masters.first())
            .map(|m| m.layouts.iter().map(|l| l.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Add a slide after the current one. `layout` is a layout name (see [`DeckDoc::layout_names`])
    /// or kind ("title", "titleAndContent", "sectionHeader", "twoContent", "comparison",
    /// "titleOnly", "blank"…); `None` = the same as the current slide. Returns its index.
    pub fn new_slide(&mut self, layout: Option<&str>) -> DeckResult<usize> {
        let p = match layout {
            Some(l) => json!({"layout": l}),
            None => json!({}),
        };
        let r = self.run("slide.new", p)?;
        Ok(r.get("index").and_then(Value::as_u64).map(|n| n as usize).unwrap_or_else(|| self.current_slide()))
    }

    /// Copy slide `index` (the copy comes right after it).
    pub fn duplicate_slide(&mut self, index: usize) -> DeckResult<()> {
        self.ok("slide.duplicate", json!({"index": index}))
    }

    pub fn delete_slide(&mut self, index: usize) -> DeckResult<()> {
        self.ok("slide.delete", json!({"index": index}))
    }

    pub fn set_slide_hidden(&mut self, index: usize, hidden: bool) -> DeckResult<()> {
        self.ok("slide.hide", json!({"index": index, "hidden": hidden}))
    }

    /// Move slide `from` so it ends up at position `to` (both 0-based, as in the final order).
    pub fn move_slide(&mut self, from: usize, to: usize) -> DeckResult<()> {
        self.ok("slide.move", json!({"from": from, "to": to}))
    }

    /// Change slide `index` to another layout (name or kind, as for [`DeckDoc::new_slide`]).
    pub fn set_slide_layout(&mut self, index: usize, layout: &str) -> DeckResult<()> {
        self.ok("slide.layout", json!({"index": index, "layout": layout}))
    }

    /// Put the placeholders of slide `index` back where its layout has them.
    pub fn reset_slide(&mut self, index: usize) -> DeckResult<()> {
        self.ok("slide.reset", json!({"index": index}))
    }

    /// Replace the speaker notes of slide `index` (plain text, `\n` between paragraphs).
    pub fn set_notes(&mut self, index: usize, text: &str) -> DeckResult<()> {
        self.ok("slide.notes", json!({"index": index, "text": text}))
    }

    // ===== Text tab ==========================================================================
    // Formatting applies to the selected text while editing text, or to all the text of the
    // selected objects otherwise.

    /// Start typing in an object (text box, shape, placeholder). `at` = (paragraph, character)
    /// for the caret; `None` puts it at the end.
    pub fn edit_text(&mut self, id: ShapeId, at: Option<(usize, usize)>) -> DeckResult<()> {
        let p = match at {
            Some((para, ch)) => json!({"id": id.0, "at": [para, ch]}),
            None => json!({"id": id.0, "end": true}),
        };
        self.ok("text.edit", p)
    }

    /// Type in a table cell.
    pub fn edit_cell(&mut self, table: ShapeId, row: usize, col: usize) -> DeckResult<()> {
        self.ok("text.edit", json!({"id": table.0, "cell": [row, col]}))
    }

    /// Type at the caret (replacing selected text). `\n` = new paragraph.
    pub fn type_text(&mut self, text: &str) -> DeckResult<()> {
        self.ok("text.insert", json!({"text": text}))
    }

    /// Backspace (or the selected text).
    pub fn backspace(&mut self) -> DeckResult<()> {
        self.ok("text.delete", json!({"dir": "backward"}))
    }

    pub fn select_all_text(&mut self) -> DeckResult<()> {
        self.ok("text.selectAll", json!({}))
    }

    /// Stop typing (the object stays selected).
    pub fn exit_text(&mut self) -> DeckResult<()> {
        self.ok("text.exit", json!({}))
    }

    /// Replace all the text of an object (keeps the first run's formatting).
    pub fn set_text(&mut self, id: ShapeId, text: &str) -> DeckResult<()> {
        self.ok("text.set", json!({"id": id.0, "text": text}))
    }

    /// The text of an object.
    pub fn text_of(&mut self, id: ShapeId) -> DeckResult<String> {
        let r = self.run("text.get", json!({"id": id.0}))?;
        Ok(r.get("text").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// `None` toggles, `Some(true/false)` sets.
    pub fn bold(&mut self, on: Option<bool>) -> DeckResult<()> {
        self.ok("format.bold", on_json(on))
    }
    pub fn italic(&mut self, on: Option<bool>) -> DeckResult<()> {
        self.ok("format.italic", on_json(on))
    }
    pub fn underline(&mut self, on: Option<bool>) -> DeckResult<()> {
        self.ok("format.underline", on_json(on))
    }
    pub fn font(&mut self, family: &str) -> DeckResult<()> {
        self.ok("format.font", json!({"family": family}))
    }
    /// Font size in points.
    pub fn font_size(&mut self, pt: f64) -> DeckResult<()> {
        self.ok("format.size", json!({"size": pt}))
    }
    pub fn font_color(&mut self, color: &str) -> DeckResult<()> {
        self.ok("format.color", json!({"color": color}))
    }
    pub fn align(&mut self, a: HAlign) -> DeckResult<()> {
        let a = match a {
            HAlign::Left => "left",
            HAlign::Center => "center",
            HAlign::Right => "right",
            HAlign::Justify => "justify",
        };
        self.ok("format.align", json!({"align": a}))
    }
    pub fn vertical_align(&mut self, a: VAlign) -> DeckResult<()> {
        let a = match a {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        };
        self.ok("format.anchor", json!({"anchor": a}))
    }
    pub fn bullets(&mut self, on: Option<bool>) -> DeckResult<()> {
        self.ok("format.bullets", on_json(on))
    }
    pub fn numbering(&mut self, on: Option<bool>) -> DeckResult<()> {
        self.ok("format.numbering", on_json(on))
    }
    /// List level down (more indented).
    pub fn list_level_down(&mut self) -> DeckResult<()> {
        self.ok("format.indent", json!({}))
    }
    /// List level up (less indented).
    pub fn list_level_up(&mut self) -> DeckResult<()> {
        self.ok("format.outdent", json!({}))
    }
    /// Line spacing in lines (1.0, 1.5, 2.0…).
    pub fn line_spacing(&mut self, lines: f64) -> DeckResult<()> {
        self.ok("format.lineSpacing", json!({"lines": lines}))
    }
    pub fn clear_formatting(&mut self) -> DeckResult<()> {
        self.ok("format.clear", json!({}))
    }
    /// The formatting at the selection, for lighting up toolbar buttons. JSON with keys such as
    /// `font`, `size`, `bold`, `italic`, `underline`, `align`, `bullets`… (engine's `format.state`).
    pub fn format_state(&mut self) -> DeckResult<Value> {
        self.run("format.state", json!({}))
    }

    // ===== Insert tab ========================================================================

    /// A text box. `rect: None` = the engine's default place.
    pub fn insert_text_box(&mut self, rect: Option<Rect>, text: &str) -> DeckResult<ShapeId> {
        let mut p = json!({"text": text});
        if let Some(r) = rect {
            p["rect"] = r.json();
        }
        id_from(&self.run("insert.textBox", p)?)
    }

    /// A picture from its file bytes (PNG, JPEG, GIF, WebP, BMP, TIFF). `name` is the file name.
    /// `rect: None` = sized from the picture and centred (or into an empty picture placeholder).
    pub fn insert_picture(&mut self, bytes: &[u8], name: &str, rect: Option<Rect>) -> DeckResult<ShapeId> {
        let mut p = json!({"data": deckcraft_engine::cmd::base64_encode(bytes), "name": name});
        if let Some(r) = rect {
            p["rect"] = r.json();
        }
        id_from(&self.run("insert.picture", p)?)
    }

    /// The shapes the engine can draw (for the shape picker). The app should show a basic subset.
    pub fn shape_presets(&mut self) -> Vec<ShapePreset> {
        let Ok(r) = self.run("shape.presets", json!({})) else { return vec![] };
        let list = r.get("presets").or(Some(&r)).and_then(Value::as_array).cloned().unwrap_or_default();
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        list.iter().map(|v| ShapePreset { name: s(v, "name"), label: s(v, "label"), category: s(v, "category") }).collect()
    }

    /// A preset shape ("rect", "roundRect", "ellipse", "triangle", "rightArrow", "star5", "line"…).
    pub fn insert_shape(&mut self, preset: &str, rect: Rect, text: Option<&str>) -> DeckResult<ShapeId> {
        let mut p = json!({"preset": preset, "rect": rect.json()});
        if let Some(t) = text {
            p["text"] = json!(t);
        }
        id_from(&self.run("shape.insert", p)?)
    }

    pub fn insert_table(&mut self, rows: usize, cols: usize, rect: Option<Rect>) -> DeckResult<ShapeId> {
        let mut p = json!({"rows": rows, "cols": cols});
        if let Some(r) = rect {
            p["rect"] = r.json();
        }
        id_from(&self.run("insert.table", p)?)
    }

    /// A web link on the selected text/object. With nothing selected and a `text`, a new text box
    /// with that text is added and linked (the engine itself needs a selection).
    pub fn insert_link(&mut self, url: &str, text: Option<&str>) -> DeckResult<()> {
        self.link(json!({"url": url}), text)
    }

    /// A link to another slide (0-based index), like [`DeckDoc::insert_link`].
    pub fn insert_slide_link(&mut self, slide: usize, text: Option<&str>) -> DeckResult<()> {
        self.link(json!({"slide": slide}), text)
    }

    fn link(&mut self, p: Value, text: Option<&str>) -> DeckResult<()> {
        if let Some(t) = text
            && self.selected().is_empty()
            && self.editing_text().is_none()
        {
            let id = self.insert_text_box(None, t)?;
            self.select(&[id])?;
        }
        self.ok("insert.hyperlink", p)
    }

    /// Slide number / date / footer on all slides.
    pub fn set_header_footer(&mut self, hf: &HeaderFooter) -> DeckResult<()> {
        let mut p = json!({
            "slideNumber": hf.slide_number,
            "date": hf.date,
            "footer": hf.footer.is_some(),
            "hideOnTitle": hf.hide_on_title,
            "all": true,
        });
        if let Some(t) = &hf.footer {
            p["footerText"] = json!(t);
        }
        if let Some(d) = &hf.date_text {
            p["dateText"] = json!(d);
        }
        self.ok("design.headerFooter", p)
    }

    /// A comment on the current slide (on the selected object when one is selected).
    pub fn add_comment(&mut self, text: &str) -> DeckResult<()> {
        self.ok("comment.add", json!({"text": text}))
    }

    // ===== Design tab ========================================================================

    /// Names of the engine's built-in themes (its own designs, not Microsoft's).
    pub fn theme_names() -> Vec<String> {
        deckcraft_model::theme::builtin_themes().into_iter().map(|t| t.name).collect()
    }

    /// Apply a built-in theme to the whole presentation.
    pub fn apply_theme(&mut self, name: &str) -> DeckResult<()> {
        self.ok("design.theme", json!({"name": name}))
    }

    /// The themes, colour sets, font pairs and effect sets the engine offers, as JSON (from the
    /// engine's `design.themes`), for the Design tab galleries.
    pub fn design_galleries(&mut self) -> DeckResult<Value> {
        self.run("design.themes", json!({}))
    }

    /// Switch to a named colour set (see [`DeckDoc::design_galleries`]).
    pub fn theme_colors(&mut self, name: &str) -> DeckResult<()> {
        self.ok("design.colors", json!({"name": name}))
    }

    /// Switch to a named font pair (see [`DeckDoc::design_galleries`]).
    pub fn theme_fonts(&mut self, name: &str) -> DeckResult<()> {
        self.ok("design.fonts", json!({"name": name}))
    }

    /// Set heading (`major`) and body (`minor`) theme fonts directly.
    pub fn theme_font_pair(&mut self, major: &str, minor: &str) -> DeckResult<()> {
        self.ok("design.fonts", json!({"major": major, "minor": minor}))
    }

    /// Plain background colour, for the current slide or (`all`) every slide.
    pub fn background_color(&mut self, color: &str, all: bool) -> DeckResult<()> {
        self.ok("design.background", json!({"color": color, "all": all}))
    }

    /// Picture background from image file bytes.
    pub fn background_picture(&mut self, bytes: &[u8], all: bool) -> DeckResult<()> {
        self.ok("design.background", json!({"picture": deckcraft_engine::cmd::base64_encode(bytes), "all": all}))
    }

    /// Back to the layout's/theme's background.
    pub fn reset_background(&mut self, all: bool) -> DeckResult<()> {
        self.ok("design.background", json!({"reset": true, "all": all}))
    }

    /// 16:9 or 4:3. Content is scaled to fit.
    pub fn set_slide_size(&mut self, size: SlideSize) -> DeckResult<()> {
        let preset = match size {
            SlideSize::Widescreen => "widescreen",
            SlideSize::Standard => "standard",
        };
        self.ok("design.slideSize", json!({"preset": preset, "scale": "ensureFit"}))
    }

    // ===== Objects (long-press menu and contextual tab) =====================================
    // `ids` empty = the current selection.

    /// Select objects on the current slide (replacing the selection).
    pub fn select(&mut self, ids: &[ShapeId]) -> DeckResult<()> {
        if ids.is_empty() {
            return self.deselect();
        }
        self.ok("edit.select", json!({"ids": ids_json(ids)}))
    }

    pub fn deselect(&mut self) -> DeckResult<()> {
        self.ok("edit.deselect", json!({}))
    }

    /// The selected objects' ids.
    pub fn selected(&self) -> Vec<ShapeId> {
        self.state().map(|s| s.selection.shapes.clone()).unwrap_or_default()
    }

    /// Is text being edited (keyboard should be up)? Gives the object id.
    pub fn editing_text(&self) -> Option<ShapeId> {
        self.state().ok().and_then(|s| s.selection.text.as_ref().filter(|t| !t.notes).map(|t| t.shape))
    }

    fn with_ids(ids: &[ShapeId], mut p: Value) -> Value {
        if !ids.is_empty() {
            p["ids"] = ids_json(ids);
        }
        p
    }

    pub fn delete_objects(&mut self, ids: &[ShapeId]) -> DeckResult<()> {
        self.ok("edit.delete", Self::with_ids(ids, json!({})))
    }
    pub fn duplicate_objects(&mut self, ids: &[ShapeId]) -> DeckResult<()> {
        self.ok("edit.duplicate", Self::with_ids(ids, json!({})))
    }
    /// Cut the selection to the engine's own clipboard. Returns its plain text (for Android's
    /// clipboard).
    pub fn cut(&mut self) -> DeckResult<String> {
        let r = self.run("edit.cut", json!({}))?;
        Ok(r.get("text").and_then(Value::as_str).unwrap_or_default().to_string())
    }
    /// Copy the selection (objects or text). Returns its plain text.
    pub fn copy(&mut self) -> DeckResult<String> {
        let r = self.run("edit.copy", json!({}))?;
        Ok(r.get("text").and_then(Value::as_str).unwrap_or_default().to_string())
    }
    /// Paste what was copied inside the app.
    pub fn paste(&mut self) -> DeckResult<()> {
        self.ok("edit.paste", json!({}))
    }
    /// Paste plain text from Android's clipboard.
    pub fn paste_text(&mut self, text: &str) -> DeckResult<()> {
        self.ok("edit.paste", json!({"text": text}))
    }
    pub fn bring_to_front(&mut self, ids: &[ShapeId]) -> DeckResult<()> {
        self.ok("arrange.bringToFront", Self::with_ids(ids, json!({})))
    }
    pub fn send_to_back(&mut self, ids: &[ShapeId]) -> DeckResult<()> {
        self.ok("arrange.sendToBack", Self::with_ids(ids, json!({})))
    }
    /// Move by dx, dy points.
    pub fn move_by(&mut self, ids: &[ShapeId], dx: f64, dy: f64) -> DeckResult<()> {
        self.ok("shape.move", Self::with_ids(ids, json!({"dx": dx, "dy": dy})))
    }
    /// Position and size in one go.
    pub fn set_bounds(&mut self, ids: &[ShapeId], r: Rect) -> DeckResult<()> {
        self.ok("shape.setBounds", Self::with_ids(ids, json!({"x": r.x, "y": r.y, "w": r.w, "h": r.h})))
    }
    /// Rotation in degrees (clockwise).
    pub fn rotate_to(&mut self, ids: &[ShapeId], degrees: f64) -> DeckResult<()> {
        self.ok("shape.rotate", Self::with_ids(ids, json!({"deg": degrees})))
    }
    /// Fill colour; `None` = no fill.
    pub fn fill_color(&mut self, ids: &[ShapeId], color: Option<&str>) -> DeckResult<()> {
        let p = match color {
            Some(c) => json!({"color": c}),
            None => json!({"none": true}),
        };
        self.ok("shape.fill", Self::with_ids(ids, p))
    }
    /// Outline colour and width (points); `color: None` = no outline.
    pub fn outline(&mut self, ids: &[ShapeId], color: Option<&str>, width: Option<f64>) -> DeckResult<()> {
        let mut p = match color {
            Some(c) => json!({"color": c}),
            None => json!({"none": true}),
        };
        if let Some(w) = width {
            p["width"] = json!(w);
        }
        self.ok("shape.line", Self::with_ids(ids, p))
    }
    /// Crop a picture: fractions 0..1 cut from each side.
    pub fn crop(&mut self, ids: &[ShapeId], left: f64, top: f64, right: f64, bottom: f64) -> DeckResult<()> {
        self.ok("picture.crop", Self::with_ids(ids, json!({"left": left, "top": top, "right": right, "bottom": bottom})))
    }

    /// Align objects: "left", "center", "right", "top", "middle" or "bottom". One object aligns to
    /// the slide; several to each other.
    pub fn align_objects(&mut self, ids: &[ShapeId], edge: &str) -> DeckResult<()> {
        self.ok("arrange.align", Self::with_ids(ids, json!({"edge": edge})))
    }
    /// Spread three or more objects evenly: `horizontal` or vertical.
    pub fn distribute_objects(&mut self, ids: &[ShapeId], horizontal: bool) -> DeckResult<()> {
        self.ok("arrange.distribute", Self::with_ids(ids, json!({"dir": if horizontal { "horizontal" } else { "vertical" }})))
    }

    // ----- tables ----------------------------------------------------------------------------

    /// Add a row below `row` (or above when `above`).
    pub fn table_insert_row(&mut self, table: ShapeId, row: usize, above: bool) -> DeckResult<()> {
        let cmd = if above { "table.insertRowAbove" } else { "table.insertRowBelow" };
        self.ok(cmd, json!({"id": table.0, "row": row}))
    }
    /// Add a column right of `col` (or left when `left`).
    pub fn table_insert_column(&mut self, table: ShapeId, col: usize, left: bool) -> DeckResult<()> {
        let cmd = if left { "table.insertColumnLeft" } else { "table.insertColumnRight" };
        self.ok(cmd, json!({"id": table.0, "col": col}))
    }
    pub fn table_delete_row(&mut self, table: ShapeId, row: usize) -> DeckResult<()> {
        self.ok("table.deleteRow", json!({"id": table.0, "row": row}))
    }
    pub fn table_delete_column(&mut self, table: ShapeId, col: usize) -> DeckResult<()> {
        self.ok("table.deleteColumn", json!({"id": table.0, "col": col}))
    }
    /// Merge the cells from (r0, c0) to (r1, c1).
    pub fn table_merge(&mut self, table: ShapeId, from: (usize, usize), to: (usize, usize)) -> DeckResult<()> {
        self.ok("table.merge", json!({"id": table.0, "from": [from.0, from.1], "to": [to.0, to.1]}))
    }
    /// Rows and columns of a table.
    pub fn table_size(&self, table: ShapeId) -> Option<(usize, usize)> {
        let st = self.state().ok()?;
        match &st.shape(table)?.kind {
            deckcraft_model::ShapeKind::Table(t) => Some((t.rows.len(), t.cols.len())),
            _ => None,
        }
    }
}
