//! tarn — a tiny, understandable terminal editor that's also a scriptable
//! key=value (.env) tool for AI harnesses.
//!
//! One binary, two behaviors:
//!   * `tarn <file>`  launches the interactive TUI editor (only on a TTY).
//!   * subcommands    are non-interactive and scriptable (the harness path).

mod editor;
mod envfile;
mod help;
mod json;
mod render;
mod structure;
mod terminal;
mod textfile;
mod toml;
mod yaml;

use render::Window;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exit codes (they matter for harness scripting):
///   0 success, 1 not-found, 2 usage error.
const EXIT_OK: u8 = 0;
const EXIT_NOT_FOUND: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_GUARD: u8 = 3; // a guarded edit's --expect / apply expectation failed

fn main() -> ExitCode {
    reset_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&args))
}

/// Restore the default SIGPIPE disposition. Rust sets SIGPIPE to *ignore* at
/// startup, which turns a closed-pipe write (`tarn find . x | head`) into a
/// panic instead of a quiet exit. Putting it back to the OS default makes tarn
/// behave like `grep`/`cat` when a reader closes early. Uses the always-linked
/// system libc directly — no crate dependency, in keeping with the std-only rule.
#[cfg(unix)]
fn reset_sigpipe() {
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    // Safety: SIG_DFL is always a valid handler value; this just restores the
    // default disposition for one signal and touches no Rust state.
    unsafe {
        signal(SIGPIPE, SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

fn run(args: &[String]) -> u8 {
    let first = match args.first() {
        Some(a) => a.as_str(),
        None => {
            print_usage();
            return EXIT_USAGE;
        }
    };

    match first {
        "--help" | "-h" => {
            print_usage();
            EXIT_OK
        }
        "help" => cmd_help(&args[1..]),
        "--version" | "-V" => {
            println!("tarn {VERSION}");
            EXIT_OK
        }
        "get" => cmd_get(&args[1..]),
        "set" => cmd_set(&args[1..]),
        "unset" | "rm" => cmd_unset(&args[1..]),
        "keys" | "list" => cmd_keys(&args[1..]),
        "view" | "cat" => cmd_view(&args[1..]),
        "show" => cmd_show(&args[1..]),
        "outline" => cmd_outline(&args[1..]),
        "peek" => cmd_peek(&args[1..]),
        "find" => cmd_find(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "replace" => cmd_replace(&args[1..]),
        "insert" => cmd_insert(&args[1..]),
        "delete" | "del" => cmd_delete(&args[1..]),
        "write" => cmd_write(&args[1..]),
        "apply" => cmd_apply(&args[1..]),
        "rename" => cmd_rename(&args[1..]),
        "json" => cmd_json(&args[1..]),
        "toml" => cmd_toml(&args[1..]),
        "yaml" => cmd_yaml(&args[1..]),
        // Anything else is treated as a filename to open.
        _ => open_file(first),
    }
}

/// Open `path` in the TUI — but only if stdout is a real terminal. Under a
/// harness or a pipe we must NOT spawn a TUI: print the file plus a hint instead.
fn open_file(path: &str) -> u8 {
    if io::stdout().is_terminal() {
        match editor::Editor::open(PathBuf::from(path)) {
            Ok(mut ed) => match ed.run() {
                Ok(()) => EXIT_OK,
                Err(e) => {
                    eprintln!("tarn: {e}");
                    EXIT_USAGE
                }
            },
            Err(e) => {
                eprintln!("tarn: {path}: {e}");
                EXIT_NOT_FOUND
            }
        }
    } else {
        // No TTY: behave like `view`, then point at the scriptable commands.
        match fs::read_to_string(path) {
            Ok(content) => {
                print!("{content}");
                let _ = io::stdout().flush();
                eprintln!(
                    "tarn: no TTY — not starting the editor. \
                     Use `tarn get/set/unset/keys {path} …` for scripting."
                );
                EXIT_OK
            }
            Err(_) => {
                eprintln!("tarn: cannot read {path}");
                EXIT_NOT_FOUND
            }
        }
    }
}

// ---- subcommands -----------------------------------------------------------

fn cmd_get(args: &[String]) -> u8 {
    let (file, key) = match (args.first(), args.get(1)) {
        (Some(f), Some(k)) => (f, k),
        _ => return usage_err("get <file> <KEY>"),
    };
    let content = read_or_empty(file);
    match envfile::get(&content, key) {
        Some(val) => {
            println!("{val}");
            EXIT_OK
        }
        None => EXIT_NOT_FOUND,
    }
}

fn cmd_set(args: &[String]) -> u8 {
    let file = match args.first() {
        Some(f) => f,
        None => return usage_err("set <file> <KEY=VAL>"),
    };

    // Accept either `set file KEY=VAL` or `set file KEY VAL`.
    let (key, value) = match args.get(1) {
        Some(second) => {
            if let Some((k, v)) = second.split_once('=') {
                (k.to_string(), v.to_string())
            } else if let Some(v) = args.get(2) {
                (second.clone(), v.clone())
            } else {
                return usage_err("set <file> <KEY=VAL>   (or: set <file> <KEY> <VAL>)");
            }
        }
        None => return usage_err("set <file> <KEY=VAL>"),
    };

    if key.is_empty() {
        return usage_err("set <file> <KEY=VAL>   (KEY must not be empty)");
    }

    let content = read_or_empty(file);
    let updated = envfile::set(&content, &key, &value);
    match fs::write(file, updated) {
        Ok(()) => EXIT_OK,
        Err(e) => {
            eprintln!("tarn: cannot write {file}: {e}");
            EXIT_USAGE
        }
    }
}

fn cmd_unset(args: &[String]) -> u8 {
    let (file, key) = match (args.first(), args.get(1)) {
        (Some(f), Some(k)) => (f, k),
        _ => return usage_err("unset <file> <KEY>"),
    };
    let content = read_or_empty(file);
    let updated = envfile::unset(&content, key);
    match fs::write(file, updated) {
        Ok(()) => EXIT_OK,
        Err(e) => {
            eprintln!("tarn: cannot write {file}: {e}");
            EXIT_USAGE
        }
    }
}

fn cmd_keys(args: &[String]) -> u8 {
    let file = match args.first() {
        Some(f) => f,
        None => return usage_err("keys <file>"),
    };
    let content = read_or_empty(file);
    for key in envfile::keys(&content) {
        println!("{key}");
    }
    EXIT_OK
}

fn cmd_view(args: &[String]) -> u8 {
    // Optional --numbers flag, in any position.
    let numbers = args.iter().any(|a| a == "--numbers");
    let file = match args.iter().find(|a| !a.starts_with("--")) {
        Some(f) => f,
        None => return usage_err("view <file> [--numbers]"),
    };

    match fs::read_to_string(file) {
        Ok(content) => {
            if numbers {
                for (i, line) in content.lines().enumerate() {
                    println!("{:>6}  {line}", i + 1);
                }
            } else {
                print!("{content}");
                let _ = io::stdout().flush();
            }
            EXIT_OK
        }
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            EXIT_NOT_FOUND
        }
    }
}

// ---- document commands (the agent "open & edit in chat" path) --------------

/// `show <file> [window flags] [--highlight A-B] [--plain|--color]`
/// Print an editor-style, windowed snapshot of the document to stdout.
fn cmd_show(args: &[String]) -> u8 {
    let mut file: Option<&str> = None;
    let mut lines: Option<(usize, usize)> = None;
    let mut around: Option<usize> = None;
    let mut context: usize = 3;
    let mut head: Option<usize> = None;
    let mut tail: Option<usize> = None;
    let mut all = false;
    let mut highlight: Option<(usize, usize)> = None;
    let mut color_pref: Option<bool> = None;
    let mut json = false;
    let mut block: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => all = true,
            "--json" => json = true,
            "--plain" => color_pref = Some(false),
            "--color" => color_pref = Some(true),
            "--block" => match next_usize(args, &mut i) {
                Some(n) => block = Some(n),
                None => return usage_err("show <file> --block N"),
            },
            "--lines" => match next_range(args, &mut i) {
                Some(r) => lines = Some(r),
                None => return usage_err("show <file> --lines A-B"),
            },
            "--around" => match next_usize(args, &mut i) {
                Some(n) => around = Some(n),
                None => return usage_err("show <file> --around N [--context K]"),
            },
            "--context" => match next_usize(args, &mut i) {
                Some(k) => context = k,
                None => return usage_err("show <file> --around N --context K"),
            },
            "--highlight" => match next_range(args, &mut i) {
                Some(r) => highlight = Some(r),
                None => return usage_err("show <file> --highlight A-B"),
            },
            "--head" => head = Some(next_usize_opt(args, &mut i).unwrap_or(20)),
            "--tail" => tail = Some(next_usize_opt(args, &mut i).unwrap_or(20)),
            s if !s.starts_with("--") => file = Some(s),
            other => {
                eprintln!("tarn: unknown flag {other}");
                return EXIT_USAGE;
            }
        }
        i += 1;
    }

    let file = match file {
        Some(f) => f,
        None => return usage_err("show <file> [--lines A-B | --around N | --head | --tail | --all]"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };

    // `--block N`: window the whole definition at line N, highlighting its head.
    if let Some(n) = block {
        match structure::block_at(file, &content, n) {
            Some(d) => {
                lines = Some((d.line, d.end));
                if highlight.is_none() {
                    highlight = Some((d.line, d.line));
                }
            }
            None => {
                eprintln!("tarn: no definition at line {n} in {file}");
                return EXIT_NOT_FOUND;
            }
        }
    }

    // Priority: explicit range (incl. --block) > around > head > tail > all > auto.
    let win = if let Some((a, b)) = lines {
        Window::Range(a, b)
    } else if let Some(n) = around {
        Window::Around(n, context)
    } else if let Some(k) = head {
        Window::Head(k)
    } else if let Some(k) = tail {
        Window::Tail(k)
    } else if all {
        Window::All
    } else {
        Window::Auto
    };

    if json {
        print!("{}", render::show_json(&base_name(file), &content, &win, highlight));
    } else {
        let color = color_pref.unwrap_or_else(use_color);
        print!("{}", render::show(&base_name(file), &content, &win, highlight, color));
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `outline <path> [--depth N] [--json] [--plain|--color]` — a structural map of
/// a file, or of a whole directory (recursive, one pass), grouped by file.
fn cmd_outline(args: &[String]) -> u8 {
    let mut path: Option<&str> = None;
    let mut json = false;
    let mut color_pref: Option<bool> = None;
    let mut depth: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--plain" => color_pref = Some(false),
            "--color" => color_pref = Some(true),
            "--depth" => match next_usize(args, &mut i) {
                Some(d) => depth = Some(d),
                None => return usage_err("outline <path> --depth N"),
            },
            s if !s.starts_with('-') => {
                if path.is_none() {
                    path = Some(s);
                }
            }
            other => {
                eprintln!("tarn: unknown flag {other}");
                return EXIT_USAGE;
            }
        }
        i += 1;
    }
    let path = match path {
        Some(p) => p,
        None => return usage_err("outline <path> [--depth N] [--json]"),
    };
    let keep = |defs: Vec<structure::Def>| -> Vec<structure::Def> {
        match depth {
            Some(d) => defs.into_iter().filter(|x| x.depth <= d).collect(),
            None => defs,
        }
    };
    let color = color_pref.unwrap_or_else(use_color);

    if PathBuf::from(path).is_dir() {
        let mut per_file: Vec<(String, Vec<structure::Def>)> = Vec::new();
        let mut total = 0usize;
        for f in collect_files(path) {
            let bytes = match fs::read(&f) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if bytes.iter().take(8192).any(|&b| b == 0) {
                continue;
            }
            let content = match std::str::from_utf8(&bytes) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let fname = f.to_string_lossy().to_string();
            let defs = keep(structure::outline(&fname, content));
            if !defs.is_empty() {
                total += defs.len();
                per_file.push((fname, defs));
            }
        }
        if per_file.is_empty() {
            eprintln!("tarn: no definitions found under {path}");
            return EXIT_NOT_FOUND;
        }
        if json {
            print!("{}", render::outline_dir_json(path, &per_file));
        } else {
            print!("{}", render::outline_dir_view(path, &per_file, total, color));
        }
    } else {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("tarn: cannot read {path}");
                return EXIT_NOT_FOUND;
            }
        };
        let defs = keep(structure::outline(path, &content));
        let name = base_name(path);
        if json {
            print!("{}", render::outline_json(&name, &defs));
        } else {
            print!("{}", render::outline_view(&name, &defs, color));
        }
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `rename <path> <old> <new> [--substring] [--dry-run] [--json] [--plain|--color]`
/// Whole-word by default. `path` may be a file or directory (recursive).
fn cmd_rename(args: &[String]) -> u8 {
    let mut substring = false;
    let mut dry_run = false;
    let mut json = false;
    let mut color_pref: Option<bool> = None;
    let mut in_def: Option<&str> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--substring" => substring = true,
            "--dry-run" => dry_run = true,
            "--json" => json = true,
            "--plain" => color_pref = Some(false),
            "--color" => color_pref = Some(true),
            "--in" => match args.get(i + 1) {
                Some(name) => {
                    in_def = Some(name);
                    i += 1;
                }
                None => return usage_err("rename <path> <old> <new> --in <def>"),
            },
            s if s.starts_with("--") => {
                eprintln!("tarn: unknown flag {s}");
                return EXIT_USAGE;
            }
            s => pos.push(s),
        }
        i += 1;
    }
    let word = !substring;
    let (path, old, new) = match (pos.first(), pos.get(1), pos.get(2)) {
        (Some(p), Some(o), Some(n)) => (*p, *o, *n),
        _ => return usage_err("rename <path> <old> <new> [--in <def>] [--substring] [--dry-run] [--json]"),
    };
    if old.is_empty() {
        return usage_err("rename: <old> must not be empty");
    }

    let files = collect_files(path);
    if files.is_empty() {
        eprintln!("tarn: no readable files at {path}");
        return EXIT_NOT_FOUND;
    }

    // Compute every change first, then write — so a dry-run is free and a real
    // run doesn't start writing until all edits are known.
    let mut changes: Vec<(String, String, usize)> = Vec::new(); // (file, new_content, count)
    let mut total = 0usize;
    for f in &files {
        let content = match fs::read_to_string(f) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let fname = f.to_string_lossy().to_string();
        let (updated, count) = if let Some(name) = in_def {
            // Scope the rename to the named definition's byte span; the rest of
            // the file is left untouched.
            match structure::def_named(&fname, &content, name)
                .and_then(|d| line_byte_span(&content, d.line, d.end))
            {
                Some((a, b)) => {
                    let (slice, count) = textfile::rename(&content[a..b], old, new, word);
                    (format!("{}{}{}", &content[..a], slice, &content[b..]), count)
                }
                None => continue, // def not in this file
            }
        } else {
            textfile::rename(&content, old, new, word)
        };
        if count > 0 {
            total += count;
            changes.push((fname, updated, count));
        }
    }

    if total == 0 {
        eprintln!("tarn: no occurrences of '{old}'");
        return EXIT_NOT_FOUND;
    }

    if !dry_run {
        for (f, content, _) in &changes {
            if let Err(e) = fs::write(f, content) {
                eprintln!("tarn: cannot write {f}: {e}");
                return EXIT_USAGE;
            }
        }
    }

    let summary: Vec<(String, usize)> = changes.iter().map(|(f, _, c)| (f.clone(), *c)).collect();
    if json {
        print!("{}", render::rename_json(old, new, &summary, total, word, dry_run));
    } else {
        let color = color_pref.unwrap_or_else(use_color);
        print!("{}", render::rename_view(old, new, &summary, total, word, dry_run, color));
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `json get|set <file> <path> [value]` — surgical, format-preserving JSON.
fn cmd_json(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("get") => json_get(&args[1..]),
        Some("set") => json_set(&args[1..]),
        _ => usage_err("json get <file> <path>   |   json set <file> <path> <value>"),
    }
}

fn json_get(args: &[String]) -> u8 {
    let (file, path) = match (args.first(), args.get(1)) {
        (Some(f), Some(p)) => (f.as_str(), p.as_str()),
        _ => return usage_err("json get <file> <path>"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match json::get(&content, path) {
        Ok(Some(v)) => {
            println!("{v}");
            EXIT_OK
        }
        Ok(None) => EXIT_NOT_FOUND, // path not present
        Err(e) => {
            eprintln!("tarn: invalid JSON in {file}: {e}");
            EXIT_USAGE
        }
    }
}

fn json_set(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, path, value) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(p), Some(v)) => (f.as_str(), p.as_str(), v.as_str()),
        _ => return usage_err("json set <file> <path> <value> [--dry-run|--diff|--json]"),
    };
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match json::set(&old, path, value) {
        Ok(Some(new)) => commit(file, "json set", &flags, &old, &new),
        Ok(None) => {
            eprintln!("tarn: path not found: {path}");
            EXIT_NOT_FOUND
        }
        Err(e) => {
            eprintln!("tarn: invalid JSON in {file}: {e}");
            EXIT_USAGE
        }
    }
}

/// `toml get|set <file> <path> [value]` — surgical, format-preserving TOML.
fn cmd_toml(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("get") => toml_get(&args[1..]),
        Some("set") => toml_set(&args[1..]),
        _ => usage_err("toml get <file> <path>   |   toml set <file> <path> <value>"),
    }
}

fn toml_get(args: &[String]) -> u8 {
    let (file, path) = match (args.first(), args.get(1)) {
        (Some(f), Some(p)) => (f.as_str(), p.as_str()),
        _ => return usage_err("toml get <file> <path>"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match toml::get(&content, path) {
        Ok(Some(v)) => {
            println!("{v}");
            EXIT_OK
        }
        Ok(None) => EXIT_NOT_FOUND,
        Err(e) => {
            eprintln!("tarn: {file}: {e}");
            EXIT_USAGE
        }
    }
}

fn toml_set(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, path, value) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(p), Some(v)) => (f.as_str(), p.as_str(), v.as_str()),
        _ => return usage_err("toml set <file> <path> <value> [--dry-run|--diff|--json]"),
    };
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match toml::set(&old, path, value) {
        Ok(Some(new)) => commit(file, "toml set", &flags, &old, &new),
        Ok(None) => {
            eprintln!("tarn: path not found: {path}");
            EXIT_NOT_FOUND
        }
        Err(e) => {
            eprintln!("tarn: {file}: {e}");
            EXIT_USAGE
        }
    }
}

/// `yaml get|set <file> <path> [value]` — surgical, format-preserving YAML.
fn cmd_yaml(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("get") => yaml_get(&args[1..]),
        Some("set") => yaml_set(&args[1..]),
        _ => usage_err("yaml get <file> <path>   |   yaml set <file> <path> <value>"),
    }
}

fn yaml_get(args: &[String]) -> u8 {
    let (file, path) = match (args.first(), args.get(1)) {
        (Some(f), Some(p)) => (f.as_str(), p.as_str()),
        _ => return usage_err("yaml get <file> <path>"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match yaml::get(&content, path) {
        Ok(Some(v)) => {
            println!("{v}");
            EXIT_OK
        }
        Ok(None) => EXIT_NOT_FOUND,
        Err(e) => {
            eprintln!("tarn: {file}: {e}");
            EXIT_USAGE
        }
    }
}

fn yaml_set(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, path, value) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(p), Some(v)) => (f.as_str(), p.as_str(), v.as_str()),
        _ => return usage_err("yaml set <file> <path> <value> [--dry-run|--diff|--json]"),
    };
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match yaml::set(&old, path, value) {
        Ok(Some(new)) => commit(file, "yaml set", &flags, &old, &new),
        Ok(None) => {
            eprintln!("tarn: path not found: {path}");
            EXIT_NOT_FOUND
        }
        Err(e) => {
            eprintln!("tarn: {file}: {e}");
            EXIT_USAGE
        }
    }
}

/// `check <file> [--json] [--plain|--color]` — fast file-hygiene gate.
/// Exit 0 if clean, 1 if any issue (or unreadable).
fn cmd_check(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let color_pref = color_flag(args);
    let file = match args.iter().find(|a| !a.starts_with('-')) {
        Some(f) => f.as_str(),
        None => return usage_err("check <file> [--json]"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    let issues = textfile::check(&content);
    let name = base_name(file);
    if json {
        print!("{}", render::check_json(&name, &issues));
    } else {
        print!("{}", render::check_view(&name, &issues, color_pref.unwrap_or_else(use_color)));
    }
    let _ = io::stdout().flush();
    if issues.is_empty() {
        EXIT_OK
    } else {
        EXIT_NOT_FOUND // nonzero = "not clean", usable as a gate
    }
}

/// `peek <file> <name> [--json] [--plain|--color]`
/// Show just the definition named `name` (its whole block). Exit 1 if not found.
fn cmd_peek(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let color_pref = color_flag(args);
    let positional: Vec<&str> = args.iter().map(|s| s.as_str()).filter(|s| !s.starts_with("--")).collect();
    let (file, name) = match (positional.first(), positional.get(1)) {
        (Some(f), Some(n)) => (*f, *n),
        _ => return usage_err("peek <file> <name> [--json]"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    let def = match structure::def_named(file, &content, name) {
        Some(d) => d,
        None => {
            eprintln!("tarn: no definition named '{name}' in {file}");
            return EXIT_NOT_FOUND;
        }
    };
    let win = Window::Range(def.line, def.end);
    let hl = Some((def.line, def.line));
    let base = base_name(file);
    if json {
        print!("{}", render::show_json(&base, &content, &win, hl));
    } else {
        print!("{}", render::show(&base, &content, &win, hl, color_pref.unwrap_or_else(use_color)));
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `find <file> <pattern> [-i] [--enclosing] [--json] [--limit N]`
/// Literal substring search; with --enclosing each hit is tagged with the
/// definition that contains it. Exit 1 if there are no matches.
fn cmd_find(args: &[String]) -> u8 {
    let mut file: Option<&str> = None;
    let mut pattern: Option<&str> = None;
    let mut ignore_case = false;
    let mut enclosing = false;
    let mut json = false;
    let mut limit = 100usize;
    let mut color_pref: Option<bool> = None;
    let mut count_only = false;
    let mut files_only = false;
    let mut word = false;
    let mut ctx_before = 0usize;
    let mut ctx_after = 0usize;
    let mut end_flags = false; // set by `--`: everything after is positional

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let positional = end_flags;
        if !positional {
            match a {
                "--" => {
                    end_flags = true;
                    i += 1;
                    continue;
                }
                "-i" | "--ignore-case" => ignore_case = true,
                "-w" | "--word" => word = true,
                "--enclosing" => enclosing = true,
                "--json" => json = true,
                "--count" | "-c" => count_only = true,
                "--files" | "-l" => files_only = true,
                "--plain" => color_pref = Some(false),
                "--color" => color_pref = Some(true),
                "--limit" => match next_usize(args, &mut i) {
                    Some(n) => limit = n,
                    None => return usage_err("find <file> <pattern> --limit N"),
                },
                "-C" | "--context" => match next_usize(args, &mut i) {
                    Some(n) => {
                        ctx_before = n;
                        ctx_after = n;
                    }
                    None => return usage_err("find <path> <pattern> -C N"),
                },
                "-B" | "--before" => match next_usize(args, &mut i) {
                    Some(n) => ctx_before = n,
                    None => return usage_err("find <path> <pattern> -B N"),
                },
                "-A" | "--after" => match next_usize(args, &mut i) {
                    Some(n) => ctx_after = n,
                    None => return usage_err("find <path> <pattern> -A N"),
                },
                // Once the file is known, the next token is the pattern verbatim
                // (so patterns like `--json` are searchable; or use `--`).
                s if s.starts_with('-') && file.is_none() => {
                    eprintln!("tarn: unknown flag {s}");
                    return EXIT_USAGE;
                }
                s if file.is_none() => file = Some(s),
                s => pattern = Some(s),
            }
            i += 1;
            continue;
        }
        // positional (after `--`)
        if file.is_none() {
            file = Some(a);
        } else {
            pattern = Some(a);
        }
        i += 1;
    }

    let (path, pattern) = match (file, pattern) {
        (Some(f), Some(p)) => (f, p),
        _ => return usage_err("find <path> <pattern> [-i] [--enclosing] [--json]   (-- to search a pattern starting with -)"),
    };

    let files = collect_files(path);
    if files.is_empty() {
        eprintln!("tarn: no readable files at {path}");
        return EXIT_NOT_FOUND;
    }
    let multi = files.len() > 1;

    let needle = pattern.as_bytes();
    let mut matches: Vec<render::FindMatch> = Vec::new();
    let mut total = 0usize;
    let mut hit_files = 0usize;
    let mut matched_files: Vec<String> = Vec::new();

    for f in &files {
        // Read raw bytes; skip binaries fast (a NUL in the first chunk is the
        // usual giveaway) and anything that isn't valid UTF-8 text.
        let bytes = match fs::read(f) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bytes.iter().take(8192).any(|&b| b == 0) {
            continue;
        }
        let content = match std::str::from_utf8(&bytes) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Parse structure at most ONCE per file, and only when it's needed.
        let defs = if enclosing && !count_only && !files_only {
            Some(structure::outline(&f.to_string_lossy(), content))
        } else {
            None
        };

        let want_ctx = ctx_before > 0 || ctx_after > 0;
        let lines: Vec<&str> = content.lines().collect();
        let mut file_hit = false;
        for (idx, line) in lines.iter().enumerate() {
            if !find_in(line.as_bytes(), needle, ignore_case, word) {
                continue;
            }
            total += 1;
            file_hit = true;
            if files_only {
                break; // one hit is enough to name the file
            }
            if !count_only && matches.len() < limit {
                let scope = defs.as_ref().and_then(|d| structure::qualified_in(d, idx + 1));
                let (before, after) = if want_ctx {
                    let (lo, hi) = context_bounds(lines.len(), idx, ctx_before, ctx_after);
                    (
                        (lo..idx).map(|j| (j + 1, lines[j].to_string())).collect(),
                        ((idx + 1)..hi).map(|j| (j + 1, lines[j].to_string())).collect(),
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
                matches.push(render::FindMatch {
                    file: f.to_string_lossy().to_string(),
                    line: idx + 1,
                    text: line.to_string(),
                    scope,
                    before,
                    after,
                });
            }
        }
        if file_hit {
            hit_files += 1;
            if files_only {
                matched_files.push(f.to_string_lossy().to_string());
            }
        }
    }

    if total == 0 {
        return EXIT_NOT_FOUND;
    }

    // -c / --count: just the number.
    if count_only {
        if json {
            println!("{{\"pattern\":{},\"total\":{}}}", render::jstr(pattern), total);
        } else {
            println!("{total}");
        }
        return EXIT_OK;
    }
    // -l / --files: just the names of files that matched.
    if files_only {
        if json {
            let items: Vec<String> = matched_files.iter().map(|f| render::jstr(f)).collect();
            println!("{{\"pattern\":{},\"files\":[{}]}}", render::jstr(pattern), items.join(","));
        } else {
            for f in &matched_files {
                println!("{f}");
            }
        }
        return EXIT_OK;
    }

    if json {
        print!("{}", render::find_json(pattern, &matches, total));
    } else {
        let color = color_pref.unwrap_or_else(use_color);
        let shown_files = if multi { hit_files } else { 1 };
        print!("{}", render::find_view(pattern, &matches, shown_files, color));
        if total > matches.len() {
            println!("… {} more match(es) (raise --limit)", total - matches.len());
        }
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// Half-open `[lo, hi)` line-index window for context around a hit at `idx`,
/// clamped to the file (no underflow at the top, no overrun at the bottom).
fn context_bounds(len: usize, idx: usize, before: usize, after: usize) -> (usize, usize) {
    (idx.saturating_sub(before), (idx + 1 + after).min(len))
}

/// Byte range `[a, b)` covering 1-based inclusive lines `start..=end`, including
/// the newline ending `end`. Returns None if `start` is out of range.
fn line_byte_span(content: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    if start == 0 {
        return None;
    }
    // Byte offset where each line begins (line k → starts[k-1]).
    let mut starts = vec![0usize];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    if start > starts.len() {
        return None;
    }
    let a = starts[start - 1];
    let b = if end < starts.len() { starts[end] } else { content.len() };
    Some((a, b))
}

/// Zero-allocation substring search over bytes. `ci` = ASCII case-insensitive;
/// `word` = whole-word only (boundaries are any non-`[A-Za-z0-9_]` byte, or the
/// line edge). A UTF-8 needle matches the same byte sequence; case-folding is
/// ASCII-only (the documented behavior of `-i`). Callers search line-by-line
/// with the terminator stripped, so CRLF is normalized — a literal `\r` won't match.
fn find_in(hay: &[u8], needle: &[u8], ci: bool, word: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let eq = |a: u8, b: u8| if ci { a.eq_ignore_ascii_case(&b) } else { a == b };
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let n0 = needle[0];
    'outer: for i in 0..=hay.len() - needle.len() {
        if !eq(hay[i], n0) {
            continue;
        }
        for k in 1..needle.len() {
            if !eq(hay[i + k], needle[k]) {
                continue 'outer;
            }
        }
        if word {
            let end = i + needle.len();
            let before_ok = i == 0 || !is_word(hay[i - 1]);
            let after_ok = end == hay.len() || !is_word(hay[end]);
            if !(before_ok && after_ok) {
                continue 'outer; // keep scanning for a word-bounded hit
            }
        }
        return true;
    }
    false
}

/// Files to search: a single file as-is, or every readable file under a
/// directory (skipping hidden entries and common build/vendor dirs).
fn collect_files(path: &str) -> Vec<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_file() {
        return vec![p];
    }
    let mut out = Vec::new();
    walk(&p, &mut out);
    out
}

fn walk(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort(); // deterministic output
    for path in paths {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') {
            continue; // .git, .venv, dotfiles
        }
        // Don't follow symlinks — avoids loops and duplicate hits.
        if fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            continue;
        }
        if path.is_dir() {
            if matches!(name.as_str(), "target" | "node_modules" | "dist" | "build") {
                continue;
            }
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// `replace <file> <N> <text> [--diff|--json] [--dry-run]`
fn cmd_replace(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);

    // Anchored mode: `replace <file> --match <anchor> <new-line>` — content-
    // addressed, no line number. Replaces the whole line(s) containing <anchor>.
    if let Some(anchor) = flags.match_anchor.clone() {
        let (file, newline) = match (flags.rest.first(), flags.rest.get(1)) {
            (Some(f), Some(t)) => (f.as_str(), t.as_str()),
            _ => return usage_err("replace <file> --match <anchor> <new-line> [--all]"),
        };
        let old = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("tarn: cannot read {file}");
                return EXIT_NOT_FOUND;
            }
        };
        let matched = textfile::find_matching_lines(&old, &anchor);
        if matched.is_empty() {
            eprintln!("tarn: no line matches '{anchor}'");
            return EXIT_NOT_FOUND;
        }
        if matched.len() > 1 && !flags.all {
            let nums: Vec<String> = matched.iter().map(usize::to_string).collect();
            eprintln!(
                "tarn: '{anchor}' matches {} lines ({}) — refine it or pass --all",
                matched.len(),
                nums.join(", ")
            );
            return EXIT_USAGE;
        }
        let new = textfile::replace_at_lines(&old, &matched, newline);
        return commit(file, "replace", &flags, &old, &new);
    }

    // Line-number mode.
    let (file, n, text) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(n), Some(t)) => match n.parse::<usize>() {
            Ok(n) => (f.as_str(), n, t.as_str()),
            Err(_) => return usage_err("replace <file> <N> <text>   (or: --match <anchor> <new-line>)"),
        },
        _ => return usage_err("replace <file> <N> <text>   (or: --match <anchor> <new-line>)"),
    };
    let exp = flags.expect.clone();
    apply_edit(
        file,
        "replace",
        &flags,
        |c| check_expect(&exp, textfile::line_at(c, n)),
        |c| textfile::replace(c, n, text),
    )
}

/// `insert <file> <after-N> <text> [...]`  (after-N = 0 inserts at the top)
fn cmd_insert(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, after, text) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(n), Some(t)) => match n.parse::<usize>() {
            Ok(n) => (f.as_str(), n, t.as_str()),
            Err(_) => return usage_err("insert <file> <after-N> <text> [--diff|--json] [--dry-run]"),
        },
        _ => return usage_err("insert <file> <after-N> <text> [--diff|--json] [--dry-run]"),
    };
    let exp = flags.expect.clone();
    apply_edit(
        file,
        "insert",
        &flags,
        |c| check_expect(&exp, textfile::line_at(c, after)),
        |c| textfile::insert(c, after, text),
    )
}

/// `delete <file> <A-B> [...]`  (alias: del)
fn cmd_delete(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, range) = match (flags.rest.first(), flags.rest.get(1)) {
        (Some(f), Some(r)) => match parse_range(r) {
            Some(r) => (f.as_str(), r),
            None => return usage_err("delete <file> <A-B> [--diff|--json] [--dry-run]"),
        },
        _ => return usage_err("delete <file> <A-B> [--diff|--json] [--dry-run]"),
    };
    let exp = flags.expect.clone();
    apply_edit(
        file,
        "delete",
        &flags,
        |c| check_expect(&exp, textfile::range_text(c, range.0, range.1)),
        |c| textfile::delete(c, range.0, range.1),
    )
}

/// `write <file> [...]` — replace the whole file with stdin.
fn cmd_write(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let file = match flags.rest.first() {
        Some(f) => f.as_str(),
        None => return usage_err("write <file> [--diff|--json] [--dry-run]   (content on stdin)"),
    };
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("tarn: failed to read stdin");
        return EXIT_USAGE;
    }
    let new = textfile::normalize(&input);
    apply_edit(file, "write", &flags, |_| Ok(()), move |_| Ok(new.clone()))
}

/// `apply [file] [...]` — apply a batch of ops from stdin, atomically. Ops (one
/// per line; `#` and blanks ignored), all against each file's ORIGINAL line
/// numbers:
///     file    <path>          (switch target; needed for multi-file batches)
///     expect  <N> <text>      replace <N> <text>
///     insert  <after-N> <text>    delete  <A-B>
/// Nothing is written unless EVERY file's ops succeed (cross-file all-or-nothing).
fn cmd_apply(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    // An optional CLI file arg is the initial target (back-compat single-file form).
    let mut current: Option<String> = flags.rest.first().cloned();

    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("tarn: failed to read stdin");
        return EXIT_USAGE;
    }

    // Group ops by file, preserving first-seen order. A `file <path>` line moves
    // the target; ops for the same file (across sections) merge into one group.
    let mut groups: Vec<(String, Vec<textfile::Op>)> = Vec::new();
    for (i, raw) in input.lines().enumerate() {
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(rest) = raw.trim_start().strip_prefix("file ") {
            current = Some(rest.trim().to_string());
            continue;
        }
        let op = match parse_op(raw) {
            Ok(Some(op)) => op,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("tarn: ops line {}: {e}", i + 1);
                return EXIT_USAGE;
            }
        };
        let target = match &current {
            Some(f) => f.clone(),
            None => {
                eprintln!("tarn: ops line {}: no target file — start with `file <path>` or pass one as an argument", i + 1);
                return EXIT_USAGE;
            }
        };
        match groups.iter_mut().find(|(p, _)| *p == target) {
            Some((_, ops)) => ops.push(op),
            None => groups.push((target, vec![op])),
        }
    }

    if groups.is_empty() {
        return usage_err("apply [file] [--diff|--json|--dry-run]   (ops on stdin)");
    }

    // Validate + compute every file first; write nothing until all succeed.
    let mut changes: Vec<(String, String, String)> = Vec::new(); // (path, old, new)
    for (path, ops) in &groups {
        let old = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("tarn: cannot read {path}");
                return EXIT_NOT_FOUND;
            }
        };
        match textfile::apply_ops(&old, ops) {
            Ok(new) => changes.push((path.clone(), old, new)),
            Err(e) => {
                let code = if e.starts_with("expect failed") { EXIT_GUARD } else { EXIT_USAGE };
                eprintln!("tarn: {path}: {e}");
                return code;
            }
        }
    }

    if !flags.dry_run {
        // Validation was all-or-nothing; keep the WRITE phase all-or-nothing too.
        // If a write fails mid-batch, restore the files already written (best
        // effort) so the transaction doesn't leave a partial result.
        let mut written: Vec<&(String, String, String)> = Vec::new();
        for change in &changes {
            if let Err(e) = fs::write(&change.0, &change.2) {
                let mut unrestored = Vec::new();
                for done in &written {
                    if fs::write(&done.0, &done.1).is_err() {
                        unrestored.push(done.0.clone());
                    }
                }
                if unrestored.is_empty() {
                    eprintln!(
                        "tarn: cannot write {}: {e} — rolled back, no changes applied",
                        change.0
                    );
                } else {
                    eprintln!(
                        "tarn: cannot write {}: {e}. ROLLBACK INCOMPLETE — could not restore: {}",
                        change.0,
                        unrestored.join(", ")
                    );
                }
                return EXIT_USAGE;
            }
            written.push(change);
        }
    }

    // Report: per-file diff, or a JSON summary across files.
    if flags.json {
        let files: Vec<(String, usize, usize)> = changes
            .iter()
            .map(|(p, o, n)| (p.clone(), o.lines().count(), n.lines().count()))
            .collect();
        print!("{}", render::apply_json(&files, flags.dry_run));
    } else {
        let multi = changes.len() > 1;
        for (path, old, new) in &changes {
            if multi {
                let header = format!("── {path} ──");
                print!("{}", if flags.color { format!("\x1b[38;2;199;117;46m{header}\x1b[0m\n") } else { format!("{header}\n") });
            }
            print!("{}", render::diff(old, new, flags.color));
        }
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// Parse one line of the `apply` op protocol.
fn parse_op(raw: &str) -> Result<Option<textfile::Op>, String> {
    let line = raw.trim_end();
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let (op, rest) = line.trim_start().split_once(' ').unwrap_or((t, ""));
    match op {
        "replace" => split_num_text(rest).map(|(n, s)| Some(textfile::Op::Replace(n, s))),
        "insert" => split_num_text(rest).map(|(n, s)| Some(textfile::Op::Insert(n, s))),
        "expect" => split_num_text(rest).map(|(n, s)| Some(textfile::Op::Expect(n, s))),
        "delete" => match parse_range(rest.trim()) {
            Some((a, b)) => Ok(Some(textfile::Op::Delete(a, b))),
            None => Err("delete needs A-B".to_string()),
        },
        other => Err(format!("unknown op '{other}'")),
    }
}

/// Split "<num> <text...>" (text may be empty).
fn split_num_text(rest: &str) -> Result<(usize, String), String> {
    let rest = rest.trim_start();
    let (num, text) = rest.split_once(' ').unwrap_or((rest, ""));
    let num: usize = num
        .parse()
        .map_err(|_| format!("expected a line number, got '{num}'"))?;
    Ok((num, text.to_string()))
}

/// Flags shared by every editing command.
struct EditFlags {
    diff: bool,
    json: bool,
    dry_run: bool,
    color: bool,
    all: bool,
    expect: Option<String>,
    match_anchor: Option<String>,
    rest: Vec<String>,
}

/// Verify a `--expect` precondition against the target's current text.
fn check_expect(expect: &Option<String>, actual: Option<String>) -> Result<(), String> {
    match expect {
        Some(e) if actual.as_deref() == Some(e.as_str()) => Ok(()),
        Some(_) => {
            Err("expect failed: target does not match (run `tarn show` to inspect)".to_string())
        }
        None => Ok(()),
    }
}

/// Run a guard, compute an edit, write it (unless `--dry-run`), and report.
fn apply_edit<G, F>(file: &str, op: &str, flags: &EditFlags, guard: G, edit: F) -> u8
where
    G: FnOnce(&str) -> Result<(), String>,
    F: FnOnce(&str) -> Result<String, String>,
{
    // `write` may create the file; the line ops require it to exist.
    let old = if op == "write" {
        read_or_empty(file)
    } else {
        match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("tarn: cannot read {file}");
                return EXIT_NOT_FOUND;
            }
        }
    };
    if let Err(e) = guard(&old) {
        eprintln!("tarn: {e}");
        return EXIT_GUARD;
    }
    let new = match edit(&old) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("tarn: {e}");
            return EXIT_USAGE;
        }
    };
    commit(file, op, flags, &old, &new)
}

/// Write the new content (unless dry-run) and emit the chosen report.
fn commit(file: &str, op: &str, flags: &EditFlags, old: &str, new: &str) -> u8 {
    if !flags.dry_run {
        if let Err(e) = fs::write(file, new) {
            eprintln!("tarn: cannot write {file}: {e}");
            return EXIT_USAGE;
        }
    }
    if flags.json {
        print!(
            "{}",
            render::edit_json(file, op, old.lines().count(), new.lines().count(), flags.dry_run)
        );
    } else if flags.diff || flags.dry_run {
        // A dry-run with no explicit output still previews via diff.
        print!("{}", render::diff(old, new, flags.color));
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

// ---- helpers ---------------------------------------------------------------

/// Color is on when stdout is a terminal, unless overridden. Honors NO_COLOR.
fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}

/// An explicit `--plain` / `--color` preference, if present.
fn color_flag(args: &[String]) -> Option<bool> {
    if args.iter().any(|a| a == "--plain") {
        Some(false)
    } else if args.iter().any(|a| a == "--color") {
        Some(true)
    } else {
        None
    }
}

/// The last path component, for the `show` title bar.
fn base_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Pull the shared editing flags out of an edit command's args. Color
/// precedence: `--plain` < `--color` < auto-detect (TTY + NO_COLOR).
fn parse_edit_flags(args: &[String]) -> EditFlags {
    let (mut diff, mut json, mut dry_run, mut all) = (false, false, false, false);
    let mut color_pref: Option<bool> = None;
    let mut expect: Option<String> = None;
    let mut match_anchor: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--diff" => diff = true,
            "--json" => json = true,
            "--dry-run" => dry_run = true,
            "--all" => all = true,
            "--color" => color_pref = Some(true),
            "--plain" => color_pref = Some(false),
            "--expect" => {
                i += 1;
                expect = args.get(i).cloned();
            }
            "--match" => {
                i += 1;
                match_anchor = args.get(i).cloned();
            }
            other => rest.push(other.to_string()),
        }
        i += 1;
    }
    EditFlags {
        diff,
        json,
        dry_run,
        color: color_pref.unwrap_or_else(use_color),
        all,
        expect,
        match_anchor,
        rest,
    }
}

/// Parse "A-B" / "A:B" / "A" into an inclusive (a, b).
fn parse_range(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    for sep in ['-', ':'] {
        if let Some((a, b)) = s.split_once(sep) {
            return Some((a.trim().parse().ok()?, b.trim().parse().ok()?));
        }
    }
    let n: usize = s.parse().ok()?;
    Some((n, n))
}

/// Consume the next arg as a range, advancing the index.
fn next_range(args: &[String], i: &mut usize) -> Option<(usize, usize)> {
    *i += 1;
    parse_range(args.get(*i)?)
}

/// Consume the next arg as a number, advancing the index.
fn next_usize(args: &[String], i: &mut usize) -> Option<usize> {
    *i += 1;
    args.get(*i)?.parse().ok()
}

/// Consume the next arg as a number only if it looks numeric (for optional counts).
fn next_usize_opt(args: &[String], i: &mut usize) -> Option<usize> {
    let n = args.get(*i + 1)?.parse().ok()?;
    *i += 1;
    Some(n)
}

/// Read a file, treating a missing file as empty (so `set` can create it).
fn read_or_empty(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn usage_err(form: &str) -> u8 {
    eprintln!("usage: tarn {form}");
    EXIT_USAGE
}

/// `help [command] [--json]` — the agent-native interface.
/// `help` → overview · `help <cmd>` → focused help · `help --json` → manifest.
fn cmd_help(args: &[String]) -> u8 {
    if args.iter().any(|a| a == "--json") {
        print!("{}", help::manifest_json(VERSION));
        let _ = io::stdout().flush();
        return EXIT_OK;
    }
    match args.iter().find(|a| !a.starts_with('-')) {
        Some(name) => match help::command_help(name) {
            Some(h) => {
                print!("{h}");
                EXIT_OK
            }
            None => {
                eprintln!("tarn: no such command '{name}' (try `tarn help`)");
                EXIT_USAGE
            }
        },
        None => {
            print_usage();
            EXIT_OK
        }
    }
}

fn print_usage() {
    println!(
        "tarn {VERSION} — a tiny, understandable terminal editor + scriptable .env tool

USAGE:
    tarn <file>                 open the interactive editor

  navigate (structure without reading the whole file):
    tarn outline <path>         map of defs/classes/headings — file OR dir
        --depth N (limit nesting)  |  --json
    tarn peek    <file> <name>  show just the definition named <name>  [--json]
    tarn find    <path> <text>  search a file OR directory; hits with file+line
        -i (ignore case) | -w (whole word) | --enclosing | --limit N | --json
        -c/--count (just the number) | -l/--files (just filenames)
        -C/-B/-A N  context lines (around / before / after each hit)
        --  <text>  search a pattern that starts with a dash
    tarn check   <file>         file-hygiene gate (0 clean / 1 issues)  [--json]

  documents (non-interactive — for scripts & AI harnesses):
    tarn show    <file>         editor-style snapshot to stdout
        --lines A-B | --around N [--context K] | --head [K] | --tail [K] | --all
        --block N (the whole def at line N) | --highlight A-B | --json | --plain | --color
    tarn replace <file> <N> <text>        replace line N
        --match <anchor> <new-line>       ...or the whole line containing <anchor> [--all]
    tarn insert  <file> <after-N> <text>  insert after line N (0=top)
    tarn delete  <file> <A-B>             delete line range  (alias: del)
    tarn write   <file>                   replace file from stdin
    tarn apply   [file]                   batch ops from stdin, atomically
        across files too: a `file <path>` line in the ops switches target
    tarn rename  <path> <old> <new>       whole-word rename in a file/dir
        --in <def> (within that def; first if names repeat) | --substring | --dry-run
    tarn json get <file> <path>           read a JSON value by path (a.b.0.c)
    tarn json set <file> <path> <value>   set it, preserving file formatting
    tarn toml get <file> <path>           read a TOML value by path (a.b.c)
    tarn toml set <file> <path> <value>   set it, preserving comments + layout
    tarn yaml get <file> <path>           read a YAML value by path (a.b.c)
    tarn yaml set <file> <path> <value>   set it (block-mapping scalars)
        edit flags:  --diff (preview) | --json | --dry-run (don't write)
        --expect <text>  guard: fail (exit 3) unless the target matches

  .env / key=value:
    tarn get   <file> <KEY>     print KEY's value (exit 1 if missing)
    tarn set   <file> <KEY=VAL> add/update KEY (preserves comments + order)
                                also: tarn set <file> <KEY> <VAL>
    tarn unset <file> <KEY>     remove KEY            (alias: rm)
    tarn keys  <file>           list keys, one per line (alias: list)
    tarn view  <file>           print the file       (alias: cat) [--numbers]

    tarn help [command]         focused help for one command
    tarn help --json            machine-readable manifest (for agents)
    tarn --help | -h            tarn --version | -V

EDITOR KEYS:
    arrows / Home / End / PageUp / PageDown   move
    Enter  split line     Backspace / Delete  edit
    ^S     save           ^Q                  quit (twice if unsaved)

EXIT CODES:
    0 success    1 not found    2 usage error    3 guard (--expect) failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_bounds_clamps_edges() {
        // hit on line 1 (idx 0): before clamps to 0, no underflow
        assert_eq!(context_bounds(10, 0, 5, 2), (0, 3));
        // hit on the last line (idx 9 of 10): after clamps to len
        assert_eq!(context_bounds(10, 9, 2, 5), (7, 10));
        // zero context = just the hit line
        assert_eq!(context_bounds(10, 4, 0, 0), (4, 5));
        // window larger than the file clamps both ends
        assert_eq!(context_bounds(3, 1, 100, 100), (0, 3));
    }

    #[test]
    fn line_byte_span_covers_inclusive_lines() {
        let c = "alpha\nbeta\ngamma\n";
        // lines 1..=2 → "alpha\nbeta\n"
        let (a, b) = line_byte_span(c, 1, 2).unwrap();
        assert_eq!(&c[a..b], "alpha\nbeta\n");
        // line 3 alone → "gamma\n"
        let (a, b) = line_byte_span(c, 3, 3).unwrap();
        assert_eq!(&c[a..b], "gamma\n");
        assert_eq!(line_byte_span(c, 0, 1), None);
    }

    #[test]
    fn find_in_matches_correctly() {
        assert!(find_in(b"hello world", b"world", false, false));
        assert!(!find_in(b"hello", b"World", false, false)); // case-sensitive
        assert!(find_in(b"hello", b"ELLO", true, false)); // ASCII case-insensitive
        assert!(find_in(b"abc", b"", false, false)); // empty needle matches
        assert!(!find_in(b"ab", b"abc", false, false)); // needle longer than line
        assert!(find_in("café".as_bytes(), "afé".as_bytes(), false, false)); // multibyte
    }

    #[test]
    fn find_in_word_boundaries() {
        // whole-word: `port` matches standalone but not inside import/use_port/port2
        assert!(find_in(b"the port is", b"port", false, true));
        assert!(!find_in(b"import socket", b"port", false, true));
        assert!(!find_in(b"use_port(x)", b"port", false, true));
        assert!(!find_in(b"port2 = 1", b"port", false, true));
        // a later word-bounded hit is still found past a non-bounded one
        assert!(find_in(b"import port", b"port", false, true));
        // word + case-insensitive together
        assert!(find_in(b"The PORT", b"port", true, true));
    }
}
