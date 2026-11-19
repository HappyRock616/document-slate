//! What the decoded text means: links, mail, phone, SMS, Wi-Fi, geo, contacts, unsafe schemes.

use slate_barcode::{Payload, classify, open_uri};

fn text(s: &str) -> Payload {
    Payload::Text(s.to_string())
}

#[test]
fn urls() {
    assert_eq!(classify("https://example.com/a?b=c"), Payload::Url("https://example.com/a?b=c".into()));
    assert_eq!(classify("  HTTP://Example.com  "), Payload::Url("HTTP://Example.com".into()));
    assert_eq!(classify("www.example.com/page"), Payload::Url("https://www.example.com/page".into()));
    assert_eq!(open_uri(&classify("www.example.com")), Some("https://www.example.com".into()));
    assert_eq!(classify("https://例え.jp/パス"), Payload::Url("https://例え.jp/パス".into()));
    // Not really URLs.
    assert_eq!(classify("https://"), text("https://"));
    assert_eq!(classify("https://exa mple.com"), text("https://exa mple.com"));
    assert_eq!(classify("http://localhost"), text("http://localhost"));
    assert_eq!(classify("www."), text("www."));
}

#[test]
fn dangerous_schemes_are_text() {
    for s in [
        "javascript:alert(1)",
        "JavaScript:alert(document.cookie)",
        "file:///sdcard/secret.txt",
        "intent://scan/#Intent;scheme=zxing;package=com.evil;end",
        "content://com.android.contacts/contacts",
        "data:text/html,<script>alert(1)</script>",
        "vbscript:msgbox",
        "market://details?id=com.evil",
        "ftp://example.com/file",
        "https://example.com/\njavascript:alert(1)",
        "https://example.com/<script>",
    ] {
        let p = classify(s);
        assert_eq!(p, text(s), "{s}");
        assert_eq!(open_uri(&p), None, "{s}");
    }
    // A hand-made Url payload with a bad scheme is not opened either.
    assert_eq!(open_uri(&Payload::Url("javascript:alert(1)".into())), None);
}

#[test]
fn email() {
    assert_eq!(
        classify("mailto:ann@example.com?subject=Hello%20there&body=Line+one"),
        Payload::Email { to: "ann@example.com".into(), subject: "Hello there".into(), body: "Line one".into() }
    );
    assert_eq!(
        classify("MATMSG:TO:bob@example.org;SUB:Meeting;BODY:See you at 3\\; ok?;;"),
        Payload::Email { to: "bob@example.org".into(), subject: "Meeting".into(), body: "See you at 3; ok?".into() }
    );
    assert_eq!(classify("carl@example.net"), Payload::Email { to: "carl@example.net".into(), subject: String::new(), body: String::new() });
    let p = Payload::Email { to: "a@b.co".into(), subject: "Hi & bye".into(), body: "Grüße".into() };
    assert_eq!(open_uri(&p), Some("mailto:a@b.co?subject=Hi%20%26%20bye&body=Gr%C3%BC%C3%9Fe".into()));
    assert_eq!(classify("mailto:not-an-address"), text("mailto:not-an-address"));
}

#[test]
fn phone_and_sms() {
    assert_eq!(classify("tel:+1 (555) 123-4567"), Payload::Phone("+15551234567".into()));
    assert_eq!(open_uri(&classify("TEL:555-0100")), Some("tel:5550100".into()));
    assert_eq!(classify("tel:call-me"), text("tel:call-me"));
    assert_eq!(classify("SMSTO:+15550100:Running late"), Payload::Sms { number: "+15550100".into(), body: "Running late".into() });
    assert_eq!(classify("sms:5550100?body=Hi%20there"), Payload::Sms { number: "5550100".into(), body: "Hi there".into() });
    assert_eq!(open_uri(&Payload::Sms { number: "5550100".into(), body: "a b".into() }), Some("sms:5550100?body=a%20b".into()));
}

#[test]
fn wifi() {
    assert_eq!(
        classify("WIFI:T:WPA;S:My Net\\;work;P:p\\:ss\\\\word;H:true;;"),
        Payload::Wifi { ssid: "My Net;work".into(), password: "p:ss\\word".into(), kind: "WPA".into(), hidden: true }
    );
    let p = classify("WIFI:S:Cafe;T:nopass;;");
    assert_eq!(p, Payload::Wifi { ssid: "Cafe".into(), password: String::new(), kind: "nopass".into(), hidden: false });
    assert_eq!(open_uri(&p), None, "never open Wi-Fi");
    assert_eq!(classify("WIFI:T:WPA;;"), text("WIFI:T:WPA;;"));
}

#[test]
fn geo() {
    assert_eq!(classify("geo:48.8584,2.2945"), Payload::Geo { lat: 48.8584, lon: 2.2945 });
    assert_eq!(classify("GEO:-33.86,151.21,40?q=Opera"), Payload::Geo { lat: -33.86, lon: 151.21 });
    assert_eq!(open_uri(&classify("geo:1.5,-2.25")), Some("geo:1.5,-2.25".into()));
    assert_eq!(classify("geo:91,0"), text("geo:91,0"));
    assert_eq!(classify("geo:abc"), text("geo:abc"));
}

#[test]
fn mecard() {
    let p = classify("MECARD:N:Doe,Jane;TEL:+15550100;EMAIL:jane@example.com;ORG:Acme;URL:https://acme.example;NOTE:Hi\\, there;;");
    assert_eq!(
        p,
        Payload::Contact {
            name: "Jane Doe".into(),
            phone: "+15550100".into(),
            email: "jane@example.com".into(),
            org: "Acme".into(),
            address: String::new(),
            url: "https://acme.example".into(),
            note: "Hi, there".into(),
        }
    );
    assert_eq!(open_uri(&p), None);
}

#[test]
fn vcard() {
    let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Müller;Jürgen;;;\r\nTEL;TYPE=CELL:+49 30 1234\r\nitem1.EMAIL;TYPE=INTERNET:jm@example.de\r\nORG:Slate GmbH;Dev\r\nADR;TYPE=WORK:;;Hauptstr. 1;Berlin;;10115;Germany\r\nNOTE:line one\\nline two with a long\r\n  continued part\r\nEND:VCARD";
    assert_eq!(
        classify(card),
        Payload::Contact {
            name: "Jürgen Müller".into(),
            phone: "+49 30 1234".into(),
            email: "jm@example.de".into(),
            org: "Slate GmbH, Dev".into(),
            address: "Hauptstr. 1, Berlin, 10115, Germany".into(),
            url: String::new(),
            note: "line one\nline two with a long continued part".into(),
        }
    );
    let p = classify("BEGIN:VCARD\nFN:Ann Lee\nN:Lee;Ann\nEND:VCARD");
    assert!(matches!(p, Payload::Contact { ref name, .. } if name == "Ann Lee"));
}

#[test]
fn plain_text() {
    assert_eq!(classify("Hello, world"), text("Hello, world"));
    assert_eq!(classify("5901234123457"), text("5901234123457"));
    assert_eq!(classify(""), text(""));
    assert_eq!(open_uri(&classify("just words")), None);
    // Keeps the original text untrimmed for Text.
    assert_eq!(classify("  spaced  "), text("  spaced  "));
}
