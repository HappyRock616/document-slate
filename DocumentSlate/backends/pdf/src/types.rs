//! Plain data types the app sees. No engine types leak out, so the app only depends on this
//! crate (and engine upgrades stay inside it).

use pdfcraft_engine::EditError;
use pdfcraft_render::OpenError;

/// Everything that can go wrong, in words the app can show.
#[derive(Debug, Clone, PartialEq)]
pub enum PdfError {
    /// The file is locked with an open password: ask for it and open again.
    NeedsPassword,
    /// The password given was wrong.
    WrongPassword,
    /// The file isn't a PDF we can read.
    NotPdf(String),
    /// The document's security settings forbid this change.
    NotPermitted(String),
    /// Bad input from the caller (a page number past the end, an empty list…).
    Invalid(String),
    /// Anything else the engine reported.
    Other(String),
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfError::NeedsPassword => write!(f, "this PDF is protected by a password"),
            PdfError::WrongPassword => write!(f, "the password is incorrect"),
            PdfError::NotPdf(m) => write!(f, "this file can't be read as a PDF: {m}"),
            PdfError::NotPermitted(m) | PdfError::Invalid(m) | PdfError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for PdfError {}

impl PdfError {
    pub(crate) fn from_open(e: OpenError) -> Self {
        match e {
            OpenError::NeedsPassword => PdfError::NeedsPassword,
            OpenError::WrongPassword => PdfError::WrongPassword,
            OpenError::Invalid(m) => PdfError::NotPdf(m),
            OpenError::Unsupported(m) => PdfError::Other(m),
        }
    }

    pub(crate) fn from_edit(e: EditError) -> Self {
        match e {
            EditError::WrongPassword => PdfError::WrongPassword,
            EditError::NotPermitted(_) | EditError::ReadOnly(_) => PdfError::NotPermitted(e.to_string()),
            other => PdfError::Other(other.to_string()),
        }
    }
}

/// What [`crate::PdfDoc::save_bytes`] produced.
#[derive(Debug, Clone)]
pub struct SaveOutput {
    /// The complete file to write.
    pub bytes: Vec<u8>,
    /// `true` when the original file is kept byte for byte at the start (an incremental save).
    /// `false` after a change that needs a full rewrite (new/removed password, a damaged file
    /// being repaired); content is still kept, only old revisions are not.
    pub incremental: bool,
    /// Things the user should know about the saved file. Empty = nothing besides their edits.
    pub notes: Vec<String>,
}

/// A rendered page picture.
#[derive(Debug, Clone)]
pub struct RenderedImage {
    pub width: u32,
    pub height: u32,
    /// RGBA, 8 bits per channel, row by row, **premultiplied alpha** (pages are opaque white,
    /// so in practice alpha is 255). egui: `ColorImage::from_rgba_premultiplied`.
    pub rgba: Vec<u8>,
    /// Non-fatal problems (e.g. "some content was too big and left out").
    pub warnings: Vec<String>,
}

/// Size and look of one page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    /// Displayed width and height in points (after the page's rotation).
    pub width: f32,
    pub height: f32,
    /// The page label ("iv", "A-3"…), or the page number as text.
    pub label: String,
    /// The page's own rotation, clockwise: 0, 90, 180 or 270.
    pub rotation: u16,
}

/// A rectangle in page view space: `[left, top, right, bottom]` in points (y down).
pub type ViewRect = [f32; 4];
/// A point in page view space: `[x, y]` in points (y down).
pub type ViewPoint = [f32; 2];
/// An RGB colour, each part 0.0 – 1.0.
pub type Rgb = [f64; 3];

/// One search match.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// 0-based page.
    pub page: usize,
    /// One rectangle per line the match covers (view space). Pass to
    /// [`crate::PdfDoc::add_text_markup`] to highlight it.
    pub rects: Vec<ViewRect>,
    /// The matched text as it appears on the page.
    pub text: String,
}

/// A bookmark (outline entry), with the bookmarks under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub title: String,
    /// The page it goes to (0-based), if it goes to a page in this file.
    pub page: Option<usize>,
    /// A web address, when the bookmark opens one instead.
    pub uri: Option<String>,
    pub children: Vec<Bookmark>,
}

/// Where a link goes.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// A page in this document (0-based).
    Page(usize),
    /// A web address. The app should ask before opening it.
    Uri(String),
    /// Something we don't follow (JavaScript, other files, named actions…), with a description.
    Other(String),
}

/// A clickable link on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLink {
    pub page: usize,
    /// Position in the page's annotation list; use with [`crate::PdfDoc::delete_link`].
    pub index: usize,
    pub rect: ViewRect,
    pub target: LinkTarget,
}

/// A comment / annotation shown in the comment list.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentInfo {
    pub page: usize,
    /// Position in the page's annotation list. Use with `delete_comment` etc. Note: deleting a
    /// comment shifts the indexes after it, so re-read the list after a change.
    pub index: usize,
    /// PDF subtype: "Highlight", "Text" (sticky note), "FreeText", "Ink", "Square"…
    pub kind: String,
    pub author: String,
    pub text: String,
    /// Last change, formatted for display, when known.
    pub modified: Option<String>,
    pub rect: ViewRect,
    pub color: Option<[f32; 3]>,
    /// `true` for a reply to another comment.
    pub is_reply: bool,
    /// `true` for Fill & Sign marks (typed text, ticks, signatures).
    pub fill_sign: bool,
    /// Locked comments can't be moved or deleted.
    pub locked: bool,
}

/// Text markup kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    Highlight,
    Underline,
    Strikeout,
    Squiggly,
}

/// Drawing shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Circle,
    Line,
    Arrow,
}

/// Optional look for new comments. `None` fields use the engine's defaults (yellow highlight,
/// red shapes, blue pen…).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Look {
    pub color: Option<Rgb>,
    /// Line width in points.
    pub width: Option<f64>,
    /// 0.0 (invisible) – 1.0 (solid).
    pub opacity: Option<f64>,
    /// Fill colour for rectangles / circles.
    pub fill: Option<Rgb>,
}

/// Kinds of form field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    CheckBox,
    Radio,
    Button,
    ComboBox,
    ListBox,
    Signature,
}

/// One interactive form field.
#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    /// Full name; pass it back to the `set_field_*` methods.
    pub name: String,
    pub kind: FieldKind,
    /// Current value(s). Text: one entry. Check box: the "on" name when checked, empty when not.
    /// Lists: the selected export values.
    pub value: Vec<String>,
    /// For combo / list boxes: (export value, shown text). For radio groups: the on-state names.
    pub options: Vec<(String, String)>,
    pub read_only: bool,
    pub required: bool,
    pub multiline: bool,
    pub tooltip: Option<String>,
    /// Where the field shows (it can show on several pages): (page, rect in view space,
    /// on-state for radio buttons / check boxes).
    pub widgets: Vec<FieldWidget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldWidget {
    pub page: usize,
    pub rect: ViewRect,
    /// For check boxes and radio buttons: the name of this widget's "on" state.
    pub on_state: Option<String>,
    /// `true` when this check box / radio button is currently on.
    pub is_on: bool,
}

/// Fill & Sign marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMark {
    Check,
    Cross,
    Dot,
    Line,
}

/// One existing line of text on a page (for "edit existing text").
#[derive(Debug, Clone, PartialEq)]
pub struct TextLineInfo {
    /// Pass back to [`crate::PdfDoc::edit_text_line`].
    pub index: usize,
    pub text: String,
    pub rect: ViewRect,
    /// Font name and size (points).
    pub font: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    /// `false` when the font has no Unicode map: new text then needs a substitute font.
    pub reusable_font: bool,
}

/// Fonts for added text (the PDF standard fonts, available everywhere).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextFont {
    #[default]
    Sans,
    Serif,
    Mono,
}

/// Style for added text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: TextFont,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: Rgb,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { font: TextFont::Sans, size: 12.0, bold: false, italic: false, color: [0.0, 0.0, 0.0] }
    }
}

/// Text or a picture added with this app (or PdfCraft); these stay editable later.
#[derive(Debug, Clone, PartialEq)]
pub struct AddedItem {
    pub page: usize,
    /// Index among the added items **on that page**; use with `move_added` / `delete_added`.
    pub index: usize,
    pub rect: ViewRect,
    /// The text, or `None` for a picture.
    pub text: Option<String>,
}

/// Document properties (File ▸ Properties).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Properties {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub pdf_version: String,
    pub page_count: usize,
    pub file_size: usize,
    pub encrypted: bool,
    pub tagged: bool,
    /// `true` when the file contains JavaScript (which this app never runs).
    pub has_javascript: bool,
    /// `true` when the file has digital signatures (an incremental save keeps them valid).
    pub signed: bool,
    pub has_form: bool,
}

/// Which property to change with [`crate::PdfDoc::set_property`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKey {
    Title,
    Author,
    Subject,
    Keywords,
}

/// What the document's security allows (shown in File ▸ Properties).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub encrypted: bool,
    pub print: bool,
    pub copy: bool,
    pub edit: bool,
    pub annotate: bool,
    pub fill_forms: bool,
    pub change_pages: bool,
}
