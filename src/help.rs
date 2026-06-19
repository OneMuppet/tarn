//! Self-description for agents: one command table drives both the machine-readable
//! manifest (`tarn help --json`) and focused per-command help (`tarn help <cmd>`),
//! so any harness can learn tarn's full surface in a single call — and the two
//! can't drift apart.

use crate::render::jstr;

pub struct Cmd {
    pub name: &'static str,
    pub group: &'static str,
    pub usage: &'static str,
    pub summary: &'static str,
    pub examples: &'static [&'static str],
}

pub const COMMANDS: &[Cmd] = &[
    Cmd {
        name: "outline",
        group: "navigate",
        usage: "tarn outline <path> [--depth N] [--json]",
        summary: "Structural map (defs/classes/headings + line ranges) of a file, or a whole directory in one pass.",
        examples: &["tarn outline src/ --depth 0", "tarn outline app.py --json"],
    },
    Cmd {
        name: "find",
        group: "navigate",
        usage: "tarn find <path> <pattern> [-i] [-w] [-c] [-l] [--enclosing] [-A/-B/-C N] [--limit N] [--json] [-- <pattern>]",
        summary: "Literal substring search across a file or directory; each hit carries file+line, optionally its enclosing definition. Faster than the system grep.",
        examples: &[
            "tarn find src/ parse_value --enclosing --json",
            "tarn find src/ port -w -C 2",
        ],
    },
    Cmd {
        name: "peek",
        group: "navigate",
        usage: "tarn peek <file> <name> [--json]",
        summary: "Show just the definition named <name> (its whole block), without reading the file.",
        examples: &["tarn peek src/main.rs cmd_find"],
    },
    Cmd {
        name: "show",
        group: "read",
        usage: "tarn show <file> [--lines A-B | --around N [--context K] | --block N | --head [K] | --tail [K] | --all] [--highlight A-B] [--json]",
        summary: "Editor-style, windowed view of a file to stdout (line gutter, optional highlight).",
        examples: &["tarn show app.py --around 40 --highlight 42", "tarn show app.py --block 27"],
    },
    Cmd {
        name: "replace",
        group: "edit",
        usage: "tarn replace <file> <N> <text> [--expect T]  |  tarn replace <file> --match <anchor> <new-line> [--all]   [--diff|--json|--dry-run]",
        summary: "Replace line N (guard with --expect T). Or, content-addressed: --match <anchor> replaces the whole line containing <anchor> (must be unique unless --all) — no line number, survives drift.",
        examples: &[
            "tarn replace app.py 27 'PORT = 9090' --expect 'PORT = 8000'",
            "tarn replace app.py --match 'PORT = 8000' 'PORT = 9090'",
        ],
    },
    Cmd {
        name: "insert",
        group: "edit",
        usage: "tarn insert <file> <after-N> <text> [--expect T] [--diff|--json] [--dry-run]",
        summary: "Insert text after line N (0 = top of file).",
        examples: &["tarn insert app.py 0 '#!/usr/bin/env python3'"],
    },
    Cmd {
        name: "delete",
        group: "edit",
        usage: "tarn delete <file> <A-B> [--expect T] [--diff|--json] [--dry-run]   (alias: del)",
        summary: "Delete an inclusive line range.",
        examples: &["tarn delete app.py 40-42 --diff"],
    },
    Cmd {
        name: "write",
        group: "edit",
        usage: "tarn write <file> [--diff|--json] [--dry-run]   (content on stdin)",
        summary: "Replace the whole file with stdin (preserves line-ending style).",
        examples: &["generate | tarn write app.py --diff"],
    },
    Cmd {
        name: "apply",
        group: "edit",
        usage: "tarn apply [file] [--diff|--json] [--dry-run]   (ops on stdin)",
        summary: "Atomic batch of ops (expect/replace/insert/delete) against original line numbers; a `file <path>` line edits across files — all-or-nothing, with rollback on write failure.",
        examples: &["printf 'file a.rs\\nreplace 3 X\\nfile b.rs\\ndelete 5-6\\n' | tarn apply --diff"],
    },
    Cmd {
        name: "rename",
        group: "edit",
        usage: "tarn rename <path> <old> <new> [--in <def>] [--substring] [--dry-run] [--json]",
        summary: "Whole-word rename in a file or directory; --in <def> scopes it to one definition; --substring matches anywhere.",
        examples: &["tarn rename src/ oldName newName --dry-run"],
    },
    Cmd {
        name: "json",
        group: "config",
        usage: "tarn json get <file> <path>  |  tarn json set <file> <path> <value> [--dry-run|--diff]",
        summary: "Read or set a JSON value by dot/index path (a.b.0.c), preserving the file's formatting (no reserialize).",
        examples: &["tarn json get config.json server.port", "tarn json set config.json server.port 9090"],
    },
    Cmd {
        name: "toml",
        group: "config",
        usage: "tarn toml get <file> <path>  |  tarn toml set <file> <path> <value> [--dry-run|--diff]",
        summary: "Read or set a TOML value by dotted path (server.port), preserving the file's formatting (comments, key order). Single-line values; errors on multiline/array-of-tables.",
        examples: &["tarn toml get Cargo.toml package.version", "tarn toml set pyproject.toml tool.ruff.line-length 100"],
    },
    Cmd {
        name: "yaml",
        group: "config",
        usage: "tarn yaml get <file> <path>  |  tarn yaml set <file> <path> <value> [--dry-run|--diff]",
        summary: "Read or set a YAML value by dotted path (server.host), preserving formatting/comments. Block-mapping scalars only; errors (never corrupts) on sequences/flow/block-scalars/anchors/multi-doc.",
        examples: &["tarn yaml get deploy.yaml spec.replicas", "tarn yaml set ci.yml jobs.build.timeout-minutes 30"],
    },
    Cmd {
        name: "check",
        group: "verify",
        usage: "tarn check <file> [--json]",
        summary: "Fast file-hygiene gate (trailing whitespace, mixed indent/EOL, missing final newline, BOM). Exit 0 clean, 1 issues.",
        examples: &["tarn check app.py"],
    },
    Cmd {
        name: "get",
        group: "env",
        usage: "tarn get <file> <KEY>",
        summary: "Print a key's value from a .env/key=value file (exit 1 if missing).",
        examples: &["tarn get .env DATABASE_URL"],
    },
    Cmd {
        name: "set",
        group: "env",
        usage: "tarn set <file> <KEY=VAL>   (or: set <file> <KEY> <VAL>)",
        summary: "Add or update a key in a .env file, preserving comments and order.",
        examples: &["tarn set .env PORT 8080"],
    },
    Cmd {
        name: "unset",
        group: "env",
        usage: "tarn unset <file> <KEY>   (alias: rm)",
        summary: "Remove a key from a .env file.",
        examples: &["tarn unset .env OLD_KEY"],
    },
    Cmd {
        name: "keys",
        group: "env",
        usage: "tarn keys <file>   (alias: list)",
        summary: "List the keys in a .env file, one per line.",
        examples: &["tarn keys .env"],
    },
    Cmd {
        name: "view",
        group: "read",
        usage: "tarn view <file> [--numbers]   (alias: cat)",
        summary: "Print a file to stdout, optionally with line numbers.",
        examples: &["tarn view .env --numbers"],
    },
];

const EXIT_CODES: &[(&str, &str)] = &[
    ("0", "success"),
    ("1", "not found (file or key/path absent, or no matches)"),
    ("2", "usage error"),
    ("3", "guard failed (--expect / apply expectation)"),
];

/// Look up a command by name (or its documented alias).
fn find_cmd(name: &str) -> Option<&'static Cmd> {
    COMMANDS.iter().find(|c| {
        c.name == name
            || (name == "del" && c.name == "delete")
            || (name == "rm" && c.name == "unset")
            || (name == "list" && c.name == "keys")
            || (name == "cat" && c.name == "view")
    })
}

/// Focused, human-readable help for one command.
pub fn command_help(name: &str) -> Option<String> {
    let c = find_cmd(name)?;
    let mut out = format!("tarn {} — {}\n\n  {}\n", c.name, c.summary, c.usage);
    if !c.examples.is_empty() {
        out.push_str("\n  examples:\n");
        for ex in c.examples {
            out.push_str(&format!("    {ex}\n"));
        }
    }
    Some(out)
}

/// Machine-readable manifest of the whole CLI: commands, flags-in-usage, exit codes.
pub fn manifest_json(version: &str) -> String {
    let cmds: Vec<String> = COMMANDS
        .iter()
        .map(|c| {
            let ex: Vec<String> = c.examples.iter().map(|e| jstr(e)).collect();
            format!(
                "{{\"name\":{},\"group\":{},\"usage\":{},\"summary\":{},\"examples\":[{}]}}",
                jstr(c.name),
                jstr(c.group),
                jstr(c.usage),
                jstr(c.summary),
                ex.join(",")
            )
        })
        .collect();
    let codes: Vec<String> = EXIT_CODES
        .iter()
        .map(|(k, v)| format!("{}:{}", jstr(k), jstr(v)))
        .collect();
    format!(
        "{{\"name\":\"tarn\",\"version\":{},\"exit_codes\":{{{}}},\"commands\":[{}]}}\n",
        jstr(version),
        codes.join(","),
        cmds.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_covers_every_dispatched_command() {
        // Keep this list in sync with the dispatcher in main.rs::run.
        for name in [
            "outline", "find", "peek", "show", "replace", "insert", "delete", "write",
            "apply", "rename", "json", "toml", "yaml", "check", "get", "set", "unset", "keys", "view",
        ] {
            assert!(find_cmd(name).is_some(), "manifest missing command: {name}");
        }
    }

    #[test]
    fn aliases_resolve() {
        assert_eq!(find_cmd("del").unwrap().name, "delete");
        assert_eq!(find_cmd("rm").unwrap().name, "unset");
        assert!(command_help("cat").unwrap().contains("view"));
    }

    #[test]
    fn manifest_is_well_formed_ish() {
        let m = manifest_json("0.1.0");
        assert!(m.starts_with("{\"name\":\"tarn\""));
        assert!(m.contains("\"version\":\"0.1.0\""));
        assert!(m.contains("\"name\":\"find\""));
        assert!(m.contains("\"exit_codes\""));
        // balanced braces as a cheap structural sanity check
        assert_eq!(m.matches('{').count(), m.matches('}').count());
    }
}
