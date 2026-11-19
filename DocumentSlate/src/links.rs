//! Links and barcodes: one pop-up for all four apps.
//!
//! A tapped link (in a document) or a decoded barcode never opens anything by itself: the
//! pop-up shows what it is — the full address, so a misleading link text can't hide where it
//! goes — with **Open** (web links in the phone's default browser, never an app that claims the
//! address; mail / phone / map links in their apps) and **Copy**.
//!
//! Any screen calls [`show`]; the shell draws the pop-up every frame with [`draw`].

use std::sync::Mutex;

use eframe::egui::{self, Color32, RichText};

/// What the pop-up is about.
#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    /// "Link", "QR code", "Data Matrix"…
    pub title: String,
    /// The raw text (link address or barcode contents).
    pub text: String,
    /// What Android should open (https://…, mailto:…, tel:…, geo:…), if anything.
    pub open: Option<String>,
    /// A plain-words description shown above the text ("Web address", "Wi-Fi network"…).
    pub kind: String,
    /// Extra lines (Wi-Fi password, contact details…), each (label, value).
    pub fields: Vec<(String, String)>,
}

static POPUP: Mutex<Option<Popup>> = Mutex::new(None);
static MESSAGE: Mutex<Option<String>> = Mutex::new(None);

/// Show the pop-up for a link found in a document (`url` as written in the file).
pub fn show_link(url: &str) {
    let url = url.trim();
    let open = safe_uri(url);
    let kind = match &open {
        Some(u) if u.starts_with("mailto:") => "Email address",
        Some(u) if u.starts_with("tel:") => "Phone number",
        Some(u) if u.starts_with("geo:") => "Map location",
        Some(_) => "Web address",
        None => "Link (this kind can't be opened from here)",
    };
    show(Popup { title: "Link".into(), text: url.to_owned(), open, kind: kind.into(), fields: Vec::new() });
}

pub fn show(p: Popup) {
    if let Ok(mut g) = POPUP.lock() {
        *g = Some(p);
    }
}

pub fn is_open() -> bool {
    POPUP.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Back pressed: close the pop-up if it's open.
pub fn back() -> bool {
    POPUP.lock().map(|mut g| g.take().is_some()).unwrap_or(false)
}

/// A message for the shell's toast / status (e.g. "Copied"), once.
pub fn take_message() -> Option<String> {
    MESSAGE.lock().ok().and_then(|mut g| g.take())
}

pub fn note(s: impl Into<String>) {
    if let Ok(mut g) = MESSAGE.lock() {
        *g = Some(s.into());
    }
}

/// The address to hand to Android, or `None` for schemes that mustn't be opened
/// (javascript:, file:, intent:, content:, data:…). "www.example.com" becomes https://.
pub fn safe_uri(s: &str) -> Option<String> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("www.") && !s.contains(char::is_whitespace) {
        return Some(format!("https://{s}"));
    }
    let scheme = lower.split_once(':').map(|(a, _)| a)?;
    match scheme {
        "http" | "https" => (lower.starts_with("http://") || lower.starts_with("https://")).then(|| s.to_owned()),
        "mailto" | "tel" | "geo" | "sms" | "smsto" => Some(s.to_owned()),
        _ => None,
    }
}

/// Open an address (from [`safe_uri`]).
pub fn open(uri: &str) -> Result<(), String> {
    let uri = safe_uri(uri).ok_or("links of this kind can't be opened")?;
    #[cfg(target_os = "android")]
    {
        let app = crate::android_bridge::app().ok_or("no Android context")?;
        crate::android_bridge::open_link(app, &uri)
    }
    #[cfg(not(target_os = "android"))]
    {
        log::info!("desktop: would open {uri}");
        note(format!("Desktop: would open {uri}"));
        Ok(())
    }
}

fn copy(text: &str, ctx: &egui::Context) {
    #[cfg(target_os = "android")]
    if let Some(app) = crate::android_bridge::app() {
        let _ = crate::android_bridge::copy_text(app, "Document Slate", text);
    }
    #[cfg(not(target_os = "android"))]
    ctx.copy_text(text.to_owned());
    #[cfg(target_os = "android")]
    let _ = ctx;
    note("Copied");
}

/// Draw the pop-up (if one is showing). Call every frame, last, from the shell.
pub fn draw(ctx: &egui::Context, top: f32, bottom: f32) {
    let Some(p) = POPUP.lock().ok().and_then(|g| g.clone()) else { return };
    let screen = ctx.content_rect();
    let mut close = false;
    egui::Area::new(egui::Id::unique("link-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(130));
        if resp.clicked() {
            close = true;
        }
    });
    let width = (screen.width() - 32.0).min(460.0);
    let fill = ctx.global_style().visuals.panel_fill;
    let fg = ctx.global_style().visuals.text_color();
    egui::Area::new(egui::Id::unique("link-popup"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .fixed_pos(egui::pos2(screen.center().x, (screen.top() + top + screen.bottom() - bottom) / 2.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(fill)
                .corner_radius(16)
                .inner_margin(16)
                .shadow(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&p.title).size(18.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::chrome::bar_button(ui, "close", "Close", fg).clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.label(RichText::new(&p.kind).weak());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height((screen.height() * 0.45).max(120.0)).show(ui, |ui| {
                        // The address in full, selectable, and tappable when it can be opened.
                        let text = RichText::new(&p.text).size(16.0);
                        if p.open.is_some() {
                            let r = ui.add(egui::Label::new(text.color(Color32::from_rgb(0x4A, 0x90, 0xE2)).underline()).sense(egui::Sense::click()).wrap());
                            if r.clicked() {
                                if let Some(u) = &p.open {
                                    match open(u) {
                                        Ok(()) => close = true,
                                        Err(e) => note(e),
                                    }
                                }
                            }
                        } else {
                            ui.add(egui::Label::new(text).selectable(true).wrap());
                        }
                        for (k, v) in &p.fields {
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(k).weak());
                                ui.add(egui::Label::new(RichText::new(v).strong()).selectable(true));
                            });
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 8.0) / 2.0;
                        if let Some(u) = &p.open {
                            let label = if u.starts_with("http") { "Open in browser" } else { "Open" };
                            let b = egui::Button::new(RichText::new(label).size(15.0).color(Color32::WHITE)).fill(Color32::from_rgb(0x2F, 0x6D, 0xB5)).min_size(egui::vec2(w, 44.0));
                            if ui.add(b).clicked() {
                                match open(u) {
                                    Ok(()) => close = true,
                                    Err(e) => note(e),
                                }
                            }
                        }
                        let copy_what = p.fields.iter().find(|(k, _)| k == "Password").map(|(_, v)| v.clone()).unwrap_or_else(|| p.text.clone());
                        let label = if p.fields.iter().any(|(k, _)| k == "Password") { "Copy password" } else { "Copy" };
                        if ui.add(egui::Button::new(RichText::new(label).size(15.0)).min_size(egui::vec2(w, 44.0))).clicked() {
                            copy(&copy_what, ui.ctx());
                        }
                    });
                });
        });
    if close {
        if let Ok(mut g) = POPUP.lock() {
            *g = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_schemes_open() {
        assert_eq!(safe_uri("https://example.com/a?b=1").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(safe_uri("HTTP://EXAMPLE.COM").as_deref(), Some("HTTP://EXAMPLE.COM"));
        assert_eq!(safe_uri("www.example.com").as_deref(), Some("https://www.example.com"));
        assert_eq!(safe_uri("mailto:a@b.c").as_deref(), Some("mailto:a@b.c"));
        assert_eq!(safe_uri("tel:+15551234").as_deref(), Some("tel:+15551234"));
        for bad in ["javascript:alert(1)", "file:///sdcard/x", "intent://x#Intent;end", "content://x", "data:text/html,hi", "http:/broken", "just text", ""] {
            assert_eq!(safe_uri(bad), None, "{bad}");
        }
    }
}
