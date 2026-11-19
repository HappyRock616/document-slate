//! Understanding decoded text: is it a link, an e-mail, a Wi-Fi login, a contact...?
//!
//! Only well-known, harmless formats are recognized. Anything else, and anything using a scheme
//! that could run code or reach into the phone (`javascript:`, `file:`, `intent:`, `content:`,
//! `data:`, ...), is plain [`Payload::Text`], which the app should only show and copy.

/// What a barcode's text means, for the pop-up the app shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    /// A web address (always `http://` or `https://`; a bare `www.example.com` becomes https).
    Url(String),
    /// An e-mail to write (from `mailto:`, `MATMSG:` or a bare address).
    Email { to: String, subject: String, body: String },
    /// A phone number to call (from `tel:`).
    Phone(String),
    /// A text message to send (from `SMSTO:` or `sms:`).
    Sms { number: String, body: String },
    /// Wi-Fi login (from `WIFI:`). `kind` is "WPA", "WEP", "SAE", "nopass" or empty.
    Wifi { ssid: String, password: String, kind: String, hidden: bool },
    /// A map location in degrees (from `geo:`).
    Geo { lat: f64, lon: f64 },
    /// A contact card (from `MECARD:` or `BEGIN:VCARD`). Missing fields are empty.
    Contact { name: String, phone: String, email: String, org: String, address: String, url: String, note: String },
    /// Anything else.
    Text(String),
}

/// Works out what the text is. Never fails: unknown text is [`Payload::Text`].
pub fn classify(text: &str) -> Payload {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let starts = |p: &str| lower.starts_with(p);

    if starts("http://") || starts("https://") {
        if let Some(u) = clean_url(t) {
            return Payload::Url(u);
        }
    } else if starts("www.") {
        if let Some(u) = clean_url(&format!("https://{t}")) {
            return Payload::Url(u);
        }
    } else if starts("mailto:") {
        if let Some(p) = parse_mailto(&t[7..]) {
            return p;
        }
    } else if starts("matmsg:") {
        let f = fields(&t[7..]);
        let to = get(&f, "TO");
        if is_email(&to) {
            return Payload::Email { to, subject: get(&f, "SUB"), body: get(&f, "BODY") };
        }
    } else if starts("tel:") {
        if let Some(n) = clean_phone(&t[4..]) {
            return Payload::Phone(n);
        }
    } else if starts("smsto:") || starts("mmsto:") {
        let rest = &t[6..];
        let (num, body) = rest.split_once(':').unwrap_or((rest, ""));
        if let Some(number) = clean_phone(num) {
            return Payload::Sms { number, body: body.to_string() };
        }
    } else if starts("sms:") {
        let rest = &t[4..];
        let (num, query) = rest.split_once('?').unwrap_or((rest, ""));
        if let Some(number) = clean_phone(num) {
            let body = query_param(query, "body").unwrap_or_default();
            return Payload::Sms { number, body };
        }
    } else if starts("wifi:") {
        let f = fields(&t[5..]);
        let ssid = get(&f, "S");
        if !ssid.is_empty() {
            return Payload::Wifi { ssid, password: get(&f, "P"), kind: get(&f, "T"), hidden: get(&f, "H").eq_ignore_ascii_case("true") };
        }
    } else if starts("geo:") {
        if let Some(p) = parse_geo(&t[4..]) {
            return p;
        }
    } else if starts("mecard:") {
        let f = fields(&t[7..]);
        let raw_name = get(&f, "N");
        // MECARD names are "Last,First".
        let name = match raw_name.split_once(',') {
            Some((last, first)) => format!("{} {}", first.trim(), last.trim()).trim().to_string(),
            None => raw_name,
        };
        return Payload::Contact {
            name,
            phone: get(&f, "TEL"),
            email: get(&f, "EMAIL"),
            org: get(&f, "ORG"),
            address: get(&f, "ADR"),
            url: get(&f, "URL"),
            note: get(&f, "NOTE"),
        };
    } else if starts("begin:vcard") {
        return parse_vcard(t);
    } else if is_email(t) {
        return Payload::Email { to: t.to_string(), subject: String::new(), body: String::new() };
    }
    Payload::Text(text.to_string())
}

/// The URI the system should open for this payload, if any: `https://…`/`http://…`, `mailto:…`,
/// `tel:…`, `sms:…` or `geo:…`. Never for Wi-Fi, contacts or plain text (the app handles those
/// itself: show, copy, add contact).
pub fn open_uri(p: &Payload) -> Option<String> {
    match p {
        Payload::Url(u) => clean_url(u),
        Payload::Email { to, subject, body } => {
            if !is_email(to) {
                return None;
            }
            let mut q = Vec::new();
            if !subject.is_empty() {
                q.push(format!("subject={}", pct_encode(subject)));
            }
            if !body.is_empty() {
                q.push(format!("body={}", pct_encode(body)));
            }
            let mut s = format!("mailto:{}", pct_encode_keep(to, "@.+-_"));
            if !q.is_empty() {
                s.push('?');
                s.push_str(&q.join("&"));
            }
            Some(s)
        }
        Payload::Phone(n) => clean_phone(n).map(|n| format!("tel:{n}")),
        Payload::Sms { number, body } => {
            clean_phone(number).map(|n| if body.is_empty() { format!("sms:{n}") } else { format!("sms:{n}?body={}", pct_encode(body)) })
        }
        Payload::Geo { lat, lon } => Some(format!("geo:{lat},{lon}")),
        Payload::Wifi { .. } | Payload::Contact { .. } | Payload::Text(_) => None,
    }
}

// ---------------------------------------------------------------------------------------------

/// Accepts only http(s) URLs with a host and no spaces or control characters.
fn clean_url(u: &str) -> Option<String> {
    let lower = u.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &u[8..]
    } else if lower.starts_with("http://") {
        &u[7..]
    } else {
        return None;
    };
    if u.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\' || c == '"' || c == '<' || c == '>') {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
        return None;
    }
    // A host needs a dot (example.com) unless it is an IPv6 literal or localhost-like name;
    // we keep it simple and require a dot or a bracket.
    if !host.contains('.') && !authority.contains('[') {
        return None;
    }
    Some(u.to_string())
}

fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | ',' | ';' | ':' | '/' | '\\'))
        && !domain.contains('@')
}

/// Keeps digits, `+` (leading), `*`, `#`, `,` and `;`; drops spaces, dashes, dots, brackets.
/// Needs at least 3 digits.
fn clean_phone(s: &str) -> Option<String> {
    let s = pct_decode(s.trim());
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '0'..='9' | '*' | '#' | ',' | ';' => out.push(c),
            '+' if out.is_empty() => out.push(c),
            ' ' | '-' | '.' | '(' | ')' | '/' => {}
            _ => return None,
        }
    }
    (out.chars().filter(char::is_ascii_digit).count() >= 3).then_some(out)
}

fn parse_mailto(rest: &str) -> Option<Payload> {
    let (addr, query) = rest.split_once('?').unwrap_or((rest, ""));
    let to = pct_decode(addr);
    let to = to.split(',').next().unwrap_or("").trim().to_string();
    if !is_email(&to) {
        return None;
    }
    Some(Payload::Email { to, subject: query_param(query, "subject").unwrap_or_default(), body: query_param(query, "body").unwrap_or_default() })
}

fn parse_geo(rest: &str) -> Option<Payload> {
    let coords = rest.split(['?', ';']).next().unwrap_or("");
    let mut it = coords.split(',');
    let lat: f64 = it.next()?.trim().parse().ok()?;
    let lon: f64 = it.next()?.trim().parse().ok()?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some(Payload::Geo { lat, lon })
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        k.eq_ignore_ascii_case(key).then(|| pct_decode(&v.replace('+', " ")))
    })
}

/// Splits `KEY:value;KEY:value;;` (MECARD / WIFI / MATMSG) honoring `\` escapes.
fn fields(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars();
    let push = |cur: &mut String, out: &mut Vec<(String, String)>| {
        if let Some((k, v)) = split_unescaped_colon(cur) {
            out.push((k.to_ascii_uppercase(), v));
        }
        cur.clear();
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push('\\');
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ';' => push(&mut cur, &mut out),
            _ => cur.push(c),
        }
    }
    push(&mut cur, &mut out);
    out
}

/// "KEY:val\:ue" -> ("KEY", "val:ue").
fn split_unescaped_colon(s: &str) -> Option<(String, String)> {
    let idx = s.find(':')?;
    let key = s[..idx].trim().to_string();
    let mut val = String::new();
    let mut chars = s[idx + 1..].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                val.push(n);
            }
        } else {
            val.push(c);
        }
    }
    (!key.is_empty()).then_some((key, val))
}

fn get(f: &[(String, String)], key: &str) -> String {
    f.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default()
}

fn parse_vcard(t: &str) -> Payload {
    // Unfold continuation lines (a line starting with a space or tab continues the last one).
    let mut lines: Vec<String> = Vec::new();
    for raw in t.lines() {
        let raw = raw.trim_end_matches('\r');
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&raw[1..]);
            }
        } else {
            lines.push(raw.to_string());
        }
    }
    let (mut name, mut n, mut phone, mut email, mut org, mut address, mut url, mut note) = (None, None, None, None, None, None, None, None);
    for line in &lines {
        let Some((head, value)) = line.split_once(':') else { continue };
        let prop = head.split(';').next().unwrap_or("").to_ascii_uppercase();
        // Drop a "group." prefix (item1.TEL).
        let prop = prop.rsplit('.').next().unwrap_or("").to_string();
        let parts = vcard_parts(value);
        let slot = match prop.as_str() {
            "FN" => &mut name,
            "N" => &mut n,
            "TEL" => &mut phone,
            "EMAIL" => &mut email,
            "ORG" => &mut org,
            "ADR" => &mut address,
            "URL" => &mut url,
            "NOTE" => &mut note,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(parts);
        }
    }
    let join =
        |p: Option<Vec<String>>, sep: &str| p.unwrap_or_default().iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(sep);
    let mut name = join(name, ";");
    if name.is_empty() {
        // N is "Last;First;Middle;Prefix;Suffix".
        let parts = n.unwrap_or_default();
        let first = parts.get(1).map(String::as_str).unwrap_or("");
        let last = parts.first().map(String::as_str).unwrap_or("");
        name = format!("{first} {last}").trim().to_string();
    }
    Payload::Contact {
        name,
        phone: join(phone, ";"),
        email: join(email, ";"),
        org: join(org, ", "),
        address: join(address, ", "),
        url: join(url, ";"),
        note: join(note, ";"),
    }
}

/// Splits a vCard value at unescaped `;` and undoes the escapes (`\n` newline, `\,` `\;` `\\`).
fn vcard_parts(v: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        let cur = parts.last_mut();
        match (c, cur) {
            ('\\', Some(cur)) => match chars.next() {
                Some('n') | Some('N') => cur.push('\n'),
                Some(o) => cur.push(o),
                None => {}
            },
            (';', _) => parts.push(String::new()),
            (c, Some(cur)) => cur.push(c),
            (_, None) => {}
        }
    }
    parts
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pct_encode(s: &str) -> String {
    pct_encode_keep(s, "-_.~")
}

fn pct_encode_keep(s: &str, keep: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || keep.as_bytes().contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
