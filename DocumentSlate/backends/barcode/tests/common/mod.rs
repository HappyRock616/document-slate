//! Picture helpers for the tests: a gray canvas, pasting codes, scaling, rotating, noise.
#![allow(dead_code)]

use slate_barcode::{Matrix, Symbology, encode, to_rgba};

pub const BLACK: [u8; 3] = [0, 0, 0];
pub const WHITE: [u8; 3] = [255, 255, 255];

pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: u32, h: u32, gray: u8) -> Self {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            px.extend_from_slice(&[gray, gray, gray, 255]);
        }
        Canvas { w, h, px }
    }

    /// Pastes an RGBA picture at (x, y). Returns its rectangle [x0, y0, x1, y1].
    pub fn paste(&mut self, x: u32, y: u32, pw: u32, ph: u32, rgba: &[u8]) -> [f32; 4] {
        for yy in 0..ph {
            for xx in 0..pw {
                let (cx, cy) = (x + xx, y + yy);
                if cx >= self.w || cy >= self.h {
                    continue;
                }
                let s = ((yy * pw + xx) * 4) as usize;
                let d = ((cy * self.w + cx) * 4) as usize;
                self.px[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
            }
        }
        [x as f32, y as f32, (x + pw) as f32, (y + ph) as f32]
    }

    /// Adds pseudo-random noise of +-amount to every channel (deterministic).
    pub fn noise(&mut self, amount: i32, seed: u64) {
        let mut r = Rng(seed);
        for (i, v) in self.px.iter_mut().enumerate() {
            if i % 4 == 3 {
                continue;
            }
            let n = (r.next() % (2 * amount as u64 + 1)) as i32 - amount;
            *v = (i32::from(*v) + n).clamp(0, 255) as u8;
        }
    }

    pub fn luma(&self) -> Vec<u8> {
        self.px.as_chunks::<4>().0.iter().map(|p| ((u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3) as u8).collect()
    }
}

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// Encodes and draws a code: (w, h, rgba). Panics in tests on bad input.
pub fn draw(sym: Symbology, text: &str, module_px: u32) -> (u32, u32, Vec<u8>) {
    let m: Matrix = encode(sym, text).unwrap_or_else(|e| panic!("encode {sym:?}: {e}"));
    to_rgba(&m, module_px, BLACK, WHITE)
}

/// Nearest-neighbor resize of an RGBA picture by a (possibly fractional) factor.
pub fn scale(w: u32, h: u32, rgba: &[u8], f: f32) -> (u32, u32, Vec<u8>) {
    let nw = ((w as f32) * f).round().max(1.0) as u32;
    let nh = ((h as f32) * f).round().max(1.0) as u32;
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        let sy = (((y as f32 + 0.5) / f) as u32).min(h - 1);
        for x in 0..nw {
            let sx = (((x as f32 + 0.5) / f) as u32).min(w - 1);
            let s = ((sy * w + sx) * 4) as usize;
            out.extend_from_slice(&rgba[s..s + 4]);
        }
    }
    (nw, nh, out)
}

/// Rotates an RGBA picture 90 degrees clockwise.
pub fn rot90(w: u32, h: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = (h, w);
    let mut out = vec![0u8; rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let s = ((y * w + x) * 4) as usize;
            let (nx, ny) = (h - 1 - y, x);
            let d = ((ny * nw + nx) * 4) as usize;
            out[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    (nw, nh, out)
}

/// Places a picture in the middle of a white canvas with `pad` pixels around it.
pub fn padded(w: u32, h: u32, rgba: &[u8], pad: u32) -> Canvas {
    let mut c = Canvas::new(w + 2 * pad, h + 2 * pad, 255);
    c.paste(pad, pad, w, h, rgba);
    c
}

pub fn center_inside(b: [f32; 4], rect: [f32; 4]) -> bool {
    let cx = (b[0] + b[2]) / 2.0;
    let cy = (b[1] + b[3]) / 2.0;
    cx >= rect[0] && cx <= rect[2] && cy >= rect[1] && cy <= rect[3]
}

/// Text the decoder reports for what we encoded (EAN/UPC gain their check digit).
pub fn expected(sym: Symbology, text: &str) -> String {
    match sym {
        Symbology::Ean13 if text.len() == 12 => format!("{text}{}", check(text)),
        Symbology::UpcA if text.len() == 11 => format!("{text}{}", check(text)),
        _ => text.to_string(),
    }
}

fn check(body: &str) -> u8 {
    let sum: u32 = body.bytes().rev().enumerate().map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 3 } else { 1 }).sum();
    ((10 - sum % 10) % 10) as u8
}

pub const SAMPLES: [(Symbology, &str); 8] = [
    (Symbology::Qr, "https://example.com/doc?id=42"),
    (Symbology::DataMatrix, "Document Slate 123"),
    (Symbology::Aztec, "Ticket #42 / Row 7"),
    (Symbology::Pdf417, "PDF417 sample text"),
    (Symbology::Code128, "Slate-128 abc"),
    (Symbology::Code39, "SLATE-39"),
    (Symbology::Ean13, "590123412345"),
    (Symbology::UpcA, "03600029145"),
];
