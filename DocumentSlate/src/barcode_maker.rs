//! Insert › Barcode (all four apps): make a QR code, Data Matrix, Aztec, PDF417, Code 128,
//! Code 39, EAN-13 or UPC-A and put it into the document as a picture.
//!
//! The card is shared (opened with [`open`], drawn by the shell with [`draw`]); "Insert" writes
//! a crisp PNG into the app's private folder and hands it to the open document the same way a
//! picture from the photo picker arrives ([`take_picture`]).

use std::path::PathBuf;
use std::sync::Mutex;

use eframe::egui::{self, Color32, RichText};
use slate_barcode::Symbology;

struct Maker {
    sym: Symbology,
    text: String,
    size: usize,
    /// (symbology, text) the preview was made for, and the preview.
    preview_for: Option<(Symbology, String)>,
    preview: Option<egui::TextureHandle>,
    error: Option<String>,
    folder: PathBuf,
    /// Put the keyboard focus in the text box on the next frame (the card just opened).
    focus: bool,
}

/// Small / Medium / Large: width in points for 2D codes and for 1D barcodes.
const SIZES: [(&str, f32, f32); 3] = [("Small", 72.0, 144.0), ("Medium", 108.0, 198.0), ("Large", 162.0, 270.0)];

static MAKER: Mutex<Option<Maker>> = Mutex::new(None);
/// (PNG path, width in points) ready to insert.
static READY: Mutex<Option<(String, f32)>> = Mutex::new(None);
/// Type, text and size of the last card, offered again next time it opens.
static LAST: Mutex<Option<(Symbology, String, usize)>> = Mutex::new(None);

/// Open the card. `folder` = the app's private folder (the PNG goes to `folder/insert/`).
pub fn open(folder: &std::path::Path) {
    if let Ok(mut g) = MAKER.lock() {
        let last = LAST.lock().ok().and_then(|l| l.clone());
        let (sym, text, size) = last.unwrap_or((Symbology::Qr, String::new(), 1));
        *g = Some(Maker { sym, text, size, preview_for: None, preview: None, error: None, folder: folder.to_path_buf(), focus: true });
    }
}

pub fn is_open() -> bool {
    MAKER.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn back() -> bool {
    let Ok(mut g) = MAKER.lock() else { return false };
    let Some(m) = g.take() else { return false };
    if let Ok(mut l) = LAST.lock() {
        *l = Some((m.sym, m.text, m.size));
    }
    true
}

/// The barcode picture to insert, once "Insert" was tapped: (PNG path, width in points).
pub fn take_picture() -> Option<(String, f32)> {
    READY.lock().ok().and_then(|mut g| g.take())
}

/// Draw the card if it's open.
pub fn draw(ctx: &egui::Context, top: f32, bottom: f32) {
    let Ok(mut guard) = MAKER.lock() else { return };
    let Some(m) = guard.as_mut() else { return };
    let mut close = false;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("barcode-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(130));
    });
    // Preview, made again whenever the type or text changes.
    let key = (m.sym, m.text.clone());
    if m.preview_for.as_ref() != Some(&key) {
        m.preview_for = Some(key);
        m.preview = None;
        m.error = None;
        if !m.text.is_empty() {
            match slate_barcode::validate(m.sym, &m.text).and_then(|()| slate_barcode::encode(m.sym, &m.text)) {
                Ok(matrix) => {
                    let px = (480 / matrix.width.max(1)).clamp(1, 12) as u32;
                    let (w, h, rgba) = slate_barcode::to_rgba(&matrix, px, [0, 0, 0], [255, 255, 255]);
                    let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    m.preview = Some(ctx.load_texture("barcode-preview", img, egui::TextureOptions::NEAREST));
                }
                Err(e) => m.error = Some(e),
            }
        }
    }
    let width = (screen.width() - 24.0).min(480.0);
    let pos = egui::pos2(screen.center().x - width / 2.0, screen.top() + top + 12.0);
    let max_h = (screen.bottom() - bottom - pos.y - 12.0).max(260.0);
    let fill = ctx.global_style().visuals.panel_fill;
    let fg = ctx.global_style().visuals.text_color();
    let mut insert = false;
    let card = egui::Area::new(egui::Id::unique("barcode-card")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(16)
            .inner_margin(14)
            .shadow(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: Color32::from_black_alpha(110) })
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.set_max_height(max_h);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Barcode").size(18.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::chrome::bar_button(ui, "close", "Close", fg).clicked() {
                            close = true;
                        }
                    });
                });
                // The Insert button stays pinned under the scrolling part (visible above the keyboard).
                egui::ScrollArea::vertical().max_height(max_h - 124.0).auto_shrink([false, true]).show(ui, |ui| {
                    egui::ComboBox::from_id_salt("barcode-type").selected_text(m.sym.label()).width(ui.available_width()).height(420.0).show_ui(ui, |ui| {
                        ui.style_mut().spacing.button_padding = egui::vec2(12.0, 10.0);
                        for s in Symbology::ALL {
                            ui.selectable_value(&mut m.sym, s, s.label());
                        }
                    });
                    ui.label(RichText::new(m.sym.hint()).weak().size(12.5));
                    let edit = if m.sym.is_2d() {
                        egui::TextEdit::multiline(&mut m.text).desired_rows(3)
                    } else {
                        egui::TextEdit::singleline(&mut m.text)
                    };
                    let text_box = ui.add(edit.hint_text(if m.sym.is_2d() { "Web address, text, Wi-Fi…" } else { "Numbers / text" }).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 9.0)));
                    // Typing goes into this box from the start. With no box focused, the editor
                    // behind the card would take the keys.
                    if std::mem::take(&mut m.focus) {
                        text_box.request_focus();
                    }
                    if m.sym.is_2d() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Start with:").weak().size(12.0));
                            for (label, start) in [("Web", "https://"), ("Email", "mailto:"), ("Phone", "tel:"), ("Wi-Fi", "WIFI:T:WPA;S:network;P:password;;")] {
                                if ui.small_button(label).clicked() && m.text.trim().is_empty() {
                                    m.text = start.to_owned();
                                }
                            }
                        });
                    }
                    ui.add_space(6.0);
                    match (&m.preview, &m.error) {
                        (_, Some(e)) => {
                            ui.label(RichText::new(e).color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
                        }
                        (Some(tex), None) => {
                            let s = tex.size_vec2();
                            let max = egui::vec2(ui.available_width(), 220.0);
                            let k = (max.x / s.x).min(max.y / s.y).min(4.0);
                            ui.vertical_centered(|ui| ui.image((tex.id(), s * k)));
                        }
                        (None, None) => {
                            ui.label(RichText::new("Type what the barcode should hold.").weak());
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        for (i, (label, _, _)) in SIZES.iter().enumerate() {
                            ui.selectable_value(&mut m.size, i, *label);
                        }
                    });
                });
                ui.add_space(8.0);
                let ok = m.preview.is_some() && m.error.is_none();
                // The open app's accent (the theme's selection color).
                let accent = ui.visuals().selection.bg_fill;
                let b = egui::Button::new(RichText::new("Insert").size(15.0).color(Color32::WHITE)).fill(accent).min_size(egui::vec2(ui.available_width(), 44.0));
                if ui.add_enabled(ok, b).clicked() {
                    insert = true;
                }
            });
    });
    // The dimmed background and the card share a layer order, and egui lifts whichever was
    // touched last: keep the card on top, or a tap beside it would bury it under the dimming.
    ctx.move_to_top(card.response.layer_id);
    if insert {
        match make_png(m) {
            Ok(ready) => {
                if let Ok(mut r) = READY.lock() {
                    *r = Some(ready);
                }
                close = true;
            }
            Err(e) => m.error = Some(e),
        }
    }
    if close {
        // Keep type, text and size for next time; only the card closes.
        if let Ok(mut l) = LAST.lock() {
            *l = Some((m.sym, m.text.clone(), m.size));
        }
        *guard = None;
    }
}

fn make_png(m: &Maker) -> Result<(String, f32), String> {
    let matrix = slate_barcode::encode(m.sym, &m.text)?;
    // ~900 px across: sharp when printed at these sizes.
    let px = (900 / matrix.width.max(1)).clamp(2, 24) as u32;
    let png = slate_barcode::to_png(&matrix, px, [0, 0, 0], [255, 255, 255]);
    let dir = m.folder.join("insert");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = dir.join(format!("barcode-{n}.png"));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    let (_, w2d, w1d) = SIZES[m.size.min(SIZES.len() - 1)];
    Ok((path.to_string_lossy().into_owned(), if m.sym.is_2d() { w2d } else { w1d }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_png_that_reads_back() {
        let dir = std::env::temp_dir().join(format!("ds-barcode-{}", std::process::id()));
        for (sym, text) in [(Symbology::Qr, "https://example.com/é"), (Symbology::DataMatrix, "Grüße"), (Symbology::Ean13, "400638133393"), (Symbology::Code128, "ABC-123")] {
            let m = Maker { sym, text: text.into(), size: 1, preview_for: None, preview: None, error: None, folder: dir.clone(), focus: false };
            let (path, w) = make_png(&m).expect("png");
            assert!(w > 50.0);
            let found = slate_barcode::decode_image_bytes(&std::fs::read(&path).expect("read"));
            assert!(found.iter().any(|f| f.text.starts_with(&text[..text.len().min(12)])), "{sym:?}: {found:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
