//! Heuristic code/document structure — no language parser, no dependencies.
//!
//! tarn can't run a real AST (that would mean big crates), so this reads
//! structure the cheap, honest way: by extension-chosen keyword patterns
//! (`def`, `class`, `fn`, `function`, `func`, `struct`, …) and indentation for
//! block extent, plus Markdown headings. It covers the common 90% and stays
//! understandable. It is *heuristic*, not semantic — documented as such.

/// A detected definition (function, class, heading, …).
#[derive(Clone, Debug)]
pub struct Def {
    pub line: usize, // 1-based start
    pub end: usize,  // 1-based end, inclusive
    pub kind: String,
    pub name: String,
    pub depth: usize, // nesting depth, for display indentation
}

struct Rules {
    markdown: bool,
    keywords: &'static [&'static str],
    js: bool,
    /// Block extent is delimited by `{`/`}` (not indentation). Enables the
    /// brace-aware `block_end`, so a multi-line signature's range covers its body.
    braces: bool,
    /// Detect keyword-less methods/functions of the form `returnType name(...)`
    /// (Java/C#/C/C++) inside class-like scopes.
    c_methods: bool,
    /// Also detect such functions at file/namespace scope (C/C++ free functions),
    /// not only inside a class body.
    c_free_fns: bool,
    /// `'…'` is a string literal (JS/PHP), not a char literal / Rust lifetime —
    /// affects how the code-delimiter scanner skips quotes.
    squote_str: bool,
}

impl Rules {
    const fn code(keywords: &'static [&'static str]) -> Rules {
        Rules {
            markdown: false,
            keywords,
            js: false,
            braces: false,
            c_methods: false,
            c_free_fns: false,
            squote_str: false,
        }
    }
}

/// Pick detection rules from a file's extension.
fn rules_for(path: &str) -> Rules {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mdown" | "mkd" => Rules {
            markdown: true,
            ..Rules::code(&[])
        },
        "py" | "pyi" => Rules::code(&["def", "class"]),
        "rs" => Rules {
            braces: true,
            ..Rules::code(&[
                "fn",
                "struct",
                "enum",
                "trait",
                "impl",
                "mod",
                "type",
                "macro_rules!",
            ])
        },
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Rules {
            js: true,
            braces: true,
            squote_str: true,
            ..Rules::code(&["function", "class", "interface", "enum", "type"])
        },
        "go" => Rules {
            braces: true,
            ..Rules::code(&["func", "type"])
        },
        // Ruby is `def`/`class` … `end` — indentation-delimited, not braces.
        "rb" => Rules::code(&["def", "class", "module"]),
        "php" => Rules {
            braces: true,
            squote_str: true,
            ..Rules::code(&["function", "class", "interface", "trait", "enum"])
        },
        // Swift methods are keyword-led (`func`), so no C-style detection needed.
        "swift" => Rules {
            braces: true,
            ..Rules::code(&["func", "class", "struct", "enum", "protocol", "extension"])
        },
        "kt" | "kts" => Rules {
            braces: true,
            ..Rules::code(&["fun", "class", "object", "interface", "enum"])
        },
        // Languages whose methods are `returnType name(...)` with no leading
        // keyword: detect class-level via keywords, methods via `detect_c_method`.
        "java" => Rules {
            braces: true,
            c_methods: true,
            ..Rules::code(&["class", "interface", "enum", "record"])
        },
        "cs" => Rules {
            braces: true,
            c_methods: true,
            ..Rules::code(&[
                "class",
                "interface",
                "struct",
                "enum",
                "namespace",
                "record",
            ])
        },
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => Rules {
            braces: true,
            c_methods: true,
            c_free_fns: true, // free functions live at file/namespace scope
            ..Rules::code(&["struct", "class", "enum", "namespace", "union"])
        },
        _ => Rules::code(&[
            "def",
            "class",
            "fn",
            "func",
            "function",
            "struct",
            "enum",
            "trait",
            "impl",
            "mod",
            "type",
            "interface",
            "namespace",
            "module",
        ]),
    }
}

const MODIFIERS: &[&str] = &[
    "pub",
    "export",
    "default",
    "async",
    "static",
    "public",
    "private",
    "protected",
    "final",
    "abstract",
    "open",
    "override",
    "suspend",
    "internal",
    "sealed",
    "data",
    "inline",
    "partial",
    "virtual",
    "readonly",
    "fileprivate",
    "companion",
    "unsafe",
    "extern",
    "lateinit",
];

fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Strip leading modifier keywords (e.g. `pub async `) once each.
fn strip_modifiers(mut t: &str) -> &str {
    loop {
        let mut changed = false;
        for m in MODIFIERS {
            if let Some(rest) = t.strip_prefix(m) {
                if rest.starts_with(' ') {
                    t = rest.trim_start();
                    changed = true;
                }
            }
        }
        if !changed {
            return t;
        }
    }
}

/// Take the name following a keyword: up to the first delimiter, kept intact
/// across spaces (so `impl Foo for Bar` survives).
fn name_after(rest: &str) -> String {
    rest.trim_start()
        .chars()
        .take_while(|c| !"({<=:;,".contains(*c))
        .collect::<String>()
        .trim()
        .to_string()
}

/// If `line` introduces a definition, return (kind, name).
fn detect(line: &str, rules: &Rules) -> Option<(String, String)> {
    let raw = line.trim_start();
    if rules.markdown {
        if raw.starts_with('#') {
            let level = raw.chars().take_while(|c| *c == '#').count();
            let name = raw.trim_start_matches('#').trim().to_string();
            if !name.is_empty() {
                return Some((format!("h{level}"), name));
            }
        }
        return None;
    }

    let t = strip_modifiers(raw);

    // JS/TS arrow- or function-valued bindings: `const fetchData = (…) =>`
    if rules.js {
        for kw in ["const", "let", "var"] {
            if let Some(rest) = t.strip_prefix(kw) {
                if rest.starts_with(' ') && (t.contains("=>") || t.contains("function")) {
                    let name = name_after(rest);
                    if !name.is_empty() {
                        return Some(("function".to_string(), name));
                    }
                }
            }
        }
    }

    for kw in rules.keywords {
        // Allocation-free `t == kw` / `t` starts with `kw ` or `kw!`. The old
        // form built `format!("{kw} ")` for every keyword on every line, which
        // dominated outline's cost on large inputs.
        let matches = t.strip_prefix(kw).is_some_and(|after| {
            after.is_empty() || after.starts_with(' ') || after.starts_with('!')
        });
        if matches {
            let mut rest = &t[kw.len()..];
            // `enum class Foo` (Kotlin, C++ scoped enums) / `enum struct Foo`:
            // the real name follows the secondary keyword.
            if *kw == "enum" {
                let r = rest.trim_start();
                if let Some(after) = r
                    .strip_prefix("class ")
                    .or_else(|| r.strip_prefix("struct "))
                {
                    rest = after;
                }
            }
            let name = name_after(rest);
            if !name.is_empty() {
                return Some((kw.trim_end_matches('!').to_string(), name));
            }
        }
    }
    None
}

/// Net brace nesting change on a line (`{` +1, `}` -1). Heuristic — counts braces
/// in strings/comments too, which is fine for the coarse scope tracking it feeds.
fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |d, c| match c {
        '{' => d + 1,
        '}' => d - 1,
        _ => d,
    })
}

/// A JS/TS class member with no leading keyword: `name(args) {`, `async name() {`,
/// `get x() {`, `*gen() {`, `name<T>(): R {`. Called only for lines directly inside
/// a class body, which keeps it from matching top-level calls like
/// `describe('x', () => {`. Arrow callbacks and control statements are excluded.
fn detect_js_method(line: &str) -> Option<(String, String)> {
    if !line.trim_end().ends_with('{') || line.contains("=>") {
        return None;
    }
    let mut t = strip_modifiers(line.trim_start());
    if let Some(r) = t.strip_prefix('*') {
        t = r.trim_start();
    }
    let kind = if let Some(r) = t.strip_prefix("get ") {
        t = r.trim_start();
        "get"
    } else if let Some(r) = t.strip_prefix("set ") {
        t = r.trim_start();
        "set"
    } else {
        "method"
    };
    let name: String = t
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if name.is_empty() {
        return None;
    }
    // After the name (skipping an optional generic list) the next char must be `(`.
    let mut after = t[name.len()..].trim_start();
    if let Some(rest) = after.strip_prefix('<') {
        let mut depth = 1usize;
        let mut cut = rest.len();
        for (i, c) in rest.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        cut = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        after = rest[cut..].trim_start();
    }
    if !after.starts_with('(') {
        return None;
    }
    const CTRL: &[&str] = &[
        "if", "for", "while", "switch", "catch", "do", "else", "return", "function", "with",
        "await", "typeof", "throw", "yield", "new", "super", "void", "delete",
    ];
    if CTRL.contains(&name.as_str()) {
        return None;
    }
    Some((kind.to_string(), name))
}

/// Lexer state carried across lines so multi-line strings and block comments
/// don't leak their braces into the code count.
#[derive(Clone, Copy, PartialEq)]
enum Span {
    Code,
    Block,      // inside /* ... */
    DQuote,     // inside "..."
    Backtick,   // inside `...` (JS template / Go raw string)
    Raw(usize), // inside a Rust raw string r#"..."# (usize = hash count)
}

/// If `b` (starting at a `'`) is a char/byte-char literal, its byte length;
/// `None` for a Rust lifetime tick (`'a`, `'static`) so we don't swallow code.
fn char_literal_len(b: &[u8]) -> Option<usize> {
    if b.len() < 2 {
        return None;
    }
    if b[1] == b'\\' {
        // Escape: `'\u{..}'` scans to the closing `'`; simple escapes are `'\X'`.
        if b.len() >= 3 && b[2] == b'u' {
            let mut j = 3;
            while j < b.len() && b[j] != b'\'' {
                j += 1;
            }
            return (j < b.len()).then_some(j + 1);
        }
        return (b.len() >= 4 && b[3] == b'\'').then_some(4);
    }
    // A real char literal is one (possibly multi-byte) char then `'`. Anything
    // else after the first char (`'a `, `'a,`) is a lifetime, not a literal.
    let clen = match b[1] {
        x if x < 0x80 => 1,
        x if x >= 0xF0 => 4,
        x if x >= 0xE0 => 3,
        _ => 2,
    };
    let close = 1 + clen;
    (close < b.len() && b[close] == b'\'').then_some(close + 1)
}

/// Net `{`/`}` and `(`/`)` change on `line`, counting CODE only — string,
/// char-literal, and comment spans are skipped — threading `span` across lines.
/// Returns `(brace_delta, paren_delta, saw_open_brace)`. `squote_str` treats
/// `'…'` as a string (JS/PHP) rather than a char literal / lifetime.
fn code_delimiters(line: &str, span: &mut Span, squote_str: bool) -> (i32, i32, bool) {
    let b = line.as_bytes();
    let n = b.len();
    let (mut brace, mut paren, mut saw_open) = (0i32, 0i32, false);
    let mut i = 0;
    while i < n {
        match *span {
            Span::Block => {
                if i + 1 < n && b[i] == b'*' && b[i + 1] == b'/' {
                    *span = Span::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Span::DQuote | Span::Backtick => {
                let close = if *span == Span::DQuote { b'"' } else { b'`' };
                if b[i] == b'\\' {
                    i += 2;
                } else if b[i] == close {
                    *span = Span::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            // Raw string ends at `"` followed by exactly `h` `#` — no escapes.
            Span::Raw(h) => {
                if b[i] == b'"' && (1..=h).all(|k| i + k < n && b[i + k] == b'#') {
                    *span = Span::Code;
                    i += 1 + h;
                } else {
                    i += 1;
                }
            }
            Span::Code => match b[i] {
                b'/' if i + 1 < n && b[i + 1] == b'/' => break, // line comment to EOL
                b'/' if i + 1 < n && b[i + 1] == b'*' => {
                    *span = Span::Block;
                    i += 2;
                }
                b'"' => {
                    // Rust raw string? `"` preceded by `#`* then `r`/`br` at an
                    // identifier boundary. Its braces/quotes are not code.
                    let mut h = 0usize;
                    let mut p = i;
                    while p > 0 && b[p - 1] == b'#' {
                        h += 1;
                        p -= 1;
                    }
                    let is_raw = !squote_str && p > 0 && b[p - 1] == b'r' && {
                        let mut q = p - 1; // index of `r`
                        if q > 0 && b[q - 1] == b'b' {
                            q -= 1; // byte raw string `br#"`
                        }
                        q == 0 || (!b[q - 1].is_ascii_alphanumeric() && b[q - 1] != b'_')
                    };
                    *span = if is_raw { Span::Raw(h) } else { Span::DQuote };
                    i += 1;
                }
                b'`' => {
                    *span = Span::Backtick;
                    i += 1;
                }
                // JS/PHP single-quoted string — scan to the matching `'` on this
                // line (these don't span lines without an escaped newline).
                b'\'' if squote_str => {
                    i += 1;
                    while i < n {
                        if b[i] == b'\\' {
                            i += 2;
                        } else if b[i] == b'\'' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }
                // Char/byte-char literal (or a Rust lifetime tick, which advances 1).
                b'\'' => match char_literal_len(&b[i..]) {
                    Some(adv) => i += adv,
                    None => i += 1,
                },
                b'{' => {
                    brace += 1;
                    saw_open = true;
                    i += 1;
                }
                b'}' => {
                    brace -= 1;
                    i += 1;
                }
                b'(' => {
                    paren += 1;
                    i += 1;
                }
                b')' => {
                    paren -= 1;
                    i += 1;
                }
                _ => i += 1,
            },
        }
    }
    (brace, paren, saw_open)
}

/// Does a brace block open on `idx` or the next non-blank line? (`{` on a K&R
/// header or inline body `() {}`, or a lone `{` on the next line for Allman.)
fn opens_block(lines: &[&str], idx: usize) -> bool {
    if lines[idx].contains('{') {
        return true;
    }
    for next in lines.iter().skip(idx + 1) {
        let t = next.trim();
        if t.is_empty() {
            continue;
        }
        return t.starts_with('{');
    }
    false
}

/// Extent of a definition block starting at line index `i`.
///
/// For brace languages (`braces`), follow `{`/`}` balance so a multi-line
/// signature's range covers its whole body (an indentation walk truncates at the
/// `)` line that returns to base indent). Braces inside strings, char literals,
/// comments, and Rust raw strings are ignored (see `code_delimiters`); the
/// residual heuristic gap is a C# verbatim string (`@"..."`, where `""` escapes).
/// For indentation languages (Python, Ruby, YAML) keep the indentation walk.
fn block_end(lines: &[&str], i: usize, braces: bool, squote_str: bool) -> usize {
    if !braces {
        return block_end_indent(lines, i);
    }
    let base = indent(lines[i]);
    let mut depth = 0i32; // brace balance (code only)
    let mut paren = 0i32; // paren balance (signature continuation)
    let mut found_open = false;
    let mut span = Span::Code;
    let mut j = i;
    while j < lines.len() {
        let line = lines[j];
        let (db, dp, saw_open) = code_delimiters(line, &mut span, squote_str);
        depth += db;
        paren += dp;
        if saw_open {
            found_open = true;
        }
        if found_open {
            if depth <= 0 {
                return j; // block opened and balanced again
            }
        } else if paren <= 0 {
            // No body brace yet and the signature parens (if any) are balanced.
            let tr = line.trim_end();
            if tr.ends_with(';') {
                return j; // bodyless declaration (`fn f();`, `type X = Y;`)
            }
            // A sibling at base indent (not the Allman `{`) means no block.
            if j + 1 < lines.len() {
                let nxt = lines[j + 1];
                if !nxt.trim().is_empty()
                    && indent(nxt) <= base
                    && !nxt.trim_start().starts_with('{')
                {
                    return j;
                }
            }
        }
        j += 1;
    }
    lines.len().saturating_sub(1)
}

/// Extent of an indentation-defined block starting at line index `i`.
fn block_end_indent(lines: &[&str], i: usize) -> usize {
    let base = indent(lines[i]);
    let mut end = i;
    let mut j = i + 1;
    while j < lines.len() {
        if lines[j].trim().is_empty() {
            j += 1;
            continue;
        }
        if indent(lines[j]) > base {
            end = j;
            j += 1;
        } else {
            break;
        }
    }
    // Brace languages put the closing delimiter at the SAME indent as the
    // opener, so the indentation walk stops just above it. Pull in that lone
    // closer so a def's range covers its whole block (its `}`/`)`/`]`).
    // Indentation-only languages (Python, YAML) have no such line, so they're
    // unaffected.
    if j < lines.len() && indent(lines[j]) == base && is_closer(lines[j].trim()) {
        end = j;
    }
    end
}

/// A C-family method/function header with no leading definition keyword:
/// `returnType name(args) {`, `public void foo() {`, `T Get<T>() {`,
/// `Foo::bar() const {`, constructors `Foo(...) {`, destructors `~Foo() {`.
/// The name is the identifier immediately before the first `(`. `opens` says a
/// brace block follows (this line or the next, for Allman style); with
/// `allow_multiline` a still-open signature (parens wrap to later lines) counts
/// too. Calls (`.`/`->`/`@`-prefixed), field initializers (`=` before the `(`),
/// expression-bodied members (`=>`), and control statements are rejected.
fn detect_c_method(line: &str, allow_multiline: bool, opens: bool) -> Option<(String, String)> {
    let open_sig = allow_multiline && code_delimiters(line, &mut Span::Code, false).1 > 0;
    if !opens && !open_sig {
        return None;
    }
    if line.contains("=>") {
        return None;
    }
    let t = strip_modifiers(line.trim_start());
    let paren = t.find('(')?;
    let before = &t[..paren];
    if before.contains('=') {
        return None; // field initializer / anonymous class, not a signature
    }
    let mut b = before.trim_end();
    // Skip a trailing generic group on the name: `Get<T>(` → name `Get`.
    if b.ends_with('>') {
        let mut depth = 0i32;
        let mut cut = None;
        for (idx, c) in b.char_indices().rev() {
            match c {
                '>' => depth += 1,
                '<' => {
                    depth -= 1;
                    if depth == 0 {
                        cut = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
        }
        b = b[..cut?].trim_end();
    }
    let name: String = {
        let rev: String = b
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        rev.chars().rev().collect()
    };
    if name.is_empty() || name.chars().next()?.is_ascii_digit() {
        return None;
    }
    // The char just before the name distinguishes a definition from a call
    // (`obj.foo`, `p->foo`) or a Java/C# annotation (`@Foo`).
    let pre = b[..b.len() - name.len()].trim_end();
    if pre.ends_with('.') || pre.ends_with('@') {
        return None;
    }
    let is_dtor = pre.ends_with('~'); // C++ destructor `~Foo()`
    const CTRL: &[&str] = &[
        "if",
        "for",
        "while",
        "switch",
        "catch",
        "do",
        "else",
        "return",
        "sizeof",
        "synchronized",
        "using",
        "namespace",
        "new",
        "delete",
        "operator",
        "throw",
        "co_await",
        "co_yield",
        "static_assert",
        "decltype",
        "typeof",
        "constexpr",
    ];
    if CTRL.contains(&name.as_str()) {
        return None;
    }
    let display = if is_dtor { format!("~{name}") } else { name };
    Some(("method".to_string(), display))
}

/// A line that is nothing but closing delimiters (optionally with a trailing
/// `,`/`;`), e.g. `}`, `})`, `};`, `},`, `)`, `]` — the tail of a brace block.
fn is_closer(t: &str) -> bool {
    !t.is_empty()
        && t.chars().all(|c| matches!(c, '}' | ')' | ']' | ',' | ';'))
        && t.contains(['}', ')', ']'])
}

/// The structural outline of a document.
pub fn outline(path: &str, content: &str) -> Vec<Def> {
    let rules = rules_for(path);
    let lines: Vec<&str> = content.lines().collect();

    // First pass: candidate definition lines. `rank` drives nesting: a heading's
    // level, or (for code) the line's indentation.
    let mut raw: Vec<(usize, String, String, usize, usize)> = Vec::new(); // (idx,kind,name,level,rank)
    let mut in_fence = false; // markdown: don't read `#` inside ``` / ~~~ code blocks
    let mut in_import = false; // js: skip multi-line `import { ... }` member lines
    let mut brace_depth: i32 = 0; // js/c: net brace nesting, for class-member detection
    let mut class_depths: Vec<i32> = Vec::new(); // js/c: body depths of open class scopes
    let mut pending_class = false; // c: class detected, Allman `{` on a later line
    let mut span = Span::Code; // c: lexer state for code-only brace counting
    for (idx, line) in lines.iter().enumerate() {
        if rules.markdown {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            if let Some((kind, name)) = detect(line, &rules) {
                let level = kind
                    .strip_prefix('h')
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0);
                raw.push((idx, kind, name, level, level));
            }
            continue;
        }

        if rules.js {
            let tr = line.trim_start();
            // Skip `import` statements so `import { type X }` members aren't read
            // as `type` definitions.
            if in_import {
                if line.contains(" from ") || tr.starts_with('}') {
                    in_import = false;
                }
                brace_depth += brace_delta(line);
                continue;
            }
            if tr.starts_with("import ")
                || tr.starts_with("import{")
                || tr == "import"
                || tr.starts_with("export {")
                || tr.starts_with("export{")
                || tr.starts_with("export type {")
                || tr.starts_with("export type{")
                || tr.starts_with("export *")
            {
                if brace_delta(line) > 0 {
                    in_import = true;
                }
                brace_depth += brace_delta(line);
                continue;
            }
            let start_depth = brace_depth;
            if let Some((kind, name)) = detect(line, &rules) {
                if (kind == "class" || kind == "interface") && line.contains('{') {
                    class_depths.push(start_depth + 1);
                }
                raw.push((idx, kind, name, 0, indent(line)));
            } else if class_depths.last() == Some(&start_depth) {
                // Directly inside a class body → maybe a keyword-less method.
                if let Some((kind, name)) = detect_js_method(line) {
                    raw.push((idx, kind, name, 0, indent(line)));
                }
            }
            brace_depth += brace_delta(line);
            while matches!(class_depths.last(), Some(&d) if brace_depth < d) {
                class_depths.pop();
            }
            continue;
        }

        // Languages whose methods are `returnType name(...)` (Java/C#/C/C++):
        // keyword detection for types, `detect_c_method` for members.
        if rules.c_methods {
            let (db, _dp, saw_open) = code_delimiters(line, &mut span, rules.squote_str);
            let start_depth = brace_depth;
            if let Some((kind, name)) = detect(line, &rules) {
                raw.push((idx, kind, name, 0, indent(line)));
                if saw_open {
                    class_depths.push(start_depth + 1);
                } else if opens_block(&lines, idx) {
                    pending_class = true; // Allman: brace opens on a later line
                }
            } else if pending_class && line.trim_start().starts_with('{') {
                class_depths.push(start_depth + 1);
                pending_class = false;
            } else {
                // Only detect a member/function where a definition can actually
                // appear: directly inside the innermost type/namespace/file scope,
                // never inside a function body or control block (which is where a
                // multi-line call or lambda would otherwise look method-shaped).
                let innermost = *class_depths.last().unwrap_or(&0);
                let attempt = if rules.c_free_fns {
                    start_depth == innermost
                } else {
                    class_depths.last() == Some(&start_depth)
                };
                if attempt {
                    if let Some((kind, name)) =
                        detect_c_method(line, true, opens_block(&lines, idx))
                    {
                        raw.push((idx, kind, name, 0, indent(line)));
                    }
                }
            }
            brace_depth += db;
            while matches!(class_depths.last(), Some(&d) if brace_depth < d) {
                class_depths.pop();
            }
            continue;
        }

        // All other languages: keyword detection, unchanged.
        if let Some((kind, name)) = detect(line, &rules) {
            raw.push((idx, kind, name, 0, indent(line)));
        }
    }

    // Second pass: compute end ranges.
    let mut defs: Vec<Def> = Vec::new();
    for (k, (idx, kind, name, level, _rank)) in raw.iter().enumerate() {
        let end = if *level > 0 {
            // Markdown: until the next heading of the same or higher level.
            let mut e = lines.len().saturating_sub(1);
            for (nidx, _, _, nlevel, _) in raw.iter().skip(k + 1) {
                if *nlevel > 0 && *nlevel <= *level {
                    e = nidx.saturating_sub(1);
                    break;
                }
            }
            e
        } else {
            block_end(&lines, *idx, rules.braces, rules.squote_str)
        };
        defs.push(Def {
            line: idx + 1,
            end: end + 1,
            kind: kind.clone(),
            name: name.clone(),
            depth: 0,
        });
    }

    // Third pass: nesting depth via an indentation/level stack. This is robust to
    // a truncated end range (e.g. a def whose body holds an unindented multi-line
    // string), which range-containment depth would get wrong.
    let mut stack: Vec<usize> = Vec::new();
    for (i, (_, _, _, _, rank)) in raw.iter().enumerate() {
        while matches!(stack.last(), Some(&top) if top >= *rank) {
            stack.pop();
        }
        defs[i].depth = stack.len();
        stack.push(*rank);
    }
    defs
}

/// A dotted, qualified name for the innermost scope enclosing `target`, computed
/// against an already-parsed outline — so a caller with many lookups in one file
/// (e.g. `find --enclosing`) parses the file ONCE, not per match.
pub fn qualified_in(defs: &[Def], target: usize) -> Option<(String, usize, usize)> {
    let mut chain: Vec<&Def> = defs
        .iter()
        .filter(|d| d.line <= target && d.end >= target)
        .collect();
    chain.sort_by_key(|d| d.line);
    let inner = chain.last()?;
    let (line, end) = (inner.line, inner.end);
    let name = chain
        .iter()
        .map(|d| d.name.as_str())
        .collect::<Vec<_>>()
        .join(".");
    Some((name, line, end))
}

/// The definition block at `line`: prefer one that *starts* there, else the
/// innermost definition that contains it.
pub fn block_at(path: &str, content: &str, line: usize) -> Option<Def> {
    let defs = outline(path, content);
    if let Some(d) = defs.iter().find(|d| d.line == line) {
        return Some(d.clone());
    }
    defs.into_iter()
        .filter(|d| d.line <= line && d.end >= line)
        .max_by_key(|d| d.line)
}

/// The first definition named `name` (exact match).
pub fn def_named(path: &str, content: &str, name: &str) -> Option<Def> {
    outline(path, content).into_iter().find(|d| d.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PY: &str = "\
import os

class Handler:
    def do_GET(self):
        return 1

    def do_POST(self):
        return 2

def main():
    pass
";

    #[test]
    fn outline_finds_class_and_methods() {
        let defs = outline("h.py", PY);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["Handler", "do_GET", "do_POST", "main"]);
    }

    #[test]
    fn methods_nest_under_class() {
        let defs = outline("h.py", PY);
        let class = defs.iter().find(|d| d.name == "Handler").unwrap();
        let m = defs.iter().find(|d| d.name == "do_GET").unwrap();
        assert_eq!(class.depth, 0);
        assert_eq!(m.depth, 1);
        assert!(class.line <= m.line && class.end >= m.end);
    }

    #[test]
    fn enclosing_is_innermost() {
        // line 5 is `return 1`, inside do_GET inside Handler
        let q = qualified_in(&outline("h.py", PY), 5).unwrap();
        assert_eq!(q.0, "Handler.do_GET");
    }

    #[test]
    fn markdown_headings() {
        let md = "# Title\n\nintro\n\n## Section\n\nbody\n";
        let defs = outline("doc.md", md);
        assert_eq!(defs[0].name, "Title");
        assert_eq!(defs[1].name, "Section");
        assert_eq!(defs[1].depth, 1);
    }

    #[test]
    fn markdown_ignores_fenced_code() {
        // a `#` line inside a ``` fence is code, not a heading
        let md = "# Real\n\n```\n# not a heading\n## also not\n```\n\n## After\n";
        let defs = outline("doc.md", md);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["Real", "After"]);
    }

    #[test]
    fn block_and_named_lookups() {
        // `def_named` finds the method; `block_at` on a body line returns it.
        let m = def_named("h.py", PY, "do_POST").unwrap();
        assert_eq!((m.line, m.end), (7, 8));
        let b = block_at("h.py", PY, 8).unwrap(); // line 8 is `return 2`
        assert_eq!(b.name, "do_POST");
        // a line that starts a def returns that def exactly
        assert_eq!(block_at("h.py", PY, 3).unwrap().name, "Handler");
    }

    #[test]
    fn broader_language_coverage() {
        let names = |path: &str, src: &str| -> Vec<String> {
            outline(path, src).iter().map(|d| d.name.clone()).collect()
        };
        let has = |v: &[String], n: &str| v.iter().any(|x| x == n);
        // Kotlin: modifiers stripped, fun/class detected
        let kt = names(
            "a.kt",
            "data class User(val n: Int)\n\nsuspend fun fetch() {\n    todo()\n}\n",
        );
        assert!(has(&kt, "User") && has(&kt, "fetch"), "{kt:?}");
        // Swift
        let sw = names(
            "a.swift",
            "struct Point {\n    func dist() -> Double { 0 }\n}\n",
        );
        assert!(has(&sw, "Point") && has(&sw, "dist"), "{sw:?}");
        // Ruby
        let rb = names("a.rb", "class Cli\n  def run\n  end\nend\n");
        assert!(has(&rb, "Cli") && has(&rb, "run"), "{rb:?}");
        // Java: class-level (methods are returnType name(), not keyword-led)
        let jv = names("a.java", "public class Foo {\n  void bar() {}\n}\n");
        assert!(has(&jv, "Foo"), "{jv:?}");
        // `enum class` (Kotlin always; C++ scoped enums) → name is the enum, not "class X"
        let ke = names("a.kt", "enum class Color { RED, GREEN }\n");
        assert!(has(&ke, "Color") && !has(&ke, "class Color"), "{ke:?}");
        let ce = names(
            "a.cpp",
            "enum class Mode { A, B };\nenum struct Flag { X };\n",
        );
        assert!(has(&ce, "Mode") && has(&ce, "Flag"), "{ce:?}");
    }

    #[test]
    fn rust_items() {
        let rs = "pub fn run() {\n    let x = 1;\n}\n\nstruct Cfg {\n    n: u8,\n}\n";
        let defs = outline("m.rs", rs);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"run"));
        assert!(names.contains(&"Cfg"));
    }

    #[test]
    fn brace_block_includes_closing_delimiter() {
        // The closing `}` sits at the opener's indent; the range must include it
        // so structural edits (delete --def) and peek cover the whole block.
        let rs = "fn run() {\n    let x = 1;\n}\n\nstruct Cfg {\n    n: u8,\n}\n";
        let defs = outline("m.rs", rs);
        let run = defs.iter().find(|d| d.name == "run").unwrap();
        assert_eq!((run.line, run.end), (1, 3)); // includes the `}` on line 3
        let cfg = defs.iter().find(|d| d.name == "Cfg").unwrap();
        assert_eq!((cfg.line, cfg.end), (5, 7));
        // is_closer: only lines that are purely closers (+ trailing ,/;) count.
        assert!(is_closer("}"));
        assert!(is_closer("});"));
        assert!(is_closer("},"));
        assert!(!is_closer("};x"));
        assert!(!is_closer(";")); // no bracket
        assert!(!is_closer("let y = 1;"));
    }

    #[test]
    fn python_block_unaffected_by_closer_logic() {
        // Indentation-only languages have no closer line; ranges stay as before.
        let py = "def a():\n    return 1\n\ndef b():\n    return 2\n";
        let defs = outline("x.py", py);
        let a = defs.iter().find(|d| d.name == "a").unwrap();
        assert_eq!((a.line, a.end), (1, 2));
    }

    const TS: &str = "\
import {
  type FileState,
  helper,
} from './x';

export class Engine {
  private count = 0;
  constructor(opts: Opts) {
    this.count = 0;
  }
  async run(input: string): Promise<void> {
    if (input) {
      doThing();
    }
  }
  get size(): number {
    return this.count;
  }
  *items() {
    yield 1;
  }
}

describe('suite', () => {
  it('works', () => {});
});
";

    #[test]
    fn ts_class_methods_detected() {
        let defs = outline("a.ts", TS);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"Engine"), "{names:?}");
        assert!(names.contains(&"constructor"), "{names:?}");
        assert!(names.contains(&"run"), "{names:?}");
        assert!(names.contains(&"size"), "{names:?}"); // get accessor
        assert!(names.contains(&"items"), "{names:?}"); // generator
                                                        // methods nest under the class
        let c = defs.iter().find(|d| d.name == "Engine").unwrap();
        let m = defs.iter().find(|d| d.name == "run").unwrap();
        assert!(c.depth < m.depth, "method should nest under class");
    }

    #[test]
    fn ts_import_type_members_not_detected() {
        // `import { type FileState, helper }` members are not definitions.
        let names: Vec<String> = outline("a.ts", TS).iter().map(|d| d.name.clone()).collect();
        assert!(!names.iter().any(|n| n.contains("FileState")), "{names:?}");
        assert!(!names.iter().any(|n| n == "helper"), "{names:?}");
    }

    #[test]
    fn ts_single_line_reexport_does_not_swallow_next_def() {
        // A single-line `export { ... }` (no `from`, no `;`) is a complete
        // statement — it must NOT start a multi-line skip and eat the next def.
        let src = "export type { A, B }\n\nfunction f() {\n  return 1;\n}\n";
        let names: Vec<String> = outline("a.ts", src)
            .iter()
            .map(|d| d.name.clone())
            .collect();
        assert!(names.contains(&"f".to_string()), "{names:?}");
        assert!(
            !names.iter().any(|n| n.contains("A") || n.contains("B")),
            "{names:?}"
        );
        let src2 = "export { x }\nclass K {}\n";
        let n2: Vec<String> = outline("a.ts", src2)
            .iter()
            .map(|d| d.name.clone())
            .collect();
        assert!(n2.contains(&"K".to_string()), "{n2:?}");
    }

    #[test]
    fn ts_reexport_members_not_detected() {
        let src = "export {\n  type Foo,\n  bar,\n} from './x';\n\nexport class K {}\n";
        let names: Vec<String> = outline("a.ts", src)
            .iter()
            .map(|d| d.name.clone())
            .collect();
        assert!(names.contains(&"K".to_string()), "{names:?}");
        assert!(
            !names.iter().any(|n| n.contains("Foo") || n == "bar"),
            "{names:?}"
        );
    }

    #[test]
    fn ts_no_false_methods_from_control_or_toplevel_calls() {
        let names: Vec<String> = outline("a.ts", TS).iter().map(|d| d.name.clone()).collect();
        // control statement inside a method body is not a method
        assert!(!names.iter().any(|n| n == "if"), "{names:?}");
        // top-level test-framework calls are not class members
        assert!(
            !names.iter().any(|n| n == "describe" || n == "it"),
            "{names:?}"
        );
    }

    fn names(path: &str, src: &str) -> Vec<String> {
        outline(path, src).iter().map(|d| d.name.clone()).collect()
    }
    fn has(v: &[String], n: &str) -> bool {
        v.iter().any(|x| x == n)
    }

    const JAVA: &str = "\
package x;

public class Calculator {
    private int total;

    public Calculator(int start) {
        this.total = start;
    }

    public int add(int a, int b) {
        return a + b;
    }

    public <T> List<T> wrap(
        T item,
        int count
    ) {
        return repeat(item, count);
    }
}
";

    #[test]
    fn java_class_methods_detected() {
        let n = names("C.java", JAVA);
        assert!(has(&n, "Calculator"), "{n:?}"); // class + constructor share name
        assert!(has(&n, "add"), "{n:?}");
        assert!(has(&n, "wrap"), "{n:?}"); // multi-line generic signature
                                           // methods nest under the class
        let defs = outline("C.java", JAVA);
        let class = defs.iter().find(|d| d.kind == "class").unwrap();
        let add = defs.iter().find(|d| d.name == "add").unwrap();
        assert!(class.depth < add.depth, "{defs:?}");
        // multi-line signature range covers the whole body, not just the params
        let wrap = defs.iter().find(|d| d.name == "wrap").unwrap();
        assert_eq!(
            (wrap.line, wrap.end),
            (14, 19),
            "{:?}",
            (wrap.line, wrap.end)
        );
    }

    #[test]
    fn java_no_false_methods_from_control_flow() {
        // `if`/`for` etc. inside a method body must not register as methods.
        let src = "class A {\n  void run() {\n    if (x) {\n      go();\n    }\n    for (int i=0;i<n;i++) {\n    }\n  }\n}\n";
        let n = names("A.java", src);
        assert!(!has(&n, "if") && !has(&n, "for"), "{n:?}");
        assert!(has(&n, "run"), "{n:?}");
    }

    const CS: &str = "\
namespace App
{
    public class Service
    {
        public async Task<int> FetchAsync(string url)
        {
            return await Get(url);
        }

        public int Count => _items.Count;

        private void Helper()
        {
            Log(\"hi\");
        }
    }
}
";

    #[test]
    fn cs_allman_methods_detected() {
        let n = names("S.cs", CS);
        assert!(has(&n, "Service"), "{n:?}");
        assert!(has(&n, "FetchAsync"), "{n:?}"); // Allman brace on next line
        assert!(has(&n, "Helper"), "{n:?}");
        // expression-bodied property (`=> ...`) is not reported as a method
        assert!(!has(&n, "Count"), "{n:?}");
        // Allman method range covers its body
        let defs = outline("S.cs", CS);
        let f = defs.iter().find(|d| d.name == "FetchAsync").unwrap();
        assert_eq!((f.line, f.end), (5, 8), "{:?}", (f.line, f.end));
    }

    const CPP: &str = "\
#include <vector>

int main(int argc, char** argv) {
    return 0;
}

class Widget {
public:
    Widget(int w) : width(w) {}
    ~Widget() {}
    int area() const {
        return width * height;
    }
private:
    int width;
};

void Widget::resize(
    int w,
    int h)
{
    width = w;
}
";

    #[test]
    fn cpp_free_fn_methods_ctor_dtor() {
        let n = names("w.cpp", CPP);
        assert!(has(&n, "main"), "{n:?}"); // free function at file scope
        assert!(has(&n, "Widget"), "{n:?}"); // class + inline constructor
        assert!(has(&n, "~Widget"), "{n:?}"); // destructor
        assert!(has(&n, "area"), "{n:?}"); // const member
        assert!(has(&n, "resize"), "{n:?}"); // multi-line Allman out-of-line def
                                             // not fooled by the field after the methods
        assert!(!has(&n, "width"), "{n:?}");
        let defs = outline("w.cpp", CPP);
        let resize = defs.iter().find(|d| d.name == "resize").unwrap();
        assert_eq!((resize.line, resize.end), (18, 23));
    }

    #[test]
    fn c_method_rejects_calls_and_initializers() {
        // a member-access call, an annotation, and a field initializer with a
        // call must not be read as method definitions.
        assert!(detect_c_method("obj.run() {", true, true).is_none());
        assert!(detect_c_method("@Override", false, false).is_none());
        assert!(detect_c_method("Runnable r = new Runnable() {", true, true).is_none());
        assert!(detect_c_method("int x = compute();", true, true).is_none());
        // a real header is accepted (K&R, Allman-via-opens, and open multi-line)
        assert!(detect_c_method("void foo() {", true, true).is_some());
        assert!(detect_c_method("void foo()", true, true).is_some()); // opens=true (next line {)
        assert!(detect_c_method("void foo(", true, false).is_some()); // open signature
        assert!(detect_c_method("void foo()", false, false).is_none()); // no body, no continuation
    }

    #[test]
    fn braces_in_strings_chars_comments_dont_skew_ranges() {
        // The block_end brace count must ignore `{`/`}` inside strings, char/
        // byte-char literals, and comments — else a def's range over/under-runs.
        let rs = "\
fn f() {
    let s = \"}\";
    let c = '{';
    let b = b'{';
    // a } in a comment
    /* and { here */
    g();
}

fn after() {}
";
        let defs = outline("x.rs", rs);
        let f = defs.iter().find(|d| d.name == "f").unwrap();
        assert_eq!((f.line, f.end), (1, 8), "{:?}", (f.line, f.end));
        assert!(defs.iter().any(|d| d.name == "after"), "{defs:?}");
        // Rust lifetimes must not be read as never-closing char literals.
        let lt = "fn g<'a>(x: &'a str) -> &'a str {\n    x\n}\n";
        let d2 = outline("y.rs", lt);
        let g = d2.iter().find(|d| d.name == "g").unwrap();
        assert_eq!((g.line, g.end), (1, 3));
    }

    #[test]
    fn rust_raw_string_braces_ignored() {
        // A multi-line raw string with an unbalanced brace must not extend the
        // enclosing fn's range over the next definition.
        let rs = "fn f() {\n    let j = r#\"\n{ \"a\": 1\n\"#;\n    g();\n}\n\nfn g() {}\n";
        let defs = outline("r.rs", rs);
        let f = defs.iter().find(|d| d.name == "f").unwrap();
        assert_eq!((f.line, f.end), (1, 6), "{:?}", (f.line, f.end));
        assert!(defs.iter().any(|d| d.name == "g"), "{defs:?}");
    }

    #[test]
    fn cpp_multiline_call_and_lambda_not_methods() {
        // A multi-line call / lambda inside a function body is not a definition.
        let cpp = "\
void process(std::vector<int>& v) {
    std::sort(v.begin(), v.end(), [](int a, int b) {
        return a < b;
    });
    callback([&](int n) {
        handle(n);
    });
}
";
        let n = names("a.cpp", cpp);
        assert!(has(&n, "process"), "{n:?}");
        assert!(!has(&n, "sort") && !has(&n, "callback"), "{n:?}");
    }

    #[test]
    fn js_string_with_brace_keeps_range() {
        // JS single-quoted strings are strings (not Rust char literals); a brace
        // inside one must not be counted.
        let js = "function f() {\n  const x = '{';\n  const y = \"}\";\n  return x + y;\n}\n";
        let defs = outline("a.js", js);
        let f = defs.iter().find(|d| d.name == "f").unwrap();
        assert_eq!((f.line, f.end), (1, 5), "{:?}", (f.line, f.end));
    }

    #[test]
    fn rust_multiline_signature_range_covers_body() {
        // Regression for the indentation-only block_end: a signature whose `)`
        // returns to base indent used to truncate the range before the body.
        let rs = "pub fn long_name(\n    a: i32,\n    b: i32,\n) -> Result<()> {\n    go(a, b)\n}\n\ntype Alias = T;\n";
        let defs = outline("m.rs", rs);
        let f = defs.iter().find(|d| d.name == "long_name").unwrap();
        assert_eq!((f.line, f.end), (1, 6), "{:?}", (f.line, f.end));
        // bodyless declaration stays a single line
        let a = defs.iter().find(|d| d.name == "Alias").unwrap();
        assert_eq!((a.line, a.end), (8, 8));
    }
}
