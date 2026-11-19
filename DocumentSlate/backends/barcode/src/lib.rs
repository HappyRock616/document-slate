//! # slate-barcode — barcodes and QR codes for Document Slate
//!
//! A small back-end that the Android app (egui) calls. It has no UI code and knows nothing
//! about egui or Android: pictures go in and out as plain pixel buffers or PNG bytes.
//!
//! What it does, in plain words:
//!
//! * **Make a barcode** ([`encode`]): pick a [`Symbology`] (QR code, Data Matrix, Aztec, PDF417,
//!   Code 128, Code 39, EAN-13, UPC-A), give it text, get back a [`Matrix`] of dark/light
//!   modules with the standard quiet zone already around it. [`to_png`] and [`to_svg`] turn the
//!   matrix into a crisp picture (each module is a whole number of pixels, no smoothing), ready
//!   to insert into a document. [`validate`] checks the text first and returns a friendly message
//!   ("EAN-13 needs 12 or 13 digits") the app can show under the input field.
//!   Text in the 2D codes can be any Unicode (accents, CJK, emoji): it is stored as UTF-8 with an
//!   ECI marker, which every modern phone scanner understands. Plain ASCII text is stored without
//!   the marker, for the widest compatibility.
//! * **Read barcodes** ([`decode_rgba`], [`decode_luma`], [`decode_region`],
//!   [`decode_image_bytes`]): finds *every* barcode in a picture (for example a rendered page)
//!   in all common formats — 1D (Code 128/39/93, Codabar, EAN/UPC, ITF, GS1 DataBar), QR,
//!   Data Matrix, Aztec, PDF417, MaxiCode — and reports each one's text, format name and where it
//!   sits in the picture ([`Found`]). It never panics: bad input simply gives an empty list.
//! * **Understand what was read** ([`classify`], [`open_uri`]): turns the text into a
//!   [`Payload`] (web link, e-mail, phone number, SMS, Wi-Fi login, map location, contact card,
//!   or plain text) so the app can show a sensible pop-up, and gives the URI the system should
//!   open — only for safe schemes (https/http, mailto, tel, sms, geo). Dangerous schemes such as
//!   `javascript:`, `file:`, `intent:`, `content:` or `data:` are treated as plain text.
//!
//! The heavy lifting is done by the [`rxing`] crate, a Rust port of ZXing (Apache-2.0).
//!
//! ## Picking a resolution for reading
//!
//! Measured in the tests (`min_pixels_per_module`, codes scaled with nearest-neighbor):
//! everything reads reliably from **2 pixels per module** up. QR codes read even at 1 px;
//! at awkward fractional scales such as 1.25 px most formats fail (uneven modules), and Aztec
//! needs a full 2 px. For tap-to-decode, re-render the tapped area so the smallest bar or square
//! comes out at **3-4 px** and call [`decode_rgba`] on that crop (or [`decode_region`] on a
//! bigger render). Roughly: 3 px/module at 440 dpi is a 0.17 mm module rendered at 2x zoom.
//!
//! ## Speed (release build, desktop x86-64 PC, one thread)
//!
//! | picture                                    | time    |
//! |--------------------------------------------|---------|
//! | 1080 x 1500 page, blank                    | ~75 ms  |
//! | 1080 x 1500 page of text-like marks        | ~160 ms |
//! | 1080 x 1500 page with one QR code          | ~85 ms  |
//! | 1080 x 1500 page with a QR and an EAN-13   | ~220 ms |
//! | 300 x 300 crop around one QR code          | ~7 ms   |
//!
//! A phone is several times slower, so run page decodes off the UI thread. Each extra code on a
//! page costs one more pass (see `find_all` in `decode.rs`); a page with nothing costs the full
//! pass, the tile pass for Data Matrix/QR, the Aztec bull's-eye search, and the same again on the
//! inverted picture (light codes on a dark page).
//!
//! ## Panics
//!
//! Decoding runs inside `catch_unwind`. That only helps when the app is built with
//! `panic = "unwind"` (the DocumentSlate release-small profile keeps unwind on purpose).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod decode;
mod encode;
mod payload;
mod render;

pub use decode::{Found, decode_image_bytes, decode_luma, decode_region, decode_rgba};
pub use encode::{Matrix, Symbology, encode, validate};
pub use payload::{Payload, classify, open_uri};
pub use render::{bar_height_modules, to_png, to_rgba, to_svg};
