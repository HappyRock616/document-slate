//! In-app folder lists for the "full" build (GitHub / F-Droid), which has Android's "All files
//! access". The Play build has no such permission: its folder tabs open Android's picker.
//!
//! Scope, on purpose small (privacy, and what a Play reviewer expects of a document app): only the
//! shared-storage folders behind the folder tabs (Download, Documents, Pictures, DCIM, Movies,
//! Music) and their subfolders. Never `Android/`, hidden (dot) folders or files, or anything outside
//! the phone's main storage. Nothing is scanned in the background: a folder is listed when it is
//! shown, on a worker thread.
#![cfg_attr(not(target_os = "android"), allow(dead_code))] // open/save by path is Android-only

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;

use crate::home::{self, Folder};
use crate::theme::Tool;

/// The phone's main shared storage on Android.
pub const ROOT: &str = "/storage/emulated/0";

/// Refuse files bigger than this (same limit as files read through the picker).
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// One row of a folder list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Last change, seconds since 1970 (UTC).
    pub modified: u64,
    pub size: u64,
}

/// Plain-text files the Word engine reads (as text) besides its own formats.
const TEXT_EXTS: &[&str] = &["log", "json", "ini", "cfg", "conf", "yaml", "yml", "toml", "srt", "vtt", "rst", "org", "adoc", "asc", "nfo"];

/// Can one of the apps open this file? Office formats, PDF, the Word engine's formats and plain
/// text. Pictures, music and videos are not listed.
pub fn opens(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if stem.is_empty() {
        return false;
    }
    let ext = ext.to_ascii_lowercase();
    home::tool_for(name) != Tool::Word || home::WORD_EXTS.contains(&ext.as_str()) || TEXT_EXTS.contains(&ext.as_str())
}

/// The folder behind a tab, e.g. `/storage/emulated/0/Download`; None for Browse.
pub fn tab_dir(root: &Path, folder: Folder) -> Option<PathBuf> {
    folder.storage_dir().map(|d| root.join(d))
}

/// Is `path` inside one of the tab folders under `root`, with no hidden or `..` parts (and, where
/// the paths exist, no link leading out of them)?
pub fn allowed(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return false };
    let mut parts = rel.components();
    let Some(Component::Normal(first)) = parts.next() else { return false };
    let tabs = Folder::TABS.iter().filter_map(|f| f.storage_dir());
    if !tabs.into_iter().any(|t| first == t) {
        return false;
    }
    for c in rel.components() {
        match c {
            Component::Normal(s) if !s.to_string_lossy().starts_with('.') => {}
            _ => return false,
        }
    }
    // A link inside a tab folder must not lead out of it.
    match (std::fs::canonicalize(root), std::fs::canonicalize(path)) {
        (Ok(r), Ok(p)) => p.starts_with(r.join(first)),
        _ => true,
    }
}

/// List a folder: subfolders first (by name), then the files the apps open, newest first.
/// `only`: on an app's screen, just that app's files.
pub fn list(root: &Path, dir: &Path, only: Option<Tool>) -> Result<Vec<Entry>, String> {
    if !allowed(root, dir) {
        return Err("This folder isn't one Document Slate lists.".into());
    }
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for item in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = item.file_type() else { continue };
        // Links are followed only if they stay inside the tab folders.
        let path = item.path();
        let is_dir = if kind.is_symlink() {
            if !allowed(root, &path) {
                continue;
            }
            path.is_dir()
        } else {
            kind.is_dir()
        };
        if !is_dir && (!opens(&name) || only.is_some_and(|t| !home::belongs(t, &name))) {
            continue;
        }
        let meta = std::fs::metadata(&path).ok();
        let modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let size = meta.as_ref().map_or(0, |m| m.len());
        let e = Entry { name, path, is_dir, modified, size };
        if is_dir { dirs.push(e) } else { files.push(e) }
    }
    dirs.sort_by_key(|e| e.name.to_lowercase());
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    dirs.extend(files);
    Ok(dirs)
}

/// Read a listed file (opening by path). Refuses paths outside the tab folders and huge files.
pub fn read(root: &Path, path: &Path) -> Result<Vec<u8>, String> {
    if !allowed(root, path) {
        return Err("Document Slate only opens files from the folder tabs this way".into());
    }
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_BYTES {
        return Err("it is larger than 200 MB".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

/// Save in place by path. The new version is first copied next to `dest` under a hidden name (so
/// a full disk shows up before the original is touched), then written over `dest` itself, which
/// keeps the file's identity in Android's media index (a rename over it made Android restore the
/// old date). The hidden copy is removed once the original holds the new version; if that write
/// fails halfway, the hidden copy still has it.
pub fn replace_file(dest: &Path, source: &Path) -> Result<(), String> {
    let name = dest.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
    let tmp = dest.with_file_name(format!(".{name}.saving"));
    if let Err(e) = std::fs::copy(source, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    std::fs::copy(&tmp, dest).map_err(|e| format!("{e} (the new version is kept as .{name}.saving)"))?;
    if let Ok(f) = std::fs::File::options().write(true).open(dest) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

/// The picker's address for the folder holding `path` (so Save as starts there), if it is on the
/// main storage.
pub fn picker_uri(path: &str) -> Option<String> {
    let rel = Path::new(path).parent()?.strip_prefix(ROOT).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let mut out = String::from("content://com.android.externalstorage.documents/document/primary%3A");
    for b in rel.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Some(out)
}

/// Is this Recent / document address a file path (full build) rather than a content:// address?
pub fn is_path(uri: &str) -> bool {
    uri.starts_with('/')
}

/// What the folder tabs show and the worker that lists them.
#[derive(Default)]
pub struct Browser {
    /// This build can list folders itself (full flavor, Android 11+).
    pub supported: bool,
    /// "All files access" is granted.
    pub access: bool,
    /// The user chose "Use Android's picker instead" on the access card (this run).
    pub use_picker: bool,
    /// Subfolders entered below the lit tab's folder.
    pub sub: Vec<String>,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
    pub loading: bool,
    /// The folder + filter `entries` belong to (None = must list again).
    shown: Option<(PathBuf, Option<Tool>)>,
    rx: Option<mpsc::Receiver<(PathBuf, Option<Tool>, Result<Vec<Entry>, String>)>>,
}

impl Browser {
    /// Folder tabs show their content in the app (the access card, or the list).
    pub fn in_app(&self) -> bool {
        self.supported && !(self.use_picker && !self.access)
    }

    /// The folder currently shown for `folder`'s tab.
    pub fn dir(&self, folder: Folder) -> Option<PathBuf> {
        let mut d = tab_dir(Path::new(ROOT), folder)?;
        for s in &self.sub {
            d.push(s);
        }
        Some(d)
    }

    /// Forget the listing, so it's read again next time (new tab, back from a document).
    pub fn invalidate(&mut self) {
        self.shown = None;
    }

    /// Make sure `dir` is listed (starts the worker if needed) and collect a finished listing.
    pub fn want(&mut self, dir: PathBuf, only: Option<Tool>) {
        if let Some(rx) = &self.rx {
            if let Ok((d, o, res)) = rx.try_recv() {
                self.rx = None;
                self.loading = false;
                if d == dir && o == only {
                    match res {
                        Ok(v) => {
                            self.entries = v;
                            self.error = None;
                        }
                        Err(e) => {
                            self.entries.clear();
                            self.error = Some(e);
                        }
                    }
                    self.shown = Some((d, o));
                }
            }
        }
        if self.shown.as_ref() == Some(&(dir.clone(), only)) || self.rx.is_some() {
            return;
        }
        if self.shown.as_ref().is_none_or(|(d, _)| *d != dir) {
            self.entries.clear();
            self.error = None;
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.loading = true;
        std::thread::Builder::new()
            .name("list-folder".into())
            .spawn(move || {
                let res = list(Path::new(ROOT), &dir, only);
                let _ = tx.send((dir, only, res));
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> PathBuf {
        let root = std::env::temp_dir().join(format!("ds-browse-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["Download/Work", "Download/.hidden", "Documents", "Android/data", "Secret"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["Download/a.docx", "Download/b.xlsx", "Download/photo.jpg", "Download/.c.pdf", "Download/notes.txt", "Download/log.json", "Secret/x.pdf"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        root
    }

    #[test]
    fn only_tab_folders_are_allowed() {
        let root = tree();
        assert!(allowed(&root, &root.join("Download")));
        assert!(allowed(&root, &root.join("Download/Work")));
        assert!(!allowed(&root, &root));
        assert!(!allowed(&root, &root.join("Android/data")));
        assert!(!allowed(&root, &root.join("Secret")));
        assert!(!allowed(&root, &root.join("Download/.hidden")));
        assert!(!allowed(&root, &root.join("Download/../Secret")));
        assert!(!allowed(&root, Path::new("/etc")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_folders_first_and_only_documents() {
        let root = tree();
        let names: Vec<String> = list(&root, &root.join("Download"), None).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names[0], "Work");
        assert!(names.contains(&"a.docx".to_owned()));
        assert!(names.contains(&"b.xlsx".to_owned()));
        assert!(names.contains(&"notes.txt".to_owned()));
        assert!(names.contains(&"log.json".to_owned()));
        assert!(!names.iter().any(|n| n.starts_with('.') || n == "photo.jpg"));
        // An app's screen: only its files (folders stay).
        let grid: Vec<String> = list(&root, &root.join("Download"), Some(Tool::Grid)).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(grid, vec!["Work".to_owned(), "b.xlsx".to_owned()]);
        assert!(list(&root, &root.join("Secret"), None).is_err());
        assert!(read(&root, &root.join("Secret/x.pdf")).is_err());
        assert!(read(&root, &root.join("Download/a.docx")).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replace_file_swaps_in_place() {
        let root = tree();
        let dest = root.join("Download/a.docx");
        let src = root.join("new.bin");
        std::fs::write(&src, b"new contents").unwrap();
        replace_file(&dest, &src).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new contents");
        assert!(!root.join("Download/.a.docx.saving").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn picker_uri_points_at_the_folder() {
        assert_eq!(
            picker_uri("/storage/emulated/0/Download/My Work/a.docx").as_deref(),
            Some("content://com.android.externalstorage.documents/document/primary%3ADownload%2FMy%20Work")
        );
        assert_eq!(picker_uri("content://x/y"), None);
        assert!(opens("Report.DOCX") && opens("data.csv") && opens("x.pdf") && !opens("song.mp3") && !opens(".docx"));
    }
}
