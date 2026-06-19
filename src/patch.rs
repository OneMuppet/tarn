//! A small, strict unified-diff parser and applier — so an agent can pipe the
//! diff it (or `git diff`) produced straight into `tarn patch`, instead of
//! translating it into `apply`'s op format.
//!
//! Strict on purpose: a hunk is applied only if its context and removed lines
//! match the file exactly at the stated location. No fuzz, no offset search — if
//! the file has drifted, the patch is refused (a guard failure) rather than
//! applied to the wrong place. That matches tarn's `--expect` philosophy: a
//! failed edit is safer than a wrong one.

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
                    // `\ No newline at end of file` — a marker, not content.
                    Some(b'\\') => {}
                    Some(_) => return Err(format!("unexpected patch line: {l:?}")),
                }
                i += 1;
            }
            cur.hunks.push(Hunk {
                old_start,
                lines: body,
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

/// Apply `hunks` to `content`, returning the patched text. Strict: every context
/// and removed line must match the file exactly at the hunk's location — any
/// mismatch (including the file being shorter than the hunk reaches) is
/// [`ApplyError::Drift`]. A structurally broken diff (overlapping hunks) is
/// [`ApplyError::Malformed`]. `old_start == 0` (empty-file/creation hunks) is
/// treated as the top of the file. Works on `\n`-split lines and rejoins with
/// `\n`; the command layer re-applies the file's own ending.
pub fn apply(content: &str, hunks: &[Hunk]) -> Result<String, ApplyError> {
    let orig: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0usize; // next original line to copy (0-based)

    for (h, hunk) in hunks.iter().enumerate() {
        // Where this hunk begins in the original (1-based → 0-based). A creation
        // or pure-append hunk may carry old_start 0.
        let begin = hunk.old_start.saturating_sub(1);
        if begin < cursor {
            return Err(ApplyError::Malformed(format!(
                "hunk {} overlaps a previous hunk",
                h + 1
            )));
        }
        if begin > orig.len() {
            // The file is shorter than the diff expects → it has drifted.
            return Err(ApplyError::Drift(format!(
                "hunk {} starts at line {} but the file has {} lines",
                h + 1,
                hunk.old_start,
                orig.len()
            )));
        }
        // Copy untouched lines before the hunk.
        for l in &orig[cursor..begin] {
            out.push((*l).to_string());
        }
        cursor = begin;
        // Walk the hunk body, verifying context/removals against the original.
        for line in &hunk.lines {
            match line {
                Line::Context(text) | Line::Remove(text) => {
                    let actual = orig.get(cursor).ok_or_else(|| {
                        ApplyError::Drift(format!(
                            "hunk {} extends past the end of the file",
                            h + 1
                        ))
                    })?;
                    if actual != text {
                        return Err(ApplyError::Drift(format!(
                            "hunk {} does not match at line {}: expected {:?}, found {:?}",
                            h + 1,
                            cursor + 1,
                            text,
                            actual
                        )));
                    }
                    if let Line::Context(_) = line {
                        out.push(text.clone());
                    }
                    cursor += 1; // both context and removed consume an original line
                }
                Line::Add(text) => out.push(text.clone()),
            }
        }
    }
    // Copy whatever follows the last hunk.
    for l in &orig[cursor..] {
        out.push((*l).to_string());
    }
    Ok(out.join("\n"))
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
        let out = apply("one\ntwo\nthree\n", files[0].hunks()).unwrap();
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
        let out = apply("a\nb\nc\n", files[0].hunks()).unwrap();
        assert!(out.contains("inserted"));

        let del = "--- a/f\n+++ b/f\n@@ -1,3 +1,2 @@\n a\n-b\n c\n";
        let files = parse(del).unwrap();
        let out = apply("a\nb\nc\n", files[0].hunks()).unwrap();
        assert_eq!(out, "a\nc");
    }

    #[test]
    fn multi_hunk_applies_in_order() {
        let d = "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-a\n+A\n@@ -3,1 +3,1 @@\n-c\n+C\n";
        let files = parse(d).unwrap();
        let out = apply("a\nb\nc\n", files[0].hunks()).unwrap();
        assert_eq!(out, "A\nb\nC");
    }

    #[test]
    fn detects_create_and_delete() {
        let create = "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+hello\n+world\n";
        let f = &parse(create).unwrap()[0];
        assert!(f.create && f.path == "new.txt");
        let out = apply("", f.hunks()).unwrap();
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
