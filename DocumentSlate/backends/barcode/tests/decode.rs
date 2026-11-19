//! Reading barcodes: several per picture, noise, scale, rotation, regions, robustness, speed.

mod common;
use common::*;
use slate_barcode::*;
use std::time::Instant;

#[test]
fn finds_several_codes_with_bounds() {
    let mut c = Canvas::new(1300, 1000, 255);
    let items = [
        (Symbology::Qr, "first code", 60u32, 60u32),
        (Symbology::DataMatrix, "SECOND", 760, 80),
        (Symbology::Ean13, "590123412345", 80, 650),
        (Symbology::Code128, "fourth-128", 620, 650),
        (Symbology::Aztec, "fifth", 450, 380),
    ];
    let mut rects = Vec::new();
    for (sym, text, x, y) in items {
        let (w, h, px) = draw(sym, text, 4);
        rects.push((sym, expected(sym, text), c.paste(x, y, w, h, &px)));
    }
    let found = decode_rgba(c.w, c.h, &c.px);
    for (sym, text, rect) in &rects {
        let f = found.iter().find(|f| &f.text == text).unwrap_or_else(|| panic!("{sym:?} {text:?} not found in {found:?}"));
        assert_eq!(f.symbology, sym.label());
        assert!(center_inside(f.bounds, *rect), "{sym:?} bounds {:?} vs drawn {rect:?}", f.bounds);
        // The box should cover most of the drawn symbol, not just a sliver.
        let fw = f.bounds[2] - f.bounds[0];
        assert!(fw > (rect[2] - rect[0]) * 0.5, "{sym:?} box too narrow: {:?} vs {rect:?}", f.bounds);
        if !sym.is_2d() {
            // 1D boxes are measured: bars start after the 11-module (44 px) quiet zone and
            // run the full drawn height.
            let q = 44.0;
            let want = [rect[0] + q, rect[1], rect[2] - q, rect[3]];
            for k in 0..4 {
                assert!((f.bounds[k] - want[k]).abs() <= 2.0, "{sym:?} bounds {:?} vs bars {want:?}", f.bounds);
            }
        }
    }
    assert_eq!(found.len(), rects.len(), "no duplicates: {found:?}");
}

#[test]
fn finds_several_qr_codes() {
    let mut c = Canvas::new(1080, 1500, 255);
    let mut want = Vec::new();
    for i in 0..6u32 {
        let text = format!("https://example.com/item/{i}");
        let (w, h, px) = draw(Symbology::Qr, &text, 4);
        c.paste(80 + (i % 2) * 520, 80 + (i / 2) * 470, w, h, &px);
        want.push(text);
    }
    let found = decode_rgba(c.w, c.h, &c.px);
    for t in &want {
        assert!(found.iter().any(|f| &f.text == t), "missing {t}: {found:?}");
    }
}

#[test]
fn scaled_noisy_offset_code_on_page() {
    // A page-sized gray picture with noise, the QR at 3x and an odd offset.
    let (w, h, px) = draw(Symbology::Qr, "noisy page code 3x", 3);
    let mut c = Canvas::new(1080, 1500, 235);
    let rect = c.paste(437, 911, w, h, &px);
    c.noise(40, 7);
    let found = decode_rgba(c.w, c.h, &c.px);
    let f = found.iter().find(|f| f.text == "noisy page code 3x").unwrap_or_else(|| panic!("{found:?}"));
    assert!(center_inside(f.bounds, rect), "{:?} vs {rect:?}", f.bounds);
    // Same through the luma entry point.
    let found = decode_luma(c.w, c.h, &c.luma());
    assert!(found.iter().any(|f| f.text == "noisy page code 3x"));
}

#[test]
fn fractional_scale_codes() {
    // Pages render at arbitrary zoom: 2.5 px per module, nearest-neighbor (uneven modules).
    for (sym, text) in SAMPLES {
        let (w, h, px) = draw(sym, text, 1);
        let (w, h, px) = scale(w, h, &px, 2.5);
        let c = padded(w, h, &px, 30);
        let found = decode_rgba(c.w, c.h, &c.px);
        assert!(found.iter().any(|f| f.text == expected(sym, text)), "{sym:?} at 2.5x: {found:?}");
    }
}

#[test]
fn rotated_codes() {
    for (sym, text) in SAMPLES {
        let (w, h, px) = draw(sym, text, 3);
        let (w, h, px) = rot90(w, h, &px);
        let c = padded(w, h, &px, 30);
        let found = decode_rgba(c.w, c.h, &c.px);
        let ok = found.iter().any(|f| f.text == expected(sym, text));
        println!("rotated 90: {sym:?} -> {}", if ok { "found" } else { "NOT found" });
        if sym == Symbology::Qr || sym.is_2d() {
            assert!(ok, "{sym:?} rotated: {found:?}");
        }
    }
}

#[test]
fn inverted_code_found() {
    let m = encode(Symbology::Qr, "light on dark").unwrap_or_else(|e| panic!("{e}"));
    let (w, h, px) = to_rgba(&m, 4, WHITE, BLACK);
    let mut c = Canvas::new(w + 40, h + 40, 0);
    c.paste(20, 20, w, h, &px);
    let found = decode_rgba(c.w, c.h, &c.px);
    assert!(found.iter().any(|f| f.text == "light on dark"), "{found:?}");
}

#[test]
fn region_maps_coordinates_back() {
    let mut c = Canvas::new(1000, 800, 255);
    let (w, h, px) = draw(Symbology::Qr, "in the region", 4);
    let rect = c.paste(600, 450, w, h, &px);
    let (w2, h2, px2) = draw(Symbology::Qr, "outside", 4);
    c.paste(40, 40, w2, h2, &px2);
    let found = decode_region(c.w, c.h, &c.px, [550, 400, 300, 300]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].text, "in the region");
    assert!(center_inside(found[0].bounds, rect), "{:?} vs {rect:?}", found[0].bounds);
    for p in &found[0].corners {
        assert!(p[0] >= rect[0] && p[0] <= rect[2] && p[1] >= rect[1] && p[1] <= rect[3]);
    }
    // Regions sticking out of the picture are clipped; empty ones give nothing.
    assert!(!decode_region(c.w, c.h, &c.px, [550, 400, 9999, 9999]).is_empty());
    assert!(decode_region(c.w, c.h, &c.px, [2000, 2000, 10, 10]).is_empty());
    assert!(decode_region(c.w, c.h, &c.px, [10, 10, 0, 0]).is_empty());
}

#[test]
fn nothing_found_in_plain_or_text_like_pictures() {
    let c = Canvas::new(400, 300, 255);
    assert!(decode_rgba(c.w, c.h, &c.px).is_empty());
    let mut c = Canvas::new(400, 300, 128);
    c.noise(100, 3);
    assert!(decode_rgba(c.w, c.h, &c.px).is_empty());
}

#[test]
fn no_panic_on_bad_input() {
    assert!(decode_rgba(0, 0, &[]).is_empty());
    assert!(decode_rgba(1, 1, &[0, 0, 0, 255]).is_empty());
    assert!(decode_rgba(7, 7, &[0; 7 * 7 * 4]).is_empty());
    assert!(decode_rgba(8, 8, &[0; 8 * 8 * 4]).is_empty());
    assert!(decode_rgba(100, 100, &[0; 10]).is_empty(), "buffer too short");
    assert!(decode_rgba(u32::MAX, u32::MAX, &[0; 16]).is_empty());
    assert!(decode_luma(0, 10, &[]).is_empty());
    assert!(decode_luma(3, 3, &[1, 2, 3]).is_empty());
    assert!(decode_region(0, 0, &[], [0, 0, 5, 5]).is_empty());
    assert!(decode_region(u32::MAX, 2, &[0; 8], [0, 0, u32::MAX, u32::MAX]).is_empty());
    assert!(decode_image_bytes(&[]).is_empty());
    assert!(decode_image_bytes(b"\x89PNG\r\n\x1a\n garbage").is_empty());
    let mut r = Rng(99);
    for size in [1usize, 16, 100, 1000, 5000] {
        let bytes: Vec<u8> = (0..size).map(|_| r.next() as u8).collect();
        assert!(decode_image_bytes(&bytes).is_empty());
    }
    for (w, h) in [(8u32, 8u32), (13, 200), (200, 13), (64, 64), (321, 123)] {
        let bytes: Vec<u8> = (0..w * h * 4).map(|_| r.next() as u8).collect();
        let _ = decode_rgba(w, h, &bytes);
        let _ = decode_luma(w, h, &bytes[..(w * h) as usize]);
    }
    // A truncated real PNG.
    let m = encode(Symbology::Qr, "cut").unwrap_or_else(|e| panic!("{e}"));
    let png = to_png(&m, 3, BLACK, WHITE);
    assert!(decode_image_bytes(&png[..png.len() / 2]).is_empty());
}

#[test]
fn transparent_png_reads_as_white_background() {
    // Dark modules on transparent pixels: alpha 0 must count as white.
    let (w, h, mut px) = draw(Symbology::Qr, "transparent", 4);
    for p in px.as_chunks_mut::<4>().0 {
        if p[0] == 255 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        }
    }
    let found = decode_rgba(w, h, &px);
    assert!(found.iter().any(|f| f.text == "transparent"), "{found:?}");
}

/// Finds the smallest pixels-per-module that still decodes, per symbology (printed; run with
/// `--nocapture`). Asserts the values the app relies on: 2 px/module reads everything.
#[test]
fn min_pixels_per_module() {
    for (sym, text) in SAMPLES {
        let mut results = Vec::new();
        for f in [1.0f32, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            let (w, h, px) = draw(sym, text, 1);
            let (w, h, px) = scale(w, h, &px, f);
            let c = padded(w, h, &px, 20);
            let ok = decode_rgba(c.w, c.h, &c.px).iter().any(|r| r.text == expected(sym, text));
            results.push(format!("{f}:{}", if ok { "ok" } else { "--" }));
            if f >= 2.0 {
                assert!(ok, "{sym:?} should read at {f} px/module");
            }
        }
        println!("min ppm {:<10} {}", sym.label(), results.join(" "));
    }
}

/// Timing on a phone-page-sized picture. Numbers are printed; run in release with
/// `cargo test --release --test decode decode_speed -- --nocapture`.
#[test]
fn decode_speed() {
    let blank = Canvas::new(1080, 1500, 255);
    let mut text_like = Canvas::new(1080, 1500, 255);
    // Fake text lines: short dark dashes in rows, like a page of text.
    let mut r = Rng(5);
    for line in 0..60u32 {
        let y = 60 + line * 22;
        let mut x = 60u32;
        while x < 1000 {
            let len = 8 + (r.next() % 40) as u32;
            let dash = vec![0u8; (len * 10 * 4) as usize];
            let mut dash = dash;
            for p in dash.as_chunks_mut::<4>().0 {
                p[3] = 255;
            }
            text_like.paste(x, y, len, 10, &dash);
            x += len + 6 + (r.next() % 8) as u32;
        }
    }
    let mut one_qr = Canvas::new(1080, 1500, 255);
    let (w, h, px) = draw(Symbology::Qr, "https://example.com/a/fairly/long/link?with=query", 4);
    one_qr.paste(700, 1100, w, h, &px);
    let mut mixed = one_qr.px.clone();
    let (w, h, px) = draw(Symbology::Ean13, "590123412345", 3);
    let mut mixed_c = Canvas { w: 1080, h: 1500, px: std::mem::take(&mut mixed) };
    mixed_c.paste(80, 200, w, h, &px);

    for (name, c) in [("blank page", &blank), ("text-like page", &text_like), ("page with 1 QR", &one_qr), ("page with QR + EAN", &mixed_c)] {
        let t = Instant::now();
        let n = decode_rgba(c.w, c.h, &c.px).len();
        println!("decode 1080x1500 {name}: {n} found in {:?}", t.elapsed());
    }
    let crop_t = Instant::now();
    let n = decode_region(one_qr.w, one_qr.h, &one_qr.px, [650, 1050, 300, 300]).len();
    println!("decode_region 300x300 around the QR: {n} found in {:?}", crop_t.elapsed());
}
