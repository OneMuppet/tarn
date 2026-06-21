//! Heuristic code/document structure — no language parser, no dependencies.
//!
//! tarn can't run a real AST (that would mean big crates), so this reads
//! structure the cheap, honest way: by extension-chosen keyword patterns
//! (`def`, `class`, `fn`, `function`, `func`, `struct`, …) and indentation for
//! block extent, plus Markdown headings. It covers the common 90% and stays
//! understandable. It is *heuristic*, not semantic — documented as such.

/// A detected definition (function, class, heading, …).
#[derive(Clone)]
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
}

/// Pick detection rules from a file's extension.
fn rules_for(path: &str) -> Rules {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mdown" | "mkd" => Rules {
            markdown: true,
            keywords: &[],
            js: false,
        },
        "py" | "pyi" => Rules {
            markdown: false,
            keywords: &["def", "class"],
            js: false,
        },
        "rs" => Rules {
            markdown: false,
            keywords: &[
                "fn",
                "struct",
                "enum",
                "trait",
                "impl",
                "mod",
                "type",
                "macro_rules!",
            ],
            js: false,
        },
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Rules {
            markdown: false,
            keywords: &["function", "class", "interface", "enum", "type"],
            js: true,
        },
        "go" => Rules {
            markdown: false,
            keywords: &["func", "type"],
            js: false,
        },
        "rb" => Rules {
            markdown: false,
            keywords: &["def", "class", "module"],
            js: false,
        },
        "php" => Rules {
            markdown: false,
            keywords: &["function", "class", "interface", "trait", "enum"],
            js: false,
        },
        "swift" => Rules {
            markdown: false,
            keywords: &["func", "class", "struct", "enum", "protocol", "extension"],
            js: false,
        },
        "kt" | "kts" => Rules {
            markdown: false,
            keywords: &["fun", "class", "object", "interface", "enum"],
            js: false,
        },
        // Class/type level for languages whose methods are `returnType name(...)`
        // (no leading keyword we can key off without a parser).
        "java" => Rules {
            markdown: false,
            keywords: &["class", "interface", "enum", "record"],
            js: false,
        },
        "cs" => Rules {
            markdown: false,
            keywords: &[
                "class",
                "interface",
                "struct",
                "enum",
                "namespace",
                "record",
            ],
            js: false,
        },
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => Rules {
            markdown: false,
            keywords: &["struct", "class", "enum", "namespace", "union"],
            js: false,
        },
        _ => Rules {
            markdown: false,
            keywords: &[
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
            ],
            js: false,
        },
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

/// Extent of an indentation-defined block starting at line index `i`.
fn block_end(lines: &[&str], i: usize) -> usize {
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
    let mut brace_depth: i32 = 0; // js: net brace nesting, for class-member detection
    let mut class_depths: Vec<i32> = Vec::new(); // js: body depths of open class/interface scopes
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
            if tr.starts_with("import ") || tr.starts_with("import{") || tr == "import" {
                if !line.contains(" from ") && !line.trim_end().ends_with(';') {
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
            block_end(&lines, *idx)
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
}
