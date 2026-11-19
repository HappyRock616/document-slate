//! Reading barcodes in documents (all four apps).
//!
//! * Every page / slide picture the viewer draws is also handed to a background thread
//!   (`barcode-scan`) that looks for QR codes, Data Matrix, Aztec, PDF417 and 1D barcodes. Found
//!   codes get a small badge; a tap on one opens the shared pop-up ([`crate::links`]) with what it
//!   says (web address → "Open in browser", Wi-Fi → network and password, contact…).
//! * Codes too small to read at screen resolution: long-press the spot. The screen draws that
//!   area again at high resolution ([`Scanner::submit_spot`]) and the closest code found opens.
//!
//! Positions are in the screen's own page units (PDF points, Word points, slide units…): the
//! caller says how many units one pixel of the picture is.

use std::collections::HashMap;
use std::sync::mpsc;

use eframe::egui::{self, Color32, Rect, Stroke};
use slate_barcode::{Found, Payload};

/// A code found on a page, in page units.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub rect: [f32; 4],
    pub found: Found,
}

struct Job {
    page: usize,
    generation: u64,
    /// Page units per picture pixel, and where the picture's top-left sits on the page.
    unit: f32,
    origin: [f32; 2],
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    /// A long-press: answer with the code nearest `spot`.
    spot: Option<[f32; 2]>,
    /// Draw the picture on the scan thread first (long-press in Word / Deck).
    render: Option<RenderFn>,
}

/// A high-resolution picture drawn for a long-press: (origin, page units per pixel, width,
/// height, RGBA), or `None` if it couldn't be drawn.
pub type SpotPicture = ([f32; 2], f32, u32, u32, Vec<u8>);
pub type RenderFn = Box<dyn FnOnce() -> Option<SpotPicture> + Send>;

struct Done {
    page: usize,
    generation: u64,
    hits: Vec<Hit>,
    spot: Option<[f32; 2]>,
}

/// What a long-press found.
pub enum SpotResult {
    Found(Hit),
    Nothing,
}

#[derive(Default)]
pub struct Scanner {
    jobs: Option<mpsc::Sender<Job>>,
    done: Option<mpsc::Receiver<Done>>,
    /// page -> (generation scanned, codes)
    pages: HashMap<usize, (u64, Vec<Hit>)>,
    /// page -> generation sent (not answered yet)
    asked: HashMap<usize, u64>,
    spot_busy: bool,
}

impl Scanner {
    fn start(&mut self, ctx: &egui::Context) {
        if self.jobs.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel::<Job>();
        let (dtx, drx) = mpsc::channel::<Done>();
        let ctx = ctx.clone();
        let ok = std::thread::Builder::new()
            .name("barcode-scan".into())
            .spawn(move || {
                let mut queue: Vec<Job> = Vec::new();
                loop {
                    // Take everything waiting; long-presses first, then the newest page per page.
                    // Wait only when nothing is queued: a batch of pages arrives together, and
                    // each loop scans one of them.
                    if queue.is_empty() {
                        match rx.recv() {
                            Ok(j) => queue.push(j),
                            Err(_) => return,
                        }
                    }
                    while let Ok(j) = rx.try_recv() {
                        queue.push(j);
                    }
                    let pick = queue.iter().position(|j| j.spot.is_some()).unwrap_or(queue.len() - 1);
                    let mut job = queue.remove(pick);
                    if let Some(render) = job.render.take() {
                        if let Some((origin, unit, w, h, rgba)) = render() {
                            job.origin = origin;
                            job.unit = unit;
                            job.width = w;
                            job.height = h;
                            job.rgba = rgba;
                        }
                    }
                    if job.spot.is_none() {
                        // Older requests for the same page are out of date.
                        queue.retain(|j| j.spot.is_some() || j.page != job.page);
                    }
                    let found = slate_barcode::decode_rgba(job.width, job.height, &job.rgba);
                    log::debug!("barcode scan: page {} {}x{} spot={} -> {} found", job.page, job.width, job.height, job.spot.is_some(), found.len());
                    let hits = found
                        .into_iter()
                        .map(|f| {
                            let b = f.bounds;
                            let rect = [job.origin[0] + b[0] * job.unit, job.origin[1] + b[1] * job.unit, job.origin[0] + b[2] * job.unit, job.origin[1] + b[3] * job.unit];
                            Hit { rect, found: f }
                        })
                        .collect();
                    if dtx.send(Done { page: job.page, generation: job.generation, hits, spot: job.spot }).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .is_ok();
        if ok {
            self.jobs = Some(tx);
            self.done = Some(drx);
        }
    }

    /// A page picture was drawn: scan it unless this version of the page was scanned already.
    /// `unit` = page units per pixel.
    pub fn submit_page(&mut self, ctx: &egui::Context, page: usize, generation: u64, unit: f32, width: u32, height: u32, rgba: &[u8]) {
        if self.pages.get(&page).is_some_and(|(g, _)| *g == generation) || self.asked.get(&page) == Some(&generation) {
            return;
        }
        if width < 16 || height < 16 || rgba.len() < (width as usize) * (height as usize) * 4 || !unit.is_finite() {
            return;
        }
        self.start(ctx);
        if let Some(tx) = &self.jobs {
            let job = Job { page, generation, unit, origin: [0.0, 0.0], width, height, rgba: rgba.to_vec(), spot: None, render: None };
            if tx.send(job).is_ok() {
                self.asked.insert(page, generation);
            }
        }
    }

    /// A long-press at `spot`: the screen drew the area around it (top-left `origin`, `unit`
    /// page units per pixel). Poll [`Scanner::poll`] for the answer.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_spot(&mut self, ctx: &egui::Context, page: usize, spot: [f32; 2], origin: [f32; 2], unit: f32, width: u32, height: u32, rgba: Vec<u8>) {
        self.start(ctx);
        if let Some(tx) = &self.jobs {
            if tx.send(Job { page, generation: u64::MAX, unit, origin, width, height, rgba, spot: Some(spot), render: None }).is_ok() {
                self.spot_busy = true;
            }
        }
    }

    /// A long-press at `spot`; `render` draws the area around it on the scan thread.
    pub fn submit_spot_render(&mut self, ctx: &egui::Context, page: usize, spot: [f32; 2], render: RenderFn) {
        if self.spot_busy {
            return;
        }
        self.start(ctx);
        if let Some(tx) = &self.jobs {
            let job = Job { page, generation: u64::MAX, unit: 1.0, origin: [0.0, 0.0], width: 0, height: 0, rgba: Vec::new(), spot: Some(spot), render: Some(render) };
            if tx.send(job).is_ok() {
                self.spot_busy = true;
            }
        }
    }

    /// Crop `rect` (page units) out of a picture of the whole page drawn at `scale` px per unit.
    pub fn crop(width: u32, height: u32, rgba: &[u8], scale: f32, rect: [f32; 4]) -> Option<SpotPicture> {
        let x0 = ((rect[0] * scale).max(0.0) as u32).min(width);
        let y0 = ((rect[1] * scale).max(0.0) as u32).min(height);
        let x1 = ((rect[2] * scale).max(0.0) as u32).min(width);
        let y1 = ((rect[3] * scale).max(0.0) as u32).min(height);
        if x1 <= x0 + 8 || y1 <= y0 + 8 || rgba.len() < (width * height * 4) as usize {
            return None;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y1 {
            let s = ((y * width + x0) * 4) as usize;
            out.extend_from_slice(&rgba[s..s + (w * 4) as usize]);
        }
        Some(([x0 as f32 / scale, y0 as f32 / scale], 1.0 / scale, w, h, out))
    }

    pub fn spot_busy(&self) -> bool {
        self.spot_busy
    }

    /// Collect finished scans; returns the answer to a long-press, if one came in.
    pub fn poll(&mut self) -> Option<SpotResult> {
        let mut spot_answer = None;
        let Some(rx) = &self.done else { return None };
        while let Ok(d) = rx.try_recv() {
            match d.spot {
                Some(at) => {
                    self.spot_busy = false;
                    let best = d.hits.into_iter().min_by(|a, b| dist(&a.rect, at).total_cmp(&dist(&b.rect, at)));
                    spot_answer = Some(match best {
                        Some(h) if dist(&h.rect, at) < 60.0 => {
                            // Remember it, so a plain tap works next time.
                            let entry = self.pages.entry(d.page).or_insert((0, Vec::new()));
                            if !entry.1.iter().any(|x| x.found.text == h.found.text && overlap(&x.rect, &h.rect)) {
                                entry.1.push(h.clone());
                            }
                            SpotResult::Found(h)
                        }
                        _ => SpotResult::Nothing,
                    });
                }
                None => {
                    if self.asked.get(&d.page) == Some(&d.generation) {
                        self.asked.remove(&d.page);
                    }
                    let keep_newer = self.pages.get(&d.page).is_some_and(|(g, _)| *g > d.generation && *g != 0);
                    if !keep_newer {
                        self.pages.insert(d.page, (d.generation, d.hits));
                    }
                }
            }
        }
        spot_answer
    }

    /// Forget everything (the document changed shape: pages added / removed).
    pub fn clear(&mut self) {
        self.pages.clear();
        self.asked.clear();
    }

    pub fn hits(&self, page: usize) -> &[Hit] {
        self.pages.get(&page).map(|(_, h)| h.as_slice()).unwrap_or(&[])
    }

    /// The code under `pt` (with a little slack for fingers).
    pub fn hit_at(&self, page: usize, pt: [f32; 2], slack: f32) -> Option<&Hit> {
        self.hits(page).iter().find(|h| pt[0] >= h.rect[0] - slack && pt[0] <= h.rect[2] + slack && pt[1] >= h.rect[1] - slack && pt[1] <= h.rect[3] + slack)
    }

    /// Draw a thin outline and a small QR badge on each found code of `page`, drawn at
    /// `page_rect` with `scale` screen points per page unit.
    pub fn draw_badges(&self, painter: &egui::Painter, page: usize, page_rect: Rect, scale: f32, accent: Color32) {
        for h in self.hits(page) {
            let r = Rect::from_min_max(page_rect.min + egui::vec2(h.rect[0], h.rect[1]) * scale, page_rect.min + egui::vec2(h.rect[2], h.rect[3]) * scale).expand(3.0);
            painter.rect_stroke(r, 4.0, Stroke::new(1.5, accent.gamma_multiply(0.8)), egui::StrokeKind::Outside);
            let c = r.right_top() + egui::vec2(-2.0, 2.0);
            painter.circle_filled(c, 11.0, accent);
            crate::icons::paint(painter, Rect::from_center_size(c, egui::vec2(14.0, 14.0)), "qr", Color32::WHITE, Color32::WHITE);
        }
    }
}

fn dist(r: &[f32; 4], p: [f32; 2]) -> f32 {
    let dx = (r[0] - p[0]).max(0.0).max(p[0] - r[2]);
    let dy = (r[1] - p[1]).max(0.0).max(p[1] - r[3]);
    (dx * dx + dy * dy).sqrt()
}

fn overlap(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Show what a code says in the shared pop-up.
pub fn show(found: &Found) {
    crate::links::show(popup_for(found));
}

/// The pop-up for a code: what kind of thing it holds, the text, and what can be opened.
pub fn popup_for(found: &Found) -> crate::links::Popup {
    let payload = slate_barcode::classify(&found.text);
    let open = slate_barcode::open_uri(&payload).and_then(|u| crate::links::safe_uri(&u));
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut add = |k: &str, v: &str| {
        if !v.trim().is_empty() {
            fields.push((k.to_owned(), v.to_owned()));
        }
    };
    let (kind, text) = match &payload {
        Payload::Url(u) => ("Web address".to_string(), u.clone()),
        Payload::Email { to, subject, body } => {
            add("Subject", subject);
            add("Message", body);
            ("Email".into(), to.clone())
        }
        Payload::Phone(n) => ("Phone number".into(), n.clone()),
        Payload::Sms { number, body } => {
            add("Message", body);
            ("Text message".into(), number.clone())
        }
        Payload::Wifi { ssid, password, kind, hidden } => {
            add("Password", password);
            add("Security", kind);
            if *hidden {
                add("Hidden network", "yes");
            }
            ("Wi-Fi network".into(), ssid.clone())
        }
        Payload::Geo { lat, lon } => ("Map location".into(), format!("{lat}, {lon}")),
        Payload::Contact { name, phone, email, org, address, url, note } => {
            add("Phone", phone);
            add("Email", email);
            add("Organization", org);
            add("Address", address);
            add("Web", url);
            add("Note", note);
            ("Contact".into(), name.clone())
        }
        Payload::Text(t) => ("Text".into(), t.clone()),
    };
    crate::links::Popup { title: found.symbology.clone(), text, open, kind, fields }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Found {
        Found { symbology: "QR code".into(), text: text.into(), corners: vec![], bounds: [0.0, 0.0, 10.0, 10.0] }
    }

    #[test]
    fn popups_say_what_the_code_holds() {
        let p = popup_for(&found("https://example.com/x"));
        assert_eq!(p.kind, "Web address");
        assert_eq!(p.open.as_deref(), Some("https://example.com/x"));
        let p = popup_for(&found("WIFI:T:WPA;S:Home Net;P:secret123;;"));
        assert_eq!(p.kind, "Wi-Fi network");
        assert_eq!(p.text, "Home Net");
        assert!(p.fields.contains(&("Password".into(), "secret123".into())));
        assert!(p.open.is_none());
        let p = popup_for(&found("javascript:alert(1)"));
        assert_eq!(p.kind, "Text");
        assert!(p.open.is_none());
        let p = popup_for(&found("4006381333931"));
        assert_eq!(p.kind, "Text");
    }

    #[test]
    fn scanner_finds_a_code_on_a_page_and_a_spot() {
        let ctx = egui::Context::default();
        let m = slate_barcode::encode(slate_barcode::Symbology::Qr, "https://example.com/scan").expect("qr");
        let (w, h, code) = slate_barcode::to_rgba(&m, 3, [0, 0, 0], [255, 255, 255]);
        // A white 600x800 "page" with the code at (100, 200).
        let (pw, ph) = (600u32, 800u32);
        let mut page = vec![255u8; (pw * ph * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let s = ((y * w + x) * 4) as usize;
                let d = (((y + 200) * pw + x + 100) * 4) as usize;
                page[d..d + 4].copy_from_slice(&code[s..s + 4]);
            }
        }
        let mut sc = Scanner::default();
        // 2 page units per pixel.
        sc.submit_page(&ctx, 0, 1, 2.0, pw, ph, &page);
        let t = std::time::Instant::now();
        while sc.hits(0).is_empty() && t.elapsed().as_secs() < 20 {
            sc.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let hit = sc.hits(0).first().cloned().expect("found on the page");
        assert_eq!(hit.found.text, "https://example.com/scan");
        assert!(hit.rect[0] > 190.0 && hit.rect[0] < 230.0, "{:?}", hit.rect);
        assert!(sc.hit_at(0, [hit.rect[0] + 5.0, hit.rect[1] + 5.0], 0.0).is_some());
        // Long-press on the same picture, as a crop placed at (50, 50).
        sc.submit_spot(&ctx, 3, [60.0, 60.0], [50.0, 50.0], 1.0, pw, ph, page.clone());
        let t = std::time::Instant::now();
        let mut answer = None;
        while answer.is_none() && t.elapsed().as_secs() < 20 {
            answer = sc.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(matches!(answer, Some(SpotResult::Nothing)), "too far from the code");
        sc.submit_spot(&ctx, 3, [160.0, 260.0], [50.0, 50.0], 1.0, pw, ph, page);
        let t = std::time::Instant::now();
        let mut answer = None;
        while answer.is_none() && t.elapsed().as_secs() < 20 {
            answer = sc.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(matches!(answer, Some(SpotResult::Found(h)) if h.found.text == "https://example.com/scan"));
        assert_eq!(sc.hits(3).len(), 1, "remembered for a plain tap");
    }

    #[test]
    fn scans_every_page_sent_together() {
        // A viewer sends several pages in one frame; each must be scanned, not just the last
        // (the thread used to wait for a new job between pages).
        let ctx = egui::Context::default();
        let m = slate_barcode::encode(slate_barcode::Symbology::Qr, "https://example.com/p").expect("qr");
        let (w, h, code) = slate_barcode::to_rgba(&m, 3, [0, 0, 0], [255, 255, 255]);
        let (pw, ph) = (300u32, 300u32);
        let mut page = vec![255u8; (pw * ph * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let s = ((y * w + x) * 4) as usize;
                let d = (((y + 40) * pw + x + 40) * 4) as usize;
                page[d..d + 4].copy_from_slice(&code[s..s + 4]);
            }
        }
        let mut sc = Scanner::default();
        for p in 0..4 {
            sc.submit_page(&ctx, p, 1, 1.0, pw, ph, &page);
        }
        let t = std::time::Instant::now();
        while (0..4).any(|p| sc.hits(p).is_empty()) && t.elapsed().as_secs() < 20 {
            sc.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for p in 0..4 {
            assert_eq!(sc.hits(p).len(), 1, "page {p}");
        }
    }
}

/// Makes a sample PDF with barcodes for trying this on a phone:
/// `DS_SAMPLE_OUT=path.pdf cargo test --lib make_barcode_sample -- --ignored`.
#[cfg(all(test, feature = "pdf"))]
mod sample {
    use slate_barcode::Symbology;

    #[test]
    #[ignore]
    fn make_barcode_sample() {
        let out = std::env::var("DS_SAMPLE_OUT").unwrap_or_else(|_| "barcodes-sample.pdf".into());
        let mut doc = slate_pdf::PdfDoc::create_blank(612.0, 792.0, 2).expect("doc");
        let codes: [(Symbology, &str, [f32; 4]); 6] = [
            (Symbology::Qr, "https://www.wikipedia.org", [72.0, 72.0, 216.0, 216.0]),
            (Symbology::Qr, "WIFI:T:WPA;S:Slate Test;P:correct horse;;", [360.0, 72.0, 504.0, 216.0]),
            (Symbology::DataMatrix, "Grüße aus Document Slate", [72.0, 300.0, 136.0, 364.0]),
            (Symbology::Aztec, "mailto:someone@example.invalid?subject=Hello", [360.0, 300.0, 460.0, 400.0]),
            (Symbology::Ean13, "400638133393", [72.0, 480.0, 270.0, 560.0]),
            (Symbology::Pdf417, "PDF417 test 12345", [330.0, 480.0, 540.0, 540.0]),
        ];
        for (sym, text, rect) in codes {
            let m = slate_barcode::encode(sym, text).expect("encode");
            let png = slate_barcode::to_png(&m, 8, [0, 0, 0], [255, 255, 255]);
            doc.add_image(0, Some(rect), "code.png", png).expect("image");
        }
        // A tiny QR (about 0.4 inch) on page 2: found by long-press.
        let m = slate_barcode::encode(Symbology::Qr, "https://example.com/tiny").expect("encode");
        doc.add_image(1, Some([280.0, 380.0, 310.0, 410.0]), "tiny.png", slate_barcode::to_png(&m, 8, [0, 0, 0], [255, 255, 255])).expect("tiny");
        let bytes = doc.save_bytes().expect("save").bytes;
        std::fs::write(&out, bytes).expect("write");
    }
}
