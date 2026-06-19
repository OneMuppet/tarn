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
        "md" | "markdown" | "mdown" | "mkd" => Rules { markdown: true, keywords: &[], js: false },
        "py" | "pyi" => Rules { markdown: false, keywords: &["def", "class"], js: false },
        "rs" => Rules {
            markdown: false,
            keywords: &["fn", "struct", "enum", "trait", "impl", "mod", "type", "macro_rules!"],
            js: false,
        },
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Rules {
            markdown: false,
            keywords: &["function", "class", "interface", "enum", "type"],
            js: true,
        },
        "go" => Rules { markdown: false, keywords: &["func", "type"], js: false },
        _ => Rules {
            markdown: false,
            keywords: &[
                "def", "class", "fn", "func", "function", "struct", "enum", "trait", "impl",
                "mod", "type", "interface", "namespace", "module",
            ],
            js: false,
        },
    }
}

const MODIFIERS: &[&str] = &[
    "pub", "export", "default", "async", "static", "public", "private", "protected", "final",
    "abstract",
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
        .take_while(|c| !"({<=:;{".contains(*c))
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
        let matches = t == *kw || t.starts_with(&format!("{kw} ")) || t.starts_with(&format!("{kw}!"));
        if matches {
            let rest = &t[kw.len()..];
            let name = name_after(rest);
            if !name.is_empty() {
                return Some((kw.trim_end_matches('!').to_string(), name));
            }
        }
    }
    None
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
    end
}

/// The structural outline of a document.
pub fn outline(path: &str, content: &str) -> Vec<Def> {
    let rules = rules_for(path);
    let lines: Vec<&str> = content.lines().collect();

    // First pass: candidate definition lines. `rank` drives nesting: a heading's
    // level, or (for code) the line's indentation.
    let mut raw: Vec<(usize, String, String, usize, usize)> = Vec::new(); // (idx,kind,name,level,rank)
    let mut in_fence = false; // markdown: don't read `#` inside ``` / ~~~ code blocks
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
        }
        if let Some((kind, name)) = detect(line, &rules) {
            let level = kind
                .strip_prefix('h')
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            let rank = if level > 0 { level } else { indent(line) };
            raw.push((idx, kind, name, level, rank));
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
    let name = chain.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(".");
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
    fn rust_items() {
        let rs = "pub fn run() {\n    let x = 1;\n}\n\nstruct Cfg {\n    n: u8,\n}\n";
        let defs = outline("m.rs", rs);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"run"));
        assert!(names.contains(&"Cfg"));
    }
}
