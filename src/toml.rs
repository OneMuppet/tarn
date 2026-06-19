//! Minimal, span-tracking TOML for `toml get`/`set` by path — same philosophy as
//! `json`: locate the target value's byte span and splice only that, so every
//! other byte (comments, key order, layout) is preserved. No serde; we parse
//! just enough to find spans, and we are honest about limits.
//!
//! Supported: `[table]` / `[table.sub]` headers, bare/quoted/dotted keys, and
//! single-line values (strings, numbers, bools, dates, single-line arrays and
//! inline tables). Multiline strings/arrays and arrays-of-tables (`[[x]]`) are
//! *tracked so they never corrupt parsing*, but `set` on them errors rather than
//! risk a bad edit. Path syntax is dotted: `server.port`, `a.b.c`.

struct Entry {
    path: Vec<String>,
    vstart: usize, // byte offset of the value (trimmed)
    vend: usize,
    // A single-line scalar/array/inline-table we can safely splice; multiline or
    // array-of-table values are recorded but flagged unsafe for `set`.
    settable: bool,
    is_string: bool, // value is a quoted string (so `get` can decode it)
}

fn is_bare(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// Parse the key portion (segments before `=`) starting at byte `i`. Returns the
/// segments and the index of the `=`. Handles bare, "quoted", 'literal', and
/// dotted keys. None if it isn't a key line.
fn parse_key(b: &[u8], mut i: usize) -> Option<(Vec<String>, usize)> {
    let mut segs = Vec::new();
    loop {
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        let seg;
        if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            let start = i + 1;
            i += 1;
            while i < b.len() && b[i] != q {
                if q == b'"' && b[i] == b'\\' {
                    i += 1; // skip escaped char (basic strings)
                }
                if b[i] == b'\n' {
                    return None;
                }
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
            seg = String::from_utf8_lossy(&b[start..i]).to_string();
            i += 1; // past closing quote
        } else {
            let start = i;
            while i < b.len() && is_bare(b[i]) {
                i += 1;
            }
            if i == start {
                return None;
            }
            seg = String::from_utf8_lossy(&b[start..i]).to_string();
        }
        segs.push(seg);
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        match b.get(i) {
            Some(b'.') => {
                i += 1;
                continue;
            }
            Some(b'=') => return Some((segs, i)),
            _ => return None,
        }
    }
}

/// Header segments for a `[a.b]` line (one set of brackets already past). Reuses
/// the key parser by treating the inside up to `]` as a dotted key.
fn parse_header(b: &[u8], inside_start: usize) -> Option<Vec<String>> {
    // find closing ']'
    let mut j = inside_start;
    while j < b.len() && b[j] != b']' && b[j] != b'\n' {
        j += 1;
    }
    if j >= b.len() || b[j] != b']' {
        return None;
    }
    // Parse the inside as dotted key segments by appending a synthetic '='.
    let mut inside = b[inside_start..j].to_vec();
    inside.push(b'=');
    parse_key(&inside, 0).map(|(segs, _)| segs)
}

/// Scan the value starting at byte `vs` (already past `=` and whitespace).
/// Returns (end_exclusive_trimmed, settable, is_string). `settable` is false for
/// multiline values; the end then spans to the closing delimiter.
fn scan_value(b: &[u8], vs: usize) -> Option<(usize, bool, bool)> {
    if vs >= b.len() {
        return None;
    }
    match b[vs] {
        b'"' | b'\'' => {
            let q = b[vs];
            // triple-quoted multiline?
            if b.get(vs + 1) == Some(&q) && b.get(vs + 2) == Some(&q) {
                let term = [q, q, q];
                let mut i = vs + 3;
                while i + 3 <= b.len() && b[i..i + 3] != term {
                    i += 1;
                }
                let end = (i + 3).min(b.len());
                return Some((end, false, true)); // multiline string: not settable
            }
            let mut i = vs + 1;
            while i < b.len() && b[i] != q {
                if q == b'"' && b[i] == b'\\' {
                    i += 1;
                }
                if b[i] == b'\n' {
                    return None;
                }
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
            Some((i + 1, true, true))
        }
        b'[' | b'{' => {
            let (open, close) = if b[vs] == b'[' { (b'[', b']') } else { (b'{', b'}') };
            let mut depth = 0i32;
            let mut i = vs;
            let mut multiline = false;
            while i < b.len() {
                match b[i] {
                    b'"' | b'\'' => {
                        // skip a single-line string inside
                        let q = b[i];
                        i += 1;
                        while i < b.len() && b[i] != q {
                            if q == b'"' && b[i] == b'\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    b'\n' => multiline = true,
                    c if c == open => depth += 1,
                    c if c == close => {
                        depth -= 1;
                        if depth == 0 {
                            return Some((i + 1, !multiline, false));
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        _ => {
            // bare scalar: up to an inline comment or end of line, trimmed
            let mut i = vs;
            while i < b.len() && b[i] != b'\n' && b[i] != b'#' {
                i += 1;
            }
            let mut end = i;
            while end > vs && (b[end - 1] == b' ' || b[end - 1] == b'\t') {
                end -= 1;
            }
            if end == vs {
                return None;
            }
            Some((end, true, false))
        }
    }
}

/// Parse the document into addressable entries. Tracks multiline strings and
/// array-of-table context so it never misreads their interior as keys.
fn entries(content: &str) -> Result<Vec<Entry>, String> {
    let b = content.as_bytes();
    let mut out = Vec::new();
    let mut table: Vec<String> = Vec::new();
    let mut in_array_of_tables = false; // current [[..]] context: set unsupported
    let mut i = 0;
    while i < b.len() {
        // start of a line
        let line_start = i;
        // skip leading whitespace
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        let c = b.get(i).copied();
        match c {
            None => break,
            Some(b'\n') => {
                i += 1;
                continue;
            }
            Some(b'#') => {
                // comment line — skip to EOL
            }
            Some(b'[') => {
                if b.get(i + 1) == Some(&b'[') {
                    in_array_of_tables = true;
                    if let Some(segs) = parse_header(b, i + 2) {
                        table = segs;
                    }
                } else {
                    in_array_of_tables = false;
                    match parse_header(b, i + 1) {
                        Some(segs) => table = segs,
                        None => return Err("malformed table header".into()),
                    }
                }
            }
            Some(_) => {
                if let Some((segs, eq)) = parse_key(b, i) {
                    let mut vs = eq + 1;
                    while vs < b.len() && (b[vs] == b' ' || b[vs] == b'\t') {
                        vs += 1;
                    }
                    match scan_value(b, vs) {
                        Some((vend, settable, is_string)) => {
                            let mut path = table.clone();
                            path.extend(segs);
                            out.push(Entry {
                                path,
                                vstart: vs,
                                vend,
                                settable: settable && !in_array_of_tables,
                                is_string,
                            });
                            // advance past a multiline value
                            i = vend;
                            // fall through to EOL skip
                        }
                        None => return Err("malformed or unterminated value".into()),
                    }
                }
                // if not a key line, just skip to EOL (unknown construct)
            }
        }
        // advance to end of current line
        while i < b.len() && b[i] != b'\n' {
            i += 1;
        }
        if i < b.len() {
            i += 1; // past newline
        }
        let _ = line_start;
    }
    Ok(out)
}

fn find<'a>(es: &'a [Entry], path: &str) -> Option<&'a Entry> {
    let want: Vec<&str> = path.split('.').collect();
    es.iter().find(|e| e.path.len() == want.len() && e.path.iter().zip(&want).all(|(a, b)| a == b))
}

/// Decode a single-line TOML string value (strip one quote layer; unescape basic).
fn decode_string(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        return s[1..s.len() - 1].to_string(); // literal: no escapes
    }
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &s[1..s.len() - 1];
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(ch) = it.next() {
            if ch == '\\' {
                match it.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(o) => {
                        out.push('\\');
                        out.push(o);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(ch);
            }
        }
        return out;
    }
    s.to_string()
}

/// Get the value at `path`. Strings are decoded; everything else is raw TOML.
/// `Ok(None)` = path not present.
pub fn get(content: &str, path: &str) -> Result<Option<String>, String> {
    let es = entries(content)?;
    Ok(find(&es, path).map(|e| {
        let raw = &content[e.vstart..e.vend];
        if e.is_string && e.settable {
            decode_string(raw)
        } else {
            raw.to_string()
        }
    }))
}

/// Set the value at `path`, preserving surrounding bytes. `value` is used verbatim
/// if it's a valid single-line TOML value, else encoded as a basic string.
/// `Ok(None)` = path not present; `Err` = unsupported target (multiline/array-of-table).
pub fn set(content: &str, path: &str, value: &str) -> Result<Option<String>, String> {
    let es = entries(content)?;
    let (vstart, vend, settable) = match find(&es, path) {
        Some(e) => (e.vstart, e.vend, e.settable),
        None => return Ok(None),
    };
    if !settable {
        return Err("cannot set a multiline or array-of-table value".into());
    }
    let encoded = encode_value(value);
    Ok(Some(format!("{}{}{}", &content[..vstart], encoded, &content[vend..])))
}

/// Keep `v` if it's a complete single-line TOML value; else make it a basic string.
fn encode_value(v: &str) -> String {
    let t = v.trim();
    if is_toml_value(t) {
        t.to_string()
    } else {
        encode_string(v)
    }
}

fn is_toml_value(t: &str) -> bool {
    let b = t.as_bytes();
    match b.first() {
        None => false,
        Some(b'"') | Some(b'\'') | Some(b'[') | Some(b'{') => {
            // must be a single complete value with nothing trailing
            matches!(scan_value(b, 0), Some((end, true, _)) if end == b.len())
        }
        // Bare value only if it's genuinely valid TOML; otherwise the caller
        // quotes it. Anything ambiguous (e.g. a semver like `0.2.0`) → quoted.
        _ => t == "true" || t == "false" || is_number(t) || is_datetime(t),
    }
}

/// A *real* (range-checked) RFC3339 date / date-time / local-time validator —
/// not a char-class scan. Anything that isn't a complete, in-range value falls
/// through to quoting, so `set` never emits a bare token tomllib would reject.
fn is_datetime(t: &str) -> bool {
    let b = t.as_bytes();
    // local time only: HH:MM:SS[.f]
    if !t.contains('-') {
        return parse_time(b, 0) == Some(b.len());
    }
    // date: YYYY-MM-DD with a real calendar check
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b[..4].iter().all(u8::is_ascii_digit) {
        return false;
    }
    let year = ((b[0] - b'0') as u16) * 1000
        + ((b[1] - b'0') as u16) * 100
        + ((b[2] - b'0') as u16) * 10
        + (b[3] - b'0') as u16;
    let (month, day) = match (two(b, 5), two(b, 8)) {
        (Some(m), Some(d)) => (m, d),
        _ => return false,
    };
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return false;
    }
    if b.len() == 10 {
        return true; // local date
    }
    // date-time: separator (T/t/space) + time + optional offset
    if !matches!(b[10], b'T' | b't' | b' ') {
        return false;
    }
    let after_time = match parse_time(b, 11) {
        Some(e) => e,
        None => return false,
    };
    if after_time == b.len() {
        return true; // local date-time, no offset
    }
    match b[after_time] {
        b'Z' | b'z' => after_time + 1 == b.len(),
        b'+' | b'-' => {
            // ±HH:MM
            after_time + 6 == b.len()
                && matches!(two(b, after_time + 1), Some(h) if h <= 23)
                && b[after_time + 3] == b':'
                && matches!(two(b, after_time + 4), Some(m) if m <= 59)
        }
        _ => false,
    }
}

/// Two ASCII digits at `i` as a value, or None.
fn two(b: &[u8], i: usize) -> Option<u8> {
    match (b.get(i), b.get(i + 1)) {
        (Some(a), Some(c)) if a.is_ascii_digit() && c.is_ascii_digit() => {
            Some((a - b'0') * 10 + (c - b'0'))
        }
        _ => None,
    }
}

/// Parse `HH:MM:SS[.fff]` at `i`; return the end index, or None if malformed/out of range.
fn parse_time(b: &[u8], i: usize) -> Option<usize> {
    let h = two(b, i)?;
    let m = two(b, i + 3)?;
    let s = two(b, i + 6)?;
    if h > 23 || m > 59 || s > 59 || b.get(i + 2) != Some(&b':') || b.get(i + 5) != Some(&b':') {
        return None;
    }
    let mut j = i + 8;
    if b.get(j) == Some(&b'.') {
        j += 1;
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            return None; // '.' with no fractional digits
        }
    }
    Some(j)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_number(t: &str) -> bool {
    let s = t.replace('_', "");
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+') | Some(b'-')) {
        i += 1;
    }
    let digits = |b: &[u8], i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if !digits(b, &mut i) {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !digits(b, &mut i) {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e') | Some(b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        if !digits(b, &mut i) {
            return false;
        }
    }
    i == b.len()
}

fn encode_string(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "\
# a config
title = \"hello\"
port = 8000
enabled = true

[server]
host = \"localhost\"   # inline comment
tags = [\"a\", \"b\"]

[server.tls]
enabled = false
";

    #[test]
    fn get_top_and_nested() {
        assert_eq!(get(T, "title").unwrap().as_deref(), Some("hello")); // decoded
        assert_eq!(get(T, "port").unwrap().as_deref(), Some("8000"));
        assert_eq!(get(T, "server.host").unwrap().as_deref(), Some("localhost"));
        assert_eq!(get(T, "server.tls.enabled").unwrap().as_deref(), Some("false"));
        assert_eq!(get(T, "server.tags").unwrap().as_deref(), Some("[\"a\", \"b\"]"));
        assert_eq!(get(T, "nope").unwrap(), None);
    }

    #[test]
    fn set_preserves_everything_else() {
        let out = set(T, "port", "9090").unwrap().unwrap();
        assert!(out.contains("port = 9090"));
        assert!(out.contains("# a config"));
        assert!(out.contains("[server]"));
        // only the value changed; comment on host stays
        assert!(out.contains("host = \"localhost\"   # inline comment"));
    }

    #[test]
    fn set_keeps_inline_comment() {
        let out = set(T, "server.host", "0.0.0.0").unwrap().unwrap();
        assert!(out.contains("host = \"0.0.0.0\"   # inline comment"));
    }

    #[test]
    fn set_bare_word_is_quoted_number_is_bare() {
        assert!(set(T, "title", "bye").unwrap().unwrap().contains("title = \"bye\""));
        assert!(set(T, "port", "1234").unwrap().unwrap().contains("port = 1234"));
        assert!(set(T, "enabled", "false").unwrap().unwrap().contains("enabled = false"));
    }

    #[test]
    fn set_missing_is_none() {
        assert_eq!(set(T, "ghost", "1").unwrap(), None);
    }

    #[test]
    fn set_never_emits_invalid_bare_value() {
        let doc = "v = \"x\"\n";
        // NOT valid bare TOML → must be quoted (semver, words, and date/time-shaped garbage)
        for bad in [
            "0.2.0", "1.2.3", "hello", "1.2.3-rc1", "a b",
            "2024-13-99zzz", "9999-99-99", "0000-00-00", "2024-01-01zzzz", "1234-56-78",
            "2024-01-15Tzzzz", "0000-00-00T00:00:00z", "2024-01-15.", "12:99:99", "99:99:99",
            "1:2:3", "2024-01-15+", "2024-02-31", "0000-01-01",
        ] {
            let out = set(doc, "v", bad).unwrap().unwrap();
            assert_eq!(out, format!("v = \"{bad}\"\n"), "{bad} must be quoted");
        }
        // genuine bare values stay bare
        for good in [
            "42", "-1", "3.14", "1e9", "true", "false",
            "2024-01-15", "07:32:00", "2024-02-29", "2024-01-15T07:32:00Z",
            "2024-01-15T07:32:00.500+02:00", "2024-01-15 07:32:00",
        ] {
            let out = set(doc, "v", good).unwrap().unwrap();
            assert_eq!(out, format!("v = {good}\n"), "{good} must be bare");
        }
    }

    #[test]
    fn multiline_string_is_get_only() {
        let m = "x = \"\"\"\nline1\nline2\n\"\"\"\ny = 1\n";
        // y still addressable past the multiline string (no misparse)
        assert_eq!(get(m, "y").unwrap().as_deref(), Some("1"));
        // setting the multiline value errors, never corrupts
        assert!(set(m, "x", "z").is_err());
    }

    #[test]
    fn array_of_tables_not_settable() {
        let m = "[[bin]]\nname = \"a\"\n[[bin]]\nname = \"b\"\n";
        assert!(set(m, "bin.name", "c").is_err());
    }
}
