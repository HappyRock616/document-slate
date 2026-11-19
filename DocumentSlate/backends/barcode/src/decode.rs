//! Reading barcodes: find every barcode in a picture and return its text and position.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use rxing::common::HybridBinarizer;
use rxing::multi::{GenericMultipleBarcodeReader, MultipleBarcodeReader};
use rxing::{BarcodeFormat, BinaryBitmap, DecodeHints, Luma8LuminanceSource, MultiUseMultiFormatReader, RXingResult};

/// One barcode found in a picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Human name of the format: "QR code", "EAN-13", "Data Matrix", ...
    pub symbology: String,
    /// The decoded text.
    pub text: String,
    /// Corner points in pixels of the input picture: for the 2D codes the symbol's corners as the
    /// reader located them (usually four, without the quiet zone; may be a tilted shape); for
    /// 1D codes the four corners of the measured bar area (see `bounds`).
    pub corners: Vec<[f32; 2]>,
    /// `[x0, y0, x1, y1]` box around the code, in pixels of the input picture. It is the box
    /// around `corners`, padded 3 % for 2D codes. For 1D codes it is measured on the
    /// picture: from the first to the last bar, and as tall as the bars are. Clamped to the picture. Good for a
    /// highlight frame or a tap target, not for exact geometry.
    pub bounds: [f32; 4],
}

/// The format name shown to people.
fn label(f: &BarcodeFormat) -> &'static str {
    match f {
        BarcodeFormat::QR_CODE => "QR code",
        BarcodeFormat::MICRO_QR_CODE => "Micro QR code",
        BarcodeFormat::RECTANGULAR_MICRO_QR_CODE => "rMQR code",
        BarcodeFormat::DATA_MATRIX => "Data Matrix",
        BarcodeFormat::AZTEC => "Aztec",
        BarcodeFormat::PDF_417 => "PDF417",
        BarcodeFormat::MAXICODE => "MaxiCode",
        BarcodeFormat::CODE_128 => "Code 128",
        BarcodeFormat::CODE_39 => "Code 39",
        BarcodeFormat::CODE_93 => "Code 93",
        BarcodeFormat::CODABAR => "Codabar",
        BarcodeFormat::EAN_8 => "EAN-8",
        BarcodeFormat::EAN_13 => "EAN-13",
        BarcodeFormat::UPC_A => "UPC-A",
        BarcodeFormat::UPC_E => "UPC-E",
        BarcodeFormat::UPC_EAN_EXTENSION => "UPC/EAN add-on",
        BarcodeFormat::ITF => "ITF",
        BarcodeFormat::RSS_14 => "GS1 DataBar",
        BarcodeFormat::RSS_EXPANDED => "GS1 DataBar Expanded",
        BarcodeFormat::TELEPEN => "Telepen",
        _ => "Barcode",
    }
}

fn is_1d(f: &BarcodeFormat) -> bool {
    !matches!(
        f,
        BarcodeFormat::QR_CODE
            | BarcodeFormat::MICRO_QR_CODE
            | BarcodeFormat::RECTANGULAR_MICRO_QR_CODE
            | BarcodeFormat::DATA_MATRIX
            | BarcodeFormat::AZTEC
            | BarcodeFormat::PDF_417
            | BarcodeFormat::MAXICODE
    )
}

/// Finds all barcodes in an 8-bit grayscale picture (`luma.len() >= width * height`, row-major,
/// 0 = black). Tries every format, works hard (rotated 1D, several rows), and also tries the
/// inverted picture (light code on dark background) when the normal pass finds nothing.
/// Returns an empty list when nothing is found or the input is unusable. Never panics.
pub fn decode_luma(width: u32, height: u32, luma: &[u8]) -> Vec<Found> {
    let (w, h) = (width as usize, height as usize);
    let Some(n) = w.checked_mul(h) else { return Vec::new() };
    if w < 8 || h < 8 || luma.len() < n {
        return Vec::new();
    }
    let found = find_all(width, height, luma[..n].to_vec());
    if !found.is_empty() {
        return found;
    }
    find_all(width, height, luma[..n].iter().map(|v| 255 - v).collect())
}

/// Most passes over one picture. Each pass finds at least one new code, or we stop.
const MAX_PASSES: usize = 8;

/// Finds codes, paints them out, and looks again, until a pass finds nothing new.
///
/// rxing's multi reader already splits the picture around each code it finds, but it misses
/// codes that sit beside each other (it stops early at sub-pictures where the first format it
/// tries finds nothing usable). Erasing what was found and looking again is simple and catches
/// them; a page with no code costs a single pass.
fn find_all(width: u32, height: u32, mut pixels: Vec<u8>) -> Vec<Found> {
    let (fw, fh) = (width as f32, height as f32);
    let mut out: Vec<Found> = Vec::new();
    for _ in 0..MAX_PASSES {
        let results = one_pass(width, height, &pixels);
        let mut new_any = false;
        for r in &results {
            let mut f = to_found(r, fw, fh);
            if is_1d(r.getBarcodeFormat()) {
                refine_1d(&pixels, width as usize, height as usize, &mut f);
            }
            erase(&mut pixels, width as usize, height as usize, &f);
            let dupe = out.iter().any(|o| o.text == f.text && o.symbology == f.symbology && overlaps(&o.bounds, &f.bounds));
            if !dupe {
                out.push(f);
                new_any = true;
            }
        }
        if !new_any {
            break;
        }
    }
    tile_pass(width, height, &mut pixels, &mut out);
    aztec_pass(width, height, &mut pixels, &mut out);
    // Top-to-bottom, left-to-right: the order a reader would expect.
    out.sort_by(|a, b| {
        let ka = ((a.bounds[1] / 16.0) as i64, a.bounds[0] as i64);
        let kb = ((b.bounds[1] / 16.0) as i64, b.bounds[0] as i64);
        ka.cmp(&kb)
    });
    out
}

/// The Data Matrix (and to a lesser degree QR) detectors start looking at the middle of the
/// picture, so a small code near the edge of a big page can be missed. As a last step we look
/// at overlapping tiles (half the short side, half-overlapping) for just those 2D formats. This
/// is cheap compared with the full pass, which also scans every row for 1D codes.
fn tile_pass(width: u32, height: u32, pixels: &mut [u8], out: &mut Vec<Found>) {
    let (w, h) = (width as usize, height as usize);
    let tile = (w.min(h) / 2).max(TILE_MIN);
    if w <= tile && h <= tile {
        return; // the whole picture is already tile-sized
    }
    let step = tile / 2;
    let starts = |len: usize| -> Vec<usize> {
        if len <= tile {
            return vec![0];
        }
        let mut v: Vec<usize> = (0..).map(|i| i * step).take_while(|s| s + tile < len).collect();
        v.push(len - tile);
        v
    };
    let formats = HashSet::from([BarcodeFormat::DATA_MATRIX, BarcodeFormat::QR_CODE]);
    for ty in starts(h) {
        for tx in starts(w) {
            let (tw, th) = (tile.min(w), tile.min(h));
            let mut crop = Vec::with_capacity(tw * th);
            for y in ty..ty + th {
                crop.extend_from_slice(&pixels[y * w + tx..y * w + tx + tw]);
            }
            for r in one_pass_formats(tw as u32, th as u32, &crop, Some(&formats)) {
                let mut f = to_found(&r, tw as f32, th as f32);
                for c in &mut f.corners {
                    c[0] += tx as f32;
                    c[1] += ty as f32;
                }
                f.bounds = [f.bounds[0] + tx as f32, f.bounds[1] + ty as f32, f.bounds[2] + tx as f32, f.bounds[3] + ty as f32];
                erase(pixels, w, h, &f);
                if !out.iter().any(|o| o.text == f.text && o.symbology == f.symbology && overlaps(&o.bounds, &f.bounds)) {
                    out.push(f);
                }
            }
        }
    }
}

/// The Aztec detector only works when the middle of the picture falls inside the symbol's
/// bull's-eye area, so on a page it finds almost nothing by itself. Here we look for the
/// bull's-eye ourselves (a dark center with alternating light/dark rings: nine equal runs across
/// and down) and decode a few square crops centered on each candidate.
fn aztec_pass(width: u32, height: u32, pixels: &mut [u8], out: &mut Vec<Found>) {
    let (w, h) = (width as usize, height as usize);
    let formats = HashSet::from([BarcodeFormat::AZTEC]);
    for (cx, cy, module) in bullseye_candidates(pixels, w, h) {
        // Compact Aztec is 15-27 modules, full-range up to 151; try growing crops.
        for modules in [32.0f32, 64.0, 128.0, 170.0] {
            let side = (modules * module).ceil() as usize;
            let half = side / 2;
            let x0 = cx.saturating_sub(half);
            let y0 = cy.saturating_sub(half);
            let x1 = (cx + half).min(w);
            let y1 = (cy + half).min(h);
            if x1 <= x0 + 8 || y1 <= y0 + 8 {
                continue;
            }
            let (tw, th) = (x1 - x0, y1 - y0);
            let mut crop = Vec::with_capacity(tw * th);
            for y in y0..y1 {
                crop.extend_from_slice(&pixels[y * w + x0..y * w + x1]);
            }
            let results = one_pass_formats(tw as u32, th as u32, &crop, Some(&formats));
            if results.is_empty() {
                continue;
            }
            for r in results {
                let mut f = to_found(&r, tw as f32, th as f32);
                for c in &mut f.corners {
                    c[0] += x0 as f32;
                    c[1] += y0 as f32;
                }
                f.bounds = [f.bounds[0] + x0 as f32, f.bounds[1] + y0 as f32, f.bounds[2] + x0 as f32, f.bounds[3] + y0 as f32];
                erase(pixels, w, h, &f);
                if !out.iter().any(|o| o.text == f.text && o.symbology == f.symbology && overlaps(&o.bounds, &f.bounds)) {
                    out.push(f);
                }
            }
            break;
        }
    }
}

/// Most bull's-eye candidates tried per picture (each costs a few small decodes).
const MAX_BULLSEYES: usize = 40;

/// Finds possible Aztec centers: `(x, y, module_px)`.
fn bullseye_candidates(pixels: &[u8], w: usize, h: usize) -> Vec<(usize, usize, f32)> {
    let mut found: Vec<(usize, usize, f32)> = Vec::new();
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (start, len)
    for y in (0..h).step_by(2) {
        let row = &pixels[y * w..(y + 1) * w];
        runs.clear();
        let mut start = 0;
        for x in 1..=w {
            if x == w || (row[x] < 128) != (row[start] < 128) {
                runs.push((start, x - start));
                start = x;
            }
        }
        for i in 0..runs.len().saturating_sub(8) {
            if row[runs[i].0] >= 128 {
                continue; // must start with a dark ring
            }
            let lens: Vec<usize> = runs[i..i + 9].iter().map(|r| r.1).collect();
            let Some(avg) = equal_runs(&lens) else { continue };
            let cx = runs[i + 4].0 + runs[i + 4].1 / 2;
            if found.iter().any(|&(fx, fy, m)| fx.abs_diff(cx) as f32 <= 6.0 * m && fy.abs_diff(y) as f32 <= 6.0 * m) {
                continue;
            }
            if let Some(cy) = vertical_bullseye(pixels, w, h, cx, y, avg) {
                found.push((cx, cy, avg));
                if found.len() >= MAX_BULLSEYES {
                    return found;
                }
            }
        }
    }
    found
}

/// The bull's-eye seen across its center: dark, then seven runs of about equal length (light,
/// dark, light, *dark center*, light, dark, light), then dark. The two outer dark runs are the
/// outermost ring, which can touch dark data modules, so they only need to be long enough.
/// Returns the module size.
fn equal_runs(lens: &[usize]) -> Option<f32> {
    let inner = &lens[1..8];
    let avg = inner.iter().sum::<usize>() as f32 / inner.len() as f32;
    let ok = avg >= 1.0
        && inner.iter().all(|&l| (l as f32) >= avg * 0.5 && (l as f32) <= avg * 1.5)
        && lens[0] as f32 >= avg * 0.5
        && lens[8] as f32 >= avg * 0.5;
    ok.then_some(avg)
}

/// Checks the column through `(cx, y)` for the same nine-run pattern; returns the center row.
fn vertical_bullseye(pixels: &[u8], w: usize, h: usize, cx: usize, y: usize, avg: f32) -> Option<usize> {
    let reach = (avg * 8.0).ceil() as usize;
    let y0 = y.saturating_sub(reach);
    let y1 = (y + reach).min(h);
    let dark = |yy: usize| pixels[yy * w + cx] < 128;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = y0;
    for yy in y0 + 1..=y1 {
        if yy == y1 || dark(yy) != dark(start) {
            runs.push((start, yy - start));
            start = yy;
        }
    }
    for i in 0..runs.len().saturating_sub(8) {
        if !dark(runs[i].0) {
            continue;
        }
        let mid = runs[i + 4];
        if y < mid.0 || y >= mid.0 + mid.1 {
            continue;
        }
        let lens: Vec<usize> = runs[i..i + 9].iter().map(|r| r.1).collect();
        if let Some(a) = equal_runs(&lens)
            && a >= avg * 0.6
            && a <= avg * 1.6
        {
            return Some(mid.0 + mid.1 / 2);
        }
    }
    None
}

/// Smallest tile edge for [`tile_pass`], in pixels.
const TILE_MIN: usize = 256;

fn one_pass(width: u32, height: u32, pixels: &[u8]) -> Vec<RXingResult> {
    one_pass_formats(width, height, pixels, None)
}

fn one_pass_formats(width: u32, height: u32, pixels: &[u8], formats: Option<&HashSet<BarcodeFormat>>) -> Vec<RXingResult> {
    catch_unwind(AssertUnwindSafe(|| -> Vec<RXingResult> {
        let Ok(source) = Luma8LuminanceSource::new(pixels.to_vec(), width, height) else { return Vec::new() };
        let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
        let hints = DecodeHints { TryHarder: Some(true), PossibleFormats: formats.cloned(), ..Default::default() };
        let mut reader = GenericMultipleBarcodeReader::new(MultiUseMultiFormatReader::default());
        reader.decode_multiple_with_hints(&mut bitmap, &hints).unwrap_or_default()
    }))
    .unwrap_or_default()
}

/// Paints a found code's box white so the next pass does not see it again.
fn erase(pixels: &mut [u8], w: usize, h: usize, f: &Found) {
    let x0 = (f.bounds[0].floor().max(0.0) as usize).min(w);
    let y0 = (f.bounds[1].floor().max(0.0) as usize).min(h);
    let x1 = (f.bounds[2].ceil().max(0.0) as usize).min(w);
    let y1 = (f.bounds[3].ceil().max(0.0) as usize).min(h);
    for y in y0..y1 {
        if let Some(row) = pixels.get_mut(y * w + x0..y * w + x1) {
            row.fill(255);
        }
    }
}

/// Replaces a 1D code's rough box with the real extent of its bars.
///
/// The reader reports two points on one scan line, and for some formats (Code 39 in rxing
/// 0.9.4) the end point is wrong. Starting from the first point we walk along the scan line
/// over bars and gaps until a light stretch as wide as a quiet zone, both ways; then we grow the
/// box across the line for as long as the rows look like the scan line. Corners become the four
/// corners of that box. If anything looks off, the rough box is kept.
fn refine_1d(pixels: &[u8], w: usize, h: usize, f: &mut Found) {
    let [Some(p0), Some(p1)] = [f.corners.first().copied(), f.corners.get(1).copied()] else { return };
    let horizontal = (p1[0] - p0[0]).abs() >= (p1[1] - p0[1]).abs();
    // Work in "line" coordinates: u along the scan line, v across it.
    let (len_u, len_v) = if horizontal { (w, h) } else { (h, w) };
    let dark = |u: usize, v: usize| -> bool {
        let (x, y) = if horizontal { (u, v) } else { (v, u) };
        pixels.get(y * w + x).is_some_and(|p| *p < 128)
    };
    let (u0, v0) = if horizontal { (p0[0], p0[1]) } else { (p0[1], p0[0]) };
    let (u1, _) = if horizontal { (p1[0], p1[1]) } else { (p1[1], p1[0]) };
    if !(u0.is_finite() && v0.is_finite() && u1.is_finite()) || u0 < 0.0 || v0 < 0.0 {
        return;
    }
    let v = (v0.round() as usize).min(len_v.saturating_sub(1));
    let mut u = (u0.round() as usize).min(len_u.saturating_sub(1));
    // Step onto the nearest bar (the point can sit in the light just before the start bar).
    let mut tries = 0;
    while u + 1 < len_u && !dark(u, v) && tries < 8 {
        u += 1;
        tries += 1;
    }
    if !dark(u, v) {
        return;
    }
    // Narrowest run near the start gives the module size.
    let mut narrow = usize::MAX;
    let mut a = u;
    for _ in 0..12 {
        let c = dark(a, v);
        let s = a;
        while a < len_u && dark(a, v) == c {
            a += 1;
        }
        narrow = narrow.min(a - s);
        if a >= len_u {
            break;
        }
    }
    // A quiet zone is at least 6 narrow bars wide. On small, smoothed pictures the narrowest run
    // can read as 1 px while modules are ~2 px, which made a 4-module space inside an EAN-13
    // look like the end; so also require ~6 % of the reader's own extent (quiet zones are ≥ 10 %).
    let reported = (u1 - u0).abs() as usize;
    let gap = (narrow.max(1) * 6).max(reported * 6 / 100).max(4);
    // Walk right: stop at a light run of `gap` or more.
    let mut right = u;
    let mut x = u;
    while x < len_u {
        if dark(x, v) {
            right = x;
            x += 1;
        } else {
            let s = x;
            while x < len_u && !dark(x, v) {
                x += 1;
            }
            if x - s >= gap || x >= len_u {
                break;
            }
        }
    }
    // Walk left the same way.
    let mut left = u;
    let mut x = u as isize;
    while x >= 0 {
        if dark(x as usize, v) {
            left = x as usize;
            x -= 1;
        } else {
            let s = x;
            while x >= 0 && !dark(x as usize, v) {
                x -= 1;
            }
            if s - x >= gap as isize || x < 0 {
                break;
            }
        }
    }
    if right <= left + 8 {
        return;
    }
    // Grow across the line while rows match the scan line (>= 85 % of pixels equal).
    let row_matches = |vv: usize| -> bool {
        let same = (left..=right).filter(|&uu| dark(uu, vv) == dark(uu, v)).count();
        same * 100 >= (right - left + 1) * 85
    };
    let reach = (right - left) * 2;
    let mut top = v;
    while top > 0 && v - top < reach && row_matches(top - 1) {
        top -= 1;
    }
    let mut bottom = v;
    while bottom + 1 < len_v && bottom - v < reach && row_matches(bottom + 1) {
        bottom += 1;
    }
    let (l, r, t, b) = (left as f32, (right + 1) as f32, top as f32, (bottom + 1) as f32);
    let (x0, y0, x1, y1) = if horizontal { (l, t, r, b) } else { (t, l, b, r) };
    f.corners = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    f.bounds = [x0, y0, x1, y1];
}

fn overlaps(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn to_found(r: &RXingResult, w: f32, h: f32) -> Found {
    let format = r.getBarcodeFormat();
    let corners: Vec<[f32; 2]> = r.getPoints().iter().filter(|p| p.x.is_finite() && p.y.is_finite()).map(|p| [p.x, p.y]).collect();
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for c in &corners {
        b[0] = b[0].min(c[0]);
        b[1] = b[1].min(c[1]);
        b[2] = b[2].max(c[0]);
        b[3] = b[3].max(c[1]);
    }
    if corners.is_empty() {
        b = [0.0, 0.0, w, h];
    } else {
        let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
        let (px, py) = if is_1d(format) {
            // Points lie on one scan line across the bars; give the box some height.
            let len = bw.max(bh);
            if bw >= bh { (len * 0.04, len * 0.15) } else { (len * 0.15, len * 0.04) }
        } else {
            // 2D readers report the symbol's corners (QR: the outer corners of the finder
            // squares); a small margin covers rounding.
            let side = bw.max(bh);
            (side * 0.03, side * 0.03)
        };
        b = [b[0] - px, b[1] - py, b[2] + px, b[3] + py];
    }
    let bounds = [b[0].clamp(0.0, w), b[1].clamp(0.0, h), b[2].clamp(0.0, w), b[3].clamp(0.0, h)];
    Found { symbology: label(format).to_string(), text: r.getText().to_string(), corners, bounds }
}

/// RGBA (or RGBX) pixels to 8-bit luma; transparent pixels count as white (a PNG with a
/// transparent background shows on a white page).
fn rgba_to_luma(rgba: &[u8], n: usize) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .take(n)
        .map(|p| {
            let l = (u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29) >> 8;
            let a = u32::from(p[3]);
            (255 - (a * (255 - l)) / 255) as u8
        })
        .collect()
}

/// Finds all barcodes in an RGBA picture (`rgba.len() >= width * height * 4`). See
/// [`decode_luma`].
pub fn decode_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<Found> {
    let Some(n) = (width as usize).checked_mul(height as usize) else { return Vec::new() };
    if n.checked_mul(4).is_none_or(|need| rgba.len() < need) {
        return Vec::new();
    }
    decode_luma(width, height, &rgba_to_luma(rgba, n))
}

/// Decodes only `region = [x, y, w, h]` (pixels) of an RGBA picture, and reports positions in
/// the coordinates of the *whole* picture. The region is clipped to the picture.
///
/// Typical use: the app re-renders the tapped area of a page at higher resolution and calls
/// [`decode_rgba`] on that crop; or it already has a big render and wants to look at one area
/// (faster, and small codes are not lost among the rest of the page).
pub fn decode_region(width: u32, height: u32, rgba: &[u8], region: [u32; 4]) -> Vec<Found> {
    let (w, h) = (width as usize, height as usize);
    let Some(n) = w.checked_mul(h) else { return Vec::new() };
    if n.checked_mul(4).is_none_or(|need| rgba.len() < need) {
        return Vec::new();
    }
    let x0 = (region[0] as usize).min(w);
    let y0 = (region[1] as usize).min(h);
    let x1 = x0.saturating_add(region[2] as usize).min(w);
    let y1 = y0.saturating_add(region[3] as usize).min(h);
    let (cw, ch) = (x1 - x0, y1 - y0);
    if cw == 0 || ch == 0 {
        return Vec::new();
    }
    let mut crop = Vec::with_capacity(cw * ch * 4);
    for y in y0..y1 {
        let start = (y * w + x0) * 4;
        crop.extend_from_slice(&rgba[start..start + cw * 4]);
    }
    let (dx, dy) = (x0 as f32, y0 as f32);
    decode_rgba(cw as u32, ch as u32, &crop)
        .into_iter()
        .map(|mut f| {
            for c in &mut f.corners {
                c[0] += dx;
                c[1] += dy;
            }
            f.bounds = [f.bounds[0] + dx, f.bounds[1] + dy, f.bounds[2] + dx, f.bounds[3] + dy];
            f
        })
        .collect()
}

/// Decodes a PNG or JPEG file (bytes) and finds all barcodes in it. Positions are in pixels of
/// the decoded picture. Unreadable or huge files (over 40 megapixels) give an empty list.
pub fn decode_image_bytes(bytes: &[u8]) -> Vec<Found> {
    let img = catch_unwind(|| {
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
        let (w, h) = reader.into_dimensions().ok()?;
        if u64::from(w) * u64::from(h) > 40_000_000 {
            return None;
        }
        image::load_from_memory(bytes).ok()
    });
    let Ok(Some(img)) = img else { return Vec::new() };
    let rgba = img.to_rgba8();
    decode_rgba(rgba.width(), rgba.height(), rgba.as_raw())
}
