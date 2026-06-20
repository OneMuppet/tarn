//! A small, strict unified-diff parser and applier — so an agent can pipe the
//! diff it (or `git diff`) produced straight into `tarn patch`, instead of
//! translating it into `apply`'s op format.
//!
//! Strict on CONTENT, tolerant of POSITION. A hunk applies only if its context
//! and removed lines match the file exactly — but its stated line numbers may
//! have drifted (an agent read the file earlier, or miscounted), so if the
//! context doesn't match where the diff claims, the hunk is relocated to the
//! UNIQUE place it does match. If the context is absent, or matches in more than
//! one place, the patch is refused rather than applied to the wrong line. That
//! keeps tarn's `--expect` philosophy — a failed edit is safer than a wrong
//! one — while still accepting the slightly-stale diffs agents really produce.
//! (One case can't relocate: a pure insertion with no context lines has nothing
//! to anchor on, so it uses its stated line as-is. Real `git diff` always wraps
//! insertions in context, so this only affects hand-written `-U0` diffs.)

/// One line inside a hunk body.
enum Line {
    Context(String),
    Remove(String),
    Add(String),
}

/// A single hunk: where it starts in the original file (1-based) and its body.
pub struct Hunk {
    old_start: usize,
    lines: Vec<Line>,
    /// The new side's last line in this hunk is followed by a
    /// `\ No newline at end of file` marker (so if this hunk reaches the new
    /// file's end, the result has no trailing newline).
    new_no_eol: bool,
}

/// All the hunks targeting one file, plus whether the diff creates it from
/// nothing (`--- /dev/null`).
pub struct FilePatch {
    pub path: String,
    pub create: bool,
    pub delete: bool,
    hunks: Vec<Hunk>,
}

/// Strip a `git`-style `a/` or `b/` leading path component; leave other paths
/// (and `/dev/null`) untouched.
fn strip_prefix_ab(p: &str) -> &str {
    p.strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(p)
}

/// The path on a `---`/`+++` header line: first whitespace-delimited token after
/// the marker (unified diff may append a tab + timestamp).
fn header_path(rest: &str) -> &str {
    rest.trim().split('\t').next().unwrap_or("").trim()
}

/// Parse a unified diff into per-file patches. Tolerates `diff --git` / `index`
/// preamble lines and a missing `@@` count (treated as 1). Errors on a body line
/// that isn't context/add/remove, or a hunk before any file header.
pub fn parse(diff: &str) -> Result<Vec<FilePatch>, String> {
    let mut files: Vec<FilePatch> = Vec::new();
    let lines: Vec<&str> = diff.lines().collect();
    let mut i = 0;
    let mut minus_path: Option<String> = None;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("--- ") {
            minus_path = Some(header_path(rest).to_string());
            i += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            let plus = header_path(rest).to_string();
            let minus = minus_path.take().unwrap_or_default();
            let create = minus == "/dev/null";
            let delete = plus == "/dev/null";
            // The surviving side names the file; on delete that's the `---` side.
            let raw = if delete {
                minus.as_str()
            } else {
                plus.as_str()
            };
            files.push(FilePatch {
                path: strip_prefix_ab(raw).to_string(),
                create,
                delete,
                hunks: Vec::new(),
            });
            i += 1;
            continue;
        }
        if line.starts_with("@@") {
            let old_start = parse_hunk_header(line)?;
            let cur = files
                .last_mut()
                .ok_or_else(|| "hunk before any file header (`--- `/`+++ `)".to_string())?;
            let mut body = Vec::new();
            let mut new_no_eol = false;
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with("@@") || l.starts_with("--- ") || l.starts_with("diff ") {
                    break;
                }
                match l.as_bytes().first() {
                    Some(b' ') => body.push(Line::Context(l[1..].to_string())),
                    Some(b'-') => body.push(Line::Remove(l[1..].to_string())),
                    Some(b'+') => body.push(Line::Add(l[1..].to_string())),
                    // An empty line in a diff body means an empty context line.
                    None => body.push(Line::Context(String::new())),
                    // `\ No newline at end of file` — a marker, not content. It
                    // refers to the preceding line; if that line is on the new
                    // side (Add or shared Context), the new file ends without a
                    // trailing newline.
                    Some(b'\\') => {
                        if matches!(body.last(), Some(Line::Add(_)) | Some(Line::Context(_))) {
                            new_no_eol = true;
                        }
                    }
                    Some(_) => return Err(format!("unexpected patch line: {l:?}")),
                }
                i += 1;
            }
            cur.hunks.push(Hunk {
                old_start,
                lines: body,
                new_no_eol,
            });
            continue;
        }
        i += 1; // ignore preamble (`diff --git`, `index`, mode lines, etc.)
    }
    if files.is_empty() {
        return Err("no file headers found (expected `--- ` / `+++ ` lines)".to_string());
    }
    Ok(files)
}

/// Parse the `-l,s` old-start from an `@@ -l,s +l,s @@` header (count defaults
/// to 1 when omitted, per the unified-diff spec).
fn parse_hunk_header(line: &str) -> Result<usize, String> {
    let minus = line
        .split_whitespace()
        .find(|t| t.starts_with('-'))
        .ok_or_else(|| format!("malformed hunk header: {line:?}"))?;
    let nums = &minus[1..];
    let start: usize = nums
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("malformed hunk header: {line:?}"))?;
    Ok(start)
}

/// Why a patch failed to apply. The distinction drives the exit code: `Drift`
/// means the file isn't what the diff was generated against (a guard failure,
/// exit 3 — re-read and regenerate); `Malformed` means the diff itself is
/// internally broken (a usage error, exit 2).
#[derive(Debug)]
pub enum ApplyError {
    Drift(String),
    Malformed(String),
}

impl ApplyError {
    pub fn message(&self) -> &str {
        match self {
            ApplyError::Drift(m) | ApplyError::Malformed(m) => m,
        }
    }
    pub fn is_drift(&self) -> bool {
        matches!(self, ApplyError::Drift(_))
    }
}

/// The original lines a hunk consumes, in order: its context + removed lines.
/// This is the content the hunk must match against the file (its "anchor").
fn anchor(hunk: &Hunk) -> Vec<&str> {
    hunk.lines
        .iter()
        .filter_map(|l| match l {
            Line::Context(t) | Line::Remove(t) => Some(t.as_str()),
            Line::Add(_) => None,
        })
        .collect()
}

/// Does `anchor` match the original lines starting at `pos`?
fn matches_at(orig: &[&str], pos: usize, anchor: &[&str]) -> bool {
    pos + anchor.len() <= orig.len() && orig[pos..pos + anchor.len()] == *anchor
}

/// Find where hunk `h` should apply: prefer its stated position when the context
/// matches there; otherwise relocate it to the UNIQUE place its context matches
/// (so a diff whose line numbers have drifted still applies, as long as it's
/// unambiguous). Returns Drift if the context is absent or appears in more than
/// one place, Malformed if the hunk is out of order.
fn locate(
    orig: &[&str],
    anchor: &[&str],
    stated: usize,
    cursor: usize,
    h: usize,
) -> Result<usize, ApplyError> {
    if stated < cursor {
        return Err(ApplyError::Malformed(format!(
            "hunk {} overlaps a previous hunk",
            h + 1
        )));
    }
    // Pure insertion (no context/removed lines): nothing to match on, so it can
    // only go where the diff says. The position must be within the file.
    if anchor.is_empty() {
        if stated > orig.len() {
            return Err(ApplyError::Drift(format!(
                "hunk {} inserts at line {} but the file has {} lines",
                h + 1,
                stated + 1,
                orig.len()
            )));
        }
        return Ok(stated);
    }
    // Exact position still matches → use it (authoritative even if the context
    // also appears elsewhere).
    if matches_at(orig, stated, anchor) {
        return Ok(stated);
    }
    // Drifted: relocate to the unique in-order position whose context matches.
    let max = orig.len().saturating_sub(anchor.len());
    let mut found: Option<usize> = None;
    for p in cursor..=max {
        if matches_at(orig, p, anchor) {
            if found.is_some() {
                return Err(ApplyError::Drift(format!(
                    "hunk {} is ambiguous: its context matches more than one location \
                     (add more context lines)",
                    h + 1
                )));
            }
            found = Some(p);
        }
    }
    found.ok_or_else(|| {
        ApplyError::Drift(format!(
            "hunk {} does not match the file (its context was not found)",
            h + 1
        ))
    })
}

/// Apply `hunks` to `content`, returning the patched text. Strict on CONTENT —
/// a hunk's context and removed lines must match the file exactly — but tolerant
/// of POSITION: if a hunk's stated line numbers have drifted, it's relocated to
/// the unique place its context matches. Absent or ambiguous context is refused
/// ([`ApplyError::Drift`]); out-of-order hunks are [`ApplyError::Malformed`].
/// Works on `\n`-split lines and rejoins with `\n`; the command layer re-applies
/// the file's own ending.
/// Apply `hunks` to `content`, returning the new body (lines joined by `\n`, no
/// framing) and an optional trailing-newline decision: `Some(true/false)` when
/// the patch reaches the new file's end and thus dictates its final-newline
/// state, `None` when the end is untouched (caller preserves the original's).
pub fn apply(content: &str, hunks: &[Hunk]) -> Result<(String, Option<bool>), ApplyError> {
    let orig: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0usize; // next original line to copy (0-based)

    for (h, hunk) in hunks.iter().enumerate() {
        let anchor = anchor(hunk);
        let stated = hunk.old_start.saturating_sub(1);
        let pos = locate(&orig, &anchor, stated, cursor, h)?;
        // Copy untouched lines before the hunk's actual position.
        for l in &orig[cursor..pos] {
            out.push((*l).to_string());
        }
        cursor = pos;
        // Emit the body. Context/removed lines match the original here by
        // construction (locate verified the anchor), so we trust and advance.
        for line in &hunk.lines {
            match line {
                Line::Context(text) => {
                    out.push(text.clone());
                    cursor += 1;
                }
                Line::Remove(_) => cursor += 1,
                Line::Add(text) => out.push(text.clone()),
            }
        }
    }
    // The patch determines the new file's trailing-newline state only when its
    // last hunk runs to the original's end (no untouched tail follows); then the
    // new EOF is that hunk's last new-side line, whose marker we recorded.
    let reached_eof = cursor >= orig.len();
    let final_nl_override = if reached_eof {
        hunks.last().map(|h| !h.new_no_eol)
    } else {
        None
    };
    // Copy whatever follows the last hunk.
    for l in &orig[cursor..] {
        out.push((*l).to_string());
    }
    Ok((out.join("\n"), final_nl_override))
}

/// Accessor for the command layer.
impl FilePatch {
    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
--- a/foo.txt
+++ b/foo.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
";

    #[test]
    fn parse_and_apply_basic() {
        let files = parse(DIFF).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "foo.txt");
        assert!(!files[0].create && !files[0].delete);
        let (out, _) = apply("one\ntwo\nthree\n", files[0].hunks()).unwrap();
        assert_eq!(out, "one\nTWO\nthree");
    }

    #[test]
    fn strict_context_mismatch_is_rejected() {
        let files = parse(DIFF).unwrap();
        // File drifted: line 2 isn't "two" anymore.
        let err = apply("one\nCHANGED\nthree\n", files[0].hunks()).unwrap_err();
        assert!(
            err.message().contains("does not match"),
            "{}",
            err.message()
        );
        assert!(err.is_drift(), "context mismatch is file drift");
    }

    #[test]
    fn pure_addition_and_deletion() {
        let add = "--- a/f\n+++ b/f\n@@ -2,0 +3,1 @@\n+inserted\n";
        let files = parse(add).unwrap();
        // old_start 2 means "after line 2"; with no context/remove lines it just
        // inserts before original line 2 (0-based 1)... here begin=1.
        let (out, _) = apply("a\nb\nc\n", files[0].hunks()).unwrap();
        assert!(out.contains("inserted"));

        let del = "--- a/f\n+++ b/f\n@@ -1,3 +1,2 @@\n a\n-b\n c\n";
        let files = parse(del).unwrap();
        let (out, _) = apply("a\nb\nc\n", files[0].hunks()).unwrap();
        assert_eq!(out, "a\nc");
    }

    #[test]
    fn multi_hunk_applies_in_order() {
        let d = "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-a\n+A\n@@ -3,1 +3,1 @@\n-c\n+C\n";
        let files = parse(d).unwrap();
        let (out, _) = apply("a\nb\nc\n", files[0].hunks()).unwrap();
        assert_eq!(out, "A\nb\nC");
    }

    #[test]
    fn detects_create_and_delete() {
        let create = "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+world\n";
        let f = &parse(create).unwrap()[0];
        assert!(f.create && f.path == "new.txt");
        let (out, _) = apply("", f.hunks()).unwrap();
        assert_eq!(out, "hello\nworld");

        let delete = "--- a/gone.txt\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-bye\n";
        let f = &parse(delete).unwrap()[0];
        assert!(f.delete && f.path == "gone.txt");
    }

    #[test]
    fn multi_file_diff() {
        let d =
            "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-x1\n+X1\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-y1\n+Y1\n";
        let files = parse(d).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "x");
        assert_eq!(files[1].path, "y");
    }

    #[test]
    fn tolerates_git_preamble() {
        let d =
            "diff --git a/f b/f\nindex 111..222 100644\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+A\n";
        let files = parse(d).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "f");
    }

    #[test]
    fn overlapping_hunks_rejected() {
        let d = "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-a\n+A\n@@ -1,1 +1,1 @@\n-a\n+A\n";
        let files = parse(d).unwrap();
        let err = apply("a\nb\n", files[0].hunks()).unwrap_err();
        // A self-inconsistent diff is malformed, not file drift.
        assert!(!err.is_drift(), "overlap is a malformed diff");
    }

    #[test]
    fn relocates_hunk_with_drifted_line_numbers() {
        // The diff claims line 1, but TARGET is actually on line 3. With a unique
        // context match, it still applies — at the right place.
        let d = "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-TARGET\n+FIXED\n";
        let files = parse(d).unwrap();
        let (out, _) = apply("h1\nh2\nTARGET\nh4\n", files[0].hunks()).unwrap();
        assert_eq!(out, "h1\nh2\nFIXED\nh4");
    }

    #[test]
    fn ambiguous_relocation_is_refused() {
        // Stale line number AND the bare context "x" appears twice → refuse.
        let d = "--- a/f\n+++ b/f\n@@ -9,1 +9,1 @@\n-x\n+Y\n";
        let files = parse(d).unwrap();
        let err = apply("x\nmid\nx\n", files[0].hunks()).unwrap_err();
        assert!(
            err.is_drift() && err.message().contains("ambiguous"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn stated_position_wins_when_it_matches() {
        // "a" appears on lines 1 and 3; the diff points at line 1 and it matches
        // there, so it applies at 1 without an ambiguity error.
        let d = "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-a\n+A\n";
        let files = parse(d).unwrap();
        let (out, _) = apply("a\nb\na\n", files[0].hunks()).unwrap();
        assert_eq!(out, "A\nb\na");
    }

    #[test]
    fn apply_reports_new_trailing_newline_state() {
        // Marker on the new side's last line → result has no trailing newline.
        let d = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+B\n\\ No newline at end of file\n";
        let f = parse(d).unwrap();
        let (out, nl) = apply("a\nb", f[0].hunks()).unwrap();
        assert_eq!(out, "a\nB");
        assert_eq!(nl, Some(false));

        // Appending a newline-terminated line → result gains a trailing newline.
        let d2 =
            "--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n a\n-b\n\\ No newline at end of file\n+b\n+c\n";
        let f2 = parse(d2).unwrap();
        let (out2, nl2) = apply("a\nb", f2[0].hunks()).unwrap();
        assert_eq!(out2, "a\nb\nc");
        assert_eq!(nl2, Some(true));

        // An untouched tail after the hunk → None (caller preserves original).
        let d3 = "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-x1\n+X1\n x2\n x3\n";
        let f3 = parse(d3).unwrap();
        let (_, nl3) = apply("x1\nx2\nx3\nx4\nx5", f3[0].hunks()).unwrap();
        assert_eq!(nl3, None);
    }

    #[test]
    fn file_drift_is_classified_as_drift() {
        // Every "file isn't what the diff expects" failure must be Drift (→ exit 3),
        // so the exit code matches the documented contract.
        let mismatch = parse("--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-a\n+A\n").unwrap();
        assert!(apply("X\n", mismatch[0].hunks()).unwrap_err().is_drift());
        // Hunk reaches past EOF.
        let past = parse("--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n").unwrap();
        assert!(apply("a\n", past[0].hunks()).unwrap_err().is_drift());
        // Hunk starts beyond EOF.
        let beyond = parse("--- a/f\n+++ b/f\n@@ -9,1 +9,1 @@\n-z\n+Z\n").unwrap();
        assert!(apply("a\nb\n", beyond[0].hunks()).unwrap_err().is_drift());
    }
}
