//! Saving (xlsx / csv) and PDF export for printing.

use std::path::Path;

use serde_json::{Value as Json, json};

use crate::fidelity::{self, FidelityReport};
use crate::{GridDoc, SourceKind};

/// Formats Save / Save as can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    /// Full-fidelity workbook. A file opened as .xlsm/.xltx/.xltm keeps that package kind (and
    /// its macros) when saved again.
    Xlsx,
    /// The active sheet's shown values only (an export: formulas, formatting and other sheets
    /// are not in a CSV file).
    Csv,
    /// OpenDocument. GridCraft can read .ods but cannot write it yet: always an error for now.
    Ods,
}

/// The result of a save.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub bytes: Vec<u8>,
    /// File name to suggest (original stem + the right extension).
    pub suggested_name: String,
    /// What did not make it into the file. The app should show this (and ask before
    /// overwriting the original) whenever `report.lost` is not empty.
    pub report: FidelityReport,
}

/// Options for the PDF that printing / "Export PDF" produce.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PdfOptions {
    /// All visible sheets (true) or only the active one (false).
    pub all_sheets: bool,
    /// Shrink each sheet onto a single page (overrides the sheet's own Page Setup scaling).
    pub fit_on_one_page: bool,
}

impl GridDoc {
    /// Encodes the workbook. For `Xlsx`, a successful save also clears the "unsaved changes"
    /// flag. Check `report` before writing over the original file.
    pub fn save_bytes(&mut self, format: SaveFormat) -> Result<Saved, String> {
        self.save_impl(format, true, None)
    }

    /// Like [`GridDoc::save_bytes`], but leaves the "unsaved changes" flag alone: for writing a
    /// file that may not end up being the saved document (the app calls [`GridDoc::mark_clean`]
    /// once the file really replaced the original).
    pub fn encode(&mut self, format: SaveFormat) -> Result<Saved, String> {
        self.save_impl(format, false, None)
    }

    /// [`GridDoc::encode`] for a file that will be called `file_name`: its extension decides the
    /// exact kind (a plain .xlsx even when the open file is macro-enabled; tabs for .tsv/.tab/.txt).
    pub fn encode_for(&mut self, format: SaveFormat, file_name: &str) -> Result<Saved, String> {
        let ext = Path::new(file_name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        self.save_impl(format, false, ext.as_deref())
    }

    /// The workbook as it is now counts as saved (no unsaved changes).
    pub fn mark_clean(&mut self) {
        if let Some(doc) = self.session.active_mut() {
            doc.saved = doc.wb.clone();
        }
    }

    /// The document now lives in the file `name` with these bytes (after Save as): later saves
    /// compare against them (fidelity check) and the file kind follows the new name.
    pub fn adopt_saved(&mut self, name: &str, bytes: &[u8]) {
        let sniffed = gridcraft_xlsx::sniff(bytes);
        self.kind = crate::source_kind(name, sniffed);
        self.name = name.to_string();
        self.original_xlsx = (self.kind == SourceKind::Xlsx && sniffed == gridcraft_xlsx::Format::Xlsx).then(|| bytes.to_vec());
        self.opened_counts = self.doc().map(|d| fidelity::ModelCounts::of(&d.wb)).unwrap_or_default();
        self.mark_clean();
    }

    /// `target_ext`: extension of the file being written, when the caller knows it.
    fn save_impl(&mut self, format: SaveFormat, clean: bool, target_ext: Option<&str>) -> Result<Saved, String> {
        let stem = Path::new(&self.name).file_stem().and_then(|s| s.to_str()).unwrap_or("Book").to_string();
        match format {
            SaveFormat::Ods => Err("Saving as ODS isn't possible yet: the GridCraft engine reads .ods files but can't write them. Save as XLSX instead.".into()),
            SaveFormat::Xlsx => self.save_xlsx(&stem, clean, target_ext),
            SaveFormat::Csv => self.save_csv(&stem, clean, target_ext),
        }
    }

    fn save_xlsx(&mut self, stem: &str, clean: bool, target_ext: Option<&str>) -> Result<Saved, String> {
        // Keep the original package kind: .xlsm keeps its macros, a template stays a template.
        // When the target file's name is known it decides instead, so a macro-enabled package is
        // never written under a plain .xlsx name (Excel refuses such a file); the fidelity
        // check below then reports the macros as not saved.
        let own = Path::new(&self.name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        let ext = match (self.kind, target_ext.or(own.as_deref())) {
            (SourceKind::Xlsx, Some(e @ ("xlsm" | "xltx" | "xltm"))) => e.to_string(),
            _ => "xlsx".to_string(),
        };
        let name = format!("{stem}.{ext}");
        let d = self.doc().ok_or("no workbook open")?;
        let bytes = gridcraft_engine::io::save_bytes_as(&d.wb, &name, gridcraft_engine::io::FileKind::Xlsx).map_err(|e| e.to_string())?;
        let mut report = FidelityReport::default();
        match &self.original_xlsx {
            Some(original) => {
                let removed = self.opened_counts.removed_since(fidelity::ModelCounts::of(&d.wb));
                fidelity::compare_packages(original, &bytes, &removed, &mut report);
            }
            None => self.import_notes(&mut report),
        }
        fidelity::model_notes(&d.wb, &mut report);
        if self.warnings.iter().any(|w| w == gridcraft_engine::io::DECRYPTED_NOTICE) {
            report.notes.push("The original file had a password; this copy is saved without it.".into());
        }
        // The saved bytes now match the workbook: no unsaved changes.
        if clean && let Some(doc) = self.session.active_mut() {
            doc.saved = doc.wb.clone();
        }
        Ok(Saved { bytes, suggested_name: name, report })
    }

    fn save_csv(&mut self, stem: &str, clean: bool, target_ext: Option<&str>) -> Result<Saved, String> {
        // Tab-separated when the file being written is one (.tsv, .tab, .txt), else commas.
        let (ext, kind) = match target_ext {
            Some(e @ ("tsv" | "tab" | "txt")) => (e, gridcraft_engine::io::FileKind::Tsv),
            _ => ("csv", gridcraft_engine::io::FileKind::Csv),
        };
        let name = format!("{stem}.{ext}");
        let d = self.doc().ok_or("no workbook open")?;
        let bytes = gridcraft_engine::io::save_bytes_as(&d.wb, &name, kind).map_err(|e| e.to_string())?;
        let mut report = FidelityReport::default();
        fidelity::csv_losses(&d.wb, &mut report);
        // A CSV is only a "save" (clears unsaved changes) when the document *is* a CSV and the
        // workbook still fits in one (one sheet, no formulas or formatting were lost).
        if clean
            && self.kind == SourceKind::Csv
            && report.lost.is_empty()
            && let Some(doc) = self.session.active_mut()
        {
            doc.saved = doc.wb.clone();
        }
        Ok(Saved { bytes, suggested_name: name, report })
    }

    /// Notes for documents that were imported from a format we can't write back.
    fn import_notes(&self, report: &mut FidelityReport) {
        let from = match self.kind {
            SourceKind::Ods => "an OpenDocument (.ods) file",
            SourceKind::Xls => "an Excel 97-2003 (.xls) file",
            SourceKind::Xlsb => "an Excel binary (.xlsb) file",
            _ => return,
        };
        report.notes.push(format!(
            "This workbook was imported from {from}. It is saved as a new .xlsx file; anything the import couldn't read (see the warnings shown when it opened) is not in it. The original file is unchanged."
        ));
    }

    /// The PDF used for printing and "Export PDF": paginated with each sheet's Page Setup
    /// (paper, orientation, margins, print area, gridlines, headers/footers). Returns the bytes
    /// and the page count.
    pub fn pdf_bytes(&mut self, opts: PdfOptions) -> Result<(Vec<u8>, usize), String> {
        let mut p = json!({ "sheets": if opts.all_sheets { "all" } else { "active" } });
        if opts.fit_on_one_page
            && let Some(obj) = p.as_object_mut()
        {
            obj.insert("fitToPage".into(), Json::Bool(true));
        }
        let r = self.run("file.print", p)?;
        let pages = r.get("pages").and_then(Json::as_u64).unwrap_or(0) as usize;
        let b64 = r.get("base64").and_then(Json::as_str).ok_or("the engine returned no PDF")?;
        let bytes = gridcraft_engine::io::base64_decode(b64).ok_or("the engine returned a damaged PDF")?;
        Ok((bytes, pages))
    }

    /// Writes the print PDF to `path` (e.g. a file in the app's cache dir for PrintManager).
    /// Returns the page count.
    pub fn export_pdf(&mut self, path: &Path, opts: PdfOptions) -> Result<usize, String> {
        let (bytes, pages) = self.pdf_bytes(opts)?;
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(pages)
    }
}
