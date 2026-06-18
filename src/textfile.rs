//! Line-addressable, surgical editing of arbitrary text files.
//!
//! The companion to `envfile` for non-key=value documents. Line numbers are
//! **1-based** to match what `show` prints in its gutter, so an agent can read a
//! line number off the rendered view and edit exactly that line. Only the
//! targeted lines change; every other line is preserved verbatim. The result
//! always ends with a single trailing newline (same convention as `envfile`).

/// Split into lines without their terminators.
fn split(content: &str) -> Vec<String> {
    content.lines().map(|l| l.to_string()).collect()
}

/// Join lines back with a single trailing newline (empty stays empty).
fn join(lines: Vec<String>) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    }
}

/// Replace line `n` (1-based) with `text`. `text` may contain newlines, in which
/// case it expands into multiple lines.
pub fn replace(content: &str, n: usize, text: &str) -> Result<String, String> {
    let mut lines = split(content);
    if n == 0 || n > lines.len() {
        return Err(format!("line {n} is out of range (file has {} lines)", lines.len()));
    }
    let repl: Vec<String> = text.split('\n').map(str::to_string).collect();
    lines.splice(n - 1..n, repl);
    Ok(join(lines))
}

/// Insert `text` after line `after` (1-based; `0` inserts before the first line).
/// `text` may contain newlines to insert several lines at once.
pub fn insert(content: &str, after: usize, text: &str) -> Result<String, String> {
    let mut lines = split(content);
    if after > lines.len() {
        return Err(format!("line {after} is out of range (file has {} lines)", lines.len()));
    }
    let ins: Vec<String> = text.split('\n').map(str::to_string).collect();
    lines.splice(after..after, ins);
    Ok(join(lines))
}

/// Delete lines `a..=b` (1-based, inclusive).
pub fn delete(content: &str, a: usize, b: usize) -> Result<String, String> {
    let mut lines = split(content);
    if a == 0 || a > b || b > lines.len() {
        return Err(format!("range {a}-{b} is out of range (file has {} lines)", lines.len()));
    }
    lines.drain(a - 1..b);
    Ok(join(lines))
}

/// Normalize arbitrary input to a document with exactly one trailing newline.
pub fn normalize(input: &str) -> String {
    join(split(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "alpha\nbeta\ngamma\n";

    #[test]
    fn replace_one_line() {
        assert_eq!(replace(DOC, 2, "BETA").unwrap(), "alpha\nBETA\ngamma\n");
    }

    #[test]
    fn replace_expands_multiline() {
        assert_eq!(replace(DOC, 2, "b1\nb2").unwrap(), "alpha\nb1\nb2\ngamma\n");
    }

    #[test]
    fn replace_out_of_range_errors() {
        assert!(replace(DOC, 0, "x").is_err());
        assert!(replace(DOC, 4, "x").is_err());
    }

    #[test]
    fn insert_after_line() {
        assert_eq!(insert(DOC, 1, "NEW").unwrap(), "alpha\nNEW\nbeta\ngamma\n");
    }

    #[test]
    fn insert_at_top() {
        assert_eq!(insert(DOC, 0, "TOP").unwrap(), "TOP\nalpha\nbeta\ngamma\n");
    }

    #[test]
    fn insert_at_end() {
        assert_eq!(insert(DOC, 3, "END").unwrap(), "alpha\nbeta\ngamma\nEND\n");
    }

    #[test]
    fn delete_range() {
        assert_eq!(delete(DOC, 1, 2).unwrap(), "gamma\n");
    }

    #[test]
    fn delete_single() {
        assert_eq!(delete(DOC, 2, 2).unwrap(), "alpha\ngamma\n");
    }

    #[test]
    fn delete_bad_range_errors() {
        assert!(delete(DOC, 2, 1).is_err());
        assert!(delete(DOC, 1, 9).is_err());
    }

    #[test]
    fn always_single_trailing_newline() {
        assert!(replace("a", 1, "b").unwrap().ends_with("b\n"));
        assert!(!replace("a", 1, "b").unwrap().ends_with("\n\n"));
    }
}
