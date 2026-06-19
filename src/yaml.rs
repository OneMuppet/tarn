//! Minimal block-mapping YAML for `yaml get`/`set` by path — same philosophy as
//! `json`/`toml`: locate a scalar value's byte span and splice only that, so
//! comments, key order, and layout are byte-preserved.
//!
//! SUPPORTED: nested block mappings (`key: value`, indentation = nesting) with
//! single-line scalar values (plain, "double", 'single'). Dotted path `a.b.c`.
//!
//! NOT supported — and `set` ERRORS rather than risk a bad edit (it never
//! corrupts): sequences (`- item`), flow collections (`[..]` / `{..}`), block
//! scalars (`|` / `>`), anchors/aliases/tags (`&`/`*`/`!`), and multi-document
//! streams (a second `---`). Tab indentation is rejected outright.

struct Entry {
    path: Vec<String>,
    vstart: usize,
    vend: usize,
    settable: bool,
    is_string: bool, // quoted scalar (so `get` decodes it)
}

/// Leading-space count; `Err` if the indentation uses a tab (YAML forbids it,
/// and we won't guess).
fn indent_of(line: &[u8]) -> Result<usize, String> {
    let mut n = 0;
    while n < line.len() && line[n] == b' ' {
        n += 1;
    }
    if line.get(n) == Some(&b'\t') {
        return Err("tab indentation is not supported".into());
    }
    Ok(n)
}

/// Find the `:` that separates a mapping key from its value: a colon followed by
/// a space or end-of-line. Returns the byte index of the colon within `line`.
fn mapping_colon(line: &[u8]) -> Option<usize> {
    let mut i = 0;
    // a quoted key: skip the quoted span first
    if line.first() == Some(&b'"') || line.first() == Some(&b'\'') {
        let q = line[0];
        i = 1;
        while i < line.len() && line[i] != q {
            if q == b'"' && line[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        i += 1; // past closing quote
    }
    while i < line.len() {
        if line[i] == b':' && (i + 1 == line.len() || line[i + 1] == b' ') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn unquote_key(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Scan a single-line scalar value within `line` (relative bytes) starting at
/// `vs`; return (value_end_in_line_trimmed, settable, is_string). `vs` points at
/// the first non-space char after the colon.
fn scan_scalar(line: &[u8], vs: usize) -> (usize, bool, bool) {
    match line.get(vs) {
        // unsupported single-line forms: not settable (value spans to EOL as-is)
        Some(b'|') | Some(b'>') | Some(b'[') | Some(b'{') | Some(b'&') | Some(b'*')
        | Some(b'!') => (line.len(), false, false),
        Some(b'"') | Some(b'\'') => {
            let q = line[vs];
            let mut i = vs + 1;
            while i < line.len() && line[i] != q {
                if q == b'"' && line[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= line.len() {
                return (line.len(), false, false); // unterminated → not settable
            }
            (i + 1, true, true)
        }
        Some(_) => {
            // plain scalar: up to a " #" comment or EOL, trimmed
            let mut i = vs;
            while i < line.len() {
                if line[i] == b'#' && i > vs && line[i - 1] == b' ' {
                    break;
                }
                i += 1;
            }
            let mut end = i;
            while end > vs && line[end - 1] == b' ' {
                end -= 1;
            }
            (end, true, false)
        }
        None => (vs, false, false),
    }
}

fn entries(content: &str) -> Result<Vec<Entry>, String> {
    let mut out: Vec<Entry> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new(); // (indent, key) of open mappings
    let mut doc_count = 0;
    let mut offset = 0; // byte offset of current line start
    let mut seq_skip: Option<usize> = None; // inside a sequence: skip lines this-indented-or-deeper

    for raw in content.split_inclusive('\n') {
        let line_len = raw.len();
        // Parse the line WITHOUT its terminator (`\n` and any `\r`), but advance
        // `offset` by the full raw length — so a CRLF's `\r` stays outside every
        // value span and round-trips untouched.
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let lb = line.as_bytes();
        let trimmed = line.trim_start();

        // document markers: a `---` after any content (or a second `---`) means
        // a multi-document stream, which we don't address into.
        if trimmed == "---" || trimmed.starts_with("--- ") {
            if !out.is_empty() || doc_count >= 1 {
                return Err("multi-document YAML is not supported".into());
            }
            doc_count = 1;
            stack.clear();
            offset += line_len;
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "..." {
            offset += line_len;
            continue;
        }

        let ind = indent_of(lb)?;

        // Inside a sequence: its items (and their nested content) are not
        // addressable as mapping paths, so skip everything at the item indent or
        // deeper. A shallower line ends the sequence.
        if let Some(s) = seq_skip {
            if ind >= s {
                offset += line_len;
                continue;
            }
            seq_skip = None;
        }

        // sequence item: the enclosing key holds a list — mark it non-settable
        // and skip the whole sequence subtree from here.
        if trimmed == "-" || trimmed.starts_with("- ") {
            if let Some(last) = out.last_mut() {
                last.settable = false;
            }
            seq_skip = Some(ind);
            offset += line_len;
            continue;
        }

        let colon = match mapping_colon(&lb[ind..]) {
            Some(c) => ind + c,
            None => {
                // not a mapping line we understand; skip without corrupting
                offset += line_len;
                continue;
            }
        };
        let key = unquote_key(line[ind..colon].trim());

        // nesting: pop deeper-or-equal levels
        while matches!(stack.last(), Some((i, _)) if *i >= ind) {
            stack.pop();
        }
        let mut path: Vec<String> = stack.iter().map(|(_, k)| k.clone()).collect();
        path.push(key.clone());

        // value after the colon
        let mut vs_rel = colon + 1;
        while vs_rel < lb.len() && lb[vs_rel] == b' ' {
            vs_rel += 1;
        }
        if vs_rel >= lb.len() || lb[vs_rel] == b'#' {
            // empty value → this key opens a nested mapping/sequence
            stack.push((ind, key));
            // record it as a (non-settable) parent so `set` on it errors clearly
            out.push(Entry { path, vstart: offset + lb.len(), vend: offset + lb.len(), settable: false, is_string: false });
        } else {
            let (vend_rel, settable, is_string) = scan_scalar(lb, vs_rel);
            out.push(Entry {
                path,
                vstart: offset + vs_rel,
                vend: offset + vend_rel,
                settable,
                is_string,
            });
        }
        offset += line_len;
    }
    Ok(out)
}

fn find<'a>(es: &'a [Entry], path: &str) -> Option<&'a Entry> {
    let want: Vec<&str> = path.split('.').collect();
    es.iter().find(|e| e.path.len() == want.len() && e.path.iter().zip(&want).all(|(a, b)| a == b))
}

fn decode_string(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0] == b'\'' && b[b.len() - 1] == b'\'' {
        return s[1..s.len() - 1].replace("''", "'");
    }
    if b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"' {
        let inner = &s[1..s.len() - 1];
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(c) = it.next() {
            if c == '\\' {
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
                out.push(c);
            }
        }
        return out;
    }
    s.to_string()
}

/// Get the value at `path`. Quoted scalars are decoded; plain scalars returned raw.
pub fn get(content: &str, path: &str) -> Result<Option<String>, String> {
    let es = entries(content)?;
    Ok(find(&es, path).and_then(|e| {
        // A parent key (a nested mapping / sequence owner) has an empty value
        // span — there's no scalar to return, so report not-found rather than "".
        if e.vstart == e.vend {
            return None;
        }
        let raw = &content[e.vstart..e.vend];
        Some(if e.is_string { decode_string(raw) } else { raw.to_string() })
    }))
}

/// Delete the leaf scalar at `path` by removing its whole line. `Ok(None)` =
/// absent; `Err` = not a deletable leaf (parent/sequence/flow/multiline).
pub fn del(content: &str, path: &str) -> Result<Option<String>, String> {
    let es = entries(content)?;
    let pos = match find(&es, path) {
        Some(e) if e.settable => e.vstart,
        Some(_) => return Err("cannot delete a sequence, flow, multiline, or nested value".into()),
        None => return Ok(None),
    };
    let ls = content[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let le = content[pos..].find('\n').map(|i| pos + i + 1).unwrap_or(content.len());
    Ok(Some(format!("{}{}", &content[..ls], &content[le..])))
}

/// Set the value at `path`, preserving surrounding bytes. `Ok(None)` = absent;
/// `Err` = unsupported target (sequence/flow/block scalar/anchor/parent).
pub fn set(content: &str, path: &str, value: &str) -> Result<Option<String>, String> {
    let es = entries(content)?;
    let (vstart, vend, settable) = match find(&es, path) {
        Some(e) => (e.vstart, e.vend, e.settable),
        None => return Ok(None),
    };
    if !settable {
        return Err("cannot set a sequence, flow, multiline, or nested value".into());
    }
    Ok(Some(format!("{}{}{}", &content[..vstart], encode(value), &content[vend..])))
}

/// Encode a value for a plain YAML scalar slot, double-quoting whenever a plain
/// scalar would be unsafe (structural chars, indicators, edge whitespace).
fn encode(v: &str) -> String {
    if needs_quoting(v) {
        let mut o = String::from("\"");
        for c in v.chars() {
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
    } else {
        v.to_string()
    }
}

fn needs_quoting(v: &str) -> bool {
    if v.is_empty() || v != v.trim() {
        return true;
    }
    let first = v.as_bytes()[0];
    // YAML plain-scalar indicators that must not start a plain scalar
    if b"-?:,[]{}#&*!|>'\"%@`".contains(&first) {
        return true;
    }
    // a ": " or trailing ":" would break the mapping; "#" comment, control chars
    v.contains(": ") || v.ends_with(':') || v.contains(" #") || v.chars().any(|c| (c as u32) < 0x20)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Y: &str = "\
# config
name: demo
port: 8000
server:
  host: localhost   # inline comment
  tls:
    enabled: false
tags:
  - a
  - b
greeting: \"hello world\"
";

    #[test]
    fn get_nested_scalars() {
        assert_eq!(get(Y, "name").unwrap().as_deref(), Some("demo"));
        assert_eq!(get(Y, "port").unwrap().as_deref(), Some("8000"));
        assert_eq!(get(Y, "server.host").unwrap().as_deref(), Some("localhost"));
        assert_eq!(get(Y, "server.tls.enabled").unwrap().as_deref(), Some("false"));
        assert_eq!(get(Y, "greeting").unwrap().as_deref(), Some("hello world")); // decoded
        assert_eq!(get(Y, "missing").unwrap(), None);
    }

    #[test]
    fn set_preserves_layout_and_comment() {
        let out = set(Y, "server.host", "0.0.0.0").unwrap().unwrap();
        assert!(out.contains("  host: 0.0.0.0   # inline comment"));
        assert!(out.contains("# config"));
        assert!(out.contains("    enabled: false"));
    }

    #[test]
    fn set_scalar_types_and_quoting() {
        assert!(set(Y, "port", "9090").unwrap().unwrap().contains("port: 9090"));
        assert!(set(Y, "name", "a: b").unwrap().unwrap().contains("name: \"a: b\""));
        assert!(set(Y, "name", "plain").unwrap().unwrap().contains("name: plain"));
    }

    #[test]
    fn sequence_is_not_settable() {
        // `tags` owns a sequence → set must error, never corrupt
        assert!(set(Y, "tags", "x").is_err());
    }

    #[test]
    fn flow_and_block_scalars_not_settable() {
        let m = "a: [1, 2]\nb: |\n  line\nc: 1\n";
        assert!(set(m, "a", "x").is_err()); // flow
        assert!(set(m, "b", "x").is_err()); // block scalar
        assert_eq!(get(m, "c").unwrap().as_deref(), Some("1")); // still addressable after
    }

    #[test]
    fn multi_document_errors() {
        assert!(get("a: 1\n---\nb: 2\n", "b").is_err());
    }

    #[test]
    fn tab_indentation_errors() {
        assert!(get("a:\n\tb: 1\n", "a.b").is_err());
    }

    #[test]
    fn missing_set_is_none() {
        assert_eq!(set(Y, "ghost", "1").unwrap(), None);
    }

    #[test]
    fn get_on_parent_key_is_none() {
        // a key whose value is a nested mapping or sequence has no scalar to return
        let y = "server:\n  host: x\nsteps:\n  - a\n  - b\n";
        assert_eq!(get(y, "server").unwrap(), None); // nested mapping parent
        assert_eq!(get(y, "steps").unwrap(), None); // sequence owner
        assert_eq!(get(y, "server.host").unwrap().as_deref(), Some("x")); // real scalar still works
    }

    #[test]
    fn del_removes_leaf_line() {
        let y = "name: demo\nport: 8000\nhost: x\n";
        assert_eq!(del(y, "port").unwrap().unwrap(), "name: demo\nhost: x\n");
        assert_eq!(del(y, "ghost").unwrap(), None);
        assert!(del("a:\n  - 1\n", "a").is_err()); // can't delete a sequence parent
    }

    #[test]
    fn crlf_round_trips() {
        let crlf = "a: 1\r\nb: 2\r\n";
        // only the value changes; both lines keep their CRLF
        assert_eq!(set(crlf, "a", "5").unwrap().unwrap(), "a: 5\r\nb: 2\r\n");
        // get must not leak the \r
        assert_eq!(get("a: hello\r\n", "a").unwrap().as_deref(), Some("hello"));
    }

    #[test]
    fn sequence_contents_not_addressable() {
        // `image:` lives inside a `- ` list item, so it must NOT resolve as a path
        let m = "containers:\n  - name: web\n    image: nginx\nport: 80\n";
        assert_eq!(get(m, "containers.image").unwrap(), None);
        assert_eq!(get(m, "containers.name").unwrap(), None);
        // a sibling key after the sequence is still addressable
        assert_eq!(get(m, "port").unwrap().as_deref(), Some("80"));
    }
}
