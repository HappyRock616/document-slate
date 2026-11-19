//! Edit tab: edit existing text lines, add text and pictures (which stay editable), links.

use std::sync::Arc;

use pdfcraft_engine::{AddedContent, AddedText, Edit, FontFamily, LinkAction, LinkStyle, LinkZoom};

use crate::geom::{display_to_view, finite, rect64_to_view, rect_to_display, rect_to_user};
use crate::{AddedItem, PdfDoc, PdfError, TextFont, TextLineInfo, TextStyle, ViewRect};

impl PdfDoc {
    /// The existing lines of text on a page, for "edit existing text": the app shows a box per
    /// line, the user taps one and types, then [`PdfDoc::edit_text_line`] writes it.
    pub fn text_lines(&self, page: usize) -> Result<Vec<TextLineInfo>, PdfError> {
        let info = self.page_ref(page)?;
        let doc = self.doc()?;
        Ok(doc
            .text_lines(page)
            .into_iter()
            .enumerate()
            .map(|(index, l)| TextLineInfo {
                index,
                text: l.text,
                rect: rect64_to_view(&info, l.rect),
                font: pdfcraft_engine::strip_subset_tag(&l.base_font).to_string(),
                size: l.size,
                bold: l.bold,
                italic: l.italic,
                reusable_font: l.decodable,
            })
            .collect())
    }

    /// Replace the text of one existing line. The engine reuses the line's own font where it
    /// can draw the new characters; otherwise it substitutes a standard font.
    pub fn edit_text_line(&mut self, page: usize, line: usize, text: &str) -> Result<(), PdfError> {
        self.page_ref(page)?;
        self.apply(Edit::EditTextLine { page, line, text: text.to_string() })
    }

    /// Add new text in a box: `rect`'s width wraps the text, its top is the first line's top
    /// (the height grows with the text). Stays editable later ([`PdfDoc::added_items`]).
    pub fn add_text(&mut self, page: usize, rect: ViewRect, text: &str, style: TextStyle) -> Result<(), PdfError> {
        finite(&rect)?;
        if text.is_empty() {
            return Err(PdfError::Invalid("type some text first".into()));
        }
        let info = self.page_ref(page)?;
        let mut t = AddedText {
            rect: rect_to_display(&info, rect),
            text: text.to_string(),
            family: match style.font {
                TextFont::Sans => FontFamily::Helvetica,
                TextFont::Serif => FontFamily::Times,
                TextFont::Mono => FontFamily::Courier,
            },
            bold: style.bold,
            italic: style.italic,
            size: if style.size.is_finite() { style.size.clamp(1.0, 500.0) } else { 12.0 },
            ..AddedText::default()
        };
        t.set_color(style.color.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }));
        self.apply(Edit::AddText { page, text: t })
    }

    /// Add a picture (PNG, JPEG, …) in `rect`, or centred at a sensible size with `None`.
    pub fn add_image(&mut self, page: usize, rect: Option<ViewRect>, name: &str, bytes: Vec<u8>) -> Result<(), PdfError> {
        let info = self.page_ref(page)?;
        let rect = match rect {
            Some(r) => {
                finite(&r)?;
                Some(rect_to_display(&info, r))
            }
            None => None,
        };
        self.apply(Edit::AddImage { page, rect, name: name.to_string(), bytes: Arc::new(bytes) })
    }

    /// Text and pictures added with this app (or PdfCraft); these can be moved, resized,
    /// retyped or deleted. Pictures that were already in the PDF are not listed here.
    pub fn added_items(&self) -> Vec<AddedItem> {
        let Ok(doc) = self.doc() else { return Vec::new() };
        let mut per_page: Vec<usize> = vec![0; doc.info.pages.len()];
        doc.added()
            .iter()
            .filter_map(|a| {
                let info = doc.info.pages.get(a.page)?;
                let slot = per_page.get_mut(a.page)?;
                let index = *slot;
                *slot += 1;
                let (rect, text) = match &a.content {
                    AddedContent::Text(t) => (pdfcraft_engine::added_text_rect(t), Some(t.text.clone())),
                    AddedContent::Image(i) => (i.rect, None),
                };
                Some(AddedItem { page: a.page, index, rect: display_to_view(info, rect), text })
            })
            .collect()
    }

    fn added_content(&self, page: usize, index: usize) -> Result<AddedContent, PdfError> {
        let doc = self.doc()?;
        doc.added()
            .iter()
            .filter(|a| a.page == page)
            .nth(index)
            .map(|a| a.content.clone())
            .ok_or_else(|| PdfError::Invalid(format!("page {} has no added item {}", page + 1, index + 1)))
    }

    /// Move / resize an added item to `rect` (view space).
    pub fn set_added_rect(&mut self, page: usize, index: usize, rect: ViewRect) -> Result<(), PdfError> {
        finite(&rect)?;
        let info = self.page_ref(page)?;
        let r = rect_to_display(&info, rect);
        let content = match self.added_content(page, index)? {
            AddedContent::Text(t) => AddedContent::Text(AddedText { rect: r, ..t }),
            AddedContent::Image(mut i) => {
                i.rect = r;
                AddedContent::Image(i)
            }
        };
        self.apply(Edit::UpdateContent { page, index, content })
    }

    /// Change the words of an added text item.
    pub fn set_added_text(&mut self, page: usize, index: usize, text: &str) -> Result<(), PdfError> {
        let content = match self.added_content(page, index)? {
            AddedContent::Text(t) => AddedContent::Text(AddedText { text: text.to_string(), ..t }),
            AddedContent::Image(_) => return Err(PdfError::Invalid("that item is a picture, not text".into())),
        };
        self.apply(Edit::UpdateContent { page, index, content })
    }

    /// Delete an added text or picture.
    pub fn delete_added(&mut self, page: usize, index: usize) -> Result<(), PdfError> {
        self.added_content(page, index)?;
        self.apply(Edit::DeleteContent { page, index })
    }

    /// Make `rect` a link to a web address.
    pub fn add_link_uri(&mut self, page: usize, rect: ViewRect, uri: &str) -> Result<(), PdfError> {
        let uri = uri.trim();
        if uri.is_empty() {
            return Err(PdfError::Invalid("enter a web address".into()));
        }
        self.add_link(page, rect, LinkAction::Uri(uri.to_string()))
    }

    /// Make `rect` a link to another page of this document (0-based).
    pub fn add_link_to_page(&mut self, page: usize, rect: ViewRect, target: usize) -> Result<(), PdfError> {
        self.page_ref(target)?;
        self.add_link(page, rect, LinkAction::Page { page: target, zoom: LinkZoom::Inherit })
    }

    fn add_link(&mut self, page: usize, rect: ViewRect, action: LinkAction) -> Result<(), PdfError> {
        finite(&rect)?;
        let info = self.page_ref(page)?;
        // Invisible rectangle, like most viewers make by default.
        self.apply(Edit::AddLink { page, rect: rect_to_user(&info, rect), action, style: LinkStyle::default() })
    }

    /// Change where a link goes (index from [`PdfDoc::links`]).
    pub fn set_link_uri(&mut self, page: usize, index: usize, uri: &str) -> Result<(), PdfError> {
        self.apply(Edit::SetLink { page, index, rect: None, action: Some(LinkAction::Uri(uri.trim().to_string())), style: None })
    }

    pub fn delete_link(&mut self, page: usize, index: usize) -> Result<(), PdfError> {
        self.apply(Edit::DeleteLink { page, index })
    }
}
