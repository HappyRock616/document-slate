//! File tab: document properties, password protection, and the file to print.

use pdfcraft_engine::{Changes, Edit, Printing, Protection};

use crate::{Permissions, PdfDoc, PdfError, Properties, PropertyKey};

impl PdfDoc {
    /// Document properties (title, author, PDF version, size…).
    pub fn properties(&self) -> Properties {
        let Ok(doc) = self.doc() else { return Properties::default() };
        let i = &doc.info;
        Properties {
            title: doc.info_value("Title").or_else(|| i.title.clone()),
            author: doc.info_value("Author").or_else(|| i.author.clone()),
            subject: doc.info_value("Subject").or_else(|| i.subject.clone()),
            keywords: doc.info_value("Keywords").or_else(|| i.keywords.clone()),
            creator: i.creator.clone(),
            producer: i.producer.clone(),
            pdf_version: i.pdf_version.clone(),
            page_count: i.pages.len(),
            file_size: doc.bytes.len(),
            encrypted: i.encrypted,
            tagged: i.tagged,
            has_javascript: i.has_javascript,
            signed: doc.is_signed(),
            has_form: !doc.form.is_empty(),
        }
    }

    /// Change the title, author, subject or keywords. An empty value removes it.
    pub fn set_property(&mut self, key: PropertyKey, value: &str) -> Result<(), PdfError> {
        let key = match key {
            PropertyKey::Title => "Title",
            PropertyKey::Author => "Author",
            PropertyKey::Subject => "Subject",
            PropertyKey::Keywords => "Keywords",
        };
        self.apply(Edit::SetInfo { key: key.into(), value: value.to_string() })
    }

    /// What the document's security settings allow in this session.
    pub fn permissions(&self) -> Permissions {
        match self.doc() {
            Ok(d) => Permissions {
                encrypted: d.info.encrypted,
                print: d.allows_printing(),
                copy: d.allows_copying(),
                edit: d.allows_modification(),
                annotate: d.allows_annotation(),
                fill_forms: d.allows_form_filling(),
                change_pages: d.allows_assembly(),
            },
            Err(_) => Permissions { encrypted: false, print: false, copy: false, edit: false, annotate: false, fill_forms: false, change_pages: false },
        }
    }

    /// Lock the file with a password needed to **open** it (AES-256). Printing, copying and
    /// editing stay allowed for whoever knows the password. Takes effect when saved (the save is
    /// then a full rewrite, as encryption must cover the whole file).
    ///
    /// `permissions_password`, if given, adds an owner password: without it, the file opens
    /// read-only (printing still allowed). Must differ from the open password.
    pub fn protect(&mut self, open_password: &str, permissions_password: Option<&str>) -> Result<(), PdfError> {
        if open_password.is_empty() {
            return Err(PdfError::Invalid("the password can't be empty".into()));
        }
        let restricted = permissions_password.filter(|p| !p.is_empty());
        let protection = Protection {
            open_password: Some(open_password.to_string()),
            permissions_password: restricted.map(str::to_owned),
            printing: Printing::High,
            // With an owner password: only form filling and signing allowed for others.
            // Without one, nothing is restricted (the flags are ignored then).
            changes: if restricted.is_some() { Changes::FillSign } else { Changes::AnyExceptExtract },
            copy: restricted.is_none(),
            accessibility: true,
            ..Protection::default()
        };
        self.apply(Edit::Protect(protection))
    }

    /// Remove the password protection (needs the document opened with its password, and the
    /// owner password if it has one — see [`PdfDoc::unlock`]). Takes effect when saved.
    pub fn remove_protection(&mut self) -> Result<(), PdfError> {
        self.apply(Edit::RemoveProtection)
    }

    /// Enter the owner (permissions) password of a restricted document, lifting its
    /// restrictions for this session.
    pub fn unlock(&mut self, owner_password: &str) -> Result<(), PdfError> {
        self.session.unlock(self.id, owner_password).map_err(PdfError::from_edit)
    }

    /// The PDF to hand to Android's print system.
    ///
    /// Android's print services get the PDF file and print it themselves, so this produces a
    /// "print-ready" copy with the engine's print path: comments and filled fields are drawn
    /// as they look on screen (even ones without their own appearance), hidden layers stay
    /// hidden, password protection is removed from the copy (printers can't open locked
    /// files), and each page is fitted to `paper` (width, height in points, portrait; use the
    /// media size the print dialog chose, e.g. A4 = 595.28 x 841.89). The document itself is
    /// not changed. Fails if the document's security forbids printing.
    pub fn print_pdf(&self, paper: (f64, f64)) -> Result<Vec<u8>, PdfError> {
        let (w, h) = paper;
        if !(w.is_finite() && h.is_finite() && w > 72.0 && h > 72.0) {
            return Err(PdfError::Invalid("the paper size isn't valid".into()));
        }
        let settings = pdfcraft_engine::print::Settings {
            pages: (0..self.page_count()).collect(),
            paper: (w.min(h), w.max(h)),
            ..pdfcraft_engine::print::Settings::default()
        };
        self.session.print_pdf(self.id, &settings).map_err(PdfError::from_edit)
    }
}
