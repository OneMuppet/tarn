//! Minimal span-tracking JSON, just enough for `json get`/`set` by path.
//!
//! The point is **surgical, format-preserving** edits: we parse only enough to
//! record each value's byte span, so `set` splices the one target value and
//! leaves every other byte — whitespace, key order, comments-adjacent layout —
//! exactly as it was. (A reserialize-the-whole-tree approach would reorder keys
//! and reflow the file; that's the opposite of what an agent wants.) No serde,
//! no float parsing — numbers/bools/null are located, never interpreted.
//!
//! Paths are dot-separated: `a.b.0.c` walks object key `a`, key `b`, array
//! index `0`, key `c`. (A literal key containing `.` isn't addressable — a
//! documented limitation.)

pub enum Kind {
    Obj(Vec<(usize, String, Node)>), // (key_start_byte, key, value)
    Arr(Vec<Node>),
    Str(String), // decoded value
    Scalar,      // number / bool / null — span only
}

pub struct Node {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { b: s.as_bytes(), i: 0 }
    }

    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Node, String> {
        self.ws();
        let start = self.i;
        match self.b.get(start) {
            Some(b'{') => self.parse_obj(start),
            Some(b'[') => self.parse_arr(start),
            Some(b'"') => {
                let s = self.parse_string()?;
                Ok(Node { start, end: self.i, kind: Kind::Str(s) })
            }
            Some(_) => self.parse_scalar(start),
            None => Err("unexpected end of JSON".into()),
        }
    }

    fn parse_obj(&mut self, start: usize) -> Result<Node, String> {
        self.i += 1; // consume '{'
        let mut members = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Node { start, end: self.i, kind: Kind::Obj(members) });
        }
        loop {
            self.ws();
            let key_start = self.i;
            if self.b.get(self.i) != Some(&b'"') {
                return Err("expected string key in object".into());
            }
            let key = self.parse_string()?;
            self.ws();
            if self.b.get(self.i) != Some(&b':') {
                return Err("expected ':' after key".into());
            }
            self.i += 1;
            let val = self.parse_value()?;
            members.push((key_start, key, val));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                _ => return Err("expected ',' or '}' in object".into()),
            }
        }
        Ok(Node { start, end: self.i, kind: Kind::Obj(members) })
    }

    fn parse_arr(&mut self, start: usize) -> Result<Node, String> {
        self.i += 1; // consume '['
        let mut items = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Node { start, end: self.i, kind: Kind::Arr(items) });
        }
        loop {
            items.push(self.parse_value()?);
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    break;
                }
                _ => return Err("expected ',' or ']' in array".into()),
            }
        }
        Ok(Node { start, end: self.i, kind: Kind::Arr(items) })
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.i += 1; // consume opening '"'
        let mut out = String::new();
        while let Some(&c) = self.b.get(self.i) {
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    let e = *self.b.get(self.i).ok_or("bad escape")?;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.b.get(self.i + 1..self.i + 5).ok_or("bad \\u escape")?;
                            let s = std::str::from_utf8(hex).map_err(|_| "bad \\u escape")?;
                            let cp = u32::from_str_radix(s, 16).map_err(|_| "bad \\u escape")?;
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                            self.i += 4;
                        }
                        _ => return Err("bad escape".into()),
                    }
                    self.i += 1;
                }
                _ => {
                    let len = utf8_len(c);
                    let chunk = self.b.get(self.i..self.i + len).ok_or("bad utf8")?;
                    out.push_str(std::str::from_utf8(chunk).map_err(|_| "bad utf8")?);
                    self.i += len;
                }
            }
        }
        Err("unterminated string".into())
    }

    fn parse_scalar(&mut self, start: usize) -> Result<Node, String> {
        while self.i < self.b.len()
            && !matches!(self.b[self.i], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
        {
            self.i += 1;
        }
        if self.i == start {
            return Err("expected a value".into());
        }
        Ok(Node { start, end: self.i, kind: Kind::Scalar })
    }
}

fn utf8_len(b: u8) -> usize {
    if b >= 0xf0 {
        4
    } else if b >= 0xe0 {
        3
    } else if b >= 0xc0 {
        2
    } else {
        1
    }
}

fn parse(content: &str) -> Result<Node, String> {
    let mut p = Parser::new(content);
    let node = p.parse_value()?;
    p.ws();
    if p.i != content.len() {
        return Err("trailing characters after JSON value".into());
    }
    Ok(node)
}

fn navigate<'n>(root: &'n Node, path: &str) -> Option<&'n Node> {
    if path.is_empty() {
        return Some(root);
    }
    let segs: Vec<&str> = path.split('.').collect();
    navigate_segs(root, &segs)
}

fn navigate_segs<'n>(root: &'n Node, segs: &[&str]) -> Option<&'n Node> {
    let mut cur = root;
    for seg in segs {
        cur = match &cur.kind {
            Kind::Obj(members) => &members.iter().find(|(_, k, _)| k == seg)?.2,
            Kind::Arr(items) => items.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Get the value at `path`: strings decoded, everything else as raw JSON text.
/// `Ok(None)` means the path doesn't exist.
pub fn get(content: &str, path: &str) -> Result<Option<String>, String> {
    let root = parse(content)?;
    Ok(navigate(&root, path).map(|n| match &n.kind {
        Kind::Str(s) => s.clone(),
        _ => content[n.start..n.end].to_string(),
    }))
}

/// Set the value at `path`, preserving all surrounding bytes. `value` is used
/// verbatim if it's already valid JSON, otherwise encoded as a JSON string.
/// `Ok(None)` means the path doesn't exist (set never creates paths).
pub fn set(content: &str, path: &str, value: &str) -> Result<Option<String>, String> {
    let root = parse(content)?;
    let span = navigate(&root, path).map(|n| (n.start, n.end));
    Ok(span.map(|(start, end)| {
        format!("{}{}{}", &content[..start], encode_value(value), &content[end..])
    }))
}

/// Delete the value at `path`, removing the member (object key) or element
/// (array index) and exactly one adjacent comma so the result stays valid JSON.
/// `Ok(None)` = path absent; `Err` = parent isn't an object/array.
pub fn del(content: &str, path: &str) -> Result<Option<String>, String> {
    let root = parse(content)?;
    let segs: Vec<&str> = path.split('.').collect();
    if segs.is_empty() {
        return Ok(None);
    }
    let (parent_segs, last) = segs.split_at(segs.len() - 1);
    let last = last[0];
    let parent = match navigate_segs(&root, parent_segs) {
        Some(p) => p,
        None => return Ok(None),
    };
    // Compute the [a, b) byte span to splice out, comma-aware.
    let span = match &parent.kind {
        Kind::Obj(members) => {
            let i = match members.iter().position(|(_, k, _)| k == last) {
                Some(i) => i,
                None => return Ok(None),
            };
            let n = members.len();
            let (ks, _, val) = &members[i];
            if n == 1 {
                (*ks, val.end) // only member
            } else if i + 1 < n {
                (*ks, members[i + 1].0) // member + the comma up to the next key
            } else {
                (members[i - 1].2.end, val.end) // last: drop the comma before it
            }
        }
        Kind::Arr(items) => {
            let i = match last.parse::<usize>() {
                Ok(i) if i < items.len() => i,
                _ => return Ok(None),
            };
            let n = items.len();
            if n == 1 {
                (items[0].start, items[0].end)
            } else if i + 1 < n {
                (items[i].start, items[i + 1].start)
            } else {
                (items[i - 1].end, items[i].end)
            }
        }
        _ => return Err("parent is not an object or array".into()),
    };
    Ok(Some(format!("{}{}", &content[..span.0], &content[span.1..])))
}

/// Keep `v` as-is if it is a complete JSON value; otherwise encode as a string.
fn encode_value(v: &str) -> String {
    let t = v.trim();
    if is_complete_json(t) {
        t.to_string()
    } else {
        encode_string(v)
    }
}

/// Is `t` a complete, valid JSON value? Containers/strings must parse and fully
/// consume; a scalar must be exactly `true`/`false`/`null` or a JSON number.
/// (Our lenient scalar parser would otherwise accept a bare word like `hello`.)
fn is_complete_json(t: &str) -> bool {
    match t.as_bytes().first() {
        Some(b'{') | Some(b'[') | Some(b'"') => {
            let mut p = Parser::new(t);
            p.parse_value().is_ok() && {
                p.ws();
                p.i == t.len()
            }
        }
        _ => t == "true" || t == "false" || t == "null" || is_number(t),
    }
}

/// Strict JSON number grammar: `-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?`.
/// Rejects what `f64::parse` would wrongly accept (`1.`, `00`, `0.`, `+5`, `.5`).
fn is_number(t: &str) -> bool {
    let b = t.as_bytes();
    let n = b.len();
    let mut i = 0;
    let digits = |b: &[u8], i: &mut usize| -> bool {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if i < n && b[i] == b'-' {
        i += 1;
    }
    // integer part: a lone 0, or a nonzero digit followed by more digits
    match b.get(i) {
        Some(b'0') => i += 1,
        Some(c) if c.is_ascii_digit() => {
            while i < n && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        _ => return false,
    }
    // optional fraction (requires at least one digit after '.')
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !digits(b, &mut i) {
            return false;
        }
    }
    // optional exponent
    if matches!(b.get(i), Some(b'e') | Some(b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        if !digits(b, &mut i) {
            return false;
        }
    }
    i == n
}

fn encode_string(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    const J: &str = r#"{ "a": { "b": [10, 20, { "c": "hi" }] }, "port": 8000 }"#;

    #[test]
    fn get_scalar_and_nested() {
        assert_eq!(get(J, "port").unwrap().as_deref(), Some("8000"));
        assert_eq!(get(J, "a.b.0").unwrap().as_deref(), Some("10"));
        assert_eq!(get(J, "a.b.2.c").unwrap().as_deref(), Some("hi")); // decoded string
    }

    #[test]
    fn get_object_returns_raw() {
        assert_eq!(get(J, "a.b.2").unwrap().as_deref(), Some(r#"{ "c": "hi" }"#));
    }

    #[test]
    fn get_missing_is_none() {
        assert_eq!(get(J, "nope").unwrap(), None);
        assert_eq!(get(J, "a.b.9").unwrap(), None);
    }

    #[test]
    fn set_preserves_surrounding_format() {
        let out = set(J, "port", "9090").unwrap().unwrap();
        assert_eq!(out, r#"{ "a": { "b": [10, 20, { "c": "hi" }] }, "port": 9090 }"#);
    }

    #[test]
    fn set_bare_string_is_quoted() {
        let out = set(J, "a.b.2.c", "bye").unwrap().unwrap();
        assert!(out.contains(r#"{ "c": "bye" }"#));
    }

    #[test]
    fn set_json_value_used_verbatim() {
        let out = set(J, "a.b.0", "[1,2]").unwrap().unwrap();
        assert!(out.contains("[[1,2], 20,"));
    }

    #[test]
    fn set_missing_path_is_none() {
        assert_eq!(set(J, "nope", "1").unwrap(), None);
    }

    #[test]
    fn invalid_json_errors() {
        assert!(get("{bad", "x").is_err());
    }

    #[test]
    fn del_member_stays_valid() {
        let j = r#"{"a": 1, "b": 2, "c": 3}"#;
        assert_eq!(del(j, "b").unwrap().unwrap(), r#"{"a": 1, "c": 3}"#); // middle
        assert_eq!(del(j, "a").unwrap().unwrap(), r#"{"b": 2, "c": 3}"#); // first
        assert_eq!(del(j, "c").unwrap().unwrap(), r#"{"a": 1, "b": 2}"#); // last
        assert_eq!(del(r#"{"only": 1}"#, "only").unwrap().unwrap(), "{}"); // only
        assert_eq!(del(j, "z").unwrap(), None); // missing
    }

    #[test]
    fn del_array_element_stays_valid() {
        let a = "[10, 20, 30]";
        assert_eq!(del(a, "1").unwrap().unwrap(), "[10, 30]");
        assert_eq!(del(a, "0").unwrap().unwrap(), "[20, 30]");
        assert_eq!(del(a, "2").unwrap().unwrap(), "[10, 20]");
        assert_eq!(del("[5]", "0").unwrap().unwrap(), "[]");
    }

    #[test]
    fn del_nested() {
        let j = r#"{"x": {"a": 1, "b": 2}, "y": 9}"#;
        assert_eq!(del(j, "x.a").unwrap().unwrap(), r#"{"x": {"b": 2}, "y": 9}"#);
    }

    #[test]
    fn number_grammar_is_strict() {
        // not valid JSON numbers → must be quoted, never emitted bare
        for bad in ["1.", "00", "0.", "+5", ".5", "1abc", "1e", "--1"] {
            let out = set(r#"{"k":1}"#, "k", bad).unwrap().unwrap();
            assert_eq!(out, format!("{{\"k\":\"{bad}\"}}"), "{bad} must be quoted");
        }
        // valid JSON numbers → used verbatim (bare)
        for good in ["0", "-0", "12", "1.5", "1e10", "-3.2e-4", "0.5"] {
            let out = set(r#"{"k":1}"#, "k", good).unwrap().unwrap();
            assert_eq!(out, format!("{{\"k\":{good}}}"), "{good} must be bare");
        }
    }

    #[test]
    fn trailing_junk_is_rejected() {
        assert!(get(r#"{"a":1} junk"#, "a").is_err());
        assert!(get("[1,2,3]xxx", "0").is_err());
        assert!(get("42 junk", "").is_err());
        // but trailing whitespace/newline is fine
        assert_eq!(get("{\"a\":1}\n", "a").unwrap().as_deref(), Some("1"));
    }
}
