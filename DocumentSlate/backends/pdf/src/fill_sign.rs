//! Fill & Sign tab: fill form fields, type text / ticks / crosses onto flat forms, and sign with
//! a drawn, typed or picture signature. These are "simple" signatures (marks on the page), not
//! certificate-based digital signatures (those are "Later").

use pdfcraft_engine::{Edit, FieldValue, FormFieldKind, Shape, SignatureImage, Style, field_flags, typed_signature_shape};

use crate::annotate::strokes_to_user;
use crate::geom::{finite, rect64_to_view, rect_to_user, to_user};
use crate::{FieldKind, FieldWidget, FillMark, FormField, Look, PdfDoc, PdfError, Rgb, ViewPoint};

/// Fill & Sign's default text size (points).
pub const DEFAULT_TYPED_SIZE: f64 = 10.0;

impl PdfDoc {
    /// The document's interactive form fields (empty for a flat, non-form PDF — use
    /// [`PdfDoc::add_typed_text`] and [`PdfDoc::add_fill_mark`] on those).
    pub fn form_fields(&self) -> Vec<FormField> {
        let Ok(doc) = self.doc() else { return Vec::new() };
        doc.form
            .iter()
            .map(|f| FormField {
                name: f.name.clone(),
                kind: match f.kind {
                    FormFieldKind::Text => FieldKind::Text,
                    FormFieldKind::CheckBox => FieldKind::CheckBox,
                    FormFieldKind::Radio => FieldKind::Radio,
                    FormFieldKind::PushButton => FieldKind::Button,
                    FormFieldKind::Combo => FieldKind::ComboBox,
                    FormFieldKind::List => FieldKind::ListBox,
                    FormFieldKind::Signature => FieldKind::Signature,
                },
                value: f.value.clone(),
                options: f.options.clone(),
                read_only: f.has(field_flags::READ_ONLY),
                required: f.has(field_flags::REQUIRED),
                multiline: f.has(field_flags::MULTILINE),
                tooltip: f.tooltip.clone(),
                widgets: f
                    .widgets
                    .iter()
                    .filter_map(|w| {
                        let page = w.page?;
                        let info = doc.info.pages.get(page)?;
                        Some(FieldWidget {
                            page,
                            rect: rect64_to_view(info, w.rect),
                            on_state: w.on_state.clone(),
                            is_on: w.on_state.is_some() && w.state == w.on_state,
                        })
                    })
                    .collect(),
            })
            .collect()
    }

    /// Type into a text field (or set a combo box's typed value).
    pub fn set_field_text(&mut self, name: &str, text: &str) -> Result<(), PdfError> {
        self.apply(Edit::SetFieldValue { name: name.to_string(), value: FieldValue::Text(text.to_string()) })
    }

    /// Tick or untick a check box.
    pub fn set_field_checked(&mut self, name: &str, checked: bool) -> Result<(), PdfError> {
        self.apply(Edit::SetFieldValue { name: name.to_string(), value: FieldValue::Check(checked) })
    }

    /// Pick a radio button by its on-state name (see [`FieldWidget::on_state`]), or clear the
    /// group with `None`.
    pub fn set_field_radio(&mut self, name: &str, state: Option<&str>) -> Result<(), PdfError> {
        self.apply(Edit::SetFieldValue { name: name.to_string(), value: FieldValue::Radio(state.map(str::to_owned)) })
    }

    /// Choose option(s) of a combo box or list box by export value.
    pub fn set_field_choice(&mut self, name: &str, values: &[String]) -> Result<(), PdfError> {
        self.apply(Edit::SetFieldValue { name: name.to_string(), value: FieldValue::Choice(values.to_vec()) })
    }

    /// Clear every field back to its default.
    pub fn reset_form(&mut self) -> Result<(), PdfError> {
        self.apply(Edit::ResetForm { names: None })
    }

    /// Type text onto the page (for flat forms) with its top-left at `at`. `size` in points.
    pub fn add_typed_text(&mut self, page: usize, at: ViewPoint, text: &str, size: f64, color: Rgb) -> Result<(), PdfError> {
        finite(&at)?;
        if text.trim().is_empty() {
            return Err(PdfError::Invalid("type some text first".into()));
        }
        let size = if size.is_finite() { size.clamp(4.0, 300.0) } else { DEFAULT_TYPED_SIZE };
        // Measure the text with the PDF's Helvetica so the box fits it.
        let width = pdfcraft_engine::added_text_width(&pdfcraft_engine::AddedText { text: text.to_string(), size, ..Default::default() });
        let lines = text.lines().count().max(1) as f64;
        let (w, h) = ((width + 4.0) as f32, (lines * size * 1.2 + 2.0) as f32);
        let info = self.page_ref(page)?;
        let rect = rect_to_user(&info, [at[0], at[1], at[0] + w, at[1] + h]);
        let shape = Shape::Typewriter { rect, font_size: size };
        self.add_annotation(page, shape, Look { color: Some(color), width: Some(0.0), ..Look::default() }, text)
    }

    /// A tick, cross, dot or line, centred on `center`, `size` points across.
    pub fn add_fill_mark(&mut self, page: usize, center: ViewPoint, size: f32, mark: FillMark) -> Result<(), PdfError> {
        finite(&[center[0], center[1], size])?;
        let s = size.clamp(4.0, 200.0) / 2.0;
        let info = self.page_ref(page)?;
        let rect = rect_to_user(&info, [center[0] - s, center[1] - s, center[0] + s, center[1] + s]);
        let mark = match mark {
            FillMark::Check => pdfcraft_engine::FillMark::Check,
            FillMark::Cross => pdfcraft_engine::FillMark::Cross,
            FillMark::Dot => pdfcraft_engine::FillMark::Dot,
            FillMark::Line => pdfcraft_engine::FillMark::Line,
        };
        self.add_annotation(page, Shape::Mark { rect, mark }, Look::default(), "")
    }

    /// A drawn signature (finger strokes), placed exactly where the strokes are. To reuse a
    /// saved signature, the app scales its stored strokes to the target spot and calls this.
    pub fn add_drawn_signature(&mut self, page: usize, strokes: &[Vec<ViewPoint>], color: Option<Rgb>) -> Result<(), PdfError> {
        let info = self.page_ref(page)?;
        let strokes = strokes_to_user(&info, strokes)?;
        self.add_annotation(page, Shape::Signature { strokes }, Look { color, ..Look::default() }, "")
    }

    /// A typed signature (the name drawn in the bundled script font, as outlines, so it looks
    /// the same everywhere). Left edge at `at`, vertically centred on it, `height` points tall.
    pub fn add_typed_signature(&mut self, page: usize, at: ViewPoint, name: &str, height: f64) -> Result<(), PdfError> {
        finite(&at)?;
        let info = self.page_ref(page)?;
        let height = if height.is_finite() { height.clamp(8.0, 200.0) } else { 32.0 };
        let shape = typed_signature_shape(to_user(&info, at), name, height, i64::from(info.rotation))
            .ok_or_else(|| PdfError::Invalid("the signature font can't draw that name".into()))?;
        let style = Style::default_for(&shape);
        let author = self.author.clone();
        self.apply(Edit::AddAnnotation(pdfcraft_engine::NewAnnotation { page, shape, style, contents: String::new(), author }))
    }

    /// A signature (or initials) from a picture — PNG or JPEG bytes, e.g. a photo of a signature
    /// the user saved. Left edge at `at`, vertically centred; max 150 pt wide.
    pub fn add_signature_image(&mut self, page: usize, at: ViewPoint, image: &[u8], initials: bool) -> Result<(), PdfError> {
        finite(&at)?;
        let info = self.page_ref(page)?;
        let picture = SignatureImage::from_bytes(image).map_err(|e| PdfError::Invalid(format!("the picture can't be used: {e}")))?;
        let author = self.author.clone();
        let edit = picture.edit(page, &info, to_user(&info, at), initials, &author).ok_or_else(|| PdfError::Invalid("the picture can't be placed there".into()))?;
        self.apply(edit)
    }

    /// Today's date (year, month, day) in local time — for the "Date" stamp. The app formats it
    /// and places it with [`PdfDoc::add_typed_text`].
    pub fn today(&self) -> (i64, u32, u32) {
        self.session.today()
    }
}
