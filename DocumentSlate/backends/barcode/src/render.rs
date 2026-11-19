//! Turning a [`Matrix`] into pictures: RGBA pixels, PNG bytes, SVG text.
//!
//! Every module becomes a block of whole pixels (nearest neighbor), so edges stay sharp.

use crate::Matrix;

/// How many module-rows tall a 1D bar code is drawn: a quarter of its width, but at least 40.
/// 2D codes are drawn at their own height.
pub fn bar_height_modules(m: &Matrix) -> usize {
    if m.is_1d() { (m.width / 4).max(40) } else { m.height }
}

/// Drawn size in modules (1D codes stretched to [`bar_height_modules`]).
fn drawn_rows(m: &Matrix) -> usize {
    bar_height_modules(m)
}

#[inline]
fn dark_at(m: &Matrix, x: usize, row: usize) -> bool {
    if m.is_1d() { m.get(x, 0) } else { m.get(x, row) }
}

/// Draws the code as RGBA pixels: returns `(width_px, height_px, rgba)`.
/// `module_px` is clamped to 1..=64.
pub fn to_rgba(m: &Matrix, module_px: u32, dark: [u8; 3], light: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let s = module_px.clamp(1, 64) as usize;
    let rows = drawn_rows(m);
    let (w, h) = (m.width * s, rows * s);
    let mut out = Vec::with_capacity(w * h * 4);
    for py in 0..h {
        let row = py / s;
        for px in 0..w {
            let c = if dark_at(m, px / s, row) { dark } else { light };
            out.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    (w as u32, h as u32, out)
}

/// Draws the code as a PNG (1-bit palette: tiny files, perfectly sharp).
/// `module_px` (pixels per module) is clamped to 1..=64. Returns an empty vector only if the
/// PNG writer fails, which does not happen for in-memory output.
pub fn to_png(m: &Matrix, module_px: u32, dark: [u8; 3], light: [u8; 3]) -> Vec<u8> {
    let s = module_px.clamp(1, 64) as usize;
    let rows = drawn_rows(m);
    let (w, h) = (m.width * s, rows * s);
    if w == 0 || h == 0 {
        return Vec::new();
    }
    // Pack one row of 1-bit pixels (palette index 1 = dark), MSB first.
    let stride = w.div_ceil(8);
    let mut data = vec![0u8; stride * h];
    for row in 0..rows {
        let mut line = vec![0u8; stride];
        for px in 0..w {
            if dark_at(m, px / s, row) {
                line[px / 8] |= 0x80 >> (px % 8);
            }
        }
        for k in 0..s {
            let y = row * s + k;
            data[y * stride..(y + 1) * stride].copy_from_slice(&line);
        }
    }

    let mut bytes = Vec::new();
    let ok = (|| -> Result<(), png::EncodingError> {
        let mut enc = png::Encoder::new(&mut bytes, w as u32, h as u32);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::One);
        enc.set_palette(vec![light[0], light[1], light[2], dark[0], dark[1], dark[2]]);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&data)?;
        writer.finish()
    })();
    if ok.is_err() { Vec::new() } else { bytes }
}

/// Draws the code as an SVG document. One user unit = one module; `module_size` sets the
/// `width`/`height` attributes (for example in `mm` or `px` — pass the unit text, e.g. `"0.5mm"`
/// gives each module 0.5 mm). `light: None` leaves the background transparent.
pub fn to_svg(m: &Matrix, module_size: &str, dark: [u8; 3], light: Option<[u8; 3]>) -> String {
    let rows = drawn_rows(m);
    let (num, unit) = split_unit(module_size);
    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{}{unit}\" height=\"{}{unit}\" shape-rendering=\"crispEdges\">",
        fmt_num(num * m.width as f64),
        fmt_num(num * rows as f64),
        w = m.width,
        h = rows
    ));
    if let Some(l) = light {
        s.push_str(&format!("<rect width=\"{}\" height=\"{}\" fill=\"#{:02x}{:02x}{:02x}\"/>", m.width, rows, l[0], l[1], l[2]));
    }
    s.push_str(&format!("<path fill=\"#{:02x}{:02x}{:02x}\" d=\"", dark[0], dark[1], dark[2]));
    let (src_rows, bar_h) = if m.is_1d() { (1, rows) } else { (m.height, 1) };
    for y in 0..src_rows {
        let mut x = 0;
        while x < m.width {
            if m.get(x, y) {
                let start = x;
                while x < m.width && m.get(x, y) {
                    x += 1;
                }
                s.push_str(&format!("M{start} {y}h{}v{bar_h}h-{}z", x - start, x - start));
            } else {
                x += 1;
            }
        }
    }
    s.push_str("\"/></svg>");
    s
}

fn split_unit(text: &str) -> (f64, &str) {
    let t = text.trim();
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    let num = t[..end].parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    (num, &t[end..])
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}
