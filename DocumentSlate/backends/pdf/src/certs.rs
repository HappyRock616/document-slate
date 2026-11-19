//! Certificate (digital) signatures, and flattening.
//!
//! * A **digital ID** is a private key plus its certificate. It comes from a password-protected
//!   `.p12` / `.pfx` file ([`open_digital_id`]), or is made here as a self-signed ID
//!   ([`new_self_signed_id`]). The app may keep the key somewhere else entirely (Android's
//!   Keystore): [`external_digital_id`] wraps a key that signs through a callback.
//! * [`PdfDoc::sign_with_id`] signs the document as it is now (PAdES, an incremental update, so
//!   earlier signatures stay valid) and returns the signed file. The caller writes it and then
//!   calls [`PdfDoc::mark_signed`] (signing can't be undone after that).
//! * Flattening bakes marks into the page drawing so they can't be selected, moved or removed:
//!   [`PdfDoc::flatten_fill_sign`] (Fill & Sign text, ticks and signatures only) and
//!   [`PdfDoc::flatten_all`] (every comment and form field). Both are ordinary undoable edits
//!   until the file is saved.

use std::sync::Arc;

use pdfcraft_engine::sign::{self, Certificate, DigitalId, Name, PrivateKey, PublicKey};
use pdfcraft_engine::{Edit, SignOptions};

use crate::geom::rect_to_user;
use crate::{PdfDoc, PdfError, ViewRect};

pub use pdfcraft_engine::sign::keys::{DigestAlg, ExternalKey};
pub use pdfcraft_engine::sign::{DigitalId as SigningId, SignError};

/// What the app shows about a digital ID (in lists and on the "check details" card).
#[derive(Clone, Debug, PartialEq)]
pub struct IdSummary {
    pub name: String,
    pub email: String,
    pub organization: String,
    pub issuer: String,
    pub self_signed: bool,
    /// "2031-11-18"
    pub valid_from: String,
    pub valid_until: String,
    /// Valid until, as Unix seconds (to warn about expired IDs).
    pub expires_unix: i64,
    /// Why this certificate can't sign documents (wrong key usage…), if it can't.
    pub problem: Option<String>,
    /// SHA-256 fingerprint (hex).
    pub fingerprint: String,
}

/// "D:20261118175931-05'00'" -> "2026-11-18 17:59" (the signer's local time, as written).
fn pdf_date_local(d: &str) -> Option<String> {
    let d = d.strip_prefix("D:").unwrap_or(d);
    let digits: String = d.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 12 {
        return None;
    }
    Some(format!("{}-{}-{} {}:{}", &digits[0..4], &digits[4..6], &digits[6..8], &digits[8..10], &digits[10..12]))
}

fn ymd(t: &sign::Time) -> String {
    format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
}

/// The summary of a certificate (DER bytes).
pub fn certificate_summary(der: &[u8]) -> Result<IdSummary, PdfError> {
    let c = Certificate::parse(der).map_err(|e| PdfError::Invalid(format!("the certificate can't be read: {e}")))?;
    Ok(summary_of(&c))
}

fn summary_of(c: &Certificate) -> IdSummary {
    IdSummary {
        name: c.subject.common_name().map(str::to_owned).unwrap_or_else(|| c.subject.display_name()),
        email: c.subject.email().unwrap_or_default().to_owned(),
        organization: c.subject.organization().unwrap_or_default().to_owned(),
        issuer: c.issuer.common_name().map(str::to_owned).unwrap_or_else(|| c.issuer.display_name()),
        self_signed: c.is_self_signed(),
        valid_from: ymd(&c.not_before),
        valid_until: ymd(&c.not_after),
        expires_unix: c.not_after.unix(),
        problem: c.signing_problem_clause(),
        fingerprint: c.fingerprint(),
    }
}

/// The public key (SubjectPublicKeyInfo DER) of a certificate.
pub fn certificate_spki(der: &[u8]) -> Result<Vec<u8>, PdfError> {
    let c = Certificate::parse(der).map_err(|e| PdfError::Invalid(format!("the certificate can't be read: {e}")))?;
    Ok(c.public_key.spki())
}

/// A digital ID read from a `.p12` / `.pfx` file. A wrong password gives [`PdfError::WrongPassword`].
pub fn open_digital_id(bytes: &[u8], password: &str) -> Result<DigitalId, PdfError> {
    sign::pkcs12::open(bytes, password).map_err(|e| match e {
        SignError::WrongPassword => PdfError::WrongPassword,
        e => PdfError::Invalid(format!("the digital ID file can't be used: {e}")),
    })
}

/// Write `id` as a password-protected `.p12` file (a backup the user can also use in other
/// programs). The password needs at least 8 characters.
pub fn write_digital_id(id: &DigitalId, password: &str) -> Result<Vec<u8>, PdfError> {
    if password.chars().count() < sign::pkcs12::MIN_PASSWORD_CHARS {
        return Err(PdfError::Invalid(format!("the password needs at least {} characters", sign::pkcs12::MIN_PASSWORD_CHARS)));
    }
    sign::pkcs12::write(id, password).map_err(|e| PdfError::Other(e.to_string()))
}

/// The ID's summary.
pub fn id_summary(id: &DigitalId) -> IdSummary {
    summary_of(&id.certificate)
}

/// The private key as PKCS#8 DER (to move it into Android's Keystore), the certificate DER and
/// the chain's DER. Empty key for an external key.
pub fn id_parts(id: &DigitalId) -> (Vec<u8>, Vec<u8>, Vec<Vec<u8>>) {
    (id.key.pkcs8().to_vec(), id.certificate.raw.clone(), id.chain.iter().map(|c| c.raw.clone()).collect())
}

/// `"RSA"` or `"EC"` (the key family, as Java's KeyFactory names it).
pub fn key_family(id: &DigitalId) -> &'static str {
    match id.key.public_key() {
        PublicKey::Rsa { .. } => "RSA",
        _ => "EC",
    }
}

/// A new self-signed digital ID: a 2048-bit RSA key (the most widely accepted) and a
/// certificate valid for 5 years from `now_unix`, naming `name` (and `email` / `organization`
/// when given).
pub fn new_self_signed_id(name: &str, email: &str, organization: &str, now_unix: i64) -> Result<DigitalId, PdfError> {
    let key = PrivateKey::generate_rsa(2048).map_err(|e| PdfError::Other(e.to_string()))?;
    self_signed_for(key, name, email, organization, now_unix)
}

/// A self-signed certificate for an existing key (for example one created inside Android's
/// Keystore, wrapped with [`external_key`]). Signing the certificate uses the key once.
pub fn self_signed_for(key: PrivateKey, name: &str, email: &str, organization: &str, now_unix: i64) -> Result<DigitalId, PdfError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PdfError::Invalid("enter your name".into()));
    }
    let dn = Name::build(name, "", organization.trim(), email.trim(), "");
    let serial = DigestAlg::Sha256.digest(&[name.as_bytes(), &now_unix.to_be_bytes()])[..8].to_vec();
    let cert = Certificate::self_signed(&dn, &key, sign::Time::from_unix(now_unix), 5, &serial).map_err(|e| PdfError::Other(e.to_string()))?;
    Ok(DigitalId { key, certificate: cert, chain: Vec::new(), friendly_name: Some(name.to_string()) })
}

/// A key that lives outside the app and signs through `signer`. `spki` is its public key
/// (SubjectPublicKeyInfo DER, what Java's `PublicKey.getEncoded()` gives).
pub fn external_key(spki: &[u8], signer: Arc<dyn ExternalKey>) -> Result<PrivateKey, PdfError> {
    let tlv = sign::der::Tlv::parse_all(spki).map_err(|e| PdfError::Invalid(format!("the public key can't be read: {e}")))?;
    let public = PublicKey::from_spki(&tlv).map_err(|e| PdfError::Invalid(format!("the public key can't be used: {e}")))?;
    Ok(PrivateKey::external(public, signer))
}

/// A digital ID whose key signs through `signer` (Android Keystore), with its certificate and
/// chain as DER.
pub fn external_digital_id(cert_der: &[u8], chain_der: &[Vec<u8>], signer: Arc<dyn ExternalKey>) -> Result<DigitalId, PdfError> {
    let certificate = Certificate::parse(cert_der).map_err(|e| PdfError::Invalid(format!("the certificate can't be read: {e}")))?;
    let chain = chain_der.iter().filter_map(|d| Certificate::parse(d).ok()).collect();
    let key = PrivateKey::external(certificate.public_key.clone(), signer);
    let friendly_name = certificate.subject.common_name().map(str::to_owned);
    Ok(DigitalId { key, certificate, chain, friendly_name })
}

/// Where and how to sign.
#[derive(Clone, Debug, Default)]
pub struct CertSignRequest {
    /// Sign this existing, empty signature field (by name) …
    pub field: Option<String>,
    /// … or a new visible signature on `page` in `rect` (page view space) …
    pub page: usize,
    pub rect: Option<ViewRect>,
    /// (neither: an invisible signature)
    pub reason: Option<String>,
    pub location: Option<String>,
    /// Certify (author signature) with what later changes allow: 1 none, 2 form fill-in and
    /// signing, 3 also comments.
    pub certify: Option<u8>,
    /// A drawn signature (strokes, y up, the pad 1 wide), shown in the box instead of the big name.
    pub drawn: Option<Vec<Vec<[f64; 2]>>>,
}

/// One signature in the document, for the Signatures sheet.
#[derive(Clone, Debug)]
pub struct SignatureRow {
    pub field: String,
    pub signed: bool,
    pub page: Option<usize>,
    pub signer: String,
    pub date: String,
    pub reason: String,
    /// "Signature is valid" / "… validity is unknown" / "… is invalid" / "Unsigned signature field"
    pub summary: String,
    pub status: SigStatus,
    pub certified: bool,
    /// Plain-English explanation lines (why valid/unknown/invalid, changes since signing).
    pub details: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigStatus {
    Valid,
    Unknown,
    Invalid,
    Unsigned,
}

impl PdfDoc {
    /// How many Fill & Sign items (typed text, ticks, signatures) the document has.
    pub fn fill_sign_count(&self) -> usize {
        self.comments().iter().filter(|c| c.fill_sign).count()
    }

    /// Comments (any kind, Fill & Sign included) and filled form fields that "flatten all" would bake.
    pub fn flattenable_count(&self) -> usize {
        self.comments().iter().filter(|c| !c.is_reply).count() + self.form_fields().iter().filter(|f| !f.widgets.is_empty()).count()
    }

    /// Bake Fill & Sign text, marks and signatures into the page (other comments and form fields
    /// stay editable). Undoable until saved; nothing happens when there are none.
    pub fn flatten_fill_sign(&mut self) -> Result<(), PdfError> {
        self.apply(Edit::FlattenFillSign)
    }

    /// Bake every comment and form field into the page. Undoable until saved.
    pub fn flatten_all(&mut self) -> Result<(), PdfError> {
        self.apply(Edit::Flatten { comments: true, fields: true })
    }

    /// Sign the document as it is now with `id`. Returns the signed file (the original bytes,
    /// any pending edits, then the signature). Write it, then call [`PdfDoc::mark_signed`].
    pub fn sign_with_id(&self, id: &DigitalId, req: &CertSignRequest) -> Result<Vec<u8>, PdfError> {
        if let Some(problem) = id.certificate.signing_problem_clause() {
            return Err(PdfError::Invalid(format!("this digital ID can't sign documents: {problem}")));
        }
        let rect = match (&req.field, req.rect) {
            (None, Some(r)) => {
                let info = self.page_ref(req.page)?;
                Some(rect_to_user(&info, r))
            }
            _ => None,
        };
        let some = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let opts = SignOptions {
            field: req.field.clone(),
            page: req.page,
            rect,
            reason: some(&req.reason),
            location: some(&req.location),
            certify: req.certify.map(|c| c.clamp(1, 3)),
            appearance: sign::Appearance::default(),
            graphic: req.drawn.clone().filter(|s| !s.is_empty()).map(sign::Graphic::Strokes),
            ..SignOptions::default()
        };
        let signed = self.session.sign(self.id, id, opts).map_err(PdfError::from_edit)?;
        self.session.check_save(self.id, &signed).map_err(PdfError::from_edit)?;
        Ok(signed.as_ref().clone())
    }

    /// The signed file from [`PdfDoc::sign_with_id`] was written: it becomes the saved file, and
    /// the undo history before signing is dropped.
    pub fn mark_signed(&mut self, bytes: &[u8]) -> Result<(), PdfError> {
        self.session.mark_signed(self.id, Arc::new(bytes.to_vec()), None).map_err(PdfError::from_edit)?;
        self.snapshot = None;
        Ok(())
    }

    /// Every signature field with its validation result. Self-signed IDs show as "validity
    /// unknown" (no one has said to trust them), which is normal.
    pub fn signatures(&self) -> Vec<SignatureRow> {
        let Ok(doc) = self.doc() else { return Vec::new() };
        doc.signatures
            .iter()
            .map(|s| SignatureRow {
                field: s.field.clone(),
                signed: s.signed,
                page: s.page,
                signer: s.signer.clone().unwrap_or_default(),
                date: s.date.as_deref().and_then(pdf_date_local).or_else(|| s.signing_time.map(|t| format!("{} {:02}:{:02} UTC", ymd(&t), t.hour, t.minute))).unwrap_or_default(),
                reason: s.reason.clone().unwrap_or_default(),
                summary: s.summary().to_owned(),
                status: if !s.signed {
                    SigStatus::Unsigned
                } else {
                    match s.status {
                        sign::Status::Valid => SigStatus::Valid,
                        sign::Status::Unknown => SigStatus::Unknown,
                        sign::Status::Invalid => SigStatus::Invalid,
                    }
                },
                certified: s.certify.is_some(),
                details: s.details.iter().map(|d| d.message.clone()).collect(),
            })
            .collect()
    }
}
