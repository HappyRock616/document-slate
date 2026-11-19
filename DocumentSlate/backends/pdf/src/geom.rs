//! Coordinate conversions between the app's "page view space" (points, top-left origin, y down,
//! as the page is shown) and the PDF's own spaces.
//!
//! * **User space**: the PDF's native coordinates (origin usually bottom-left, y up, before the
//!   page's `/Rotate`). Annotations, links and form fields live here.
//! * **Display space**: like view space but y up from the bottom-left of the page as shown.
//!   Added text and pictures (PdfCraft's "Add content") live here.

use pdfcraft_render::PageInfo as EnginePage;

use crate::{PdfDoc, PdfError, ViewPoint, ViewRect};

impl PdfDoc {
    /// The engine's page description, or an error for a page number past the end.
    pub(crate) fn page_ref(&self, page: usize) -> Result<EnginePage, PdfError> {
        let doc = self.doc()?;
        doc.info.pages.get(page).cloned().ok_or_else(|| PdfError::Invalid(format!("there is no page {} (the document has {})", page + 1, doc.info.pages.len())))
    }

    pub(crate) fn check_pages(&self, pages: &[usize]) -> Result<(), PdfError> {
        if pages.is_empty() {
            return Err(PdfError::Invalid("no pages were chosen".into()));
        }
        for &p in pages {
            self.page_ref(p)?;
        }
        Ok(())
    }
}

pub(crate) fn finite(values: &[f32]) -> Result<(), PdfError> {
    if values.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(PdfError::Invalid("a position is not a number".into())) }
}

/// View point → user space point.
pub(crate) fn to_user(page: &EnginePage, p: ViewPoint) -> [f64; 2] {
    let u = page.view_to_user(p[0], p[1]);
    [f64::from(u[0]), f64::from(u[1])]
}

/// View rectangle → normalised user-space rectangle.
pub(crate) fn rect_to_user(page: &EnginePage, r: ViewRect) -> [f64; 4] {
    page.view_rect_to_user(norm(r)).map(f64::from)
}

/// User-space rectangle → normalised view rectangle.
pub(crate) fn rect_to_view(page: &EnginePage, r: [f32; 4]) -> ViewRect {
    let a = page.user_to_view(r[0], r[1]);
    let b = page.user_to_view(r[2], r[3]);
    norm([a[0], a[1], b[0], b[1]])
}

pub(crate) fn rect64_to_view(page: &EnginePage, r: [f64; 4]) -> ViewRect {
    rect_to_view(page, r.map(|v| v as f32))
}

/// View rectangle → display-space rectangle `[x0, y0, x1, y1]`, y up (y1 = top).
pub(crate) fn rect_to_display(page: &EnginePage, r: ViewRect) -> [f64; 4] {
    let r = norm(r);
    let h = page.height;
    [r[0], h - r[3], r[2], h - r[1]].map(f64::from)
}

pub(crate) fn display_to_view(page: &EnginePage, r: [f64; 4]) -> ViewRect {
    let h = f64::from(page.height);
    norm([r[0], h - r[3], r[2], h - r[1]].map(|v| v as f32))
}

/// Left < right, top < bottom.
pub(crate) fn norm(r: [f32; 4]) -> [f32; 4] {
    [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])]
}
