//! Certificate signatures and flattening in Page Slate.
//!
//! * Fill & Sign › **Certificate**: Fill & Sign marks are flattened first (with a warning, as a
//!   certificate signature locks the file), then drag a box on the page (or tap an empty
//!   signature field, or choose Invisible), then one card: digital ID, reason, location, lock.
//!   **Sign** asks for the screen lock (the key is in Android's Keystore), signs, and saves the
//!   signed file as a new copy "… (signed).pdf" that becomes the open document.
//! * **Save** with Fill & Sign marks or comments asks: Save (editable) or Save flattened.
//!   **Save flattened** warns that it can't be undone and offers a flattened copy (the open
//!   document stays editable) or flattening this file.

use eframe::egui::{self, Color32, RichText};
use slate_pdf::{CertSignRequest, SigStatus, ViewRect};

use super::tools::TouchTool;
use super::PdfScreen;
use crate::chrome;
use crate::digital_ids::{IdEntry, IdManager, Purpose};
use crate::edit_bar::FileAction;

const REASONS: [&str; 5] = ["", "I approve this document", "I am the author of this document", "I have reviewed this document", "I agree to the terms"];

/// Where the signature goes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Rect(usize, ViewRect),
    Field(String),
    Invisible,
}

pub(super) struct Details {
    target: Target,
    id: Option<IdEntry>,
    reason: usize,
    location: String,
    lock: bool,
    use_drawing: bool,
}

pub(super) enum Stage {
    None,
    /// "Your Fill & Sign marks become permanent" before signing.
    WarnBeforeSign,
    /// Drag a box / tap a field (TouchTool::CertBox is on).
    Place,
    Details(Details),
    Unlocking(Details),
    /// Save pressed with marks or comments: Save or Save flattened?
    SaveChoice,
    /// Save flattened: what, and copy or this file.
    FlattenSave { all: bool },
    /// The document's signatures and whether they're valid (viewer ⋮ › Signatures).
    List,
}

pub(super) struct CertState {
    pub stage: Stage,
    ids: Option<IdManager>,
    /// The next `write_to` writes these signed bytes (and `mark_saved` then marks them signed).
    pub sign_pending: Option<Vec<u8>>,
    /// Set by the last `write_to` that wrote signed bytes.
    pub wrote_signed: bool,
    /// The next `write_to` writes a flattened copy (true = everything, false = Fill & Sign only).
    pub flatten_copy: Option<bool>,
    /// An action for the shell, handed over by the next `edit_bar` call.
    pub action: Option<FileAction>,
    /// The next Save goes straight through (chosen in the Save card / File tab).
    pub save_direct: bool,
    /// The app's private folder (where digital-ids.json lives), learned on the first frame.
    pub dir: Option<std::path::PathBuf>,
}

impl Default for CertState {
    fn default() -> Self {
        CertState { stage: Stage::None, ids: None, sign_pending: None, wrote_signed: false, flatten_copy: None, action: None, save_direct: false, dir: None }
    }
}

impl PdfScreen {
    fn id_manager(&mut self, scratch: &std::path::Path) -> &mut IdManager {
        self.cert.ids.get_or_insert_with(|| IdManager::new(scratch))
    }

    /// Fill & Sign › Certificate.
    pub(super) fn start_cert_sign(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        if let Some(r) = doc.read_only_reason() {
            self.say(format!("Can't sign: {r}"));
            return;
        }
        self.e.selected = None;
        self.e.entry = None;
        self.cert.stage = if doc.fill_sign_count() > 0 { Stage::WarnBeforeSign } else { self.place_stage() };
    }

    fn place_stage(&mut self) -> Stage {
        self.e.tool = TouchTool::CertBox;
        Stage::Place
    }

    /// The box was dragged / a signature field tapped / Invisible chosen.
    pub(super) fn cert_target(&mut self, target: Target) {
        self.e.tool = TouchTool::None;
        let id = self.cert.dir.clone().map(|d| crate::digital_ids::IdStore::load(&d)).and_then(|s| s.ids.into_iter().find(|e| !e.expired()));
        let has_drawing = self.sigs.list.iter().any(|s| matches!(s, super::sig::SavedSig::Drawn { .. }));
        self.cert.stage = Stage::Details(Details { target, id, reason: 0, location: String::new(), lock: false, use_drawing: has_drawing });
    }

    /// The shell's Save: ask first when there's something that could be flattened.
    pub(super) fn intercept_save_choice(&mut self) -> bool {
        if std::mem::take(&mut self.cert.save_direct) {
            return false;
        }
        let Some(doc) = self.doc.as_ref() else { return false };
        if !doc.is_dirty() || doc.flattenable_count() == 0 {
            return false;
        }
        self.cert.stage = Stage::SaveChoice;
        true
    }

    pub(super) fn cert_back(&mut self) -> bool {
        if let Some(m) = self.cert.ids.as_mut() {
            if m.back() {
                return true;
            }
        }
        match self.cert.stage {
            Stage::None => false,
            Stage::Unlocking(_) => true,
            _ => {
                if self.e.tool == TouchTool::CertBox {
                    self.e.tool = TouchTool::None;
                }
                self.cert.stage = Stage::None;
                true
            }
        }
    }

    /// Draw the open card (and the ID manager), each frame.
    pub(super) fn cert_cards(&mut self, ctx: &egui::Context, area: egui::Rect, scratch: &std::path::Path, top: f32, bottom: f32) {
        if self.cert.dir.is_none() {
            self.cert.dir = Some(scratch.to_path_buf());
        }
        // The ID manager first: when it closes with a choice, the details card picks it up.
        if let Some(m) = self.cert.ids.as_mut() {
            m.show(ctx, top, bottom);
            if let Some(chosen) = m.take_chosen() {
                if let Stage::Details(d) = &mut self.cert.stage {
                    d.id = Some(chosen);
                }
            }
            if m.open {
                return;
            }
        }
        let stage = std::mem::replace(&mut self.cert.stage, Stage::None);
        let gap = if self.editing { 8.0 } else { 70.0 };
        self.cert.stage = match stage {
            Stage::None => Stage::None,
            Stage::Place => Stage::Place,
            Stage::WarnBeforeSign => {
                let n = self.doc.as_ref().map_or(0, |d| d.fill_sign_count());
                let mut next = Some(Stage::WarnBeforeSign);
                let closed = card(ctx, area, gap, "Sign with a certificate", |ui| {
                    ui.label(format!(
                        "Your {n} Fill & Sign item{} (typed text, ticks, signatures) will be flattened into the page first, so the certificate covers exactly what you see.",
                        if n == 1 { "" } else { "s" }
                    ));
                    ui.label(RichText::new("Flattened marks can't be edited or removed after signing.").strong());
                    ui.add_space(6.0);
                    if wide(ui, "Flatten and continue", true).clicked() {
                        next = None;
                    }
                    if wide(ui, "Cancel", false).clicked() {
                        next = Some(Stage::None);
                    }
                });
                match next {
                    _ if closed => Stage::None,
                    Some(s) => s,
                    None => {
                        if self.edit(|d| d.flatten_fill_sign()) {
                            self.v.clear_textures = true;
                            self.place_stage()
                        } else {
                            Stage::None
                        }
                    }
                }
            }
            Stage::Details(mut d) => {
                let mut sign = false;
                let mut choose = false;
                let mut cancel = false;
                let closed = card(ctx, area, gap, "Sign with a certificate", |ui| {
                    ui.label(RichText::new(match &d.target {
                        Target::Rect(p, _) => format!("Visible signature on page {}", p + 1),
                        Target::Field(f) => format!("In the signature field \"{f}\""),
                        Target::Invisible => "Invisible signature (shown in the signature list, not on the page)".into(),
                    }).weak());
                    ui.add_space(4.0);
                    let id_label = match &d.id {
                        Some(e) => format!("{}  ›", e.name),
                        None => "Choose a digital ID  ›".into(),
                    };
                    ui.label(RichText::new("Digital ID").weak().size(12.0));
                    if wide(ui, &id_label, false).clicked() {
                        choose = true;
                    }
                    ui.label(RichText::new("Reason").weak().size(12.0));
                    egui::ComboBox::from_id_salt("cert-reason")
                        .selected_text(if d.reason == 0 { "None" } else { REASONS[d.reason] })
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            ui.style_mut().spacing.button_padding = egui::vec2(12.0, 10.0);
                            for (i, r) in REASONS.iter().enumerate() {
                                ui.selectable_value(&mut d.reason, i, if i == 0 { "None" } else { r });
                            }
                        });
                    ui.add(egui::TextEdit::singleline(&mut d.location).hint_text("Location (optional)").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 9.0)));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        crate::prefs::switch(ui, &mut d.lock);
                        ui.label("Lock the document");
                    });
                    if d.lock {
                        ui.label(RichText::new("Certifies you as the author: afterwards only form filling and further signatures are allowed.").weak().size(12.0));
                    }
                    let has_drawing = self.sigs.list.iter().any(|s| matches!(s, super::sig::SavedSig::Drawn { .. }));
                    if has_drawing && !matches!(d.target, Target::Invisible) {
                        ui.horizontal(|ui| {
                            crate::prefs::switch(ui, &mut d.use_drawing);
                            ui.label("Show my drawn signature");
                        });
                    }
                    ui.add_space(6.0);
                    if wide(ui, "Sign", true).clicked() {
                        sign = true;
                    }
                    ui.label(RichText::new("Saves a signed copy. You'll confirm with your fingerprint or screen lock.").weak().size(12.0));
                    if wide(ui, "Cancel", false).clicked() {
                        cancel = true;
                    }
                });
                if choose {
                    let author = self.author.clone();
                    self.id_manager(scratch).show_for(Purpose::Choose, &author);
                    Stage::Details(d)
                } else if closed || cancel {
                    Stage::None
                } else if sign {
                    if d.id.is_none() {
                        let author = self.author.clone();
                        self.id_manager(scratch).show_for(Purpose::Choose, &author);
                        Stage::Details(d)
                    } else {
                        match crate::digital_ids::begin_unlock("Sign with your digital ID") {
                            Ok(()) => Stage::Unlocking(d),
                            Err(e) => {
                                self.say(e);
                                Stage::Details(d)
                            }
                        }
                    }
                } else {
                    Stage::Details(d)
                }
            }
            Stage::Unlocking(d) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
                match crate::digital_ids::poll_unlock() {
                    None => Stage::Unlocking(d),
                    Some(Err(e)) => {
                        if e != "Cancelled" {
                            self.say(e);
                        }
                        Stage::Details(d)
                    }
                    Some(Ok(())) => match self.sign_now(&d) {
                        Ok(()) => Stage::None,
                        Err(e) => {
                            self.say(format!("Signing failed: {e}"));
                            Stage::Details(d)
                        }
                    },
                }
            }
            Stage::List => {
                let rows = self.doc.as_ref().map(|d| d.signatures()).unwrap_or_default();
                let mut go = None;
                let closed = card(ctx, area, gap, &format!("Signatures ({})", rows.iter().filter(|r| r.signed).count()), |ui| {
                    if rows.is_empty() {
                        ui.label(RichText::new("This PDF has no signatures. Fill & Sign › Certificate signs it with your digital ID.").weak());
                    }
                    for r in &rows {
                        let (color, mark) = match r.status {
                            SigStatus::Valid => (Color32::from_rgb(0x2E, 0x8B, 0x57), ()),
                            SigStatus::Unknown => (Color32::from_rgb(0xD9, 0xA4, 0x2B), ()),
                            SigStatus::Invalid => (Color32::from_rgb(0xE0, 0x6C, 0x5F), ()),
                            SigStatus::Unsigned => (ui.visuals().weak_text_color(), ()),
                        };
                        ui.horizontal(|ui| {
                            let _ = mark;
                            let (dot, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot.center(), 6.0, color);
                            ui.vertical(|ui| {
                                let who = if r.signed { format!("{}{}", r.signer, if r.certified { " (certified)" } else { "" }) } else { r.field.clone() };
                                ui.label(RichText::new(who).strong());
                                ui.label(RichText::new(&r.summary).color(color));
                                let mut sub = r.date.clone();
                                if !r.reason.is_empty() {
                                    sub = format!("{sub} · {}", r.reason);
                                }
                                if let Some(p) = r.page {
                                    sub = format!("{sub} · page {}", p + 1);
                                }
                                ui.label(RichText::new(sub).weak().size(12.0));
                                for d in &r.details {
                                    ui.label(RichText::new(d).weak().size(12.0));
                                }
                            });
                        });
                        if let Some(p) = r.page {
                            if ui.small_button("Show").clicked() {
                                go = Some(p);
                            }
                        }
                        ui.separator();
                    }
                    ui.label(RichText::new("\"Validity unknown\" is normal for self-signed IDs: the document is intact, but no authority vouches for who signed it.").weak().size(11.5));
                });
                if let Some(p) = go {
                    self.v.jump_to = Some((p, 0.0));
                    self.v.current_page = p;
                    Stage::None
                } else if closed {
                    Stage::None
                } else {
                    Stage::List
                }
            }
            Stage::SaveChoice => {
                let mut next = Some(Stage::SaveChoice);
                let closed = card(ctx, area, gap, "Save", |ui| {
                    if wide(ui, "Save", true).clicked() {
                        self.cert.save_direct = true;
                        self.cert.action = Some(FileAction::Save);
                        next = Some(Stage::None);
                    }
                    ui.label(RichText::new("Signatures, filled-in text and comments stay editable.").weak().size(12.5));
                    ui.add_space(8.0);
                    if wide(ui, "Save flattened…", false).clicked() {
                        next = None;
                    }
                    ui.label(RichText::new("Makes them part of the page: they can't be edited or removed after saving.").weak().size(12.5));
                });
                match next {
                    _ if closed => Stage::None,
                    Some(s) => s,
                    None => Stage::FlattenSave { all: false },
                }
            }
            Stage::FlattenSave { mut all } => {
                let mut next = None;
                let closed = card(ctx, area, gap, "Save flattened", |ui| {
                    ui.label(RichText::new("Flattening can't be undone once saved.").strong().color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
                    ui.label("Flattened items become part of the page drawing: they can't be selected, edited or removed by anyone, in any app.");
                    ui.add_space(6.0);
                    ui.radio_value(&mut all, false, "Fill & Sign items only (typed text, ticks, signatures)");
                    ui.radio_value(&mut all, true, "Everything (also comments and form fields)");
                    ui.add_space(6.0);
                    if wide(ui, "Save a flattened copy (recommended)", true).clicked() {
                        next = Some(1);
                    }
                    ui.label(RichText::new("The document you have open stays editable.").weak().size(12.0));
                    if wide(ui, "Flatten this file", false).clicked() {
                        next = Some(2);
                    }
                    if wide(ui, "Cancel", false).clicked() {
                        next = Some(0);
                    }
                });
                match next {
                    _ if closed => Stage::None,
                    None => Stage::FlattenSave { all },
                    Some(0) => Stage::None,
                    Some(1) => {
                        self.cert.flatten_copy = Some(all);
                        self.cert.action = Some(FileAction::SaveCopy { suffix: " (flattened)", adopt: false });
                        Stage::None
                    }
                    Some(_) => {
                        if self.edit(|doc| if all { doc.flatten_all() } else { doc.flatten_fill_sign() }) {
                            self.v.clear_textures = true;
                            self.cert.save_direct = true;
                            self.cert.action = Some(FileAction::Save);
                        }
                        Stage::None
                    }
                }
            }
        };
    }

    /// The screen lock was confirmed: sign and hand the signed bytes to the shell's Save as.
    fn sign_now(&mut self, d: &Details) -> Result<(), String> {
        let entry = d.id.as_ref().ok_or("choose a digital ID")?;
        let id = entry.signing_id()?;
        let drawn = if d.use_drawing && !matches!(d.target, Target::Invisible) {
            self.sigs.list.iter().find_map(|s| match s {
                super::sig::SavedSig::Drawn { strokes, w, h } => {
                    let w = f64::from(w.max(1.0));
                    let h = f64::from(*h);
                    Some(strokes.iter().map(|st| st.iter().map(|p| [f64::from(p[0]) / w, (h - f64::from(p[1])) / w]).collect()).collect::<Vec<Vec<[f64; 2]>>>())
                }
                _ => None,
            })
        } else {
            None
        };
        let mut req = CertSignRequest {
            reason: (d.reason > 0).then(|| REASONS[d.reason].to_owned()),
            location: Some(d.location.clone()),
            certify: d.lock.then_some(2),
            drawn,
            ..CertSignRequest::default()
        };
        match &d.target {
            Target::Rect(p, r) => {
                req.page = *p;
                req.rect = Some(*r);
            }
            Target::Field(f) => req.field = Some(f.clone()),
            Target::Invisible => {}
        }
        let doc = self.doc.as_ref().ok_or("no document")?;
        let bytes = doc.sign_with_id(&id, &req).map_err(|e| e.to_string())?;
        self.cert.sign_pending = Some(bytes);
        self.cert.action = Some(FileAction::SaveCopy { suffix: " (signed)", adopt: true });
        Ok(())
    }

    /// `write_to` for the special saves. `None`: an ordinary save.
    pub(super) fn cert_write(&mut self, path: &std::path::Path) -> Option<Result<Vec<String>, String>> {
        self.cert.wrote_signed = false;
        if let Some(bytes) = self.cert.sign_pending.take() {
            let r = std::fs::write(path, &bytes).map_err(|e| e.to_string());
            self.cert.wrote_signed = r.is_ok();
            self.record_written(path, bytes);
            return Some(r.map(|()| Vec::new()));
        }
        let all = self.cert.flatten_copy.take()?;
        let doc = self.doc.as_mut()?;
        let r = (|| {
            let before = doc.generation();
            if all { doc.flatten_all() } else { doc.flatten_fill_sign() }.map_err(|e| e.to_string())?;
            let out = doc.save_bytes().map_err(|e| e.to_string());
            // Take the flatten back: the open document stays editable. Only when the flatten was
            // an edit at all: with nothing to bake the engine adds no undo step, and an Undo here
            // would take back the user's own last edit instead.
            if doc.generation() != before {
                let _ = doc.undo();
            }
            std::fs::write(path, out?.bytes).map_err(|e| e.to_string())
        })();
        self.cache.generation = None;
        Some(r.map(|()| Vec::new()))
    }
}

/// A rounded card under the bars with a title and close button (same look as the other sheets).
fn card(ctx: &egui::Context, area: egui::Rect, top_gap: f32, title: &str, body: impl FnOnce(&mut egui::Ui)) -> bool {
    let width = (area.width() - 16.0).min(520.0);
    let pos = egui::pos2(area.center().x - width / 2.0, area.top() + top_gap);
    let max_h = (area.bottom() - pos.y - 12.0).max(220.0);
    let mut close = false;
    let fill = ctx.global_style().visuals.panel_fill;
    let text = ctx.global_style().visuals.text_color();
    egui::Area::new(egui::Id::unique("pdf-cert-card")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(16)
            .inner_margin(14)
            .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(90) })
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.set_max_height(max_h);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if chrome::bar_button(ui, "close", "Close", text).clicked() {
                            close = true;
                        }
                    });
                });
                egui::ScrollArea::vertical().max_height(max_h - 64.0).auto_shrink([false, true]).show(ui, body);
            });
    });
    close
}

/// A full-width button; `primary` gets the accent-ish fill.
fn wide(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let text = RichText::new(label).size(15.0);
    let text = if primary { text.color(Color32::WHITE) } else { text };
    let mut b = egui::Button::new(text).min_size(egui::vec2(ui.available_width(), 44.0));
    if primary {
        b = b.fill(Color32::from_rgb(0xC2, 0x42, 0x3A));
    }
    ui.add(b)
}
