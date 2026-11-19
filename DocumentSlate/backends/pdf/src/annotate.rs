//! Annotate tab: highlight / underline / strikethrough, sticky notes, text boxes, freehand pen,
//! shapes, and the comment list.

use pdfcraft_engine::{Edit, LineEnding, Markup, NewAnnotation, NoteIcon, Shape, Style};

use crate::geom::{finite, rect_to_user, rect_to_view, to_user};
use crate::{CommentInfo, Look, MarkupKind, PdfDoc, PdfError, ShapeKind, ViewPoint, ViewRect};

/// Most points we accept in one pen stroke / strokes in one drawing (keeps files sane).
const MAX_STROKE_POINTS: usize = 20_000;
const MAX_STROKES: usize = 500;

impl PdfDoc {
    /// Add a new comment with the engine's default look for that kind, adjusted by `look`.
    pub(crate) fn add_annotation(&mut self, page: usize, shape: Shape, look: Look, text: &str) -> Result<(), PdfError> {
        let mut style = Style::default_for(&shape);
        if let Some(c) = look.color {
            style.color = clamp_rgb(c);
        }
        if let Some(w) = look.width.filter(|w| w.is_finite() && *w >= 0.0) {
            style.width = w.min(100.0);
        }
        if let Some(o) = look.opacity.filter(|o| o.is_finite()) {
            style.opacity = o.clamp(0.0, 1.0);
        }
        if let Some(f) = look.fill {
            style.fill = Some(clamp_rgb(f));
        }
        let author = self.author.clone();
        self.apply(Edit::AddAnnotation(NewAnnotation { page, shape, style, contents: text.to_string(), author }))
    }

    /// Highlight, underline, strike out or squiggle text. `rects` are line rectangles in view
    /// space — e.g. [`crate::SearchHit::rects`], or a text selection. `note` is an optional
    /// comment attached to the mark.
    pub fn add_text_markup(&mut self, page: usize, kind: MarkupKind, rects: &[ViewRect], look: Look, note: &str) -> Result<(), PdfError> {
        if rects.is_empty() {
            return Err(PdfError::Invalid("select some text first".into()));
        }
        let info = self.page_ref(page)?;
        for r in rects {
            finite(r)?;
        }
        let quads: Vec<[f64; 8]> = rects.iter().map(|r| info.view_rect_to_quad(crate::geom::norm(*r))).collect();
        let kind = match kind {
            MarkupKind::Highlight => Markup::Highlight,
            MarkupKind::Underline => Markup::Underline,
            MarkupKind::Strikeout => Markup::StrikeOut,
            MarkupKind::Squiggly => Markup::Squiggly,
        };
        self.add_annotation(page, Shape::TextMarkup { kind, quads }, look, note)
    }

    /// A sticky note whose icon's top-left corner is at `at`.
    pub fn add_note(&mut self, page: usize, at: ViewPoint, text: &str) -> Result<(), PdfError> {
        finite(&at)?;
        let info = self.page_ref(page)?;
        self.add_annotation(page, Shape::Note { at: to_user(&info, at), icon: NoteIcon::Comment }, Look::default(), text)
    }

    /// A text box (typed comment shown on the page) in `rect`.
    pub fn add_text_box(&mut self, page: usize, rect: ViewRect, text: &str, font_size: f64, look: Look) -> Result<(), PdfError> {
        finite(&rect)?;
        let info = self.page_ref(page)?;
        let size = if font_size.is_finite() { font_size.clamp(4.0, 300.0) } else { 12.0 };
        self.add_annotation(page, Shape::TextBox { rect: rect_to_user(&info, rect), font_size: size }, look, text)
    }

    /// Freehand pen drawing: one or more strokes, each a list of points.
    pub fn add_ink(&mut self, page: usize, strokes: &[Vec<ViewPoint>], look: Look) -> Result<(), PdfError> {
        let info = self.page_ref(page)?;
        let strokes = strokes_to_user(&info, strokes)?;
        self.add_annotation(page, Shape::Ink { strokes }, look, "")
    }

    /// A rectangle, circle (oval), line or arrow from `from` to `to` (the drag's start and end;
    /// for rectangles and circles they're opposite corners).
    pub fn add_shape(&mut self, page: usize, kind: ShapeKind, from: ViewPoint, to: ViewPoint, look: Look) -> Result<(), PdfError> {
        finite(&[from[0], from[1], to[0], to[1]])?;
        let info = self.page_ref(page)?;
        let rect = rect_to_user(&info, [from[0], from[1], to[0], to[1]]);
        let shape = match kind {
            ShapeKind::Rectangle => Shape::Rectangle { rect },
            ShapeKind::Circle => Shape::Oval { rect },
            ShapeKind::Line => Shape::Line { from: to_user(&info, from), to: to_user(&info, to), start: LineEnding::None, end: LineEnding::None },
            ShapeKind::Arrow => Shape::Line { from: to_user(&info, from), to: to_user(&info, to), start: LineEnding::None, end: LineEnding::OpenArrow },
        };
        self.add_annotation(page, shape, look, "")
    }

    /// Every comment / mark in the document, page by page (links and form fields aren't
    /// comments and aren't listed).
    pub fn comments(&self) -> Vec<CommentInfo> {
        let Ok(doc) = self.doc() else { return Vec::new() };
        doc.info
            .annotations
            .iter()
            .filter(|a| a.subtype != "Popup")
            .filter_map(|a| {
                let info = doc.info.pages.get(a.page)?;
                Some(CommentInfo {
                    page: a.page,
                    index: a.index,
                    kind: a.subtype.clone(),
                    author: a.author.clone().unwrap_or_default(),
                    text: a.contents.clone().unwrap_or_default(),
                    modified: a.modified.clone(),
                    rect: rect_to_view(info, a.rect),
                    color: a.color,
                    is_reply: a.parent.is_some() && !a.grouped,
                    fill_sign: a.fill_sign,
                    locked: a.locked,
                })
            })
            .collect()
    }

    /// Delete a comment (with its pop-up and replies).
    pub fn delete_comment(&mut self, page: usize, index: usize) -> Result<(), PdfError> {
        self.apply(Edit::DeleteAnnotation { page, index })
    }

    /// Change a comment's text.
    pub fn set_comment_text(&mut self, page: usize, index: usize, text: &str) -> Result<(), PdfError> {
        self.apply(Edit::SetAnnotationContents { page, index, text: text.to_string() })
    }

    /// Reply to a comment.
    pub fn reply_to_comment(&mut self, page: usize, index: usize, text: &str) -> Result<(), PdfError> {
        let author = self.author.clone();
        self.apply(Edit::ReplyToAnnotation { page, index, text: text.to_string(), author })
    }

    /// Move a comment by (dx, dy) points in view space (y down).
    pub fn move_comment(&mut self, page: usize, index: usize, dx: f32, dy: f32) -> Result<(), PdfError> {
        finite(&[dx, dy])?;
        let info = self.page_ref(page)?;
        let a = to_user(&info, [0.0, 0.0]);
        let b = to_user(&info, [dx, dy]);
        self.apply(Edit::MoveAnnotation { page, index, dx: b[0] - a[0], dy: b[1] - a[1] })
    }
}

pub(crate) fn strokes_to_user(info: &pdfcraft_render::PageInfo, strokes: &[Vec<ViewPoint>]) -> Result<Vec<Vec<[f64; 2]>>, PdfError> {
    let strokes: Vec<&Vec<ViewPoint>> = strokes.iter().filter(|s| !s.is_empty()).collect();
    if strokes.is_empty() {
        return Err(PdfError::Invalid("nothing was drawn".into()));
    }
    if strokes.len() > MAX_STROKES || strokes.iter().any(|s| s.len() > MAX_STROKE_POINTS) {
        return Err(PdfError::Invalid("the drawing is too large".into()));
    }
    let mut out = Vec::with_capacity(strokes.len());
    for s in strokes {
        let mut line = Vec::with_capacity(s.len());
        for p in s {
            finite(p)?;
            line.push(to_user(info, *p));
        }
        out.push(line);
    }
    Ok(out)
}

fn clamp_rgb(c: [f64; 3]) -> [f64; 3] {
    c.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 })
}
