//! Certificate signatures and flattening.

use std::sync::Arc;

use slate_pdf::*;

const NOW: i64 = 1_795_000_000; // 2026-11

fn blank() -> PdfDoc {
    PdfDoc::create_blank(612.0, 792.0, 1).expect("blank")
}

#[test]
fn self_signed_id_backup_round_trip() {
    let id = new_self_signed_id("Test Person", "test@example.invalid", "", NOW).expect("id");
    let s = id_summary(&id);
    assert_eq!(s.name, "Test Person");
    assert_eq!(s.email, "test@example.invalid");
    assert!(s.self_signed);
    assert!(s.problem.is_none(), "{:?}", s.problem);
    assert!(s.valid_until.starts_with("2031"), "{}", s.valid_until);
    assert!(write_digital_id(&id, "short").is_err());
    let p12 = write_digital_id(&id, "longenough").expect("p12");
    assert!(matches!(open_digital_id(&p12, "wrong-password"), Err(PdfError::WrongPassword)));
    let back = open_digital_id(&p12, "longenough").expect("reopen");
    assert_eq!(id_summary(&back).fingerprint, s.fingerprint);
    let (pkcs8, cert, chain) = id_parts(&back);
    assert!(!pkcs8.is_empty() && !cert.is_empty() && chain.is_empty());
    assert_eq!(key_family(&back), "RSA");
    assert_eq!(certificate_summary(&cert).expect("summary").name, "Test Person");
}

#[test]
fn flatten_then_sign_visible() {
    let mut doc = blank();
    doc.add_typed_text(0, [72.0, 72.0], "Filled in", 12.0, [0.0, 0.0, 0.0]).expect("text");
    doc.add_fill_mark(0, [100.0, 120.0], 12.0, FillMark::Check).expect("mark");
    assert_eq!(doc.fill_sign_count(), 2);
    doc.flatten_fill_sign().expect("flatten");
    assert_eq!(doc.fill_sign_count(), 0);
    assert!(doc.can_undo().is_some(), "flatten is undoable before saving");

    let id = new_self_signed_id("Signer Name", "", "Org", NOW).expect("id");
    let req = CertSignRequest { page: 0, rect: Some([300.0, 600.0, 520.0, 660.0]), reason: Some("I approve".into()), ..Default::default() };
    let signed = doc.sign_with_id(&id, &req).expect("sign");
    doc.mark_signed(&signed).expect("mark signed");
    assert!(doc.can_undo().is_none(), "signing drops the undo history");

    let reopened = PdfDoc::open_bytes(signed, None).expect("reopen");
    let sigs = reopened.signatures();
    assert_eq!(sigs.len(), 1, "{sigs:?}");
    assert!(sigs[0].signed);
    assert_eq!(sigs[0].signer, "Signer Name");
    assert_eq!(sigs[0].reason, "I approve");
    // Self-signed: intact, but no one has said to trust it.
    assert_eq!(sigs[0].status, SigStatus::Unknown, "{:?}", sigs[0].details);
    assert!(reopened.properties().signed);
}

/// A key that signs through a callback, as the Android Keystore will.
struct Callback(PrivateKeyHolder);
struct PrivateKeyHolder(SigningId);

impl ExternalKey for Callback {
    fn sign(&self, alg: DigestAlg, msg: &[u8]) -> Result<Vec<u8>, SignError> {
        self.0.0.key.sign(alg, msg)
    }
}

#[test]
fn external_key_signs_invisible_and_certifies() {
    let software = new_self_signed_id("Keystore Person", "", "", NOW).expect("id");
    let cert = software.certificate.raw.clone();
    let id = external_digital_id(&cert, &[], Arc::new(Callback(PrivateKeyHolder(software)))).expect("external");
    let doc = blank();
    let req = CertSignRequest { certify: Some(2), ..Default::default() };
    let signed = doc.sign_with_id(&id, &req).expect("sign");
    let reopened = PdfDoc::open_bytes(signed, None).expect("reopen");
    let sigs = reopened.signatures();
    assert_eq!(sigs.len(), 1);
    assert!(sigs[0].certified);
    assert_ne!(sigs[0].status, SigStatus::Invalid, "{:?}", sigs[0].details);
}

#[test]
fn self_signed_for_external_key() {
    let software = new_self_signed_id("Temp", "", "", NOW).expect("id");
    let spki = software.certificate.public_key.spki();
    let key = external_key(&spki, Arc::new(Callback(PrivateKeyHolder(software)))).expect("key");
    let id = self_signed_for(key, "Made Inside", "a@b.invalid", "", NOW).expect("cert");
    assert_eq!(id_summary(&id).name, "Made Inside");
    let signed = blank().sign_with_id(&id, &CertSignRequest::default()).expect("sign");
    let sigs = PdfDoc::open_bytes(signed, None).expect("reopen").signatures();
    assert_eq!(sigs[0].status, SigStatus::Unknown, "{:?}", sigs[0].details);
}

#[test]
fn flatten_all_bakes_comments() {
    let mut doc = blank();
    doc.add_typed_text(0, [72.0, 72.0], "Note", 12.0, [0.0, 0.0, 0.0]).expect("text");
    assert!(doc.flattenable_count() >= 1);
    doc.flatten_all().expect("flatten");
    assert_eq!(doc.comments().len(), 0);
    doc.undo().expect("undo");
    assert_eq!(doc.comments().len(), 1);
}
