//! Full-fidelity guard (Project rule: never lose objects on save, even if editing them is limited).
//!
//! Before a save overwrites the original file, the freshly written copy is compared with the file
//! as it was opened, two ways:
//!
//! 1. **Census**: the engine reads both and counts pictures, charts, SmartArt, shapes, equations,
//!    tables, notes, comments and embedded objects. Fewer in the new file = something was lost.
//! 2. **Package parts** (DOCX/ODT are zip files): media, charts, diagrams, embedded objects and
//!    ink parts are counted straight from the zip, so even content the engine never understood is
//!    noticed if it would disappear.
//!
//! What the user deleted on purpose must not count as a loss, so the comparison is against the
//! document *as it is now in the editor* for the census (an object deleted in the editor is gone
//! from both), and the zip check only runs when the census of the editor matches the original
//! (nothing deleted) — otherwise a deleted picture would look like a lost media part.

use super::*;
use wordcraft_engine::doc::Document;
use wordcraft_engine::doc::para::InlineObject;

/// How many of each kind of thing a document holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Census {
    pub pictures: usize,
    pub charts: usize,
    pub diagrams: usize,
    pub shapes: usize,
    pub equations: usize,
    pub tables: usize,
    pub notes: usize,
    pub comments: usize,
    pub embedded: usize,
}

impl Census {
    pub fn of(doc: &Document) -> Census {
        let mut c = Census::default();
        let stories = std::iter::once(StoryRef::Body).chain(doc.parts.keys().map(|k| StoryRef::Part(*k)));
        for story in stories {
            let mut table_prefixes = std::collections::BTreeSet::new();
            for path in doc.para_paths(story) {
                if path.0.len() >= 4 {
                    // [block, row, cell, block]: the table is the path without its last three.
                    table_prefixes.insert(path.0[..path.0.len() - 3].to_vec());
                }
                let Some(p) = doc.para(story, &path) else { continue };
                for o in &p.objects {
                    c.count(o);
                }
            }
            c.tables += table_prefixes.len();
        }
        c
    }

    fn count(&mut self, o: &InlineObject) {
        match o {
            InlineObject::Image { .. } => self.pictures += 1,
            InlineObject::Graphic { graphic, .. } => {
                if graphic.chart.is_some() || graphic.kind == wordcraft_engine::doc::graphic::GraphicKind::Chart {
                    self.charts += 1
                } else {
                    self.diagrams += 1
                }
            }
            InlineObject::Shape { .. } | InlineObject::Group { .. } => self.shapes += 1,
            InlineObject::Equation { .. } => self.equations += 1,
            InlineObject::NoteRef { .. } => self.notes += 1,
            InlineObject::CommentStart { .. } => self.comments += 1,
            InlineObject::Opaque { .. } => self.embedded += 1,
            _ => {}
        }
    }

    fn rows(&self) -> [(usize, &'static str); 9] {
        [
            (self.pictures, "picture"),
            (self.charts, "chart"),
            (self.diagrams, "diagram"),
            (self.shapes, "shape"),
            (self.equations, "equation"),
            (self.tables, "table"),
            (self.notes, "footnote"),
            (self.comments, "comment"),
            (self.embedded, "embedded object"),
        ]
    }

    /// What `self` has that `after` lacks: ["1 chart", "2 pictures"].
    pub fn lost_in(&self, after: &Census) -> Vec<String> {
        self.rows()
            .iter()
            .zip(after.rows().iter())
            .filter(|((b, _), (a, _))| a < b)
            .map(|((b, what), (a, _))| plural((b - a) as u64, what))
            .collect()
    }

    /// Does `after` have at least as much of everything?
    fn covers(&self, after: &Census) -> bool {
        self.lost_in(after).is_empty()
    }
}

/// Package folders whose parts are content (not styles/settings that may legitimately be
/// rewritten): media, charts, SmartArt, OLE/embedded files, ink, ODF objects and pictures.
const CONTENT_DIRS: [&str; 7] = ["word/media/", "word/charts/", "word/diagrams/", "word/embeddings/", "word/ink/", "Pictures/", "Object "];

/// Content parts per folder in a zip package, or None when it isn't a zip (TXT, RTF, .doc…).
fn package_parts(bytes: &[u8]) -> Option<Vec<(&'static str, usize)>> {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    Some(
        CONTENT_DIRS
            .iter()
            .map(|dir| (*dir, names.iter().filter(|n| n.starts_with(dir) && !n.ends_with('/') && !n.contains("/_rels/")).count()))
            .collect(),
    )
}

impl WordViewer {
    /// Called once when a file is opened: remember what it contained.
    pub(super) fn take_census(&mut self) {
        self.census = Census::of(&self.session.doc);
    }

    /// Would replacing the original with the file just written to `path` lose content?
    /// Returns what would be lost, in words (empty = safe to save over the original).
    pub fn save_losses(&self, path: &Path) -> Vec<String> {
        let Ok(written) = std::fs::read(path) else { return vec!["the saved file couldn't be read back".into()] };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let reread = match wordcraft_engine::io::open_bytes(&name, &written) {
            Ok(doc) => doc,
            Err(e) => return vec![format!("the saved file couldn't be reopened ({e})")],
        };
        let now = Census::of(&self.session.doc);
        let mut lost = now.lost_in(&Census::of(&reread));
        // Package parts: only meaningful when nothing was deleted in the editor (a deleted
        // picture legitimately removes its media part).
        if lost.is_empty() && self.census.covers(&now) {
            if let (Some(before), Some(after)) = (package_parts(&self.bytes), package_parts(&written)) {
                for ((dir, b), (_, a)) in before.iter().zip(after.iter()) {
                    if a < b {
                        lost.push(format!("{} from {}", plural((b - a) as u64, "file"), dir.trim_end_matches('/')));
                    }
                }
            }
        }
        lost
    }
}
