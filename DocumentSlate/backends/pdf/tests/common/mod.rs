//! Test helpers: tiny PDFs and PNGs built by hand, so the tests need no files from outside.
#![allow(dead_code, clippy::same_item_push)]

/// A PDF with one page per entry of `pages`, each showing that text in Helvetica 24 pt near the
/// top-left. With `form`, page 1 also gets a text field "name" and a check box "agree".
pub fn make_pdf(pages: &[&str], form: bool) -> Vec<u8> {
    // Object numbers: 1 catalog, 2 pages, 3 font, then per page: page object, content stream.
    // Form (optional): text field, check box at the end.
    let n = pages.len();
    let first_page = 4;
    let field_obj = first_page + 2 * n;
    let check_obj = field_obj + 1;
    let mut objects: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", first_page + 2 * i)).collect();
    let acro = if form { format!(" /AcroForm << /Fields [{field_obj} 0 R {check_obj} 0 R] /DR << /Font << /Helv 3 0 R >> >> /DA (/Helv 0 Tf 0 g) >>") } else { String::new() };
    objects.push(format!("<< /Type /Catalog /Pages 2 0 R{acro} >>"));
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into());
    for (i, text) in pages.iter().enumerate() {
        let annots = if form && i == 0 { format!(" /Annots [{field_obj} 0 R {check_obj} 0 R]") } else { String::new() };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R{annots} >>",
            first_page + 2 * i + 1
        ));
        let stream = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET\n0 0 1 RG 4 w 72 600 m 300 600 l S\n");
        objects.push(format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()));
    }
    if form {
        let p1 = first_page;
        objects.push(format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V () /Rect [72 500 300 524] /P {p1} 0 R /F 4 /DA (/Helv 12 Tf 0 g) >>"
        ));
        objects.push(format!(
            "<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /V /Off /AS /Off /Rect [72 450 90 468] /P {p1} 0 R /F 4 /MK << /CA (4) >> /DA (/ZaDb 0 Tf 0 g) /AP << /N << /Yes {} 0 R /Off {} 0 R >> >> >>",
            check_obj + 1,
            check_obj + 2
        ));
        // Appearance streams: a filled square for "Yes", an empty frame for "Off".
        let yes = "0 g 2 2 14 14 re f\n";
        let off = "0 G 1 w 0.5 0.5 17 17 re S\n";
        for ap in [yes, off] {
            objects.push(format!("<< /Type /XObject /Subtype /Form /BBox [0 0 18 18] /Length {} >>\nstream\n{ap}endstream", ap.len()));
        }
    }
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// A small solid-colour RGBA PNG (stored, uncompressed deflate — valid, just not small).
pub fn make_png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..h {
        raw.push(0u8); // filter: none
        for _ in 0..w {
            raw.extend_from_slice(&rgba);
        }
    }
    // zlib stream with stored blocks.
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        let len = c.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// Where tests leave their output PDFs for a human to look at (the git-ignored `_cleanup/` folder).
pub fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../_cleanup/slate-pdf-tests");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name)
}

/// Count pixels that are clearly not white.
pub fn ink_pixels(rgba: &[u8]) -> usize {
    rgba.as_chunks::<4>().0.iter().filter(|p| p[0] < 200 || p[1] < 200 || p[2] < 200).count()
}
