//! Pages tab: reorder, rotate, delete, insert blank or from another PDF, extract, crop.
//! (Merging several files is [`PdfDoc::merge`] in lib.rs.)

use std::sync::Arc;

use pdfcraft_engine::{BoxSpec, Edit, FieldConflicts, PageBox};

use crate::geom::{finite, rect_to_user};
use crate::{PdfDoc, PdfError, ViewRect};

impl PdfDoc {
    /// Turn pages clockwise by `degrees` (90, 180, 270 or -90…). Pages are 0-based.
    pub fn rotate_pages(&mut self, pages: &[usize], degrees: i64) -> Result<(), PdfError> {
        self.check_pages(pages)?;
        if degrees % 90 != 0 {
            return Err(PdfError::Invalid("pages can only be turned in steps of 90°".into()));
        }
        self.apply(Edit::RotatePages { pages: pages.to_vec(), degrees })
    }

    /// Delete pages. A document must keep at least one page.
    pub fn delete_pages(&mut self, pages: &[usize]) -> Result<(), PdfError> {
        self.check_pages(pages)?;
        let mut unique = pages.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() >= self.page_count() {
            return Err(PdfError::Invalid("a PDF needs at least one page; keep one".into()));
        }
        self.apply(Edit::DeletePages { pages: unique })
    }

    /// Move `pages` (keeping their order) so the first of them ends up at position `to`
    /// (0-based, counted in the result). Dragging thumbnail 5 to the front = `move_pages(&[4], 0)`.
    pub fn move_pages(&mut self, pages: &[usize], to: usize) -> Result<(), PdfError> {
        self.check_pages(pages)?;
        self.apply(Edit::MovePages { pages: pages.to_vec(), to })
    }

    /// Insert a blank page at position `at` (0 = before the first page, `page_count()` = at the
    /// end). With `size` `None` it copies the size of the neighbouring page.
    pub fn insert_blank_page(&mut self, at: usize, size: Option<(f64, f64)>) -> Result<(), PdfError> {
        let count = self.page_count();
        if at > count {
            return Err(PdfError::Invalid(format!("position {at} is past the end ({count} pages)")));
        }
        let (width, height) = match size {
            Some((w, h)) if w.is_finite() && h.is_finite() && w >= 3.0 && h >= 3.0 && w <= 14_400.0 && h <= 14_400.0 => (w, h),
            Some(_) => return Err(PdfError::Invalid("the page size must be between 3 and 14,400 points".into())),
            None => {
                let near = self.page_ref(at.min(count.saturating_sub(1)))?;
                // The displayed size: a landscape neighbour gives a landscape blank page.
                (f64::from(near.width), f64::from(near.height))
            }
        };
        self.apply(Edit::InsertBlankPage { at, width, height })
    }

    /// Insert pages from another PDF at position `at`. `pages` = which of its pages (0-based),
    /// or `None` for all. Form fields with clashing names are renamed so they stay separate.
    pub fn insert_pages_from(&mut self, name: &str, bytes: Vec<u8>, pages: Option<Vec<usize>>, at: usize) -> Result<(), PdfError> {
        if at > self.page_count() {
            return Err(PdfError::Invalid("the insert position is past the end".into()));
        }
        self.apply(Edit::InsertPagesFrom { name: name.to_string(), bytes: Arc::new(bytes), pages, at, field_conflicts: FieldConflicts::Rename })
    }

    /// Copy `pages` into a new PDF file (the document itself is unchanged).
    pub fn extract_pages(&self, pages: &[usize]) -> Result<Vec<u8>, PdfError> {
        self.check_pages(pages)?;
        self.session.extract(self.id, pages).map(|b| b.as_ref().clone()).map_err(PdfError::from_edit)
    }

    /// Crop `pages` to `keep`, a rectangle in view space of the **first** page in the list
    /// (the app shows a crop frame on one page; the same area is applied to all listed pages).
    /// Cropping only hides the outside (the crop box); nothing is deleted, and
    /// [`PdfDoc::reset_crop`] brings it back.
    pub fn crop_pages(&mut self, pages: &[usize], keep: ViewRect) -> Result<(), PdfError> {
        self.check_pages(pages)?;
        finite(&keep)?;
        let first = pages.first().copied().unwrap_or(0);
        let info = self.page_ref(first)?;
        let r = rect_to_user(&info, keep);
        if r[2] - r[0] < 3.0 || r[3] - r[1] < 3.0 {
            return Err(PdfError::Invalid("the crop area is too small".into()));
        }
        self.apply(Edit::SetPageBox { pages: pages.to_vec(), which: PageBox::Crop, spec: BoxSpec::Rect(r) })
    }

    /// Undo any crop on `pages` (show the full page again).
    pub fn reset_crop(&mut self, pages: &[usize]) -> Result<(), PdfError> {
        self.check_pages(pages)?;
        self.apply(Edit::SetPageBox { pages: pages.to_vec(), which: PageBox::Crop, spec: BoxSpec::Remove })
    }

    /// Copies of `pages`, inserted right after the last of them.
    pub fn duplicate_pages(&mut self, pages: &[usize]) -> Result<(), PdfError> {
        self.check_pages(pages)?;
        self.apply(Edit::DuplicatePages { pages: pages.to_vec(), at: None })
    }
}
