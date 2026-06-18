//! Surgical key=value (.env) editing.
//!
//! This is the AI-harness contract: reads and edits are deterministic and
//! *surgical*. Comments, blank lines, and ordering are always preserved — we
//! only ever touch the one line that owns the target key.
//!
//! A "key line" looks like:
//!
//! ```text
//!     [whitespace] [export ] KEY [whitespace] = value
//! ```
//!
//! where KEY is made of `[A-Za-z0-9_.]`. Anything that doesn't match (comments,
//! blanks, junk) is opaque and passes through untouched.

/// What we learned by parsing a single line.
struct KeyLine {
    /// The key name.
    key: String,
    /// Did the line carry an `export ` prefix? (We keep it on rewrite.)
    had_export: bool,
}

/// Try to read a `KEY=...` line. Returns `None` for comments, blanks, or any
/// line that isn't a well-formed key assignment.
fn parse_key_line(line: &str) -> Option<KeyLine> {
    let trimmed = line.trim_start();
    // A comment or blank line is never a key line.
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    // Optional `export ` prefix.
    let (rest, had_export) = match trimmed.strip_prefix("export ") {
        Some(r) => (r.trim_start(), true),
        None => (trimmed, false),
    };

    // The key runs up to the first char that isn't part of a key.
    let key_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(rest.len());
    if key_end == 0 {
        return None; // no key characters at all
    }
    let key = &rest[..key_end];

    // After the key, allow whitespace, then we MUST hit '='.
    let after = rest[key_end..].trim_start();
    if !after.starts_with('=') {
        return None;
    }

    Some(KeyLine {
        key: key.to_string(),
        had_export,
    })
}

/// Strip exactly one layer of matching surrounding quotes (`'` or `"`).
fn strip_one_quote_layer(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Get a key's value. Returns the value of the *last* occurrence (so later lines
/// win, matching shell semantics). Value = text after the first `=`, trimmed,
/// with one layer of surrounding quotes removed.
pub fn get(content: &str, key: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for line in content.lines() {
        if let Some(parsed) = parse_key_line(line) {
            if parsed.key == key {
                // Everything after the first '='.
                let value = match line.split_once('=') {
                    Some((_, v)) => v.trim(),
                    None => "",
                };
                found = Some(strip_one_quote_layer(value).to_string());
            }
        }
    }
    found
}

/// Unique keys in first-seen order.
pub fn keys(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        if let Some(parsed) = parse_key_line(line) {
            if !out.iter().any(|k| k == &parsed.key) {
                out.push(parsed.key);
            }
        }
    }
    out
}

/// Set (add or update) a key. If the key already exists, its line is rewritten in
/// place (preserving an `export ` prefix it had); otherwise `KEY=value` is
/// appended. The value is written verbatim — the caller owns any quoting. The
/// result always ends with exactly one trailing newline.
pub fn set(content: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut replaced = false;

    for line in lines.iter_mut() {
        if let Some(parsed) = parse_key_line(line) {
            if parsed.key == key {
                let prefix = if parsed.had_export { "export " } else { "" };
                *line = format!("{prefix}{key}={value}");
                replaced = true;
            }
        }
    }

    if !replaced {
        lines.push(format!("{key}={value}"));
    }

    finish(lines)
}

/// Remove every line that assigns `key`. Other lines (including comments and
/// blanks) are preserved exactly.
pub fn unset(content: &str, key: &str) -> String {
    let lines: Vec<String> = content
        .lines()
        .filter(|line| match parse_key_line(line) {
            Some(parsed) => parsed.key != key,
            None => true,
        })
        .map(|l| l.to_string())
        .collect();

    finish(lines)
}

/// Join lines back together with a single trailing newline. An empty result is
/// the empty string (no stray newline).
fn finish(lines: Vec<String>) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# a comment
FOO=1

export BAR=hello
BAZ = \"quoted value\"
# trailing comment
FOO=2
";

    #[test]
    fn get_returns_last_occurrence_trimmed() {
        assert_eq!(get(SAMPLE, "FOO").as_deref(), Some("2"));
    }

    #[test]
    fn get_strips_one_quote_layer() {
        assert_eq!(get(SAMPLE, "BAZ").as_deref(), Some("quoted value"));
    }

    #[test]
    fn get_missing_is_none() {
        assert_eq!(get(SAMPLE, "NOPE"), None);
    }

    #[test]
    fn keys_are_unique_in_first_seen_order() {
        assert_eq!(keys(SAMPLE), vec!["FOO", "BAR", "BAZ"]);
    }

    #[test]
    fn set_updates_in_place_and_preserves_comments_and_order() {
        let out = set(SAMPLE, "BAR", "world");
        // BAR line rewritten, export kept, everything else intact.
        assert!(out.contains("export BAR=world"));
        assert!(out.contains("# a comment"));
        assert!(out.contains("# trailing comment"));
        // Order preserved: comment still first.
        assert!(out.starts_with("# a comment"));
    }

    #[test]
    fn set_appends_when_missing() {
        let out = set("A=1\n", "B", "2");
        assert_eq!(out, "A=1\nB=2\n");
    }

    #[test]
    fn set_only_touches_target_line() {
        let out = set(SAMPLE, "FOO", "9");
        // Both FOO lines become 9; nothing else changes.
        assert!(out.contains("FOO=9"));
        assert!(!out.contains("FOO=1"));
        assert!(!out.contains("FOO=2"));
        assert!(out.contains("export BAR=hello"));
    }

    #[test]
    fn unset_removes_all_occurrences_keeps_rest() {
        let out = unset(SAMPLE, "FOO");
        assert!(!out.contains("FOO="));
        assert!(out.contains("export BAR=hello"));
        assert!(out.contains("# a comment"));
    }

    #[test]
    fn always_single_trailing_newline() {
        assert!(set("A=1", "A", "2").ends_with("2\n"));
        assert!(!set("A=1", "A", "2").ends_with("\n\n"));
    }

    #[test]
    fn empty_stays_empty() {
        assert_eq!(unset("", "X"), "");
    }
}
