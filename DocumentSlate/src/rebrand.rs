//! Keeps the upstream app names out of files Document Slate writes.
//!
//! The engines are forks of the ArtCraft apps (WordCraft, GridCraft, DeckCraft, PdfCraft). Their
//! names are trademarks that forks must not ship, but the engines hard-code them in file metadata:
//! "Application" in OOXML docProps/app.xml, default author names, the PDF Producer/Creator and the
//! XMP metadata. Rather than patching the upstream code (lost on every upstream pull), every file is
//! scrubbed here on its way out of the app: the Android bridge calls [`scrub_file`] before a file is
//! saved to the user's location, shared or printed.
//!
//! Only metadata is touched (OOXML/ODF property parts, PDF Info dictionary and XMP stream), never
//! the document's own content, so a document that mentions these names keeps its text.

use std::io::{Read, Write};
use std::path::Path;

/// Upstream name -> ours. Longer names first (so "WordCraft User" wins over "WordCraft").
const NAMES: [(&str, &str); 8] = [
    ("WordCraft User", "Author"),
    ("GridCraft User", "Author"),
    ("GridCraft PDF", "Grid Slate"),
    ("WordCraft", "Word Slate"),
    ("GridCraft", "Grid Slate"),
    ("DeckCraft", "Deck Slate"),
    ("PdfCraft", "Page Slate"),
    ("ArtCraft", "Document Slate"),
];

/// `s` with the upstream app names replaced by ours. Also used for engine messages shown to the
/// user ("DeckCraft can't open password-protected presentations" -> "Deck Slate can't…").
pub fn rename(s: &str) -> String {
    NAMES.iter().fold(s.to_string(), |acc, (from, to)| acc.replace(from, to))
}

fn has_name(b: &[u8]) -> bool {
    NAMES.iter().any(|(n, _)| b.windows(n.len()).any(|w| w == n.as_bytes()))
}

/// Package parts that hold metadata (not document content).
fn is_meta_part(name: &str) -> bool {
    name.starts_with("docProps/") || name == "meta.xml"
}

/// Scrub `path` in place if it is a zip package or a PDF that names an upstream app.
/// Errors are logged and leave the file as it was (a file with a stray name beats no file).
pub fn scrub_file(path: &Path) {
    let Ok(bytes) = std::fs::read(path) else { return };
    match scrub(&bytes) {
        Ok(Some(new)) => {
            if let Err(e) = std::fs::write(path, new) {
                log::warn!("rebrand: write {}: {e}", path.display());
            }
        }
        Ok(None) => {}
        Err(e) => log::warn!("rebrand: {}: {e}", path.display()),
    }
}

/// The scrubbed bytes, or `None` when nothing needed changing (or the format isn't one we touch).
pub fn scrub(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if bytes.starts_with(b"PK\x03\x04") {
        scrub_zip(bytes)
    } else if bytes.starts_with(b"%PDF") {
        scrub_pdf(bytes)
    } else {
        Ok(None)
    }
}

fn scrub_zip(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    // First pass: does any metadata part name an upstream app?
    let mut fixed: Vec<(usize, Vec<u8>)> = vec![];
    for i in 0..src.len() {
        let mut f = src.by_index(i).map_err(|e| e.to_string())?;
        if !is_meta_part(&f.name().map(|n| n.to_string()).unwrap_or_default()) {
            continue;
        }
        let mut s = String::new();
        if f.read_to_string(&mut s).is_ok() && has_name(s.as_bytes()) {
            fixed.push((i, rename(&s).into_bytes()));
        }
    }
    if fixed.is_empty() {
        return Ok(None);
    }
    // Second pass: copy every entry as it is (raw, no recompression) except the fixed ones, which
    // are written uncompressed (they're tiny). Order is kept: ODF needs "mimetype" first.
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..src.len() {
        if let Some((_, data)) = fixed.iter().find(|(j, _)| *j == i) {
            let name = src.by_index(i).map_err(|e| e.to_string())?.name().map_err(|e| e.to_string())?.to_string();
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            out.start_file(name, opts).map_err(|e| e.to_string())?;
            out.write_all(data).map_err(|e| e.to_string())?;
        } else {
            let f = src.by_index_raw(i).map_err(|e| e.to_string())?;
            out.raw_copy_file(f).map_err(|e| e.to_string())?;
        }
    }
    Ok(Some(out.finish().map_err(|e| e.to_string())?.into_inner()))
}

fn scrub_pdf(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    use lopdf::{Object, ObjectId};
    let mut doc = lopdf::Document::load_mem(bytes).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Ok(None);
    }
    let mut changed = false;
    // Info dictionary (trailer /Info): Producer, Creator, Author.
    let info_id: Option<ObjectId> = doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok());
    if let Some(id) = info_id {
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(id) {
            for key in [&b"Producer"[..], b"Creator", b"Author"] {
                if let Ok(Object::String(v, _)) = d.get(key) {
                    let text = lopdf::decode_text_string(&Object::String(v.clone(), lopdf::StringFormat::Literal)).unwrap_or_default();
                    if has_name(text.as_bytes()) {
                        d.set(key, Object::string_literal(rename(&text)));
                        changed = true;
                    }
                }
            }
        }
    }
    // XMP metadata stream (catalog /Metadata).
    let meta_id: Option<ObjectId> =
        doc.catalog().ok().and_then(|c| c.get(b"Metadata").ok()).and_then(|o| o.as_reference().ok());
    if let Some(id) = meta_id {
        if let Ok(Object::Stream(s)) = doc.get_object_mut(id) {
            let xml = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
            if has_name(&xml) {
                let new = rename(&String::from_utf8_lossy(&xml));
                s.dict.remove(b"Filter");
                s.dict.remove(b"DecodeParms");
                s.set_plain_content(new.into_bytes());
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    // A signed PDF must keep its original bytes: append the fix as an incremental update instead
    // of rewriting. (Only the changed objects are written after the original.)
    if bytes.windows(10).any(|w| w == b"/ByteRange") {
        let mut inc = lopdf::IncrementalDocument::load_from(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
        for id in [info_id, meta_id].into_iter().flatten() {
            if let Ok(obj) = doc.get_object(id) {
                inc.new_document.objects.insert(id, obj.clone());
            }
        }
        let mut out = Vec::new();
        inc.save_to(&mut out).map_err(|e| e.to_string())?;
        return Ok(Some(out));
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(Some(out))
}

/// Every upstream name found anywhere in a saved file (zip parts, PDF strings and decompressed
/// streams), as "where: name". For tests.
#[cfg(test)]
pub fn leaks(bytes: &[u8]) -> Vec<String> {
    let mut found = vec![];
    let mut check = |place: &str, b: &[u8]| {
        for (n, _) in NAMES {
            if b.windows(n.len()).any(|w| w == n.as_bytes()) {
                found.push(format!("{place}: {n}"));
            }
        }
    };
    if bytes.starts_with(b"PK\x03\x04") {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let name = f.name().map(|n| n.to_string()).unwrap_or_default();
            let mut b = vec![];
            if f.read_to_end(&mut b).is_ok() {
                check(&name, &b);
            }
        }
    } else if bytes.starts_with(b"%PDF") {
        check("raw bytes", bytes);
        let doc = lopdf::Document::load_mem(bytes).unwrap();
        for (id, o) in &doc.objects {
            match o {
                lopdf::Object::Stream(s) => {
                    if let Ok(b) = s.decompressed_content() {
                        check(&format!("stream {id:?}"), &b);
                    }
                }
                other => check(&format!("object {id:?}"), format!("{other:?}").as_bytes()),
            }
        }
        check("trailer", format!("{:?}", doc.trailer).as_bytes());
    } else {
        check("bytes", bytes);
    }
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests;
