//! Making barcodes: round trips, Unicode, quiet zones, PNG/SVG output, validation messages.

mod common;
use common::*;
use slate_barcode::*;

#[test]
fn every_symbology_round_trips() {
    for (sym, text) in SAMPLES {
        let (w, h, px) = draw(sym, text, 3);
        let c = padded(w, h, &px, 20);
        let found = decode_rgba(c.w, c.h, &c.px);
        let want = expected(sym, text);
        assert!(found.iter().any(|f| f.text == want && f.symbology == sym.label()), "{sym:?}: wanted {want:?}, got {found:?}");
    }
}

#[test]
fn unicode_round_trips_in_2d_codes() {
    let text = "Grüße — 東京 🚀";
    for sym in [Symbology::Qr, Symbology::DataMatrix, Symbology::Aztec, Symbology::Pdf417] {
        let (w, h, px) = draw(sym, text, 4);
        let c = padded(w, h, &px, 16);
        let found = decode_rgba(c.w, c.h, &c.px);
        assert!(found.iter().any(|f| f.text == text), "{sym:?}: got {found:?}");
    }
}

#[test]
fn latin1_and_long_unicode_round_trip() {
    for text in ["Crème brûlée à la carte", "Ünïcödé ñ ß ø å", &"日本語のテキスト。".repeat(10)] {
        for sym in [Symbology::Qr, Symbology::DataMatrix, Symbology::Aztec, Symbology::Pdf417] {
            let (w, h, px) = draw(sym, text, 3);
            let c = padded(w, h, &px, 16);
            let found = decode_rgba(c.w, c.h, &c.px);
            assert!(found.iter().any(|f| f.text == text), "{sym:?} {text:?}: got {found:?}");
        }
    }
}

#[test]
fn png_round_trips_through_image_bytes() {
    for (sym, text) in SAMPLES {
        let m = encode(sym, text).unwrap_or_else(|e| panic!("{e}"));
        let png = to_png(&m, 4, BLACK, WHITE);
        assert!(png.starts_with(b"\x89PNG"), "{sym:?} not a PNG");
        // decode_image_bytes reads the PNG back (no extra margin: the quiet zone is enough).
        let found = decode_image_bytes(&png);
        let want = expected(sym, text);
        assert!(found.iter().any(|f| f.text == want), "{sym:?} from PNG: got {found:?}");
    }
}

#[test]
fn png_size_and_colors() {
    let m = encode(Symbology::Qr, "hello").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((m.width, m.height), (29, 29), "version 1 QR + 4-module quiet zone");
    let png = to_png(&m, 5, [0x2b, 0x2f, 0x33], [0xf5, 0xf6, 0xf7]);
    let img = image::load_from_memory(&png).unwrap_or_else(|e| panic!("{e}")).to_rgb8();
    assert_eq!((img.width(), img.height()), (145, 145));
    assert_eq!(img.get_pixel(0, 0).0, [0xf5, 0xf6, 0xf7], "quiet zone is light");
    assert_eq!(img.get_pixel(20, 20).0, [0x2b, 0x2f, 0x33], "finder corner is dark");
    // Crisp: only the two colors appear.
    assert!(img.pixels().all(|p| p.0 == [0x2b, 0x2f, 0x33] || p.0 == [0xf5, 0xf6, 0xf7]));
}

#[test]
fn one_d_png_has_bar_height() {
    let m = encode(Symbology::Ean13, "590123412345").unwrap_or_else(|e| panic!("{e}"));
    assert!(m.is_1d());
    assert_eq!(m.width, 95 + 22);
    assert_eq!(bar_height_modules(&m), 40);
    let (w, h, _) = to_rgba(&m, 2, BLACK, WHITE);
    assert_eq!((w, h), (234, 80));
}

#[test]
fn quiet_zone_is_empty() {
    for (sym, text) in SAMPLES {
        let m = encode(sym, text).unwrap_or_else(|e| panic!("{e}"));
        let q = sym.quiet_zone();
        for y in 0..m.height {
            for x in 0..m.width {
                let in_margin = x < q || x >= m.width - q || (sym.is_2d() && (y < q || y >= m.height - q));
                if in_margin {
                    assert!(!m.get(x, y), "{sym:?} dark module in quiet zone at {x},{y}");
                }
            }
        }
        assert_eq!(m.modules.len(), m.width * m.height);
        assert!(m.modules.iter().any(|d| *d));
    }
}

#[test]
fn svg_output() {
    let m = encode(Symbology::Qr, "svg").unwrap_or_else(|e| panic!("{e}"));
    let svg = to_svg(&m, "0.5mm", [0, 0, 0], Some([255, 255, 255]));
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("viewBox=\"0 0 29 29\""));
    assert!(svg.contains("width=\"14.5mm\""));
    assert!(svg.contains("fill=\"#ffffff\""));
    assert!(svg.ends_with("</svg>"));
    let bar = encode(Symbology::Code128, "ABC").unwrap_or_else(|e| panic!("{e}"));
    let svg = to_svg(&bar, "2", [0, 0, 0], None);
    assert!(svg.contains(&format!("viewBox=\"0 0 {} 40\"", bar.width)));
    assert!(!svg.contains("<rect"));
}

#[test]
fn labels_and_hints() {
    assert_eq!(Symbology::Qr.label(), "QR code");
    assert_eq!(Symbology::DataMatrix.label(), "Data Matrix");
    assert_eq!(Symbology::Ean13.label(), "EAN-13");
    assert!(Symbology::Pdf417.is_2d());
    assert!(!Symbology::UpcA.is_2d());
    assert!(Symbology::Ean13.hint().contains("12 digits"));
    for s in Symbology::ALL {
        assert!(!s.hint().is_empty());
    }
}

#[test]
fn validate_messages() {
    fn err(s: Symbology, t: &str) -> String {
        validate(s, t).err().unwrap_or_default()
    }
    assert_eq!(err(Symbology::Ean13, "12345"), "EAN-13 needs 12 or 13 digits.");
    assert_eq!(err(Symbology::Ean13, "59012341234A"), "EAN-13 takes digits only.");
    assert!(err(Symbology::Ean13, "5901234123450").contains("should be 7"));
    assert!(validate(Symbology::Ean13, "5901234123457").is_ok());
    assert!(validate(Symbology::Ean13, "590123412345").is_ok());
    assert_eq!(err(Symbology::UpcA, "123"), "UPC-A needs 11 or 12 digits.");
    assert!(validate(Symbology::UpcA, "036000291452").is_ok());
    assert!(err(Symbology::UpcA, "036000291453").contains("should be 2"));
    assert!(err(Symbology::Code39, "abc").contains("capital letters"));
    assert!(err(Symbology::Code39, "A*B").contains("can't hold \"*\""));
    assert!(err(Symbology::Code128, "café").contains("can't hold \"é\""));
    assert!(err(Symbology::Code128, &"A".repeat(81)).contains("80"));
    assert!(err(Symbology::Qr, "").starts_with("Type something"));
    assert!(err(Symbology::Qr, &"x".repeat(5000)).contains("too much text"));
    assert!(validate(Symbology::Qr, "Grüße 🚀").is_ok());
}

#[test]
fn encode_refuses_bad_input_politely() {
    assert!(encode(Symbology::Ean13, "abc").is_err());
    assert!(encode(Symbology::Code39, "lower").is_err());
    // Near the limit: validate passes, the encoder decides; must not panic either way.
    for sym in [Symbology::Qr, Symbology::DataMatrix, Symbology::Aztec, Symbology::Pdf417] {
        let r = encode(sym, &"€".repeat(700));
        if let Err(e) = r {
            assert!(e.contains(sym.label()), "{e}");
        }
    }
}
