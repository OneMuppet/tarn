//! Presentation for the non-interactive path: the `show` view (an editor-style
//! snapshot printed to stdout, so an agent can "open" a document right in the
//! chat transcript) and the `--diff` renderer (so edits are reviewable inline).
//!
//! Everything here returns a `String`; the caller prints it. Color is optional
//! and off by default when stdout is not a terminal, so harness-captured output
//! stays clean (box-drawing only, no escape soup).

use crate::structure::Def;
use crate::textfile::Issue;

// --- brand palette (truecolor) ----------------------------------------------
const COPPER: &str = "\x1b[38;2;199;117;46m";
const MINT: &str = "\x1b[38;2;127;209;176m"; // additions
const RUST: &str = "\x1b[38;2;205;110;90m"; // removals
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("{code}{s}{RESET}")
    } else {
        s.to_string()
    }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

// --- windowing ----------------------------------------------------------------
/// Which slice of a document `show` should render.
pub enum Window {
    All,
    Range(usize, usize),
    Around(usize, usize), // (line, context)
    Head(usize),
    Tail(usize),
    Auto,
}

// Auto mode: show the whole file if short, else the first AUTO_HEAD lines.
const AUTO_FULL: usize = 40;
const AUTO_HEAD: usize = 30;

impl Window {
    /// Resolve to an inclusive 1-based (start, end). For an empty file returns
    /// (1, 0), an empty range.
    fn resolve(&self, total: usize) -> (usize, usize) {
        if total == 0 {
            return (1, 0);
        }
        let clamp = |n: usize| n.clamp(1, total);
        match *self {
            Window::All => (1, total),
            Window::Range(a, b) => {
                let a = clamp(a);
                (a, clamp(b).max(a))
            }
            Window::Around(n, k) => (n.saturating_sub(k).max(1), clamp(n + k)),
            Window::Head(k) => (1, clamp(k)),
            Window::Tail(k) => (total.saturating_sub(k.saturating_sub(1)).max(1), total),
            Window::Auto => {
                if total <= AUTO_FULL {
                    (1, total)
                } else {
                    (1, AUTO_HEAD)
                }
            }
        }
    }
}

/// Render the editor-style "open" view of `content`.
pub fn show(
    name: &str,
    content: &str,
    win: &Window,
    highlight: Option<(usize, usize)>,
    color: bool,
) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let (start, end) = win.resolve(total);

    let gutter = total.max(1).to_string().len().max(2);

    // Frame width: fit the shown content, but stay sane for a chat window.
    let mut inner = 44;
    if start <= end {
        for ln in start..=end {
            let w = gutter + 3 + width(lines[ln - 1]);
            inner = inner.max(w);
        }
    }
    inner = inner.min(110);

    let hit = |ln: usize| matches!(highlight, Some((a, b)) if ln >= a && ln <= b);

    let mut out = String::new();

    // top rule:  ┌─ name ─ N lines ───────
    let head = format!("┌─ {name} ─ {total} lines ");
    let top = format!("{head}{}", "─".repeat(inner.saturating_sub(width(&head))));
    out.push_str(&paint(color, COPPER, &top));
    out.push('\n');

    if total == 0 {
        out.push_str(&paint(color, DIM, "│  (empty file)"));
        out.push('\n');
    }

    for ln in start..=end {
        if ln == 0 || ln > total {
            break;
        }
        let marker = if hit(ln) { "▸" } else { " " };
        let num = format!("{:>w$}", ln, w = gutter);
        let body = lines[ln - 1];
        // Uniform layout: "<marker><number> │ <text>"
        let pre = format!("{marker}{num}");
        let pre = if hit(ln) {
            paint(color, &format!("{COPPER}{BOLD}"), &pre)
        } else {
            paint(color, DIM, &pre)
        };
        let sep = paint(color, DIM, "│");
        out.push_str(&format!("{pre} {sep} {body}"));
        out.push('\n');
    }

    // bottom rule with window context
    let above = start.saturating_sub(1);
    let below = total.saturating_sub(end);
    let mut foot = if total == 0 {
        "└─ no lines ".to_string()
    } else {
        format!("└─ lines {start}–{end} ")
    };
    if above > 0 {
        foot.push_str(&format!("· {above} above "));
    }
    if below > 0 {
        foot.push_str(&format!("· {below} below "));
    }
    let bottom = format!("{foot}{}", "─".repeat(inner.saturating_sub(width(&foot))));
    out.push_str(&paint(color, COPPER, &bottom));
    out.push('\n');
    out
}

// --- diff ---------------------------------------------------------------------
enum Op {
    Eq(String),
    Del(String),
    Ins(String),
}

/// A compact, line-numbered diff of `old` -> `new`, with `context` unchanged
/// lines around each change and `⋯` where unchanged runs are skipped.
pub fn diff(old: &str, new: &str, color: bool) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());

    // Longest common subsequence table (suffix form).
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    // Walk the table into an edit script.
    let mut ops: Vec<Op> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Eq(a[i].to_string()));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Del(a[i].to_string()));
            i += 1;
        } else {
            ops.push(Op::Ins(b[j].to_string()));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(a[i].to_string()));
        i += 1;
    }
    while j < m {
        ops.push(Op::Ins(b[j].to_string()));
        j += 1;
    }

    const CONTEXT: usize = 3;
    let changed: Vec<bool> = ops.iter().map(|o| !matches!(o, Op::Eq(_))).collect();
    if !changed.iter().any(|c| *c) {
        return paint(color, DIM, "(no changes)") + "\n";
    }
    // Keep an Eq op only if it's within CONTEXT of some change.
    let keep: Vec<bool> = (0..ops.len())
        .map(|idx| {
            let lo = idx.saturating_sub(CONTEXT);
            let hi = (idx + CONTEXT).min(ops.len() - 1);
            (lo..=hi).any(|k| changed[k])
        })
        .collect();

    let gw = n.max(m).max(1).to_string().len().max(2);
    let mut out = String::new();
    let (mut anum, mut bnum) = (0usize, 0usize); // 1-based as we advance
    let mut skipping = false;
    for (idx, op) in ops.iter().enumerate() {
        match op {
            Op::Eq(_) => {
                anum += 1;
                bnum += 1;
            }
            Op::Del(_) => anum += 1,
            Op::Ins(_) => bnum += 1,
        }
        if !keep[idx] {
            if !skipping {
                out.push_str(&paint(color, DIM, "  ⋯"));
                out.push('\n');
                skipping = true;
            }
            continue;
        }
        skipping = false;
        match op {
            Op::Eq(t) => {
                let line = format!("   {:>w$}   {}", bnum, t, w = gw);
                out.push_str(&paint(color, DIM, &line));
            }
            Op::Del(t) => {
                let line = format!("-  {:>w$}   {}", anum, t, w = gw);
                out.push_str(&paint(color, RUST, &line));
            }
            Op::Ins(t) => {
                let line = format!("+  {:>w$}   {}", bnum, t, w = gw);
                out.push_str(&paint(color, MINT, &line));
            }
        }
        out.push('\n');
    }
    out
}

// --- JSON (hand-rolled; no serde, keeping the zero-dependency rule) ----------
fn jesc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
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
    o
}

/// A JSON string literal (quoted + escaped).
pub fn jstr(s: &str) -> String {
    format!("\"{}\"", jesc(s))
}

/// Machine-readable `show`: path, totals, window, highlight, and windowed lines.
pub fn show_json(
    name: &str,
    content: &str,
    win: &Window,
    highlight: Option<(usize, usize)>,
) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let (start, end) = win.resolve(total);
    let hi = match highlight {
        Some((a, b)) => format!("[{a},{b}]"),
        None => "null".to_string(),
    };
    let mut items = Vec::new();
    if start <= end {
        for ln in start..=end {
            if ln >= 1 && ln <= total {
                items.push(format!("{{\"n\":{},\"text\":{}}}", ln, jstr(lines[ln - 1])));
            }
        }
    }
    format!(
        "{{\"path\":{},\"total\":{},\"window\":[{},{}],\"highlight\":{},\"lines\":[{}]}}\n",
        jstr(name),
        total,
        start,
        end,
        hi,
        items.join(",")
    )
}

/// Machine-readable result of a (possibly multi-file) `apply`.
pub fn apply_json(files: &[(String, usize, usize)], dry_run: bool) -> String {
    let items: Vec<String> = files
        .iter()
        .map(|(p, before, after)| {
            format!(
                "{{\"file\":{},\"before\":{},\"after\":{}}}",
                jstr(p),
                before,
                after
            )
        })
        .collect();
    format!(
        "{{\"op\":\"apply\",\"dry_run\":{},\"files\":[{}]}}\n",
        dry_run,
        items.join(",")
    )
}

/// Machine-readable result of an edit.
pub fn edit_json(path: &str, op: &str, before: usize, after: usize, dry_run: bool) -> String {
    format!(
        "{{\"ok\":true,\"path\":{},\"op\":\"{}\",\"before\":{},\"after\":{},\"dry_run\":{}}}\n",
        jstr(path),
        op,
        before,
        after,
        dry_run
    )
}

// --- outline ------------------------------------------------------------------
/// Human-readable structural map.
pub fn outline_view(path: &str, defs: &[Def], color: bool) -> String {
    let mut out = String::new();
    let head = format!("┌─ outline: {path} ─ {} defs ", defs.len());
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(40))));
    out.push('\n');
    if defs.is_empty() {
        out.push_str(&paint(color, DIM, "│  (no definitions found)"));
        out.push('\n');
    }
    let gw = defs
        .iter()
        .map(|d| d.line)
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(2);
    for d in defs {
        let num = paint(color, DIM, &format!("{:>w$}", d.line, w = gw));
        let sep = paint(color, DIM, "│");
        let indent = "  ".repeat(d.depth);
        let tag = paint(color, DIM, &format!("{} ", d.kind));
        let range = paint(color, DIM, &format!("({}–{})", d.line, d.end));
        out.push_str(&format!("{num} {sep} {indent}{tag}{} {range}\n", d.name));
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(48))));
    out.push('\n');
    out
}

fn def_json(d: &Def) -> String {
    format!(
        "{{\"line\":{},\"end\":{},\"kind\":{},\"name\":{},\"depth\":{}}}",
        d.line,
        d.end,
        jstr(&d.kind),
        jstr(&d.name),
        d.depth
    )
}

/// Machine-readable outline.
pub fn outline_json(path: &str, defs: &[Def]) -> String {
    let items: Vec<String> = defs.iter().map(def_json).collect();
    format!(
        "{{\"path\":{},\"defs\":[{}]}}\n",
        jstr(path),
        items.join(",")
    )
}

/// Human-readable structural map of a whole directory (one pass, grouped by file).
pub fn outline_dir_view(
    root: &str,
    files: &[(String, Vec<Def>)],
    total: usize,
    color: bool,
) -> String {
    let mut out = String::new();
    let head = format!("┌─ map: {root} ─ {total} defs in {} files ", files.len());
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(16))));
    out.push('\n');
    let gw = files
        .iter()
        .flat_map(|(_, d)| d.iter())
        .map(|d| d.line)
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(2);
    for (file, defs) in files {
        out.push_str(&paint(color, &format!("{COPPER}{BOLD}"), file));
        out.push('\n');
        for d in defs {
            let num = paint(color, DIM, &format!("{:>w$}", d.line, w = gw));
            let sep = paint(color, DIM, "│");
            let indent = "  ".repeat(d.depth);
            let kind = paint(color, DIM, &format!("{} ", d.kind));
            out.push_str(&format!("{num} {sep} {indent}{kind}{}\n", d.name));
        }
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(48))));
    out.push('\n');
    out
}

/// Machine-readable directory map.
pub fn outline_dir_json(root: &str, files: &[(String, Vec<Def>)]) -> String {
    let groups: Vec<String> = files
        .iter()
        .map(|(f, defs)| {
            let items: Vec<String> = defs.iter().map(def_json).collect();
            format!("{{\"file\":{},\"defs\":[{}]}}", jstr(f), items.join(","))
        })
        .collect();
    format!(
        "{{\"root\":{},\"files\":[{}]}}\n",
        jstr(root),
        groups.join(",")
    )
}

// --- find ---------------------------------------------------------------------
/// One search hit: its file, line, text, and optional enclosing scope.
pub struct FindMatch {
    pub file: String,
    pub line: usize,
    pub text: String,
    pub scope: Option<(String, usize, usize)>,
    pub before: Vec<(usize, String)>,
    pub after: Vec<(usize, String)>,
}

/// Human-readable search results. When the hits span more than one file, they're
/// grouped under per-file headers; a single file stays flat. With context lines,
/// each hit becomes a block separated by a faint rule.
pub fn find_view(pattern: &str, matches: &[FindMatch], files: usize, color: bool) -> String {
    let mut out = String::new();
    let scope_note = if files > 1 {
        format!(" in {files} files")
    } else {
        String::new()
    };
    let head = format!(
        "┌─ find {} ─ {} match{}{scope_note} ",
        jstr(pattern),
        matches.len(),
        if matches.len() == 1 { "" } else { "es" }
    );
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(20))));
    out.push('\n');

    let has_ctx = matches
        .iter()
        .any(|m| !m.before.is_empty() || !m.after.is_empty());
    let max_line = matches
        .iter()
        .map(|m| m.after.last().map(|(n, _)| *n).unwrap_or(m.line))
        .max()
        .unwrap_or(1);
    let gw = max_line.to_string().len().max(2);
    let sep = paint(color, DIM, "│");
    let multi = files > 1;
    let mut cur = "";
    let mut first_in_file = true;
    for m in matches {
        if multi && m.file != cur {
            out.push_str(&paint(color, &format!("{COPPER}{BOLD}"), &m.file));
            out.push('\n');
            cur = &m.file;
            first_in_file = true;
        }
        if has_ctx && !first_in_file {
            out.push_str(&paint(color, DIM, "┄"));
            out.push('\n');
        }
        for (n, t) in &m.before {
            let num = paint(color, DIM, &format!("{:>w$}", n, w = gw));
            out.push_str(&format!(
                "{num} {sep} {}\n",
                paint(color, DIM, t.trim_end())
            ));
        }
        let num = paint(
            color,
            &format!("{COPPER}{BOLD}"),
            &format!("{:>w$}", m.line, w = gw),
        );
        let mut line = format!("{num} {sep} {}", m.text.trim_end());
        if let Some((scope, a, b)) = &m.scope {
            line.push_str(&paint(color, DIM, &format!("   ↳ {scope} ({a}–{b})")));
        }
        out.push_str(&line);
        out.push('\n');
        for (n, t) in &m.after {
            let num = paint(color, DIM, &format!("{:>w$}", n, w = gw));
            out.push_str(&format!(
                "{num} {sep} {}\n",
                paint(color, DIM, t.trim_end())
            ));
        }
        first_in_file = false;
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(48))));
    out.push('\n');
    out
}

/// Machine-readable search results. `total` is the full match count; `matches`
/// may be fewer (capped by `--limit`), so a consumer can tell it was truncated.
pub fn find_json(pattern: &str, matches: &[FindMatch], total: usize) -> String {
    let ctx = |lines: &[(usize, String)]| -> String {
        let items: Vec<String> = lines
            .iter()
            .map(|(n, t)| format!("{{\"n\":{},\"text\":{}}}", n, jstr(t.trim_end())))
            .collect();
        format!("[{}]", items.join(","))
    };
    let items: Vec<String> = matches
        .iter()
        .map(|m| {
            let scope = match &m.scope {
                Some((name, a, b)) => format!(",\"in\":{},\"range\":[{},{}]", jstr(name), a, b),
                None => String::new(),
            };
            let context = if m.before.is_empty() && m.after.is_empty() {
                String::new()
            } else {
                format!(",\"before\":{},\"after\":{}", ctx(&m.before), ctx(&m.after))
            };
            format!(
                "{{\"file\":{},\"line\":{},\"text\":{}{}{}}}",
                jstr(&m.file),
                m.line,
                jstr(m.text.trim_end()),
                scope,
                context
            )
        })
        .collect();
    format!(
        "{{\"pattern\":{},\"total\":{},\"shown\":{},\"matches\":[{}]}}\n",
        jstr(pattern),
        total,
        matches.len(),
        items.join(",")
    )
}

// --- rename -------------------------------------------------------------------
/// Human-readable rename summary: per-file counts.
pub fn rename_view(
    from: &str,
    to: &str,
    files: &[(String, usize)],
    total: usize,
    word: bool,
    dry_run: bool,
    color: bool,
) -> String {
    let mut out = String::new();
    let tags = format!(
        "{}{}",
        if word { " word" } else { " substring" },
        if dry_run { " dry-run" } else { "" }
    );
    let head = format!(
        "┌─ rename {} → {} ─ {total} in {} file(s){tags} ",
        jstr(from),
        jstr(to),
        files.len()
    );
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(12))));
    out.push('\n');
    let cw = files
        .iter()
        .map(|(_, c)| c.to_string().len())
        .max()
        .unwrap_or(1)
        .max(2);
    for (f, c) in files {
        let count = paint(
            color,
            &format!("{COPPER}{BOLD}"),
            &format!("{:>w$}", c, w = cw),
        );
        let sep = paint(color, DIM, "│");
        out.push_str(&format!("{count} {sep} {f}\n"));
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(40))));
    out.push('\n');
    out
}

/// Machine-readable rename result.
pub fn rename_json(
    from: &str,
    to: &str,
    files: &[(String, usize)],
    total: usize,
    word: bool,
    dry_run: bool,
) -> String {
    let items: Vec<String> = files
        .iter()
        .map(|(f, c)| format!("{{\"file\":{},\"count\":{}}}", jstr(f), c))
        .collect();
    format!(
        "{{\"from\":{},\"to\":{},\"word\":{},\"dry_run\":{},\"total\":{},\"files\":[{}]}}\n",
        jstr(from),
        jstr(to),
        word,
        dry_run,
        total,
        items.join(",")
    )
}

// --- check --------------------------------------------------------------------
/// Human-readable hygiene report.
pub fn check_view(path: &str, issues: &[Issue], color: bool) -> String {
    if issues.is_empty() {
        return paint(color, MINT, &format!("✓ {path} — clean")) + "\n";
    }
    let mut out = String::new();
    let head = format!("┌─ check: {path} ─ {} issue(s) ", issues.len());
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(20))));
    out.push('\n');
    let gw = issues
        .iter()
        .filter_map(|i| i.line)
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(2);
    for it in issues {
        let loc = match it.line {
            Some(n) => paint(
                color,
                &format!("{COPPER}{BOLD}"),
                &format!("{:>w$}", n, w = gw),
            ),
            None => paint(color, DIM, &format!("{:>w$}", "·", w = gw)),
        };
        let sep = paint(color, DIM, "│");
        let kind = paint(color, DIM, &format!("{}:", it.kind));
        out.push_str(&format!("{loc} {sep} {kind} {}\n", it.msg));
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(48))));
    out.push('\n');
    out
}

/// Machine-readable hygiene report.
pub fn check_json(path: &str, issues: &[Issue]) -> String {
    let items: Vec<String> = issues
        .iter()
        .map(|i| {
            let line = match i.line {
                Some(n) => n.to_string(),
                None => "null".to_string(),
            };
            format!(
                "{{\"line\":{},\"kind\":\"{}\",\"msg\":{}}}",
                line,
                i.kind,
                jstr(&i.msg)
            )
        })
        .collect();
    format!(
        "{{\"path\":{},\"clean\":{},\"issues\":[{}]}}\n",
        jstr(path),
        issues.is_empty(),
        items.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_resolves_clamped() {
        assert_eq!(Window::Range(2, 5).resolve(3), (2, 3));
        assert_eq!(Window::Around(5, 2).resolve(10), (3, 7));
        assert_eq!(Window::Tail(2).resolve(10), (9, 10));
        assert_eq!(Window::Head(3).resolve(10), (1, 3));
        assert_eq!(Window::Auto.resolve(10), (1, 10));
        assert_eq!(Window::Auto.resolve(100), (1, AUTO_HEAD));
    }

    #[test]
    fn show_has_frame_and_numbers() {
        let v = show("f.txt", "a\nb\nc\n", &Window::All, None, false);
        assert!(v.contains("f.txt"));
        assert!(v.contains("3 lines"));
        assert!(v.contains("1 │ a"));
        assert!(v.contains("3 │ c"));
    }

    #[test]
    fn show_marks_highlight() {
        let v = show("f.txt", "a\nb\nc\n", &Window::All, Some((2, 2)), false);
        assert!(v.contains("▸ 2 │ b"));
    }

    #[test]
    fn show_reports_hidden_lines() {
        let v = show("f.txt", &"x\n".repeat(100), &Window::Head(5), None, false);
        assert!(v.contains("95 below"));
    }

    #[test]
    fn diff_shows_add_and_remove() {
        let d = diff("a\nb\nc\n", "a\nB\nc\n", false);
        assert!(d.lines().any(|l| l.starts_with('-') && l.ends_with('b')));
        assert!(d.lines().any(|l| l.starts_with('+') && l.ends_with('B')));
        // unchanged context line "a" is kept and not marked
        assert!(d
            .lines()
            .any(|l| l.ends_with('a') && !l.starts_with('-') && !l.starts_with('+')));
    }

    #[test]
    fn diff_no_change() {
        assert!(diff("a\n", "a\n", false).contains("no changes"));
    }

    #[test]
    fn show_json_has_window_and_lines() {
        let j = show_json("f.txt", "a\nb\nc\n", &Window::Range(2, 3), Some((2, 2)));
        assert!(j.contains("\"path\":\"f.txt\""));
        assert!(j.contains("\"total\":3"));
        assert!(j.contains("\"window\":[2,3]"));
        assert!(j.contains("\"highlight\":[2,2]"));
        assert!(j.contains("{\"n\":2,\"text\":\"b\"}"));
        assert!(!j.contains("\"text\":\"a\"")); // outside window
    }

    #[test]
    fn json_escapes_quotes_and_tabs() {
        assert_eq!(jstr("a\"b\tc"), "\"a\\\"b\\tc\"");
    }

    #[test]
    fn find_json_context_shape() {
        let with = FindMatch {
            file: "f".into(),
            line: 5,
            text: "hit".into(),
            scope: None,
            before: vec![(4, "b".into())],
            after: vec![(6, "a".into())],
        };
        let j = find_json("x", &[with], 1);
        assert!(j.contains("\"before\":[{\"n\":4,\"text\":\"b\"}]"));
        assert!(j.contains("\"after\":[{\"n\":6,\"text\":\"a\"}]"));
        // no context → before/after omitted entirely
        let without = FindMatch {
            file: "f".into(),
            line: 5,
            text: "hit".into(),
            scope: None,
            before: vec![],
            after: vec![],
        };
        assert!(!find_json("x", &[without], 1).contains("before"));
    }

    #[test]
    fn edit_json_shape() {
        let j = edit_json("f.txt", "replace", 5, 5, true);
        assert!(j.contains("\"ok\":true"));
        assert!(j.contains("\"op\":\"replace\""));
        assert!(j.contains("\"before\":5"));
        assert!(j.contains("\"dry_run\":true"));
    }
}
