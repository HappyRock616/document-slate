//! Digital IDs for certificate signatures (Page Slate › Fill & Sign › Certificate, and
//! Preferences › Digital IDs).
//!
//! * The **private key** lives in Android's Keystore (the phone's hardware key vault), stored
//!   "user authentication required": it signs only right after the user unlocks with
//!   fingerprint / face / PIN / pattern / password — Android's own prompt, so the app never sees
//!   the PIN or fingerprint, and there is no app password. Keys can't be copied out of the phone.
//! * The **certificate** (public, names the signer) is kept here in `digital-ids.json` in the
//!   app's private folder, with a summary for lists.
//! * Create my ID: a self-signed ID (2048-bit RSA, valid 5 years). With "Save a backup" the key is
//!   made in memory, written to a password-protected .p12 the user saves anywhere, then moved
//!   into the Keystore. Without it the key is made inside the Keystore and never exists elsewhere.
//! * Import: a .p12 / .pfx file and its password (asked once); the key moves into the Keystore,
//!   the app keeps no copy of the file or the password.
//!
//! Desktop test builds have no Keystore: keys are kept as .p12 files under the scratch folder
//! with a fixed password, and "unlocking" always succeeds. Never shipped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, Sense};
use serde::{Deserialize, Serialize};
use slate_pdf::{DigestAlg, ExternalKey, IdSummary, SignError, SigningId};

/// One digital ID the app knows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct IdEntry {
    /// The Keystore alias of its key.
    pub alias: String,
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub organization: String,
    pub issuer: String,
    pub self_signed: bool,
    pub valid_until: String,
    pub expires_unix: i64,
    /// Certificate and chain, DER as hex.
    pub cert: String,
    #[serde(default)]
    pub chain: Vec<String>,
    /// A backup .p12 was saved when it was created / it came from a file.
    #[serde(default)]
    pub has_backup: bool,
}

impl IdEntry {
    fn new(alias: String, s: &IdSummary, cert: &[u8], chain: &[Vec<u8>], has_backup: bool) -> Self {
        IdEntry {
            alias,
            name: s.name.clone(),
            email: s.email.clone(),
            organization: s.organization.clone(),
            issuer: s.issuer.clone(),
            self_signed: s.self_signed,
            valid_until: s.valid_until.clone(),
            expires_unix: s.expires_unix,
            cert: hex(cert),
            chain: chain.iter().map(|c| hex(c)).collect(),
            has_backup,
        }
    }

    pub fn expired(&self) -> bool {
        self.expires_unix < crate::settings::now_secs() as i64
    }

    /// The ID, signing through the Keystore.
    pub fn signing_id(&self) -> Result<SigningId, String> {
        let cert = unhex(&self.cert).ok_or("the stored certificate is damaged")?;
        let chain: Vec<Vec<u8>> = self.chain.iter().filter_map(|c| unhex(c)).collect();
        slate_pdf::external_digital_id(&cert, &chain, Arc::new(KeySigner { alias: self.alias.clone() })).map_err(|e| e.to_string())
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// `digital-ids.json` in the app's private folder.
#[derive(Default)]
pub struct IdStore {
    pub ids: Vec<IdEntry>,
    path: PathBuf,
}

impl IdStore {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("digital-ids.json");
        let ids = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        IdStore { ids, path }
    }

    fn save(&self) {
        match serde_json::to_vec_pretty(&self.ids) {
            Ok(b) => {
                if let Err(e) = std::fs::write(&self.path, b) {
                    log::warn!("digital IDs: {e}");
                }
            }
            Err(e) => log::warn!("digital IDs: {e}"),
        }
    }
}

// ---------------------------------------------------------------- the key store (platform)

/// A Keystore key, used by the signing engine through [`ExternalKey`].
struct KeySigner {
    alias: String,
}

fn digest_name(alg: DigestAlg) -> Option<&'static str> {
    match alg {
        DigestAlg::Sha256 => Some("SHA-256"),
        DigestAlg::Sha384 => Some("SHA-384"),
        DigestAlg::Sha512 => Some("SHA-512"),
        _ => None,
    }
}

impl ExternalKey for KeySigner {
    fn sign(&self, alg: DigestAlg, msg: &[u8]) -> Result<Vec<u8>, SignError> {
        let digest = digest_name(alg).ok_or_else(|| SignError::Unsupported(format!("{alg:?} signatures")))?;
        keys::sign(&self.alias, digest, msg).map_err(|e| {
            SignError::Crypto(if e == "auth" { "the phone wasn't unlocked in time; try again".into() } else { e })
        })
    }
}

/// Where desktop test builds keep keys (set once at start).
static DEV_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn set_dir(dir: &Path) {
    let _ = DEV_DIR.set(dir.to_path_buf());
}

fn new_alias() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("ds-id-{n:x}")
}

#[cfg(target_os = "android")]
mod keys {
    use crate::android_bridge as b;

    fn app() -> Result<&'static winit::platform::android::activity::AndroidApp, String> {
        b::app().ok_or_else(|| "no Android context".to_string())
    }
    pub fn device_secure() -> bool {
        app().and_then(b::device_secure).unwrap_or(false)
    }
    pub fn begin_auth(title: &str, subtitle: &str) -> Result<(), String> {
        b::authenticate(app()?, title, subtitle)
    }
    pub fn poll_auth() -> Option<Result<(), String>> {
        app().ok().and_then(b::take_auth_result)
    }
    pub fn import(alias: &str, id: &slate_pdf::SigningId) -> Result<(), String> {
        let (pkcs8, cert, _) = slate_pdf::id_parts(id);
        b::ks_import(app()?, alias, &pkcs8, slate_pdf::key_family(id), &cert)
    }
    pub fn generate(alias: &str) -> Result<Vec<u8>, String> {
        b::ks_generate(app()?, alias)
    }
    pub fn sign(alias: &str, digest: &str, msg: &[u8]) -> Result<Vec<u8>, String> {
        b::ks_sign(app()?, alias, digest, msg)
    }
    pub fn delete(alias: &str) -> Result<(), String> {
        b::ks_delete(app()?, alias)
    }
    pub fn has(alias: &str) -> bool {
        app().map(|a| b::ks_has(a, alias)).unwrap_or(false)
    }
}

#[cfg(not(target_os = "android"))]
mod keys {
    //! Desktop stand-in (tests and screenshots only): .p12 files with a fixed password.
    use std::path::PathBuf;

    const PW: &str = "desktop-test-only";

    fn file(alias: &str) -> Result<PathBuf, String> {
        let dir = super::DEV_DIR.get().ok_or("no folder")?.join("dev-keys");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir.join(format!("{alias}.p12")))
    }
    pub fn device_secure() -> bool {
        true
    }
    pub fn begin_auth(_title: &str, _subtitle: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn poll_auth() -> Option<Result<(), String>> {
        Some(Ok(()))
    }
    pub fn import(alias: &str, id: &slate_pdf::SigningId) -> Result<(), String> {
        let p12 = slate_pdf::write_digital_id(id, PW).map_err(|e| e.to_string())?;
        std::fs::write(file(alias)?, p12).map_err(|e| e.to_string())
    }
    pub fn generate(alias: &str) -> Result<Vec<u8>, String> {
        let id = slate_pdf::new_self_signed_id("key", "", "", 0).map_err(|e| e.to_string())?;
        import(alias, &id)?;
        let (_, cert, _) = slate_pdf::id_parts(&id);
        slate_pdf::certificate_spki(&cert).map_err(|e| e.to_string())
    }
    fn load(alias: &str) -> Result<slate_pdf::SigningId, String> {
        let b = std::fs::read(file(alias)?).map_err(|e| e.to_string())?;
        slate_pdf::open_digital_id(&b, PW).map_err(|e| e.to_string())
    }
    pub fn sign(alias: &str, digest: &str, msg: &[u8]) -> Result<Vec<u8>, String> {
        let alg = match digest {
            "SHA-384" => slate_pdf::DigestAlg::Sha384,
            "SHA-512" => slate_pdf::DigestAlg::Sha512,
            _ => slate_pdf::DigestAlg::Sha256,
        };
        load(alias)?.key.sign(alg, msg).map_err(|e| e.to_string())
    }
    pub fn delete(alias: &str) -> Result<(), String> {
        std::fs::remove_file(file(alias)?).map_err(|e| e.to_string())
    }
    pub fn has(alias: &str) -> bool {
        file(alias).map(|f| f.exists()).unwrap_or(false)
    }
}

/// Ask for the screen lock now; then poll [`poll_unlock`]. Errors in plain words.
pub fn begin_unlock(title: &str) -> Result<(), String> {
    if !keys::device_secure() {
        return Err(NO_LOCK.into());
    }
    keys::begin_auth(title, "Confirm it's you")
}

pub fn poll_unlock() -> Option<Result<(), String>> {
    keys::poll_auth().map(|r| r.map_err(|e| match e.as_str() {
        "cancelled" => "Cancelled".to_string(),
        "nolock" => NO_LOCK.to_string(),
        other => other.to_string(),
    }))
}

const NO_LOCK: &str = "Digital IDs need a screen lock. Set a PIN, pattern or password in Android Settings › Security first.";

/// Does the Keystore still hold this ID's key? (Android deletes keys when the screen lock is removed.)
pub fn key_present(e: &IdEntry) -> bool {
    keys::has(&e.alias)
}

// ---------------------------------------------------------------- the screens

/// Who opened the manager.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Purpose {
    /// Preferences › Digital IDs: list, details, delete, export.
    #[default]
    Manage,
    /// Signing: tap an ID to use it (a new or imported ID is chosen at once).
    Choose,
}

#[derive(Default)]
struct CreateForm {
    name: String,
    email: String,
    org: String,
    backup: bool,
    pw: String,
    pw2: String,
}

enum Page {
    List,
    Create(CreateForm),
    ImportPassword { file: String, bytes: Vec<u8>, pw: String },
    ImportCheck { summary: IdSummary, id: Box<SigningId>, from_file: String },
    Detail { index: usize, confirm_delete: bool },
}

/// Something waiting on Android (prompt, picker, save screen).
enum Waiting {
    None,
    PickFile,
    Unlock(Op),
    Backup { name: String },
    Export,
}

enum Op {
    Create(CreateForm),
    Import(Box<SigningId>),
    Delete(usize),
}

pub struct IdManager {
    pub open: bool,
    purpose: Purpose,
    page: Page,
    store: IdStore,
    dir: PathBuf,
    waiting: Waiting,
    error: Option<String>,
    note: Option<String>,
    /// Choose: the alias picked, for the caller to take.
    chosen: Option<String>,
    /// Your name (preferences), to fill in a new ID.
    author_hint: String,
}

impl IdManager {
    pub fn new(dir: &Path) -> Self {
        set_dir(dir);
        IdManager {
            open: false,
            purpose: Purpose::Manage,
            page: Page::List,
            store: IdStore::load(dir),
            dir: dir.to_path_buf(),
            waiting: Waiting::None,
            error: None,
            note: None,
            chosen: None,
            author_hint: String::new(),
        }
    }

    /// Open on the list (or straight on "Create my ID" when there are none and we're choosing).
    pub fn show_for(&mut self, purpose: Purpose, author: &str) {
        self.store = IdStore::load(&self.dir);
        self.open = true;
        self.purpose = purpose;
        self.error = None;
        self.note = None;
        self.chosen = None;
        self.page = Page::List;
        if purpose == Purpose::Choose && self.store.ids.is_empty() {
            self.page = Page::Create(CreateForm { name: author.to_owned(), backup: true, ..Default::default() });
        }
        self.author_hint = author.to_owned();
    }

    pub fn ids(&mut self) -> &[IdEntry] {
        if self.store.path.as_os_str().is_empty() {
            self.store = IdStore::load(&self.dir);
        }
        &self.store.ids
    }

    pub fn reload(&mut self) {
        self.store = IdStore::load(&self.dir);
    }

    /// The ID chosen in [`Purpose::Choose`], once.
    pub fn take_chosen(&mut self) -> Option<IdEntry> {
        let a = self.chosen.take()?;
        self.store.ids.iter().find(|e| e.alias == a).cloned()
    }

    /// Back pressed: one page back, or close. False if it wasn't open.
    pub fn back(&mut self) -> bool {
        if !self.open {
            return false;
        }
        if matches!(self.page, Page::List) || (self.purpose == Purpose::Choose && self.store.ids.is_empty()) {
            self.open = false;
        } else {
            self.page = Page::List;
            self.error = None;
        }
        true
    }

    /// Draw (when open) as a card over everything.
    pub fn show(&mut self, ctx: &egui::Context, top: f32, bottom: f32) {
        if !self.open {
            return;
        }
        self.poll(ctx);
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("ids-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(140));
        });
        let width = (screen.width() - 24.0).min(520.0);
        let pos = egui::pos2(screen.center().x - width / 2.0, screen.top() + top + 16.0);
        let max_h = (screen.bottom() - bottom - pos.y - 16.0).max(240.0);
        let fill = ctx.global_style().visuals.panel_fill;
        let fg = ctx.global_style().visuals.text_color();
        let mut close = false;
        let card = egui::Area::new(egui::Id::unique("ids-card")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::new()
                .fill(fill)
                .corner_radius(16)
                .inner_margin(14)
                .shadow(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    ui.set_max_height(max_h);
                    ui.horizontal(|ui| {
                        let title = match &self.page {
                            Page::List if self.purpose == Purpose::Choose => "Sign with",
                            Page::List => "Digital IDs",
                            Page::Create(_) => "Create my digital ID",
                            Page::ImportPassword { .. } => "Import a digital ID",
                            Page::ImportCheck { .. } => "Check details",
                            Page::Detail { .. } => "Digital ID",
                        };
                        ui.label(RichText::new(title).size(18.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::chrome::bar_button(ui, "close", "Close", fg).clicked() {
                                close = true;
                            }
                        });
                    });
                    if let Some(e) = &self.error {
                        ui.label(RichText::new(e).color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
                    }
                    if let Some(n) = &self.note {
                        ui.label(RichText::new(n).weak());
                    }
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(max_h - 70.0).auto_shrink([false, true]).show(ui, |ui| {
                        if !matches!(self.waiting, Waiting::None) {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(match self.waiting {
                                    Waiting::PickFile => "Choose the .p12 / .pfx file…",
                                    Waiting::Unlock(_) => "Confirm with your screen lock…",
                                    Waiting::Backup { .. } => "Choose where to save the backup…",
                                    Waiting::Export => "Choose where to save the certificate…",
                                    Waiting::None => "",
                                });
                            });
                            return;
                        }
                        self.page_ui(ui, fg);
                    });
                });
        });
        // Keep the card above the dimmed background (egui lifts whichever layer was touched last).
        ctx.move_to_top(card.response.layer_id);
        if close {
            self.open = false;
        }
        if !matches!(self.waiting, Waiting::None) {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn page_ui(&mut self, ui: &mut egui::Ui, fg: Color32) {
        let page = std::mem::replace(&mut self.page, Page::List);
        self.page = match page {
            Page::List => self.list_ui(ui, fg),
            Page::Create(f) => self.create_ui(ui, f),
            Page::ImportPassword { file, bytes, mut pw } => {
                ui.label(format!("File: {file}"));
                ui.label(RichText::new("Enter the file's password. It's used once, to move the key into this phone's secure key store, and isn't kept.").weak());
                let r = ui.add(egui::TextEdit::singleline(&mut pw).password(true).hint_text("Password").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 9.0)));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if big_button(ui, "Continue", true).clicked() || enter {
                    match slate_pdf::open_digital_id(&bytes, &pw) {
                        Ok(id) => {
                            self.error = None;
                            let summary = slate_pdf::id_summary(&id);
                            Page::ImportCheck { summary, id: Box::new(id), from_file: file }
                        }
                        Err(slate_pdf::PdfError::WrongPassword) => {
                            self.error = Some("That password isn't right.".into());
                            Page::ImportPassword { file, bytes, pw: String::new() }
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                            Page::ImportPassword { file, bytes, pw }
                        }
                    }
                } else {
                    Page::ImportPassword { file, bytes, pw }
                }
            }
            Page::ImportCheck { summary, id, from_file } => {
                summary_ui(ui, &summary);
                let expired = summary.expires_unix < crate::settings::now_secs() as i64;
                if expired {
                    ui.label(RichText::new("This certificate has expired: signatures made with it show as invalid.").color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
                }
                if let Some(p) = &summary.problem {
                    ui.label(RichText::new(format!("It can't sign documents: {p}.")).color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
                }
                ui.label(RichText::new(format!("Keep {from_file} somewhere safe: it's your backup. This phone keeps only the key, locked to your screen lock.")).weak());
                if big_button(ui, "Import", summary.problem.is_none() && !expired).clicked() {
                    self.start_unlock("Import your digital ID", Op::Import(id));
                    Page::List
                } else {
                    Page::ImportCheck { summary, id, from_file }
                }
            }
            Page::Detail { index, mut confirm_delete } => {
                let Some(e) = self.store.ids.get(index).cloned() else { return };
                let summary = IdSummary {
                    name: e.name.clone(),
                    email: e.email.clone(),
                    organization: e.organization.clone(),
                    issuer: e.issuer.clone(),
                    self_signed: e.self_signed,
                    valid_from: String::new(),
                    valid_until: e.valid_until.clone(),
                    expires_unix: e.expires_unix,
                    problem: None,
                    fingerprint: unhex(&e.cert).and_then(|c| slate_pdf::certificate_summary(&c).ok()).map(|s| s.fingerprint).unwrap_or_default(),
                };
                summary_ui(ui, &summary);
                if !key_present(&e) {
                    ui.label(RichText::new("Its key is no longer on this phone (Android removes keys when the screen lock is turned off). Delete it and import your backup again.").color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
                } else if !e.has_backup {
                    ui.label(RichText::new("No backup: this ID can't be recovered if the phone is lost or reset.").weak());
                }
                ui.add_space(6.0);
                if big_button(ui, "Export certificate", true).on_hover_text("The public part only: send it to people so their PDF reader trusts your signatures").clicked() {
                    self.export_certificate(&e);
                }
                ui.label(RichText::new("The public part only. Send it to people so their PDF reader can trust your signatures.").weak().size(12.0));
                ui.add_space(10.0);
                if confirm_delete {
                    ui.label(RichText::new("Delete this digital ID from the phone? Documents you already signed stay signed. Without a backup it's gone for good.").strong());
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new("Delete").min_size(egui::vec2(110.0, 40.0)).fill(Color32::from_rgb(0xB0, 0x3A, 0x32))).clicked() {
                            self.start_unlock("Delete your digital ID", Op::Delete(index));
                        }
                        if ui.add(egui::Button::new("Keep").min_size(egui::vec2(110.0, 40.0))).clicked() {
                            confirm_delete = false;
                        }
                    });
                } else if ui.add(egui::Button::new("Delete…").min_size(egui::vec2(120.0, 40.0))).clicked() {
                    confirm_delete = true;
                }
                if matches!(self.waiting, Waiting::None) { Page::Detail { index, confirm_delete } } else { Page::List }
            }
        };
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, fg: Color32) -> Page {
        let mut next = Page::List;
        if self.store.ids.is_empty() {
            ui.label(RichText::new("No digital IDs yet. A digital ID lets you sign PDFs so anyone can check they weren't changed afterwards.").weak());
        }
        let ids = self.store.ids.clone();
        for (i, e) in ids.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 58.0), Sense::click());
            if resp.is_pointer_button_down_on() {
                ui.painter().rect_filled(r, 8.0, fg.gamma_multiply(0.1));
            }
            let p = ui.painter_at(r);
            p.text(r.left_top() + egui::vec2(8.0, 10.0), egui::Align2::LEFT_TOP, &e.name, egui::FontId::proportional(16.0), fg);
            let mut sub = if e.self_signed { "Self-signed".to_string() } else { format!("Issued by {}", e.issuer) };
            sub.push_str(&format!(" · until {}", e.valid_until));
            if e.expired() {
                sub = format!("Expired {}", e.valid_until);
            }
            p.text(r.left_top() + egui::vec2(8.0, 33.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(12.5), fg.gamma_multiply(0.65));
            if resp.clicked() {
                if self.purpose == Purpose::Choose {
                    if e.expired() {
                        self.error = Some("That digital ID has expired.".into());
                    } else {
                        self.chosen = Some(e.alias.clone());
                        self.open = false;
                    }
                } else {
                    next = Page::Detail { index: i, confirm_delete: false };
                }
            }
        }
        ui.add_space(8.0);
        if big_button(ui, "Create my ID", true).clicked() {
            self.error = None;
            next = Page::Create(CreateForm { name: self.author_hint.clone(), backup: true, ..Default::default() });
        }
        if big_button(ui, "Import from file (.p12, .pfx)", true).clicked() {
            self.error = None;
            self.pick_file();
        }
        next
    }

    fn create_ui(&mut self, ui: &mut egui::Ui, mut f: CreateForm) -> Page {
        ui.label(RichText::new("Shown to people who check your signature.").weak());
        let field = |ui: &mut egui::Ui, v: &mut String, hint: &str| {
            ui.add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 9.0)));
        };
        field(ui, &mut f.name, "Your name");
        field(ui, &mut f.email, "Email (optional)");
        field(ui, &mut f.org, "Organization (optional)");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            crate::prefs::switch(ui, &mut f.backup);
            ui.label("Save a backup copy");
        });
        if f.backup {
            ui.label(RichText::new("Recommended. You'll choose where to save a password-protected .p12 file (Drive, a USB stick…). It also works in Acrobat on a computer.").weak().size(12.5));
            ui.add(egui::TextEdit::singleline(&mut f.pw).password(true).hint_text("Backup password (8+ characters)").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 9.0)));
            ui.add(egui::TextEdit::singleline(&mut f.pw2).password(true).hint_text("Repeat password").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 9.0)));
        } else {
            ui.label(RichText::new("The key is made inside this phone's secure chip and never leaves it. If the phone is lost or reset, the ID is gone.").weak().size(12.5));
        }
        ui.label(RichText::new("Valid for 5 years. Self-signed: it proves a document wasn't changed after signing; readers show your identity as \"not verified\" until they choose to trust your certificate.").weak().size(12.0));
        if big_button(ui, "Create", true).clicked() {
            let problem = if f.name.trim().is_empty() {
                Some("Enter your name.")
            } else if f.backup && f.pw.chars().count() < 8 {
                Some("The backup password needs at least 8 characters.")
            } else if f.backup && f.pw != f.pw2 {
                Some("The two passwords don't match.")
            } else {
                None
            };
            match problem {
                Some(p) => self.error = Some(p.into()),
                None => {
                    self.error = None;
                    self.start_unlock("Create your digital ID", Op::Create(f));
                    return Page::List;
                }
            }
        }
        Page::Create(f)
    }

    fn start_unlock(&mut self, title: &str, op: Op) {
        match begin_unlock(title) {
            Ok(()) => self.waiting = Waiting::Unlock(op),
            Err(e) => self.error = Some(e),
        }
    }

    fn pick_file(&mut self) {
        #[cfg(target_os = "android")]
        if let Some(app) = crate::android_bridge::app() {
            match crate::android_bridge::pick_id_file(app) {
                Ok(()) => self.waiting = Waiting::PickFile,
                Err(e) => self.error = Some(e),
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            // Desktop: DS_ID_FILE=path picks a file.
            match std::env::var("DS_ID_FILE").ok().and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, b)) => self.page = Page::ImportPassword { file: p, bytes: b, pw: String::new() },
                None => self.error = Some("Desktop: set DS_ID_FILE to a .p12 file".into()),
            }
        }
    }

    fn export_certificate(&mut self, e: &IdEntry) {
        let Some(der) = unhex(&e.cert) else { return };
        let dir = self.dir.join("save");
        let _ = std::fs::create_dir_all(&dir);
        let name = format!("{}.cer", safe(&e.name));
        let tmp = dir.join(&name);
        if let Err(err) = std::fs::write(&tmp, der) {
            self.error = Some(err.to_string());
            return;
        }
        #[cfg(target_os = "android")]
        if let Some(app) = crate::android_bridge::app() {
            match crate::android_bridge::save_copy_picker(app, &name, "application/pkix-cert", &tmp) {
                Ok(()) => self.waiting = Waiting::Export,
                Err(err) => self.error = Some(err),
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.note = Some(format!("Desktop: wrote {}", tmp.display()));
        }
    }

    /// Collect answers from Android and finish the waiting step.
    fn poll(&mut self, ctx: &egui::Context) {
        match std::mem::replace(&mut self.waiting, Waiting::None) {
            Waiting::None => {}
            Waiting::PickFile => {
                #[cfg(target_os = "android")]
                match crate::android_bridge::app().and_then(crate::android_bridge::take_pending_id_file) {
                    Some((file, bytes)) => self.page = Page::ImportPassword { file, bytes, pw: String::new() },
                    None => self.waiting = Waiting::PickFile,
                }
                // A cancelled picker never answers: the card's close button / back ends the wait.
            }
            Waiting::Unlock(op) => match poll_unlock() {
                None => self.waiting = Waiting::Unlock(op),
                Some(Err(e)) => {
                    if e != "Cancelled" {
                        self.error = Some(e);
                    }
                    self.page = match op {
                        Op::Create(f) => Page::Create(f),
                        _ => Page::List,
                    };
                }
                Some(Ok(())) => self.run(op, ctx),
            },
            Waiting::Backup { name } => {
                #[cfg(target_os = "android")]
                match crate::android_bridge::app().and_then(crate::android_bridge::take_copy_result) {
                    None => self.waiting = Waiting::Backup { name },
                    Some(Ok(saved)) => self.note = Some(format!("Your digital ID is ready. Backup saved as {saved}.")),
                    Some(Err(_)) => {
                        self.note = Some(format!("Your digital ID \"{name}\" is ready, but no backup was saved: it can't be recovered if this phone is lost. To get a backup, delete it and create a new one."));
                        self.mark_no_backup(&name);
                    }
                }
                #[cfg(not(target_os = "android"))]
                let _ = name;
            }
            Waiting::Export => {
                #[cfg(target_os = "android")]
                match crate::android_bridge::app().and_then(crate::android_bridge::take_copy_result) {
                    None => self.waiting = Waiting::Export,
                    Some(Ok(saved)) => self.note = Some(format!("Certificate saved as {saved}.")),
                    Some(Err(e)) if e == "cancelled" => {}
                    Some(Err(e)) => self.error = Some(e),
                }
            }
        }
    }

    fn mark_no_backup(&mut self, name: &str) {
        if let Some(e) = self.store.ids.iter_mut().rev().find(|e| e.name == name) {
            e.has_backup = false;
        }
        self.store.save();
    }

    /// The screen lock was confirmed: do the step (the Keystore allows its keys for 30 s now).
    fn run(&mut self, op: Op, _ctx: &egui::Context) {
        let now = crate::settings::now_secs() as i64;
        let result: Result<(), String> = match op {
            Op::Create(f) => {
                let alias = new_alias();
                if f.backup {
                    (|| {
                        let id = slate_pdf::new_self_signed_id(&f.name, &f.email, &f.org, now).map_err(|e| e.to_string())?;
                        let p12 = slate_pdf::write_digital_id(&id, &f.pw).map_err(|e| e.to_string())?;
                        keys::import(&alias, &id)?;
                        let (_, cert, chain) = slate_pdf::id_parts(&id);
                        let entry = IdEntry::new(alias.clone(), &slate_pdf::id_summary(&id), &cert, &chain, true);
                        let name = entry.name.clone();
                        self.add(entry);
                        self.save_backup(&name, &p12);
                        Ok(())
                    })()
                } else {
                    (|| {
                        let spki = keys::generate(&alias)?;
                        let key = slate_pdf::external_key(&spki, Arc::new(KeySigner { alias: alias.clone() })).map_err(|e| e.to_string())?;
                        let id = slate_pdf::self_signed_for(key, &f.name, &f.email, &f.org, now).map_err(|e| e.to_string())?;
                        let (_, cert, chain) = slate_pdf::id_parts(&id);
                        self.add(IdEntry::new(alias.clone(), &slate_pdf::id_summary(&id), &cert, &chain, false));
                        self.note = Some("Your digital ID is ready.".into());
                        Ok(())
                    })()
                    .inspect_err(|_| {
                        let _ = keys::delete(&alias);
                    })
                }
            }
            Op::Import(id) => {
                let alias = new_alias();
                keys::import(&alias, &id).map(|()| {
                    let (_, cert, chain) = slate_pdf::id_parts(&id);
                    self.add(IdEntry::new(alias, &slate_pdf::id_summary(&id), &cert, &chain, true));
                    self.note = Some("Digital ID imported.".into());
                })
            }
            Op::Delete(i) => {
                if let Some(e) = self.store.ids.get(i).cloned() {
                    let _ = keys::delete(&e.alias);
                    self.store.ids.remove(i);
                    self.store.save();
                    self.note = Some(format!("Deleted {}.", e.name));
                }
                Ok(())
            }
        };
        if let Err(e) = result {
            self.error = Some(match e.as_str() {
                "nolock" => NO_LOCK.into(),
                "auth" => "The phone wasn't unlocked in time. Try again.".into(),
                other => format!("That didn't work: {other}"),
            });
        }
    }

    fn add(&mut self, e: IdEntry) {
        let alias = e.alias.clone();
        self.store.ids.push(e);
        self.store.save();
        if self.purpose == Purpose::Choose {
            self.chosen = Some(alias);
        }
    }

    fn save_backup(&mut self, name: &str, p12: &[u8]) {
        let dir = self.dir.join("save");
        let _ = std::fs::create_dir_all(&dir);
        let file = format!("{}.p12", safe(name));
        let tmp = dir.join(&file);
        if let Err(e) = std::fs::write(&tmp, p12) {
            self.error = Some(format!("The backup couldn't be written: {e}"));
            self.mark_no_backup(name);
            return;
        }
        #[cfg(target_os = "android")]
        if let Some(app) = crate::android_bridge::app() {
            match crate::android_bridge::save_copy_picker(app, &file, "application/x-pkcs12", &tmp) {
                Ok(()) => self.waiting = Waiting::Backup { name: name.to_owned() },
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    self.error = Some(format!("The backup couldn't be saved: {e}"));
                    self.mark_no_backup(name);
                }
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            self.note = Some(format!("Your digital ID is ready. Desktop: backup at {}", tmp.display()));
        }
    }
}

fn safe(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    if s.is_empty() { "Digital ID".into() } else { s }
}

fn big_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).size(15.0)).min_size(egui::vec2(ui.available_width(), 44.0)))
}

fn summary_ui(ui: &mut egui::Ui, s: &IdSummary) {
    let row = |ui: &mut egui::Ui, k: &str, v: &str| {
        if !v.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(k).weak());
                ui.label(v);
            });
        }
    };
    row(ui, "Name", &s.name);
    row(ui, "Email", &s.email);
    row(ui, "Organization", &s.organization);
    row(ui, "Issued by", if s.self_signed { "Self-signed" } else { &s.issuer });
    row(ui, "Valid from", &s.valid_from);
    row(ui, "Valid until", &s.valid_until);
    if !s.fingerprint.is_empty() {
        ui.label(RichText::new(format!("Fingerprint (SHA-256): {}", s.fingerprint)).weak().size(11.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_import_sign_delete_on_desktop_store() {
        let dir = std::env::temp_dir().join(format!("ds-ids-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let ctx = egui::Context::default();
        let mut m = IdManager::new(&dir);
        m.run(Op::Create(CreateForm { name: "Kept Inside".into(), backup: false, ..Default::default() }), &ctx);
        assert!(m.error.is_none(), "{:?}", m.error);
        m.run(Op::Create(CreateForm { name: "With Backup".into(), backup: true, pw: "longenough".into(), pw2: "longenough".into(), ..Default::default() }), &ctx);
        assert!(m.error.is_none(), "{:?}", m.error);
        assert_eq!(IdStore::load(&dir).ids.len(), 2);
        // Import the backup again.
        let p12 = std::fs::read(dir.join("save").join("With Backup.p12")).expect("backup written");
        let id = slate_pdf::open_digital_id(&p12, "longenough").expect("open backup");
        m.run(Op::Import(Box::new(id)), &ctx);
        assert_eq!(IdStore::load(&dir).ids.len(), 3);
        // Each one signs.
        for e in IdStore::load(&dir).ids {
            assert!(key_present(&e));
            let id = e.signing_id().expect("id");
            let doc = slate_pdf::PdfDoc::create_blank(612.0, 792.0, 1).expect("doc");
            let signed = doc.sign_with_id(&id, &slate_pdf::CertSignRequest::default()).expect("sign");
            let sigs = slate_pdf::PdfDoc::open_bytes(signed, None).expect("reopen").signatures();
            assert_eq!(sigs[0].signer, e.name);
            assert_ne!(sigs[0].status, slate_pdf::SigStatus::Invalid);
        }
        m.run(Op::Delete(0), &ctx);
        assert_eq!(IdStore::load(&dir).ids.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_round_trip() {
        let b = vec![0u8, 1, 0xab, 0xff];
        assert_eq!(unhex(&hex(&b)), Some(b));
        assert_eq!(unhex("abc"), None);
    }
}
