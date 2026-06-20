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

/// Edit script (Eq/Del/Ins) aligning `a` -> `b` via a longest-common-subsequence
/// table. O(n·m) time and space, so `align` trims the common ends first and only
/// calls this on the part that differs.
fn lcs_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
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
    let mut ops = Vec::new();
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
    ops
}

/// Align `a` -> `b` into an Eq/Del/Ins script. Trims the common prefix and
/// suffix (emitting them as Eq) and runs the quadratic LCS only on the differing
/// middle — so a one-line change in a 40k-line file builds a tiny table, not a
/// 40k×40k one. The result is always a faithful alignment (its Eq+Del stream is
/// `a`, its Eq+Ins stream is `b`); the exact anchoring of repeated lines may
/// differ from a full-matrix LCS, which is fine for a human-readable preview.
fn align(a: &[&str], b: &[&str]) -> Vec<Op> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut ops: Vec<Op> = Vec::new();
    for line in &a[..pre] {
        ops.push(Op::Eq((*line).to_string()));
    }
    ops.extend(lcs_ops(&a[pre..a.len() - suf], &b[pre..b.len() - suf]));
    for line in &a[a.len() - suf..] {
        ops.push(Op::Eq((*line).to_string()));
    }
    ops
}

/// A compact, line-numbered diff of `old` -> `new`, with `context` unchanged
/// lines around each change and `⋯` where unchanged runs are skipped.
pub fn diff(old: &str, new: &str, color: bool) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = align(&a, &b);

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

    let gw = a.len().max(b.len()).max(1).to_string().len().max(2);
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

/// Split into lines like [`str::lines`] but **preserving** a trailing `\r`, so a
/// CRLF file's diff body matches the bytes on disk (and strict `git apply`
/// accepts the patch instead of rejecting on a context mismatch).
fn lines_keep_cr(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<&str> = s.split('\n').collect();
    if s.ends_with('\n') {
        v.pop(); // drop the empty segment after the final newline
    }
    v
}

/// A standard **unified diff** (`--- a` / `+++ b` / `@@ -l,c +l,c @@`) of `old`
/// -> `new`. Unlike [`diff`], this is machine format — no color, no line-number
/// gutter — applyable by `git apply`, `patch(1)`, or `tarn patch`. CRLF endings
/// are preserved, and a missing final newline emits the standard
/// `\ No newline at end of file` marker, so even strict `git apply` accepts the
/// output. `label_a` / `label_b` name the `---` / `+++` headers (callers pass
/// `a/<path>` / `b/<path>`). Returns the empty string when there is no change.
pub fn diff_unified(old: &str, new: &str, label_a: &str, label_b: &str) -> String {
    // Attach a sentinel to a side's final line when that side lacks a trailing
    // newline. This makes the LCS treat a no-newline last line as *distinct* from
    // an otherwise-identical newline-terminated line — so when you delete or
    // append at a no-newline EOF, the trailing line splits into `-`/`+` (exactly
    // as git does) and the `\ No newline` marker rides the correct line, instead
    // of being wrongly stamped on a shared context line.
    const NONL: &str = "\u{0}\u{0}tarn:no-newline\u{0}\u{0}";
    const NO_NL: &str = "\\ No newline at end of file\n";
    let mut a_lines: Vec<String> = lines_keep_cr(old).iter().map(|s| s.to_string()).collect();
    let mut b_lines: Vec<String> = lines_keep_cr(new).iter().map(|s| s.to_string()).collect();
    if !old.is_empty() && !old.ends_with('\n') {
        if let Some(l) = a_lines.last_mut() {
            l.push_str(NONL);
        }
    }
    if !new.is_empty() && !new.ends_with('\n') {
        if let Some(l) = b_lines.last_mut() {
            l.push_str(NONL);
        }
    }
    let a: Vec<&str> = a_lines.iter().map(|s| s.as_str()).collect();
    let b: Vec<&str> = b_lines.iter().map(|s| s.as_str()).collect();
    let ops = align(&a, &b);

    const CONTEXT: usize = 3;
    let changed: Vec<bool> = ops.iter().map(|o| !matches!(o, Op::Eq(_))).collect();
    if !changed.iter().any(|c| *c) {
        return String::new();
    }
    // Keep an Eq op only if it's within CONTEXT of some change; gaps wider than
    // 2·CONTEXT split into separate hunks, matching `diff -u`.
    let keep: Vec<bool> = (0..ops.len())
        .map(|idx| {
            let lo = idx.saturating_sub(CONTEXT);
            let hi = (idx + CONTEXT).min(ops.len() - 1);
            (lo..=hi).any(|k| changed[k])
        })
        .collect();

    // Emit one body line: strip the no-newline sentinel and, when it was present,
    // append the `\ No newline` marker right after that line.
    let push_line = |body: &mut String, prefix: char, text: &str| match text.strip_suffix(NONL) {
        Some(stripped) => {
            body.push(prefix);
            body.push_str(stripped);
            body.push('\n');
            body.push_str(NO_NL);
        }
        None => {
            body.push(prefix);
            body.push_str(text);
            body.push('\n');
        }
    };

    let mut out = format!("--- {label_a}\n+++ {label_b}\n");
    // `*_done` = lines consumed before the current op (so 1-based start = +1).
    let (mut a_done, mut b_done) = (0usize, 0usize);
    let mut idx = 0;
    while idx < ops.len() {
        if !keep[idx] {
            match &ops[idx] {
                Op::Eq(_) => {
                    a_done += 1;
                    b_done += 1;
                }
                Op::Del(_) => a_done += 1,
                Op::Ins(_) => b_done += 1,
            }
            idx += 1;
            continue;
        }
        let (a_before, b_before) = (a_done, b_done);
        let (mut a_count, mut b_count) = (0usize, 0usize);
        let mut body = String::new();
        while idx < ops.len() && keep[idx] {
            match &ops[idx] {
                Op::Eq(t) => {
                    push_line(&mut body, ' ', t);
                    a_done += 1;
                    b_done += 1;
                    a_count += 1;
                    b_count += 1;
                }
                Op::Del(t) => {
                    push_line(&mut body, '-', t);
                    a_done += 1;
                    a_count += 1;
                }
                Op::Ins(t) => {
                    push_line(&mut body, '+', t);
                    b_done += 1;
                    b_count += 1;
                }
            }
            idx += 1;
        }
        // When a side contributes 0 lines (pure insert/delete), its start is the
        // line *before* the hunk, per the unified-diff convention.
        let a_start = if a_count == 0 { a_before } else { a_before + 1 };
        let b_start = if b_count == 0 { b_before } else { b_before + 1 };
        out.push_str(&format!(
            "@@ -{a_start},{a_count} +{b_start},{b_count} @@\n"
        ));
        out.push_str(&body);
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
pub fn apply_json(op: &str, files: &[(String, usize, usize)], dry_run: bool) -> String {
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
        "{{\"op\":{},\"dry_run\":{},\"files\":[{}]}}\n",
        jstr(op),
        dry_run,
        items.join(",")
    )
}

// --- defs (go-to-definition) --------------------------------------------------
/// Human-readable definition sites for a symbol: `file:line  kind name (a–b)`.
pub fn defs_view(name: &str, items: &[(String, Def)], color: bool) -> String {
    let mut out = String::new();
    let head = format!("┌─ defs {} ─ {} found ", jstr(name), items.len());
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(20))));
    out.push('\n');
    for (file, d) in items {
        let loc = paint(
            color,
            &format!("{COPPER}{BOLD}"),
            &format!("{file}:{}", d.line),
        );
        let kind = paint(color, DIM, &d.kind);
        let range = paint(color, DIM, &format!("({}–{})", d.line, d.end));
        out.push_str(&format!("{loc}  {kind} {}  {range}\n", d.name));
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(40))));
    out.push('\n');
    out
}

/// Machine-readable definition sites.
pub fn defs_json(name: &str, items: &[(String, Def)]) -> String {
    let arr: Vec<String> = items
        .iter()
        .map(|(file, d)| {
            format!(
                "{{\"file\":{},\"line\":{},\"end\":{},\"kind\":{}}}",
                jstr(file),
                d.line,
                d.end,
                jstr(&d.kind)
            )
        })
        .collect();
    format!("{{\"name\":{},\"defs\":[{}]}}\n", jstr(name), arr.join(","))
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

// --- tree (repo orientation) -------------------------------------------------
/// One node of a directory tree. `lines` is the file's line count when the
/// caller asked for it (`--lines`); `None` for directories or when not counted.
pub struct TreeEntry {
    pub name: String,
    pub is_dir: bool,
    pub lines: Option<usize>,
    pub children: Vec<TreeEntry>,
}

impl TreeEntry {
    fn counts(&self) -> (usize, usize) {
        let mut files = 0;
        let mut dirs = 0;
        for c in &self.children {
            if c.is_dir {
                dirs += 1;
            } else {
                files += 1;
            }
            let (f, d) = c.counts();
            files += f;
            dirs += d;
        }
        (files, dirs)
    }
}

/// Human-readable directory tree with `├──`/`└──` connectors, directories
/// first. The root row is the path itself; the footer summarizes the counts.
pub fn tree_view(root: &TreeEntry, color: bool) -> String {
    let (files, dirs) = root.counts();
    let mut out = String::new();
    let plural = |n: usize, s: &str| {
        if n == 1 {
            s.to_string()
        } else {
            format!("{s}s")
        }
    };
    let head = format!(
        "┌─ tree {} ─ {} {} · {} {} ",
        jstr(&root.name),
        files,
        plural(files, "file"),
        dirs,
        plural(dirs, "dir")
    );
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(16))));
    out.push('\n');
    out.push_str(&paint(color, &format!("{COPPER}{BOLD}"), &root.name));
    out.push('\n');
    tree_rows(&root.children, "", &mut out, color);
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(48))));
    out.push('\n');
    out
}

fn tree_rows(nodes: &[TreeEntry], prefix: &str, out: &mut String, color: bool) {
    for (i, n) in nodes.iter().enumerate() {
        let last = i + 1 == nodes.len();
        let connector = if last { "└── " } else { "├── " };
        out.push_str(&paint(color, DIM, prefix));
        out.push_str(&paint(color, DIM, connector));
        if n.is_dir {
            out.push_str(&paint(
                color,
                &format!("{COPPER}{BOLD}"),
                &format!("{}/", n.name),
            ));
        } else {
            out.push_str(&n.name);
            if let Some(l) = n.lines {
                out.push_str(&paint(color, DIM, &format!("  ({l} ln)")));
            }
        }
        out.push('\n');
        if !n.children.is_empty() {
            let next = format!("{prefix}{}", if last { "    " } else { "│   " });
            tree_rows(&n.children, &next, out, color);
        }
    }
}

/// Machine-readable tree: a nested `{name,type,lines?,children?}` structure.
pub fn tree_json(root: &TreeEntry) -> String {
    fn node(e: &TreeEntry) -> String {
        let mut parts = vec![
            format!("\"name\":{}", jstr(&e.name)),
            format!("\"type\":\"{}\"", if e.is_dir { "dir" } else { "file" }),
        ];
        if let Some(l) = e.lines {
            parts.push(format!("\"lines\":{l}"));
        }
        if e.is_dir {
            let kids: Vec<String> = e.children.iter().map(node).collect();
            parts.push(format!("\"children\":[{}]", kids.join(",")));
        }
        format!("{{{}}}", parts.join(","))
    }
    format!("{}\n", node(root))
}

// --- refs (find-references / callers) ----------------------------------------
/// Human-readable usage sites for a symbol: each hit with its enclosing scope,
/// grouped by file. The definition site itself is excluded by the caller, so
/// this answers "who uses this", not "where is it".
pub fn refs_view(name: &str, matches: &[FindMatch], files: usize, color: bool) -> String {
    let mut out = String::new();
    let scopes = matches.iter().filter(|m| m.scope.is_some()).count();
    let span = if files > 1 {
        format!(" in {files} files")
    } else {
        String::new()
    };
    let head = format!(
        "┌─ refs {} ─ {} use{}{span} ─ {} in a scope ",
        jstr(name),
        matches.len(),
        if matches.len() == 1 { "" } else { "s" },
        scopes
    );
    out.push_str(&paint(color, COPPER, &format!("{head}{}", "─".repeat(16))));
    out.push('\n');

    let gw = matches
        .iter()
        .map(|m| m.line)
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(2);
    let sep = paint(color, DIM, "│");
    let multi = files > 1;
    let mut cur = "";
    for m in matches {
        if multi && m.file != cur {
            out.push_str(&paint(color, &format!("{COPPER}{BOLD}"), &m.file));
            out.push('\n');
            cur = &m.file;
        }
        let num = paint(
            color,
            &format!("{COPPER}{BOLD}"),
            &format!("{:>w$}", m.line, w = gw),
        );
        let mut line = format!("{num} {sep} {}", m.text.trim_end());
        match &m.scope {
            Some((scope, a, b)) => {
                line.push_str(&paint(color, DIM, &format!("   ↳ {scope} ({a}–{b})")))
            }
            None => line.push_str(&paint(color, DIM, "   ↳ (top level)")),
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&paint(color, COPPER, &format!("└{}", "─".repeat(48))));
    out.push('\n');
    out
}

/// Machine-readable usage sites. `total` is the full count; `matches` may be
/// fewer (capped by `--limit`).
pub fn refs_json(name: &str, matches: &[FindMatch], total: usize) -> String {
    let items: Vec<String> = matches
        .iter()
        .map(|m| {
            let scope = match &m.scope {
                Some((s, a, b)) => format!(",\"in\":{},\"range\":[{},{}]", jstr(s), a, b),
                None => String::new(),
            };
            format!(
                "{{\"file\":{},\"line\":{},\"text\":{}{}}}",
                jstr(&m.file),
                m.line,
                jstr(m.text.trim_end()),
                scope
            )
        })
        .collect();
    format!(
        "{{\"name\":{},\"total\":{},\"shown\":{},\"uses\":[{}]}}\n",
        jstr(name),
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
    fn diff_unified_basic_shape() {
        let d = diff_unified("a\nb\nc\n", "a\nB\nc\n", "a/f", "b/f");
        assert!(d.starts_with("--- a/f\n+++ b/f\n"));
        assert!(d.contains("@@ -1,3 +1,3 @@"));
        assert!(d.lines().any(|l| l == "-b"));
        assert!(d.lines().any(|l| l == "+B"));
        assert!(d.lines().any(|l| l == " a")); // context, space-prefixed
    }

    #[test]
    fn diff_unified_empty_when_identical() {
        assert_eq!(diff_unified("x\ny\n", "x\ny\n", "a/f", "b/f"), "");
    }

    #[test]
    fn diff_unified_pure_insertion_into_empty() {
        // Inserting into an empty file: old side has 0 lines -> `-0,0`.
        let d = diff_unified("", "new\n", "a/f", "b/f");
        assert!(d.contains("@@ -0,0 +1,1 @@"), "got:\n{d}");
        assert!(d.lines().any(|l| l == "+new"));
    }

    #[test]
    fn diff_unified_no_trailing_newline_emits_marker() {
        // Without the `\ No newline` marker, strict `git apply` rejects this.
        let d = diff_unified("a\nb", "a\nB", "a/f", "b/f");
        assert!(d.contains("\\ No newline at end of file"), "got:\n{d}");
        assert!(d.lines().any(|l| l == "-b"));
        assert!(d.lines().any(|l| l == "+B"));
    }

    #[test]
    fn diff_unified_preserves_crlf() {
        // CRLF must survive into the body so the context matches a CRLF file.
        let d = diff_unified("a\r\nb\r\n", "a\r\nB\r\n", "a/f", "b/f");
        assert!(d.contains(" a\r\n"), "got: {d:?}");
        assert!(d.contains("-b\r\n"), "got: {d:?}");
        assert!(d.contains("+B\r\n"), "got: {d:?}");
    }

    #[test]
    fn diff_unified_delete_trailing_no_newline_line() {
        // `a\nb\nc` (no final nl) -> `a\nb` (no final nl): the marker must NOT
        // land on context ` b` (old's b HAS a newline); git rejects that. The
        // trailing line splits: `-b -c\<nonl>` / `+b\<nonl>`.
        let d = diff_unified("a\nb\nc", "a\nb", "a/f", "b/f");
        let expected = "--- a/f\n+++ b/f\n@@ -1,3 +1,2 @@\n a\n-b\n-c\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n";
        assert_eq!(d, expected);
        // never a marker directly after a context line
        assert!(
            !d.contains(" b\n\\ No newline"),
            "marker on context line:\n{d}"
        );
    }

    #[test]
    fn diff_unified_append_to_no_newline_file() {
        // `a\nb` (no final nl) -> `a\nb\nc\n`: old's b had no newline (marker on
        // `-b`), new's b/c are newline-terminated (no marker).
        let d = diff_unified("a\nb", "a\nb\nc\n", "a/f", "b/f");
        let expected =
            "--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n a\n-b\n\\ No newline at end of file\n+b\n+c\n";
        assert_eq!(d, expected);
    }

    #[test]
    fn diff_unified_shared_last_line_both_no_newline() {
        // When the unchanged last line is genuinely last on BOTH sides and both
        // lack a newline, one marker on the shared context line is correct.
        let d = diff_unified("x\ntail", "y\ntail", "a/f", "b/f");
        assert!(
            d.contains(" tail\n\\ No newline at end of file"),
            "got:\n{d}"
        );
        assert_eq!(d.matches("\\ No newline").count(), 1, "got:\n{d}");
    }

    #[test]
    fn diff_unified_hunk_counts_are_self_consistent() {
        // For every @@ header the declared counts must equal the actual number of
        // context/`-`/`+` body lines in that hunk. This is what makes the output
        // applyable by git/patch, so check it exhaustively over small inputs.
        fn seqs(alpha: &[&str], maxlen: usize) -> Vec<Vec<String>> {
            let mut out = vec![vec![]];
            let mut frontier = vec![vec![]];
            for _ in 0..maxlen {
                let mut next = Vec::new();
                for s in &frontier {
                    for c in alpha {
                        let mut t = s.clone();
                        t.push((*c).to_string());
                        out.push(t.clone());
                        next.push(t);
                    }
                }
                frontier = next;
            }
            out
        }
        let alpha = ["x", "y", "z"];
        for a in seqs(&alpha, 4) {
            for b in seqs(&alpha, 4) {
                let old: String = a.iter().map(|l| format!("{l}\n")).collect();
                let new: String = b.iter().map(|l| format!("{l}\n")).collect();
                let d = diff_unified(&old, &new, "a/f", "b/f");
                if d.is_empty() {
                    assert_eq!(a, b);
                    continue;
                }
                let lines: Vec<&str> = d.lines().collect();
                let mut i = 2; // skip --- / +++
                while i < lines.len() {
                    let hdr = lines[i];
                    assert!(hdr.starts_with("@@ -"), "expected hunk header: {hdr:?}");
                    // parse "-as,ac +bs,bc"
                    let parts: Vec<&str> = hdr.trim_start_matches("@@ ").split(' ').collect();
                    let ac: usize = parts[0].split(',').nth(1).unwrap().parse().unwrap();
                    let bc: usize = parts[1].split(',').nth(1).unwrap().parse().unwrap();
                    i += 1;
                    let (mut got_a, mut got_b) = (0usize, 0usize);
                    while i < lines.len() && !lines[i].starts_with("@@") {
                        match lines[i].as_bytes().first() {
                            Some(b' ') => {
                                got_a += 1;
                                got_b += 1;
                            }
                            Some(b'-') => got_a += 1,
                            Some(b'+') => got_b += 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    assert_eq!(got_a, ac, "old count mismatch in:\n{d}");
                    assert_eq!(got_b, bc, "new count mismatch in:\n{d}");
                }
            }
        }
    }

    #[test]
    fn align_is_always_a_faithful_alignment() {
        // Exhaustively over a small alphabet/length: whatever alignment `align`
        // picks, its Eq+Del stream must reproduce `a` and its Eq+Ins stream must
        // reproduce `b`. This guards correctness without pinning a specific
        // (valid) alignment — the property the prefix/suffix trim must preserve.
        fn seqs<'a>(alpha: &[&'a str], maxlen: usize) -> Vec<Vec<&'a str>> {
            let mut out = vec![vec![]];
            let mut frontier = vec![vec![]];
            for _ in 0..maxlen {
                let mut next = Vec::new();
                for s in &frontier {
                    for &c in alpha {
                        let mut t = s.clone();
                        t.push(c);
                        next.push(t);
                    }
                }
                out.extend(next.clone());
                frontier = next;
            }
            out
        }
        let alpha = ["x", "y", "z"];
        let space = seqs(&alpha, 4);
        for a in &space {
            for b in &space {
                let ops = align(a, b);
                let from_a: Vec<&str> = ops
                    .iter()
                    .filter_map(|o| match o {
                        Op::Eq(t) | Op::Del(t) => Some(t.as_str()),
                        Op::Ins(_) => None,
                    })
                    .collect();
                let from_b: Vec<&str> = ops
                    .iter()
                    .filter_map(|o| match o {
                        Op::Eq(t) | Op::Ins(t) => Some(t.as_str()),
                        Op::Del(_) => None,
                    })
                    .collect();
                assert_eq!(from_a, *a, "Eq+Del must reproduce a");
                assert_eq!(from_b, *b, "Eq+Ins must reproduce b");
            }
        }
    }

    #[test]
    fn diff_prefix_suffix_trim_keeps_correct_line_numbers() {
        // A change in the middle of a longer file: numbering must survive the trim.
        let old = "1\n2\n3\nFOUR\n5\n6\n7\n";
        let new = "1\n2\n3\nIV\n5\n6\n7\n";
        let d = diff(old, new, false);
        assert!(d.lines().any(|l| l.starts_with('-') && l.contains("FOUR")));
        assert!(d.lines().any(|l| l.starts_with('+') && l.contains("IV")));
        assert!(d.contains("4 "), "changed line's gutter reads 4: {d}");
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
    fn refs_json_shape() {
        let m = FindMatch {
            file: "a.rs".into(),
            line: 9,
            text: "    foo();".into(),
            scope: Some(("bar".into(), 5, 12)),
            before: vec![],
            after: vec![],
        };
        let j = refs_json("foo", &[m], 3);
        assert!(j.contains("\"name\":\"foo\""));
        assert!(j.contains("\"total\":3"));
        assert!(j.contains("\"shown\":1"));
        assert!(j.contains("\"uses\":["));
        assert!(j.contains("\"in\":\"bar\",\"range\":[5,12]"));
        // a top-level use omits the scope fields
        let top = FindMatch {
            file: "a.rs".into(),
            line: 1,
            text: "foo();".into(),
            scope: None,
            before: vec![],
            after: vec![],
        };
        assert!(!refs_json("foo", &[top], 1).contains("\"in\":"));
    }

    #[test]
    fn refs_view_marks_top_level() {
        let top = FindMatch {
            file: "a.rs".into(),
            line: 1,
            text: "foo();".into(),
            scope: None,
            before: vec![],
            after: vec![],
        };
        let v = refs_view("foo", &[top], 1, false);
        assert!(v.contains("refs \"foo\""));
        assert!(v.contains("↳ (top level)"));
    }

    #[test]
    fn tree_json_and_view_shape() {
        let root = TreeEntry {
            name: "proj".into(),
            is_dir: true,
            lines: None,
            children: vec![
                TreeEntry {
                    name: "src".into(),
                    is_dir: true,
                    lines: None,
                    children: vec![TreeEntry {
                        name: "main.rs".into(),
                        is_dir: false,
                        lines: Some(42),
                        children: vec![],
                    }],
                },
                TreeEntry {
                    name: "README.md".into(),
                    is_dir: false,
                    lines: None,
                    children: vec![],
                },
            ],
        };
        let j = tree_json(&root);
        assert!(j.contains("\"name\":\"proj\",\"type\":\"dir\""));
        assert!(j.contains("\"name\":\"main.rs\",\"type\":\"file\",\"lines\":42"));
        // files omit children; a file with no count omits "lines"
        assert!(j.contains("\"name\":\"README.md\",\"type\":\"file\"}"));
        // counts: 2 files (main.rs, README.md), 1 dir (src)
        assert_eq!(root.counts(), (2, 1));
        let v = tree_view(&root, false);
        assert!(v.contains("2 files · 1 dir "));
        assert!(v.contains("└── README.md"));
        assert!(v.contains("main.rs  (42 ln)"));
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
