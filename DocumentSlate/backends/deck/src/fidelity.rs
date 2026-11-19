//! "Will saving lose anything?" (project rule: never lose content on save.)
//!
//! The engine reads a .pptx into its own model and writes a fresh .pptx from that model. Whatever
//! the model doesn't hold is gone after a save. The upstream docs list known gaps (equations,
//! macros, some media playback options…), but rather than trust a list we *measure*:
//!
//! 1. Save the presentation exactly as it was opened (no edits) to .pptx in memory.
//! 2. Take an inventory of both packages (the original file and that re-save): how many slides,
//!    notes, charts, diagrams, comments…; the exact bytes of every picture, media file, embedded
//!    object and font; how many pictures, shapes, tables, links, equations, transitions,
//!    animations… the slide XML contains; and how much text there is.
//! 3. Anything the re-save has *less* of is reported.
//!
//! Because the check uses the unedited presentation, the user's own deletions never show up as
//! losses. It can't see changes in *how* something looks (only whether it's still there), and a
//! deck written by other software may report a few false alarms (things written differently,
//! not lost); wording to the user therefore says "may".

use std::collections::HashMap;
use std::io::Read;

use deckcraft_model::Presentation;

/// One kind of content that a save would lose.
#[derive(Clone, Debug, PartialEq)]
pub struct FidelityIssue {
    /// Short name of what is lost ("equations", "macros (VBA)", "pictures or media files"…).
    pub kind: String,
    /// How many are lost (characters, for text).
    pub lost: usize,
    /// A sentence for the user.
    pub message: String,
}

/// Everything a save would lose. Empty = no loss found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FidelityReport {
    pub issues: Vec<FidelityIssue>,
}

impl FidelityReport {
    pub fn is_lossless(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Check what saving `opened` (the presentation as read from `original`) as .pptx would lose.
/// Safe to run on a background thread.
pub fn probe(original: &[u8], opened: &Presentation) -> FidelityReport {
    match deckcraft_pptx::export(opened) {
        Ok(resaved) => compare(original, &resaved),
        Err(e) => FidelityReport {
            issues: vec![FidelityIssue {
                kind: "whole file".into(),
                lost: 1,
                message: format!("This presentation can't be saved as PowerPoint right now: {e}"),
            }],
        },
    }
}

/// Compare two .pptx packages: what does `resaved` have less of than `original`?
pub fn compare(original: &[u8], resaved: &[u8]) -> FidelityReport {
    // An original that isn't a readable package has nothing to compare against.
    let Some(a) = Inventory::of(original) else {
        return FidelityReport::default();
    };
    // A re-save that can't be read back is the worst loss there is: never "nothing lost".
    let Some(b) = Inventory::of(resaved) else {
        return FidelityReport {
            issues: vec![FidelityIssue { kind: "whole file".into(), lost: 1, message: "The saved presentation couldn't be read back, so it can't be checked.".into() }],
        };
    };
    let mut issues = vec![];
    let mut push = |kind: &str, lost: usize, message: String| issues.push(FidelityIssue { kind: kind.into(), lost, message });

    // Counted parts.
    for (key, label) in PART_LABELS {
        let (was, now) = (a.parts.get(key).copied().unwrap_or(0), b.parts.get(key).copied().unwrap_or(0));
        if now < was {
            push(label, was - now, format!("{} {label} may be lost when saving.", was - now));
        }
    }
    // Binary payloads (pictures, media, embedded objects, fonts, macros): each original file's
    // exact bytes must still be somewhere in the re-save.
    let mut missing: HashMap<&'static str, (usize, usize)> = HashMap::new();
    for ((hash, len), cat) in &a.payloads {
        if !b.payloads.contains_key(&(*hash, *len)) {
            let e = missing.entry(cat).or_default();
            e.0 += 1;
            e.1 += len;
        }
    }
    let mut cats: Vec<_> = missing.into_iter().collect();
    cats.sort();
    for (cat, (n, bytes)) in cats {
        push(cat, n, format!("{n} {cat} ({} KB) may be lost when saving.", bytes.div_ceil(1024)));
    }
    // Elements inside slides and notes.
    for (i, (_, _, label)) in ELEMENTS.iter().enumerate() {
        let (was, now) = (a.elements.get(i).copied().unwrap_or(0), b.elements.get(i).copied().unwrap_or(0));
        if now < was {
            push(label, was - now, format!("{} {label} may be lost when saving.", was - now));
        }
    }
    if b.animations < a.animations {
        let n = a.animations - b.animations;
        push("animation effects", n, format!("{n} animation effects may be lost when saving."));
    }
    // Text: letters (not spaces) in slides and notes.
    if b.text_chars < a.text_chars {
        let n = a.text_chars - b.text_chars;
        push("text", n, format!("About {n} characters of text may be lost when saving."));
    }
    FidelityReport { issues }
}

/// Parts we count, by the inventory key, with a name for the user.
const PART_LABELS: &[(&str, &str)] = &[
    ("slides", "slides"),
    ("notes", "speaker notes pages"),
    ("charts", "charts"),
    ("diagrams", "SmartArt diagrams"),
    ("comments", "comment files"),
    ("customXml", "custom data parts"),
    ("tags", "add-in tag parts"),
    ("ink", "ink drawings"),
    ("activeX", "ActiveX controls"),
];

/// Elements counted in slide and notes XML: (name to match, match the full `prefix:name`
/// (else just the local name), name for the user).
const ELEMENTS: &[(&str, bool, &str)] = &[
    ("pic", false, "pictures"),
    ("sp", false, "shapes or text boxes"),
    ("cxnSp", false, "connectors"),
    ("grpSp", false, "groups"),
    ("graphicFrame", false, "tables, charts or diagrams"),
    ("tbl", false, "tables"),
    ("oleObj", false, "embedded objects"),
    ("contentPart", false, "ink or 3-D objects"),
    ("svgBlip", false, "SVG pictures"),
    ("hlinkClick", false, "links or click actions"),
    ("hlinkHover", false, "hover actions"),
    ("fld", false, "fields (slide numbers, dates)"),
    ("audioFile", false, "audio clips"),
    ("videoFile", false, "video clips"),
    ("p14:trim", true, "media trim settings"),
    ("p14:fade", true, "media fade settings"),
    ("p14:bmk", true, "media bookmarks"),
    ("transition", false, "slide transitions"),
    ("oMath", false, "equations"),
    ("custDataLst", false, "add-in data lists"),
];

/// What a package contains.
#[derive(Default)]
struct Inventory {
    parts: HashMap<&'static str, usize>,
    /// (content hash, length) -> category.
    payloads: HashMap<(u64, usize), &'static str>,
    /// Count per entry of [`ELEMENTS`].
    elements: Vec<usize>,
    animations: usize,
    text_chars: usize,
}

/// Limits, because the file is untrusted: entries looked at, and bytes read per entry.
const MAX_ENTRIES: usize = 20_000;
const MAX_XML: u64 = 64 << 20;
const MAX_BINARY: u64 = 512 << 20;

impl Inventory {
    fn of(bytes: &[u8]) -> Option<Inventory> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
        let mut inv = Inventory { elements: vec![0; ELEMENTS.len()], ..Default::default() };
        for i in 0..zip.len().min(MAX_ENTRIES) {
            let Ok(mut f) = zip.by_index(i) else { continue };
            if f.is_dir() {
                continue;
            }
            let name = f.name().to_string();
            let lower = name.to_ascii_lowercase();
            if let Some(key) = part_key(&lower) {
                *inv.parts.entry(key).or_default() += 1;
            }
            if let Some(cat) = payload_category(&lower) {
                let mut data = Vec::new();
                if (&mut f).take(MAX_BINARY).read_to_end(&mut data).is_ok() {
                    inv.payloads.insert((hash(&data), data.len()), cat);
                }
                continue;
            }
            let slide_xml = (lower.starts_with("ppt/slides/slide") || lower.starts_with("ppt/notesslides/")) && lower.ends_with(".xml");
            if slide_xml {
                let mut data = Vec::new();
                if (&mut f).take(MAX_XML).read_to_end(&mut data).is_ok() {
                    inv.scan_xml(&String::from_utf8_lossy(&data));
                }
            }
        }
        Some(inv)
    }

    /// Count elements, animation effects and text in one slide/notes XML part. Alternate
    /// "fallback" copies (`mc:Fallback`, written for old readers) are skipped so the same object
    /// isn't counted twice.
    fn scan_xml(&mut self, xml: &str) {
        let mut rest = xml;
        let mut fallback_depth = 0usize;
        while let Some(lt) = rest.find('<') {
            rest = &rest[lt + 1..];
            let closing = rest.starts_with('/');
            if rest.starts_with('?') || rest.starts_with('!') {
                continue;
            }
            let body = if closing { &rest[1..] } else { rest };
            let end = body.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(body.len());
            let qname = &body[..end];
            let tag_end = body.find('>').unwrap_or(body.len());
            let self_closing = body[..tag_end].ends_with('/');
            if qname == "mc:Fallback" {
                if closing {
                    fallback_depth = fallback_depth.saturating_sub(1);
                } else if !self_closing {
                    fallback_depth += 1;
                }
                continue;
            }
            if closing || fallback_depth > 0 {
                continue;
            }
            let local = qname.rsplit(':').next().unwrap_or(qname);
            for (i, (want, qualified, _)) in ELEMENTS.iter().enumerate() {
                let hit = if *qualified { qname == *want } else { local == *want };
                if hit && let Some(c) = self.elements.get_mut(i) {
                    *c += 1;
                }
            }
            if body[..tag_end].contains("presetClass=") {
                self.animations += 1;
            }
            // Text runs: <a:t>text</a:t>. Count letters (entities like &amp; count as one).
            if qname == "a:t" && !self_closing {
                let content = &body[tag_end.saturating_add(1).min(body.len())..];
                let text = &content[..content.find('<').unwrap_or(content.len())];
                self.text_chars += count_chars(text);
            }
        }
    }
}

/// Non-whitespace characters, with each `&...;` entity counted as one.
fn count_chars(text: &str) -> usize {
    let mut n = 0;
    let mut in_entity = false;
    for c in text.chars() {
        if in_entity {
            if c == ';' {
                in_entity = false;
                n += 1;
            }
        } else if c == '&' {
            in_entity = true;
        } else if !c.is_whitespace() {
            n += 1;
        }
    }
    n
}

/// Which counted part kind a zip entry is (names are lower-cased).
fn part_key(name: &str) -> Option<&'static str> {
    let xml = name.ends_with(".xml");
    let key = if name.starts_with("ppt/slides/slide") && xml {
        "slides"
    } else if name.starts_with("ppt/notesslides/") && xml {
        "notes"
    } else if name.starts_with("ppt/charts/chart") && xml {
        "charts"
    } else if name.starts_with("ppt/diagrams/data") && xml {
        "diagrams"
    } else if name.starts_with("ppt/comments/") && xml {
        "comments"
    } else if name.starts_with("customxml/item") && xml && !name.contains("props") {
        "customXml"
    } else if name.starts_with("ppt/tags/") && xml {
        "tags"
    } else if name.starts_with("ppt/ink/") {
        "ink"
    } else if name.starts_with("ppt/activex/") && xml {
        "activeX"
    } else {
        return None;
    };
    Some(key)
}

/// Binary parts whose exact bytes must survive.
fn payload_category(name: &str) -> Option<&'static str> {
    if name.starts_with("ppt/media/") {
        Some("pictures or media files")
    } else if name.starts_with("ppt/embeddings/") {
        Some("embedded objects or chart data")
    } else if name.starts_with("ppt/fonts/") {
        Some("embedded fonts")
    } else if name.ends_with("vbaproject.bin") {
        Some("macros (VBA)")
    } else if name.starts_with("ppt/activex/") && name.ends_with(".bin") {
        Some("ActiveX control data")
    } else {
        None
    }
}

/// A content fingerprint (std's SipHash with fixed keys: the same bytes always give the same
/// number within one run, which is all we need).
fn hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}
