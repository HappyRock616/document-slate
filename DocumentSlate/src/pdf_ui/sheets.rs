//! Sheets that slide over the pages: Outline, Page thumbnails, Comments, Signature pad,
//! Properties, Password, and the choice list of a form field.

use eframe::egui::{self, Color32, Rect, RichText, Sense, Stroke};
use slate_pdf::{Bookmark, FormField, PropertyKey};

use super::sig::SavedSig;
use super::tools::{kind_name, TouchTool};
use super::PdfScreen;
use crate::chrome;
use crate::doc_screen::ScreenCtx;

pub(super) enum Sheet {
    None,
    /// (depth, title, page, web address)
    Outline(Vec<(usize, String, Option<usize>, Option<String>)>),
    Thumbs,
    Comments { reply: Option<(usize, usize)>, text: String },
    SignPad { typed: bool, strokes: Vec<Vec<[f32; 2]>>, name: String, initials: bool },
    Properties { title: String, author: String, subject: String, keywords: String },
    Password { pw: String, pw2: String, owner: String },
    Choice(FormField),
}

impl Sheet {
    pub fn comments() -> Self {
        Sheet::Comments { reply: None, text: String::new() }
    }
    pub fn sign_pad(typed: bool) -> Self {
        Sheet::SignPad { typed, strokes: Vec::new(), name: String::new(), initials: false }
    }
}

fn flatten(items: &[Bookmark], depth: usize, out: &mut Vec<(usize, String, Option<usize>, Option<String>)>) {
    for b in items {
        if out.len() > 2000 {
            return;
        }
        out.push((depth, b.title.clone(), b.page, b.uri.clone()));
        flatten(&b.children, depth + 1, out);
    }
}

/// Draw a sheet: a rounded card under the bars with a title and a close button.
/// Returns whether it was closed.
fn card(ctx: &egui::Context, area: Rect, top_gap: f32, title: &str, body: impl FnOnce(&mut egui::Ui, f32)) -> bool {
    let width = (area.width() - 16.0).min(560.0);
    let pos = egui::pos2(area.center().x - width / 2.0, area.top() + top_gap);
    let max_h = (area.bottom() - pos.y - 12.0).max(220.0);
    let mut close = false;
    let fill = ctx.global_style().visuals.panel_fill;
    let text = ctx.global_style().visuals.text_color();
    egui::Area::new(egui::Id::unique("pdf-sheet")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(16)
            .inner_margin(12)
            .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(90) })
            .show(ui, |ui| {
                ui.set_width(width - 24.0);
                ui.set_max_height(max_h);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if chrome::bar_button(ui, "close", "Close", text).clicked() {
                            close = true;
                        }
                    });
                });
                body(ui, max_h - 60.0);
            });
    });
    close
}

impl PdfScreen {
    pub(super) fn open_outline(&mut self) {
        let mut list = Vec::new();
        if let Some(d) = self.doc.as_ref() {
            flatten(&d.outline(), 0, &mut list);
        }
        self.sheet = Sheet::Outline(list);
    }

    pub(super) fn properties_sheet(&self) -> Sheet {
        let p = &self.cache.props;
        Sheet::Properties {
            title: p.title.clone().unwrap_or_default(),
            author: p.author.clone().unwrap_or_default(),
            subject: p.subject.clone().unwrap_or_default(),
            keywords: p.keywords.clone().unwrap_or_default(),
        }
    }

    /// Draw the open sheet (if any).
    pub(super) fn sheets(&mut self, ctx: &egui::Context, area: Rect, cx: &ScreenCtx) {
        // In viewer mode the floating bar covers the top of the page area.
        let gap = if self.editing { 8.0 } else { 70.0 };
        let accent = cx.accent;
        let sheet = std::mem::replace(&mut self.sheet, Sheet::None);
        let mut keep = true;
        let next = match sheet {
            Sheet::None => Sheet::None,
            Sheet::Outline(list) => {
                let mut go = None;
                keep = !card(ctx, area, gap, "Outline", |ui, h| {
                    if list.is_empty() {
                        ui.label(RichText::new("This PDF has no bookmarks.").weak());
                    }
                    egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
                        for (i, (depth, title, page, _uri)) in list.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add_space((*depth).min(6) as f32 * 16.0);
                                let size = if *depth == 0 { 16.0 } else { 15.0 };
                                let label = match page {
                                    Some(p) => format!("{title}   ·  p. {}", p + 1),
                                    None => title.clone(),
                                };
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), Sense::click());
                                let fg = ui.visuals().text_color();
                                if resp.is_pointer_button_down_on() {
                                    ui.painter().rect_filled(r, 6.0, fg.gamma_multiply(0.12));
                                }
                                ui.painter_at(r).text(r.left_center() + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(size), fg);
                                if resp.clicked() {
                                    go = Some(i);
                                }
                            });
                        }
                    });
                });
                if let Some((_, _, page, uri)) = go.and_then(|i| list.get(i)) {
                    if let Some(p) = page {
                        self.v.jump_to = Some((*p, 0.0));
                        self.v.current_page = *p;
                    } else if let Some(u) = uri {
                        crate::links::show_link(u);
                    }
                    keep = false;
                }
                Sheet::Outline(list)
            }
            Sheet::Thumbs => {
                let mut go = None;
                let n = self.page_count();
                let cur = self.v.current_page;
                let mut wanted = Vec::new();
                let thumbs: Vec<Option<egui::TextureId>> = self.v.thumbs.iter().map(|t| t.as_ref().map(|t| t.tex.id())).collect();
                let sizes: Vec<(f32, f32)> = self.cache.pages.iter().map(|p| (p.width, p.height)).collect();
                let labels: Vec<String> = self.cache.pages.iter().map(|p| p.label.clone()).collect();
                keep = !card(ctx, area, gap, &format!("Pages ({n})"), |ui, h| {
                    egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                            for i in 0..n {
                                let (w, hgt) = sizes[i];
                                let tw = 96.0_f32;
                                let th = (tw * hgt / w.max(1.0)).clamp(40.0, 150.0);
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(tw + 8.0, th + 26.0), Sense::click());
                                let img = Rect::from_min_size(r.min + egui::vec2(4.0, 4.0), egui::vec2(tw, th));
                                if ui.is_rect_visible(r) {
                                    wanted.push(i);
                                }
                                let p = ui.painter();
                                if i == cur {
                                    p.rect_filled(r, 8.0, accent.gamma_multiply(0.3));
                                }
                                p.rect_filled(img, 0.0, Color32::WHITE);
                                if let Some(Some(t)) = thumbs.get(i) {
                                    p.image(*t, img, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                                }
                                p.rect_stroke(img, 0.0, Stroke::new(1.0, Color32::from_black_alpha(80)), egui::StrokeKind::Middle);
                                let label = if labels[i] == (i + 1).to_string() { labels[i].clone() } else { format!("{} ({})", i + 1, labels[i]) };
                                p.text(egui::pos2(r.center().x, r.bottom() - 10.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), ui.visuals().text_color());
                                if resp.clicked() {
                                    go = Some(i);
                                }
                            }
                        });
                    });
                });
                for i in wanted {
                    self.request_thumb(i);
                }
                if let Some(p) = go {
                    self.v.jump_to = Some((p, 0.0));
                    self.v.current_page = p;
                    keep = false;
                }
                Sheet::Thumbs
            }
            Sheet::Comments { mut reply, mut text } => {
                let list: Vec<_> = self.cache.comments.clone();
                let mut go = None;
                let mut delete = None;
                let mut send = None;
                keep = !card(ctx, area, gap, &format!("Comments ({})", list.iter().filter(|c| !c.is_reply).count()), |ui, h| {
                    if list.is_empty() {
                        ui.label(RichText::new("No comments yet. Annotate › Highlight, Note, Pen… add some.").weak());
                    }
                    egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
                        for c in &list {
                            let key = (c.page, c.index);
                            ui.horizontal(|ui| {
                                if c.is_reply {
                                    ui.add_space(20.0);
                                    ui.label("↳");
                                }
                                if let Some(col) = c.color {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                                    ui.painter().circle_filled(r.center(), 5.0, Color32::from_rgb((col[0] * 255.0) as u8, (col[1] * 255.0) as u8, (col[2] * 255.0) as u8));
                                }
                                let who = if c.author.is_empty() { String::new() } else { format!(" · {}", c.author) };
                                ui.label(RichText::new(format!("p. {} · {}{who}", c.page + 1, kind_name(&c.kind))).strong());
                            });
                            if !c.text.is_empty() {
                                ui.label(&c.text);
                            }
                            ui.horizontal(|ui| {
                                let b = |t: &str| egui::Button::new(t).min_size(egui::vec2(64.0, 34.0));
                                if ui.add(b("Go to")).clicked() {
                                    go = Some((c.page, c.rect[1]));
                                }
                                if !self.editing {
                                    return;
                                }
                                if ui.add(b("Reply")).clicked() {
                                    reply = if reply == Some(key) { None } else { Some(key) };
                                    text.clear();
                                }
                                if !c.locked && ui.add(b("Delete")).clicked() {
                                    delete = Some(key);
                                }
                            });
                            if reply == Some(key) {
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut text).hint_text("Your reply").desired_width(ui.available_width() - 80.0));
                                    if ui.add(egui::Button::new("Send").fill(accent.gamma_multiply(0.35)).min_size(egui::vec2(64.0, 34.0))).clicked() && !text.trim().is_empty() {
                                        send = Some((key, text.clone()));
                                    }
                                });
                            }
                            ui.separator();
                        }
                    });
                });
                if let Some((p, y)) = go {
                    self.v.jump_to = Some((p, (y - 40.0).max(0.0)));
                    self.v.current_page = p;
                    keep = false;
                }
                if let Some((p, i)) = delete {
                    self.edit(|d| d.delete_comment(p, i));
                    reply = None;
                }
                if let Some(((p, i), t)) = send {
                    if self.edit(|d| d.reply_to_comment(p, i, &t)) {
                        self.say("Reply added");
                    }
                    reply = None;
                    text.clear();
                }
                Sheet::Comments { reply, text }
            }
            Sheet::SignPad { mut typed, mut strokes, mut name, mut initials } => {
                let mut save: Option<SavedSig> = None;
                keep = !card(ctx, area, gap, "Signature", |ui, _h| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut typed, false, RichText::new("  Draw  ").size(15.0));
                        ui.selectable_value(&mut typed, true, RichText::new("  Type  ").size(15.0));
                    });
                    ui.add_space(6.0);
                    if typed {
                        ui.add(egui::TextEdit::singleline(&mut name).hint_text(if initials { "Your initials" } else { "Your name" }).desired_width(ui.available_width()));
                        ui.checkbox(&mut initials, "Initials (smaller)");
                        if !name.trim().is_empty() {
                            ui.label(RichText::new(name.trim()).italics().size(if initials { 22.0 } else { 30.0 }));
                        }
                    } else {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 180.0), Sense::drag());
                        let p = ui.painter_at(r);
                        p.rect_filled(r, 10.0, Color32::WHITE);
                        p.rect_stroke(r, 10.0, Stroke::new(1.0, Color32::from_black_alpha(90)), egui::StrokeKind::Middle);
                        let base = r.bottom() - 40.0;
                        p.line_segment([egui::pos2(r.left() + 16.0, base), egui::pos2(r.right() - 16.0, base)], Stroke::new(1.0, Color32::from_gray(170)));
                        if strokes.is_empty() {
                            p.text(egui::pos2(r.left() + 20.0, base - 14.0), egui::Align2::LEFT_BOTTOM, "Sign here with your finger", egui::FontId::proportional(14.0), Color32::from_gray(150));
                        }
                        if resp.drag_started() {
                            strokes.push(Vec::new());
                        }
                        if resp.dragged() {
                            if let (Some(pos), Some(s)) = (resp.interact_pointer_pos(), strokes.last_mut()) {
                                let l = pos - r.min;
                                s.push([l.x, l.y]);
                            }
                        }
                        for s in &strokes {
                            let pts: Vec<egui::Pos2> = s.iter().map(|q| r.min + egui::vec2(q[0], q[1])).collect();
                            if pts.len() > 1 {
                                p.add(egui::Shape::line(pts, Stroke::new(2.5, Color32::from_rgb(15, 30, 110))));
                            }
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if !typed && ui.add(egui::Button::new("Clear").min_size(egui::vec2(72.0, 40.0))).clicked() {
                            strokes.clear();
                        }
                        let ok = ui.add(egui::Button::new(RichText::new("Save & use").strong()).fill(accent.gamma_multiply(0.35)).min_size(egui::vec2(120.0, 40.0)));
                        if ok.clicked() {
                            save = if typed {
                                (!name.trim().is_empty()).then(|| SavedSig::Typed { name: name.trim().to_owned(), initials })
                            } else {
                                SavedSig::drawn(&strokes)
                            };
                            if save.is_none() {
                                ui.label("Draw or type first");
                            }
                        }
                    });
                    ui.label(RichText::new("Saved on this phone only, for reuse in other PDFs.").weak().size(12.0));
                });
                if let Some(s) = save {
                    let i = self.sigs.add(s);
                    self.e.sig_choice = Some(i);
                    self.e.tool = TouchTool::Sign;
                    self.e.selected = None;
                    self.say("Now tap where the signature goes");
                    keep = false;
                }
                Sheet::SignPad { typed, strokes, name, initials }
            }
            Sheet::Properties { mut title, mut author, mut subject, mut keywords } => {
                let p = self.cache.props.clone();
                let mut apply = false;
                let editable = self.editing;
                keep = !card(ctx, area, gap, "Properties", |ui, h| {
                    egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
                        egui::Grid::new("pdf-props").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                            for (label, v) in [("Title", &mut title), ("Author", &mut author), ("Subject", &mut subject), ("Keywords", &mut keywords)] {
                                ui.label(label);
                                ui.add_enabled(editable, egui::TextEdit::singleline(v).desired_width(220.0));
                                ui.end_row();
                            }
                        });
                        if editable && ui.add(egui::Button::new("Apply").fill(accent.gamma_multiply(0.35)).min_size(egui::vec2(100.0, 40.0))).clicked() {
                            apply = true;
                        }
                        ui.separator();
                        let yes = |b: bool| if b { "yes" } else { "no" };
                        let mut rows = vec![
                            ("Pages", p.page_count.to_string()),
                            ("PDF version", p.pdf_version.clone()),
                            ("File size", format!("{} KB", p.file_size.div_ceil(1024))),
                            ("Password protected", yes(p.encrypted).into()),
                            ("Digitally signed", yes(p.signed).into()),
                            ("Form", yes(p.has_form).into()),
                            ("Tagged (accessible)", yes(p.tagged).into()),
                        ];
                        if p.has_javascript {
                            rows.push(("JavaScript", "present — never run by Page Slate".into()));
                        }
                        if let Some(c) = &p.creator {
                            rows.push(("Created with", c.clone()));
                        }
                        if let Some(c) = &p.producer {
                            rows.push(("PDF made by", c.clone()));
                        }
                        egui::Grid::new("pdf-info").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                            for (k, v) in rows {
                                ui.label(RichText::new(k).weak());
                                ui.label(v);
                                ui.end_row();
                            }
                        });
                    });
                });
                if apply {
                    let before = self.properties_sheet();
                    if let Sheet::Properties { title: t0, author: a0, subject: s0, keywords: k0 } = before {
                        for (key, old, new) in [(PropertyKey::Title, t0, &title), (PropertyKey::Author, a0, &author), (PropertyKey::Subject, s0, &subject), (PropertyKey::Keywords, k0, &keywords)] {
                            if old != *new {
                                let v = new.clone();
                                self.edit(|d| d.set_property(key, &v));
                            }
                        }
                    }
                    self.say("Properties changed — Save to keep them");
                    keep = false;
                }
                Sheet::Properties { title, author, subject, keywords }
            }
            Sheet::Password { mut pw, mut pw2, mut owner } => {
                let encrypted = self.cache.props.encrypted;
                let restricted = self.doc.as_ref().and_then(|d| d.read_only_reason());
                enum Act {
                    Unlock,
                    Remove,
                    Protect,
                }
                let mut act = None;
                keep = !card(ctx, area, gap, "Password", |ui, _h| {
                    if let Some(r) = &restricted {
                        ui.label(format!("Changes are limited: {r}"));
                        ui.label(RichText::new("Enter the permissions (owner) password to lift that for now.").weak());
                        ui.add(egui::TextEdit::singleline(&mut owner).password(true).hint_text("Permissions password").desired_width(ui.available_width()));
                        if ui.add(egui::Button::new("Unlock").fill(accent.gamma_multiply(0.35)).min_size(egui::vec2(100.0, 40.0))).clicked() {
                            act = Some(Act::Unlock);
                        }
                        ui.separator();
                    }
                    if encrypted {
                        ui.label("This PDF needs a password to open.");
                        if ui.add(egui::Button::new("Remove the password").min_size(egui::vec2(180.0, 40.0))).clicked() {
                            act = Some(Act::Remove);
                        }
                    } else {
                        ui.label("Lock this PDF with a password (AES-256). It is needed to open the file.");
                        ui.add(egui::TextEdit::singleline(&mut pw).password(true).hint_text("New password").desired_width(ui.available_width()));
                        ui.add(egui::TextEdit::singleline(&mut pw2).password(true).hint_text("Repeat it").desired_width(ui.available_width()));
                        ui.label(RichText::new("A forgotten password can't be recovered.").weak().size(12.0));
                        if ui.add(egui::Button::new("Protect").fill(accent.gamma_multiply(0.35)).min_size(egui::vec2(100.0, 40.0))).clicked() {
                            act = Some(Act::Protect);
                        }
                    }
                });
                match act {
                    Some(Act::Unlock) => {
                        let res = self.doc.as_mut().map(|d| d.unlock(&owner));
                        match res {
                            Some(Ok(())) => {
                                self.cache.generation = None;
                                self.refresh();
                                self.say("Unlocked for this session");
                                keep = false;
                            }
                            Some(Err(e)) => self.say(super::capitalise(&e.to_string())),
                            None => {}
                        }
                        owner.clear();
                    }
                    Some(Act::Remove) => {
                        if self.edit(|d| d.remove_protection()) {
                            self.say("Password removed — Save to keep the change");
                            keep = false;
                        }
                    }
                    Some(Act::Protect) => {
                        if pw.is_empty() {
                            self.say("Type a password first");
                        } else if pw != pw2 {
                            self.say("The two passwords are different");
                            pw2.clear();
                        } else {
                            let p = pw.clone();
                            if self.edit(|d| d.protect(&p, None)) {
                                self.say("Protected — the password applies when you save");
                                keep = false;
                            }
                        }
                    }
                    None => {}
                }
                Sheet::Password { pw, pw2, owner }
            }
            Sheet::Choice(f) => {
                let mut pick = None;
                keep = !card(ctx, area, gap, f.tooltip.as_deref().unwrap_or(&f.name), |ui, h| {
                    egui::ScrollArea::vertical().max_height(h).show(ui, |ui| {
                        for (export, shown) in &f.options {
                            let on = f.value.contains(export);
                            let label = if on { format!("• {shown}") } else { shown.clone() };
                            if chrome::menu_item(ui, &label) {
                                pick = Some(export.clone());
                            }
                        }
                    });
                });
                if let Some(v) = pick {
                    let name = f.name.clone();
                    self.edit(|d| d.set_field_choice(&name, &[v]));
                    keep = false;
                }
                Sheet::Choice(f)
            }
        };
        // A sheet opened by an action inside this one wins over keeping it.
        if keep && matches!(self.sheet, Sheet::None) {
            self.sheet = next;
        }
    }
}
