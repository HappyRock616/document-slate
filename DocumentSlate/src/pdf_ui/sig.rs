//! Saved signatures (Fill & Sign › Sign): kept in the app's private folder so they can be reused
//! in any PDF. Never leaves the phone.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum SavedSig {
    /// Finger strokes, moved so the drawing starts at (0, 0); `w` × `h` is its size (pad points).
    Drawn { strokes: Vec<Vec<[f32; 2]>>, w: f32, h: f32 },
    /// A typed name drawn in the bundled script font.
    Typed { name: String, initials: bool },
}

impl SavedSig {
    /// Build a drawn signature from pad strokes (any position); None if nothing was drawn.
    pub fn drawn(strokes: &[Vec<[f32; 2]>]) -> Option<Self> {
        let pts = strokes.iter().flatten();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in pts {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        if x0 > x1 || (x1 - x0).max(y1 - y0) < 4.0 {
            return None;
        }
        let strokes: Vec<Vec<[f32; 2]>> = strokes.iter().filter(|s| s.len() > 1).map(|s| s.iter().map(|p| [p[0] - x0, p[1] - y0]).collect()).collect();
        if strokes.is_empty() {
            return None;
        }
        Some(SavedSig::Drawn { strokes, w: (x1 - x0).max(1.0), h: (y1 - y0).max(1.0) })
    }

    pub fn label(&self, i: usize) -> String {
        match self {
            SavedSig::Drawn { .. } => format!("Signature {} (drawn)", i + 1),
            SavedSig::Typed { name, initials: true } => format!("Initials: {name}"),
            SavedSig::Typed { name, .. } => format!("Typed: {name}"),
        }
    }
}

#[derive(Default)]
pub(super) struct Store {
    path: Option<PathBuf>,
    pub list: Vec<SavedSig>,
}

impl Store {
    /// Read the saved signatures once (from `<app folder>/page-slate/signatures.json`).
    pub fn load(&mut self, scratch: &Path) {
        if self.path.is_some() {
            return;
        }
        let path = scratch.join("page-slate").join("signatures.json");
        self.list = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        self.path = Some(path);
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_vec(&self.list) {
            Ok(b) => {
                if let Err(e) = std::fs::write(path, b) {
                    log::warn!("saving signatures: {e}");
                }
            }
            Err(e) => log::warn!("saving signatures: {e}"),
        }
    }

    /// Add one; returns its index.
    pub fn add(&mut self, sig: SavedSig) -> usize {
        if let Some(i) = self.list.iter().position(|s| *s == sig) {
            return i;
        }
        self.list.push(sig);
        self.save();
        self.list.len() - 1
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.list.len() {
            self.list.remove(i);
            self.save();
        }
    }
}
