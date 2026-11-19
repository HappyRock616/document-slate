//! Codes anywhere on a phone-sized page (1080 x 1500): corners, edges, middle, at 2 and 3 px
//! per module. Data Matrix and especially Aztec detectors look at the middle of the picture
//! first; the tile and bull's-eye passes in `decode.rs` must still find them near the edges.

mod common;
use common::*;
use slate_barcode::*;

#[test]
fn codes_anywhere_on_a_page() {
    let long = "Full-range symbol with enough text to need a bigger size, 0123456789. ".repeat(3);
    let cases = [
        (Symbology::Aztec, "fifth"),
        (Symbology::Aztec, long.as_str()),
        (Symbology::DataMatrix, "dm small"),
        (Symbology::DataMatrix, long.as_str()),
        (Symbology::Pdf417, "pdf417 on page"),
        (Symbology::Qr, long.as_str()),
        (Symbology::Code39, "EDGE-39"),
    ];
    for (sym, text) in cases {
        for (x, y) in [(20u32, 20u32), (900, 40), (500, 700), (40, 1300), (850, 1250)] {
            for s in [2u32, 3] {
                let (w, h, px) = draw(sym, text, s);
                let mut c = Canvas::new(1080, 1500, 255);
                let (x, y) = (x.min(1080 - w), y.min(1500 - h));
                let rect = c.paste(x, y, w, h, &px);
                let found = decode_rgba(c.w, c.h, &c.px);
                let f =
                    found.iter().find(|f| f.text == text).unwrap_or_else(|| panic!("{sym:?} ({} chars) at ({x},{y}) {s}px: {found:?}", text.len()));
                assert!(center_inside(f.bounds, rect), "{sym:?} bounds {:?} vs {rect:?}", f.bounds);
            }
        }
    }
}
