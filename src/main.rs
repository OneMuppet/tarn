//! tarn — a tiny, understandable terminal editor that's also a scriptable
//! key=value (.env) tool for AI harnesses.
//!
//! One binary, two behaviors:
//!   * `tarn <file>`  launches the interactive TUI editor (only on a TTY).
//!   * subcommands    are non-interactive and scriptable (the harness path).

mod editor;
mod envfile;
mod render;
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
        "replace" => cmd_replace(&args[1..]),
        "insert" => cmd_insert(&args[1..]),
        "delete" | "del" => cmd_delete(&args[1..]),
        "write" => cmd_write(&args[1..]),
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => all = true,
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

    let color = color_pref.unwrap_or_else(use_color);
    print!("{}", render::show(&base_name(file), &content, &win, highlight, color));
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `replace <file> <N> <text> [--diff]`
fn cmd_replace(args: &[String]) -> u8 {
    let (diff, color, rest) = parse_edit_flags(args);
    let (file, n, text) = match (rest.first(), rest.get(1), rest.get(2)) {
        (Some(f), Some(n), Some(t)) => match n.parse::<usize>() {
            Ok(n) => (f.as_str(), n, t.as_str()),
            Err(_) => return usage_err("replace <file> <N> <text> [--diff]"),
        },
        _ => return usage_err("replace <file> <N> <text> [--diff]"),
    };
    apply_edit(file, diff, color, |c| textfile::replace(c, n, text))
}

/// `insert <file> <after-N> <text> [--diff]`  (after-N = 0 inserts at the top)
fn cmd_insert(args: &[String]) -> u8 {
    let (diff, color, rest) = parse_edit_flags(args);
    let (file, after, text) = match (rest.first(), rest.get(1), rest.get(2)) {
        (Some(f), Some(n), Some(t)) => match n.parse::<usize>() {
            Ok(n) => (f.as_str(), n, t.as_str()),
            Err(_) => return usage_err("insert <file> <after-N> <text> [--diff]"),
        },
        _ => return usage_err("insert <file> <after-N> <text> [--diff]"),
    };
    apply_edit(file, diff, color, |c| textfile::insert(c, after, text))
}

/// `delete <file> <A-B> [--diff]`  (alias: del)
fn cmd_delete(args: &[String]) -> u8 {
    let (diff, color, rest) = parse_edit_flags(args);
    let (file, range) = match (rest.first(), rest.get(1)) {
        (Some(f), Some(r)) => match parse_range(r) {
            Some(r) => (f.as_str(), r),
            None => return usage_err("delete <file> <A-B> [--diff]"),
        },
        _ => return usage_err("delete <file> <A-B> [--diff]"),
    };
    apply_edit(file, diff, color, |c| textfile::delete(c, range.0, range.1))
}

/// `write <file> [--diff]` — replace the whole file with stdin.
fn cmd_write(args: &[String]) -> u8 {
    let (diff, color, rest) = parse_edit_flags(args);
    let file = match rest.first() {
        Some(f) => f.as_str(),
        None => return usage_err("write <file> [--diff]   (content on stdin)"),
    };
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("tarn: failed to read stdin");
        return EXIT_USAGE;
    }
    let new = textfile::normalize(&input);
    let old = read_or_empty(file);
    match fs::write(file, &new) {
        Ok(()) => {
            if diff {
                print!("{}", render::diff(&old, &new, color));
            }
            EXIT_OK
        }
        Err(e) => {
            eprintln!("tarn: cannot write {file}: {e}");
            EXIT_USAGE
        }
    }
}

/// Run a line edit, write it back, and optionally print the diff.
fn apply_edit<F>(file: &str, diff: bool, color: bool, edit: F) -> u8
where
    F: FnOnce(&str) -> Result<String, String>,
{
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    let new = match edit(&old) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("tarn: {e}");
            return EXIT_USAGE;
        }
    };
    match fs::write(file, &new) {
        Ok(()) => {
            if diff {
                print!("{}", render::diff(&old, &new, color));
                let _ = io::stdout().flush();
            }
            EXIT_OK
        }
        Err(e) => {
            eprintln!("tarn: cannot write {file}: {e}");
            EXIT_USAGE
        }
    }
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

/// Pull `--diff`, `--color`, `--plain` out of an edit command's args.
/// Returns (diff?, color?, remaining args). Color: `--plain` < `--color` <
/// auto-detect (TTY + NO_COLOR).
fn parse_edit_flags(args: &[String]) -> (bool, bool, Vec<String>) {
    let diff = args.iter().any(|a| a == "--diff");
    let color = if args.iter().any(|a| a == "--plain") {
        false
    } else if args.iter().any(|a| a == "--color") {
        true
    } else {
        use_color()
    };
    let rest: Vec<String> = args
        .iter()
        .filter(|a| !matches!(a.as_str(), "--diff" | "--color" | "--plain"))
        .cloned()
        .collect();
    (diff, color, rest)
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

  documents (non-interactive — for scripts & AI harnesses):
    tarn show    <file>         editor-style snapshot to stdout
        --lines A-B | --around N [--context K] | --head [K] | --tail [K] | --all
        --highlight A-B | --plain | --color
    tarn replace <file> <N> <text>        replace line N           [--diff]
    tarn insert  <file> <after-N> <text>  insert after line N (0=top)[--diff]
    tarn delete  <file> <A-B>             delete line range  (del)  [--diff]
    tarn write   <file>                   replace file from stdin   [--diff]

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
    0 success    1 not found    2 usage error"
    );
}
