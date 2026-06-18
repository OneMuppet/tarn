//! tarn — a tiny, understandable terminal editor that's also a scriptable
//! key=value (.env) tool for AI harnesses.
//!
//! One binary, two behaviors:
//!   * `tarn <file>`  launches the interactive TUI editor (only on a TTY).
//!   * subcommands    are non-interactive and scriptable (the harness path).

mod editor;
mod envfile;
mod render;
mod structure;
mod terminal;
mod textfile;

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(run(&args))
}

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
        "find" => cmd_find(&args[1..]),
        "replace" => cmd_replace(&args[1..]),
        "insert" => cmd_insert(&args[1..]),
        "delete" | "del" => cmd_delete(&args[1..]),
        "write" => cmd_write(&args[1..]),
        "apply" => cmd_apply(&args[1..]),
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => all = true,
            "--json" => json = true,
            "--plain" => color_pref = Some(false),
            "--color" => color_pref = Some(true),
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

    // Priority: explicit range > around > head > tail > all > auto.
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

/// `outline <file> [--json]` — a structural map of the file (defs, headings).
fn cmd_outline(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let file = match args.iter().find(|a| !a.starts_with("--")) {
        Some(f) => f.as_str(),
        None => return usage_err("outline <file> [--json]"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    let defs = structure::outline(file, &content);
    let name = base_name(file);
    if json {
        print!("{}", render::outline_json(&name, &defs));
    } else {
        print!("{}", render::outline_view(&name, &defs, use_color()));
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--ignore-case" => ignore_case = true,
            "--enclosing" => enclosing = true,
            "--json" => json = true,
            "--limit" => match next_usize(args, &mut i) {
                Some(n) => limit = n,
                None => return usage_err("find <file> <pattern> --limit N"),
            },
            s if s.starts_with("--") || (s.starts_with('-') && s.len() == 2 && file.is_some()) => {
                eprintln!("tarn: unknown flag {s}");
                return EXIT_USAGE;
            }
            s if file.is_none() => file = Some(s),
            s => pattern = Some(s),
        }
        i += 1;
    }

    let (file, pattern) = match (file, pattern) {
        (Some(f), Some(p)) => (f, p),
        _ => return usage_err("find <file> <pattern> [-i] [--enclosing] [--json]"),
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };

    let needle = if ignore_case { pattern.to_lowercase() } else { pattern.to_string() };
    let mut matches: Vec<render::FindMatch> = Vec::new();
    let mut total = 0usize;
    for (idx, line) in content.lines().enumerate() {
        let hay = if ignore_case { line.to_lowercase() } else { line.to_string() };
        if hay.contains(&needle) {
            total += 1;
            if matches.len() < limit {
                let scope = if enclosing {
                    structure::qualified(file, &content, idx + 1)
                } else {
                    None
                };
                matches.push(render::FindMatch { line: idx + 1, text: line.to_string(), scope });
            }
        }
    }

    if total == 0 {
        return EXIT_NOT_FOUND;
    }

    let name = base_name(file);
    if json {
        print!("{}", render::find_json(&name, pattern, &matches));
    } else {
        print!("{}", render::find_view(&name, pattern, &matches, use_color()));
        if total > matches.len() {
            println!("… {} more match(es) (raise --limit)", total - matches.len());
        }
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `replace <file> <N> <text> [--diff|--json] [--dry-run]`
fn cmd_replace(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, n, text) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(n), Some(t)) => match n.parse::<usize>() {
            Ok(n) => (f.as_str(), n, t.as_str()),
            Err(_) => return usage_err("replace <file> <N> <text> [--diff|--json] [--dry-run]"),
        },
        _ => return usage_err("replace <file> <N> <text> [--diff|--json] [--dry-run]"),
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

/// `apply <file> [...]` — apply a batch of ops from stdin, atomically.
/// Ops (one per line; `#` and blanks ignored), all against ORIGINAL line numbers:
///     expect  <N> <text>      replace <N> <text>
///     insert  <after-N> <text>    delete  <A-B>
fn cmd_apply(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let file = match flags.rest.first() {
        Some(f) => f.as_str(),
        None => return usage_err("apply <file> [--diff|--json] [--dry-run]   (ops on stdin)"),
    };
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("tarn: failed to read stdin");
        return EXIT_USAGE;
    }
    let mut ops = Vec::new();
    for (i, raw) in input.lines().enumerate() {
        match parse_op(raw) {
            Ok(Some(op)) => ops.push(op),
            Ok(None) => {}
            Err(e) => {
                eprintln!("tarn: ops line {}: {e}", i + 1);
                return EXIT_USAGE;
            }
        }
    }
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    let new = match textfile::apply_ops(&old, &ops) {
        Ok(n) => n,
        Err(e) => {
            let code = if e.starts_with("expect failed") { EXIT_GUARD } else { EXIT_USAGE };
            eprintln!("tarn: {e}");
            return code;
        }
    };
    commit(file, "apply", &flags, &old, &new)
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
    expect: Option<String>,
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
    let (mut diff, mut json, mut dry_run) = (false, false, false);
    let mut color_pref: Option<bool> = None;
    let mut expect: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--diff" => diff = true,
            "--json" => json = true,
            "--dry-run" => dry_run = true,
            "--color" => color_pref = Some(true),
            "--plain" => color_pref = Some(false),
            "--expect" => {
                i += 1;
                expect = args.get(i).cloned();
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
        expect,
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

fn print_usage() {
    println!(
        "tarn {VERSION} — a tiny, understandable terminal editor + scriptable .env tool

USAGE:
    tarn <file>                 open the interactive editor

  navigate (structure without reading the whole file):
    tarn outline <file>         map of defs/classes/headings  [--json]
    tarn find    <file> <text>  search; each hit + line number [--json]
        -i (ignore case) | --enclosing (tag hits with their def) | --limit N

  documents (non-interactive — for scripts & AI harnesses):
    tarn show    <file>         editor-style snapshot to stdout
        --lines A-B | --around N [--context K] | --head [K] | --tail [K] | --all
        --highlight A-B | --json | --plain | --color
    tarn replace <file> <N> <text>        replace line N
    tarn insert  <file> <after-N> <text>  insert after line N (0=top)
    tarn delete  <file> <A-B>             delete line range  (alias: del)
    tarn write   <file>                   replace file from stdin
    tarn apply   <file>                   batch ops from stdin, atomically
        edit flags:  --diff (preview) | --json | --dry-run (don't write)
        --expect <text>  guard: fail (exit 3) unless the target matches

  .env / key=value:
    tarn get   <file> <KEY>     print KEY's value (exit 1 if missing)
    tarn set   <file> <KEY=VAL> add/update KEY (preserves comments + order)
                                also: tarn set <file> <KEY> <VAL>
    tarn unset <file> <KEY>     remove KEY            (alias: rm)
    tarn keys  <file>           list keys, one per line (alias: list)
    tarn view  <file>           print the file       (alias: cat) [--numbers]

    tarn --help | -h            tarn --version | -V

EDITOR KEYS:
    arrows / Home / End / PageUp / PageDown   move
    Enter  split line     Backspace / Delete  edit
    ^S     save           ^Q                  quit (twice if unsaved)

EXIT CODES:
    0 success    1 not found    2 usage error    3 guard (--expect) failed"
    );
}
