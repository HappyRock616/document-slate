//! Full-fidelity check: what would be lost by saving?
//!
//! Project rule: never lose content on save. GridCraft keeps most of an .xlsx (cells, styles,
//! charts, pictures, comments, pivots...) and passes many parts it doesn't understand through
//! unchanged (OLE objects, form controls, custom XML, external links...). Its own docs list what
//! it does NOT keep: slicers, timelines, the data model (Power Pivot), query tables attached to
//! tables, unknown extensions, and fine chart details.
//!
//! Rather than trusting a list, we compare the two zip packages: an .xlsx is a zip of "parts"
//! (xl/worksheets/sheet1.xml, xl/charts/chart1.xml, xl/slicers/slicer1.xml...). We group part
//! names by kind (numbers removed: `sheet#.xml`) and count them. If the saved file has fewer
//! parts of some kind than the original, something was dropped and we say what, in plain words.
//! It can't see losses *inside* a part (e.g. a chart's custom colours), so those are "notes".

use std::collections::BTreeMap;
use std::io::Cursor;
#[cfg(test)]
use std::io::Read;

use gridcraft_engine::model::Workbook;

/// Biggest [Content_Types].xml we read (protects against hostile zips).
const MAX_PART_LIST: usize = 100_000;

/// What a save keeps and loses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FidelityReport {
    /// Things that are definitely not in the saved file. Non-empty = ask before overwriting.
    pub lost: Vec<String>,
    /// Things that may be simplified, or general remarks (shown as information).
    pub notes: Vec<String>,
}

impl FidelityReport {
    /// True when nothing is known to be lost.
    pub fn is_lossless(&self) -> bool {
        self.lost.is_empty()
    }
}

/// Part names in a zip, or `None` if the bytes aren't a readable zip.
fn part_names(bytes: &[u8]) -> Option<Vec<String>> {
    let zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    Some(zip.file_names().take(MAX_PART_LIST).map(|n| n.trim_start_matches('/').to_ascii_lowercase()).collect())
}

/// The "kind" of a part: its folder plus its file name with digits replaced by `#`
/// (`xl/worksheets/sheet12.xml` → `xl/worksheets/sheet#.xml`).
fn kind_of(name: &str) -> String {
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    let mut out = String::with_capacity(name.len());
    let mut last_digit = false;
    for ch in file.chars() {
        if ch.is_ascii_digit() {
            if !last_digit {
                out.push('#');
            }
            last_digit = true;
        } else {
            out.push(ch);
            last_digit = false;
        }
    }
    // Media keeps its folder but not its extension (a .jpeg may be re-encoded as .png).
    if dir.ends_with("media") {
        return format!("{dir}/image#");
    }
    format!("{dir}/{out}")
}

/// Parts the writer legitimately rebuilds or leaves out without losing anything the person made.
fn ignorable(kind: &str) -> bool {
    kind.ends_with(".rels") // relationship lists: rebuilt to match the parts actually written
        || kind == "xl/calcchain.xml" // calculation order cache: Excel rebuilds it
        || kind == "[content_types].xml"
        || kind.starts_with("docprops/") // app name, thumbnail, statistics (core props are modelled)
        || kind == "xl/sharedstrings.xml" // strings may be written inline instead
        || kind == "xl/metadata.xml" // dynamic-array flags; re-derived from the formulas
}

/// A plain-words label for a kind of part.
fn label(kind: &str) -> String {
    let table: &[(&str, &str)] = &[
        ("xl/slicercaches/", "slicer data"),
        ("xl/slicers/", "slicers"),
        ("xl/timelinecaches/", "timeline data"),
        ("xl/timelines/", "timelines"),
        ("xl/model/", "data model (Power Pivot)"),
        ("xl/querytables/", "query tables"),
        ("xl/connections", "data connections"),
        ("xl/externallinks/", "links to other workbooks"),
        ("xl/pivotcache/", "PivotTable data"),
        ("xl/pivottables/", "PivotTables"),
        ("xl/charts/style", "chart styles"),
        ("xl/charts/colors", "chart color schemes"),
        ("xl/charts/", "charts"),
        ("xl/chartsheets/", "chart sheets"),
        // VML drawings (note boxes, old-style controls) have writer-specific names.
        ("xl/drawings/vmldrawing", "legacy drawings (notes, controls)"),
        ("xl/drawings/commentsdrawing", "legacy drawings (notes, controls)"),
        ("xl/drawings/", "drawings (pictures, shapes, charts)"),
        ("xl/media/", "pictures"),
        ("xl/embeddings/", "embedded objects (OLE)"),
        ("xl/activex/", "ActiveX controls"),
        ("xl/ctrlprops/", "form controls"),
        ("xl/comments", "notes"),
        ("xl/threadedcomments/", "threaded comments"),
        ("xl/persons/", "comment authors"),
        ("xl/tables/", "tables"),
        ("xl/worksheets/", "worksheets"),
        ("xl/vbaproject", "macros (VBA)"),
        ("xl/richdata/", "rich values (data types, images in cells)"),
        ("xl/printersettings/", "printer settings"),
        ("xl/theme/", "theme"),
        ("customxml/", "custom XML data"),
        ("xl/customproperty", "custom properties"),
        ("xl/webextensions/", "add-in task panes"),
    ];
    if kind.starts_with("xl/drawings/") && kind.ends_with(".vml") {
        return "legacy drawings (notes, controls)".into();
    }
    table
        .iter()
        .find(|(prefix, _)| kind.starts_with(prefix))
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| format!("file part `{kind}`"))
}

fn count_kinds(names: &[String]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for n in names {
        if n.ends_with('/') {
            continue; // a folder entry, not a part
        }
        let k = kind_of(n);
        if !ignorable(&k) {
            *map.entry(k).or_insert(0usize) += 1;
        }
    }
    map
}

/// Compares the original .xlsx with the saved one and records lost kinds of parts.
///
/// `removed` = how many parts of a kind the user took out on purpose since the file was opened
/// (label -> count, see `ModelCounts`): that many fewer parts are expected, not a loss.
pub(crate) fn compare_packages(original: &[u8], saved: &[u8], removed: &BTreeMap<&'static str, usize>, report: &mut FidelityReport) {
    let Some(before) = part_names(original) else {
        report.notes.push("Couldn't compare the saved file with the original package.".into());
        return;
    };
    // A saved file that can't be read back must never count as "nothing lost".
    let Some(after) = part_names(saved) else {
        report.lost.push("the whole workbook (the saved file couldn't be read back)".into());
        return;
    };
    // Add up parts per plain-words label. Comparing labels rather than exact names means a
    // different naming scheme (drawing1.xml written as drawing2.xml) isn't mistaken for a loss.
    let by_label = |names: &[String]| {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for (kind, n) in count_kinds(names) {
            *m.entry(label(&kind)).or_insert(0) += n;
        }
        m
    };
    let (before, after) = (by_label(&before), by_label(&after));
    for (what, b) in &before {
        let a = after.get(what).copied().unwrap_or(0);
        let allowed = removed.get(what.as_str()).copied().unwrap_or(0);
        if a + allowed < *b {
            report.lost.push(format!("{what} ({} of {b} not saved)", b - a - allowed));
        }
    }
}

/// What the workbook model holds of the things that are their own parts in the package. Taken
/// when the file is opened and again at save: the difference is what the user deleted (a sheet,
/// a table, a sheet's last note), which must not be reported as a loss.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModelCounts {
    sheets: usize,
    tables: usize,
    sheets_with_notes: usize,
}

impl ModelCounts {
    pub(crate) fn of(wb: &Workbook) -> ModelCounts {
        ModelCounts {
            sheets: wb.sheets.len(),
            tables: wb.sheets.iter().map(|s| s.tables.len()).sum(),
            sheets_with_notes: wb.sheets.iter().filter(|s| !s.comments.is_empty()).count(),
        }
    }

    /// Parts removed on purpose between `self` (at open) and `now`, by the labels of [`label`].
    pub(crate) fn removed_since(self, now: ModelCounts) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for (what, was, is) in [("worksheets", self.sheets, now.sheets), ("tables", self.tables, now.tables), ("notes", self.sheets_with_notes, now.sheets_with_notes)] {
            if is < was {
                m.insert(what, was - is);
            }
        }
        m
    }
}

/// Remarks based on the workbook model: features GridCraft keeps only in simplified form.
pub(crate) fn model_notes(wb: &Workbook, report: &mut FidelityReport) {
    let charts: usize = wb.sheets.iter().map(|s| s.charts.len()).sum();
    if charts > 0 {
        report.notes.push(format!(
            "{charts} chart(s): data, type, titles and series are kept, but some fine formatting made in Excel (custom colors, label positions, effects) may be simplified."
        ));
    }
}

/// What a CSV file can't hold.
pub(crate) fn csv_losses(wb: &Workbook, report: &mut FidelityReport) {
    let active = wb.active_sheet();
    let others = wb.sheets.len().saturating_sub(1);
    if others > 0 {
        report.lost.push(format!("{others} other sheet(s): a CSV file holds only the current sheet"));
    }
    let Some(sh) = wb.sheet(active) else { return };
    let formulas = sh.cells.iter().filter(|(_, c)| c.formula.is_some()).count();
    if formulas > 0 {
        report.lost.push(format!("{formulas} formula(s): saved as their results"));
    }
    let styled = sh.cells.iter().any(|(_, c)| c.style != gridcraft_engine::model::StyleId::DEFAULT);
    if styled || !sh.merges.is_empty() || !sh.cols.is_empty() {
        report.lost.push("formatting (fonts, colors, borders, merged cells, column widths)".into());
    }
    let objects = sh.charts.len() + sh.images.len() + sh.shapes.len() + sh.comments.len();
    if objects > 0 {
        report.lost.push(format!("{objects} chart(s), picture(s), shape(s) or note(s)"));
    }
}

/// Reads one part's text (used by tests).
#[cfg(test)]
pub(crate) fn part_text(bytes: &[u8], name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}
