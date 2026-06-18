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

/// The text of line `n` (1-based), without terminator.
pub fn line_at(content: &str, n: usize) -> Option<String> {
    if n == 0 {
        return None;
    }
    content.lines().nth(n - 1).map(str::to_string)
}

/// The text of lines `a..=b` (1-based), joined by newlines.
pub fn range_text(content: &str, a: usize, b: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if a == 0 || a > b || b > lines.len() {
        return None;
    }
    Some(lines[a - 1..b].join("\n"))
}

/// A single operation in a transactional `apply`. All line numbers refer to the
/// **original** document, so order doesn't matter and numbers never drift.
pub enum Op {
    Expect(usize, String),
    Replace(usize, String),
    Insert(usize, String), // insert after this line (0 = top)
    Delete(usize, usize),
}

/// Apply a batch of ops atomically. Every op is resolved against the original
/// line numbers; expectations are checked first, conflicts (two ops touching the
/// same line) are rejected, and the whole batch fails as a unit (returns `Err`)
/// without producing partial output.
pub fn apply_ops(content: &str, ops: &[Op]) -> Result<String, String> {
    let orig: Vec<String> = split(content);
    let n = orig.len();

    // 1. Check every expectation up front.
    for op in ops {
        if let Op::Expect(line, want) = op {
            match orig.get(line.wrapping_sub(1)) {
                Some(actual) if line >= &1 && actual == want => {}
                _ => {
                    return Err(format!(
                        "expect failed at line {line}: file does not match"
                    ))
                }
            }
        }
    }

    // 2. Plan edits against original indices, rejecting conflicts.
    let mut deleted = vec![false; n];
    let mut replaced: Vec<Option<Vec<String>>> = vec![None; n];
    let mut inserts: Vec<Vec<String>> = vec![Vec::new(); n + 1]; // index = "after" line
    for op in ops {
        match op {
            Op::Expect(..) => {}
            Op::Replace(line, text) => {
                if *line == 0 || *line > n {
                    return Err(format!("replace: line {line} out of range (1..={n})"));
                }
                let i = line - 1;
                if deleted[i] || replaced[i].is_some() {
                    return Err(format!("conflict: line {line} edited twice"));
                }
                replaced[i] = Some(text.split('\n').map(str::to_string).collect());
            }
            Op::Delete(a, b) => {
                if *a == 0 || a > b || *b > n {
                    return Err(format!("delete: range {a}-{b} out of range (1..={n})"));
                }
                for i in (a - 1)..*b {
                    if deleted[i] || replaced[i].is_some() {
                        return Err(format!("conflict: line {} edited twice", i + 1));
                    }
                    deleted[i] = true;
                }
            }
            Op::Insert(after, text) => {
                if *after > n {
                    return Err(format!("insert: line {after} out of range (0..={n})"));
                }
                inserts[*after].extend(text.split('\n').map(str::to_string));
            }
        }
    }

    // 3. Rebuild the document.
    let mut out: Vec<String> = Vec::new();
    out.extend(inserts[0].clone());
    for i in 0..n {
        if deleted[i] {
            // dropped
        } else if let Some(rep) = &replaced[i] {
            out.extend(rep.clone());
        } else {
            out.push(orig[i].clone());
        }
        out.extend(inserts[i + 1].clone());
    }
    Ok(join(out))
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

    #[test]
    fn line_and_range_lookups() {
        assert_eq!(line_at(DOC, 2).as_deref(), Some("beta"));
        assert_eq!(line_at(DOC, 9), None);
        assert_eq!(range_text(DOC, 1, 2).as_deref(), Some("alpha\nbeta"));
    }

    #[test]
    fn apply_multiple_ops_on_original_numbers() {
        // delete 1, replace 3 — both use ORIGINAL numbering, order-independent.
        let ops = vec![Op::Delete(1, 1), Op::Replace(3, "GAMMA".to_string())];
        assert_eq!(apply_ops(DOC, &ops).unwrap(), "beta\nGAMMA\n");
    }

    #[test]
    fn apply_insert_and_replace_together() {
        let ops = vec![Op::Insert(0, "TOP".to_string()), Op::Replace(2, "BETA".to_string()), Op::Insert(3, "BOT".to_string())];
        assert_eq!(apply_ops(DOC, &ops).unwrap(), "TOP\nalpha\nBETA\ngamma\nBOT\n");
    }

    #[test]
    fn apply_is_atomic_on_expect_failure() {
        let ops = vec![Op::Expect(2, "WRONG".to_string()), Op::Replace(1, "X".to_string())];
        assert!(apply_ops(DOC, &ops).is_err());
    }

    #[test]
    fn apply_rejects_conflicts() {
        let ops = vec![Op::Replace(2, "x".to_string()), Op::Delete(2, 2)];
        assert!(apply_ops(DOC, &ops).is_err());
    }

    #[test]
    fn apply_expect_passes() {
        let ops = vec![Op::Expect(2, "beta".to_string()), Op::Replace(2, "BETA".to_string())];
        assert_eq!(apply_ops(DOC, &ops).unwrap(), "alpha\nBETA\ngamma\n");
    }
}
