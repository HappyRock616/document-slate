//! Making barcodes: text in, a grid of dark/light modules out.

use rxing::datamatrix::encoder::SymbolShapeHint;
use rxing::{BarcodeFormat, EncodeHintValue, EncodeHints, MultiFormatWriter, Writer};

/// The kinds of barcode the app can make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Symbology {
    /// QR code (2D, square). The everyday phone-scanner code. Any text, up to ~2300 bytes.
    Qr,
    /// Data Matrix (2D, square). Common on parts, parcels and medicine packs.
    DataMatrix,
    /// Aztec (2D, square, no quiet zone needed). Used on tickets and boarding passes.
    Aztec,
    /// PDF417 (2D, stacked rows). Used on ID cards and shipping labels.
    Pdf417,
    /// Code 128 (1D). Letters, digits and punctuation (printable ASCII).
    Code128,
    /// Code 39 (1D). Capital letters, digits, space and `- . $ / + %`.
    Code39,
    /// EAN-13 (1D). Retail product code, 13 digits (the last one is a check digit).
    Ean13,
    /// UPC-A (1D). North American retail code, 12 digits (the last one is a check digit).
    UpcA,
}

impl Symbology {
    /// Every symbology, in the order the app lists them.
    pub const ALL: [Symbology; 8] = [
        Symbology::Qr,
        Symbology::DataMatrix,
        Symbology::Aztec,
        Symbology::Pdf417,
        Symbology::Code128,
        Symbology::Code39,
        Symbology::Ean13,
        Symbology::UpcA,
    ];

    /// The name shown to people ("QR code", "Data Matrix", ...).
    pub fn label(self) -> &'static str {
        match self {
            Symbology::Qr => "QR code",
            Symbology::DataMatrix => "Data Matrix",
            Symbology::Aztec => "Aztec",
            Symbology::Pdf417 => "PDF417",
            Symbology::Code128 => "Code 128",
            Symbology::Code39 => "Code 39",
            Symbology::Ean13 => "EAN-13",
            Symbology::UpcA => "UPC-A",
        }
    }

    /// True for the 2D codes (QR, Data Matrix, Aztec, PDF417), false for the bar codes.
    pub fn is_2d(self) -> bool {
        matches!(self, Symbology::Qr | Symbology::DataMatrix | Symbology::Aztec | Symbology::Pdf417)
    }

    /// A short line telling the person what they may type.
    pub fn hint(self) -> &'static str {
        match self {
            Symbology::Qr => "Any text, link or contact. Up to about 2,000 characters.",
            Symbology::DataMatrix => "Any text. Up to about 1,500 characters.",
            Symbology::Aztec => "Any text. Up to about 1,500 characters.",
            Symbology::Pdf417 => "Any text. Up to about 1,000 characters.",
            Symbology::Code128 => "Letters, digits and punctuation, no accents. Up to 80 characters.",
            Symbology::Code39 => "Capital letters A-Z, digits, space and - . $ / + %. Up to 80 characters.",
            Symbology::Ean13 => "12 digits (the check digit is added) or 13 digits.",
            Symbology::UpcA => "11 digits (the check digit is added) or 12 digits.",
        }
    }

    /// Quiet zone (empty margin) in modules on each side, as `encode` adds it.
    /// QR needs 4; Data Matrix 1 (we use 2); Aztec none (we use 2); PDF417 2; bar codes need
    /// about 10 times the narrow bar (EAN-13 asks for 11 on the left), so 1D gets 11.
    pub fn quiet_zone(self) -> usize {
        match self {
            Symbology::Qr => 4,
            Symbology::DataMatrix | Symbology::Aztec | Symbology::Pdf417 => 2,
            Symbology::Code128 | Symbology::Code39 | Symbology::Ean13 | Symbology::UpcA => 11,
        }
    }

    pub(crate) fn format(self) -> BarcodeFormat {
        match self {
            Symbology::Qr => BarcodeFormat::QR_CODE,
            Symbology::DataMatrix => BarcodeFormat::DATA_MATRIX,
            Symbology::Aztec => BarcodeFormat::AZTEC,
            Symbology::Pdf417 => BarcodeFormat::PDF_417,
            Symbology::Code128 => BarcodeFormat::CODE_128,
            Symbology::Code39 => BarcodeFormat::CODE_39,
            Symbology::Ean13 => BarcodeFormat::EAN_13,
            Symbology::UpcA => BarcodeFormat::UPC_A,
        }
    }
}

/// A barcode as a grid of modules (the smallest squares or bars), quiet zone included.
///
/// `modules` is row-major (`modules[y * width + x]`), `true` = dark.
///
/// **1D codes** (Code 128, Code 39, EAN-13, UPC-A) are a single row (`height == 1`): every row of
/// a bar code is the same, so the picture height is a drawing choice. [`crate::to_png`] and
/// [`crate::to_svg`] stretch the row to [`crate::bar_height_modules`] rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub width: usize,
    pub height: usize,
    pub modules: Vec<bool>,
}

impl Matrix {
    /// Is the module at column `x`, row `y` dark? Out of range = light.
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.modules.get(y * self.width + x).copied().unwrap_or(false)
    }

    /// True when this is a one-row bar code.
    pub fn is_1d(&self) -> bool {
        self.height == 1
    }
}

const CODE39_CHARS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";

/// Checks that `text` can be turned into `sym`. The error is a friendly sentence for the user.
///
/// Size limits of the 2D codes depend on the exact characters, so a text that passes here can
/// still be refused by [`encode`] when it is near the limit (with an equally friendly message).
pub fn validate(sym: Symbology, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err(format!("Type something to put in the {}.", sym.label()));
    }
    let bytes = text.len();
    let chars = text.chars().count();
    match sym {
        Symbology::Qr => limit(sym, bytes, 2331),
        Symbology::DataMatrix => limit(sym, bytes, 1550),
        Symbology::Aztec => limit(sym, bytes, 1900),
        Symbology::Pdf417 => limit(sym, bytes, 1100),
        Symbology::Code128 => {
            if let Some(c) = text.chars().find(|c| !(' '..='~').contains(c)) {
                return Err(format!("Code 128 can't hold \"{c}\". Use letters, digits and punctuation without accents."));
            }
            if chars > 80 {
                return Err("Code 128 holds at most 80 characters.".into());
            }
            Ok(())
        }
        Symbology::Code39 => {
            if let Some(c) = text.chars().find(|c| !CODE39_CHARS.contains(*c)) {
                let more = if c.is_ascii_lowercase() { " Use capital letters." } else { "" };
                return Err(format!("Code 39 can't hold \"{c}\". It takes A-Z, 0-9, space and - . $ / + %.{more}"));
            }
            if chars > 80 {
                return Err("Code 39 holds at most 80 characters.".into());
            }
            Ok(())
        }
        Symbology::Ean13 => digits_with_check(sym, text, 12),
        Symbology::UpcA => digits_with_check(sym, text, 11),
    }
}

fn limit(sym: Symbology, bytes: usize, max: usize) -> Result<(), String> {
    if bytes > max { Err(format!("That's too much text for a {} (about {max} bytes at most).", sym.label())) } else { Ok(()) }
}

/// EAN-13 (body 12) and UPC-A (body 11): digits only, body length or body + correct check digit.
fn digits_with_check(sym: Symbology, text: &str, body: usize) -> Result<(), String> {
    if !text.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{} takes digits only.", sym.label()));
    }
    let n = text.len();
    if n != body && n != body + 1 {
        return Err(format!("{} needs {body} or {} digits.", sym.label(), body + 1));
    }
    if n == body + 1 {
        let want = check_digit(&text[..body]);
        let got = text.as_bytes()[body] - b'0';
        if want != got {
            return Err(format!("The last digit should be {want} (check digit). Or type just the first {body} digits."));
        }
    }
    Ok(())
}

/// GS1 check digit (EAN-13 / UPC-A / EAN-8): weights 3,1,3,1,... from the right.
pub(crate) fn check_digit(body: &str) -> u8 {
    let sum: u32 = body.bytes().rev().enumerate().map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 3 } else { 1 }).sum();
    ((10 - sum % 10) % 10) as u8
}

/// Makes the barcode. The result includes the quiet zone (see [`Symbology::quiet_zone`]).
pub fn encode(sym: Symbology, text: &str) -> Result<Matrix, String> {
    validate(sym, text)?;
    let ascii = text.is_ascii();
    let mut hints = EncodeHints::default().with(EncodeHintValue::Margin("0".into()));
    match sym {
        Symbology::Qr => {
            hints = hints.with(EncodeHintValue::ErrorCorrection("M".into()));
        }
        Symbology::DataMatrix => {
            hints = hints.with(EncodeHintValue::DataMatrixShape(SymbolShapeHint::FORCE_SQUARE));
            if !ascii {
                // Only the "compact" (minimal) encoder writes an ECI for UTF-8.
                hints = hints.with(EncodeHintValue::DataMatrixCompact(true));
            }
        }
        _ => {}
    }
    if sym.is_2d() && !ascii {
        hints = hints.with(EncodeHintValue::CharacterSet("UTF-8".into()));
    }

    let format = sym.format();
    let result = std::panic::catch_unwind(|| MultiFormatWriter.encode_with_hints(text, &format, 0, 0, &hints));
    let bits = match result {
        Ok(Ok(bits)) => bits,
        Ok(Err(e)) => return Err(friendly_encode_error(sym, &format!("{e:?}"))),
        Err(_) => return Err(format!("Couldn't make a {} from that text.", sym.label())),
    };

    let (w, h) = (bits.width() as usize, bits.height() as usize);
    if w == 0 || h == 0 {
        return Err(format!("Couldn't make a {} from that text.", sym.label()));
    }
    // 1D writers give a one-pixel-high row at scale 1; keep just that row.
    let rows = if sym.is_2d() { h } else { 1 };
    let q = sym.quiet_zone();
    let qy = if sym.is_2d() { q } else { 0 };
    let width = w + 2 * q;
    let height = rows + 2 * qy;
    let mut modules = vec![false; width * height];
    for y in 0..rows {
        for x in 0..w {
            if bits.get(x as u32, y as u32) {
                modules[(y + qy) * width + x + q] = true;
            }
        }
    }
    Ok(Matrix { width, height, modules })
}

fn friendly_encode_error(sym: Symbology, raw: &str) -> String {
    let low = raw.to_ascii_lowercase();
    if low.contains("too")
        || low.contains("large")
        || low.contains("big")
        || low.contains("capacity")
        || low.contains("fit")
        || low.contains("symbol info")
    {
        format!("That's too much text for a {}. Try a shorter text or a QR code.", sym.label())
    } else {
        format!("Couldn't make a {} from that text.", sym.label())
    }
}
