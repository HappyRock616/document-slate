//! Small things remembered between launches: the preferences (Dark/Light, keep screen on, open in
//! edit mode, keep Recent, spelling language, your name) and the Recent files list.
//! Stored as `settings.json` in the app's private data folder (Android: internal storage,
//! only this app can read it). Nothing ever leaves the device.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Keep at most this many recent files.
const MAX_RECENT: usize = 30;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Recent {
    /// File name as shown to the user, e.g. "Budget 2026.docx".
    pub name: String,
    /// Where to reopen it: a content:// address on Android, a file path on desktop.
    pub uri: String,
    /// When it was last opened, seconds since 1970 (UTC).
    pub opened: u64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub dark: bool,
    pub recent: Vec<Recent>,
    /// Keep the screen on while a document is open.
    pub keep_awake: bool,
    /// Open documents straight in edit mode (instead of the viewer).
    pub open_in_edit: bool,
    /// Remember opened files in Recent (off = no history kept).
    pub keep_recent: bool,
    /// Spelling language for new documents (BCP 47, e.g. "en-US").
    pub spell_lang: String,
    /// Your name on comments and tracked changes (empty = the engine's default).
    pub author: String,
    #[serde(skip)]
    path: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark: true,
            recent: Vec::new(),
            keep_awake: false,
            open_in_edit: false,
            keep_recent: true,
            spell_lang: "en-US".into(),
            author: String::new(),
            path: PathBuf::new(),
        }
    }
}

impl Settings {
    /// Load from `dir/settings.json`; a missing or unreadable file gives the defaults.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("settings.json");
        let mut s: Settings = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        s.path = path;
        s
    }

    /// Write back. Errors are logged, not fatal: losing the recent list is not worth a crash.
    pub fn save(&self) {
        let result = serde_json::to_vec_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|b| std::fs::write(&self.path, b).map_err(|e| e.to_string()));
        if let Err(e) = result {
            log::warn!("saving settings: {e}");
        }
    }

    /// Put a file at the top of Recent (moving it if already listed).
    pub fn touch_recent(&mut self, name: &str, uri: &str) {
        if uri.is_empty() || !self.keep_recent {
            return;
        }
        // The same file reached by another route (e.g. Downloads vs. Files) gets another
        // content:// address: one entry per file name, the latest address wins.
        self.recent.retain(|r| r.uri != uri && r.name != name);
        self.recent.insert(0, Recent { name: name.to_owned(), uri: uri.to_owned(), opened: now_secs() });
        self.recent.truncate(MAX_RECENT);
        self.save();
    }

    /// Empty the Recent list.
    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.save();
    }

    pub fn forget_recent(&mut self, uri: &str) {
        self.recent.retain(|r| r.uri != uri);
        self.save();
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn ago(then: u64) -> String {
    let s = now_secs().saturating_sub(then);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => {
            let d = s / 86_400;
            if d == 1 { "yesterday".into() } else { format!("{d} days ago") }
        }
    }
}
