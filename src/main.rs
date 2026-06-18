//! tarn — a tiny, understandable terminal editor that's also a scriptable
//! key=value (.env) tool for AI harnesses.
//!
//! One binary, two behaviors:
//!   * `tarn <file>`  launches the interactive TUI editor (only on a TTY).
//!   * subcommands    are non-interactive and scriptable (the harness path).

mod editor;
mod envfile;
mod terminal;

use std::fs;
use std::io::{self, IsTerminal, Write};
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

// ---- helpers ---------------------------------------------------------------

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
    tarn get   <file> <KEY>     print KEY's value (exit 1 if missing)
    tarn set   <file> <KEY=VAL> add/update KEY (preserves comments + order)
                                also: tarn set <file> <KEY> <VAL>
    tarn unset <file> <KEY>     remove KEY            (alias: rm)
    tarn keys  <file>           list keys, one per line (alias: list)
    tarn view  <file>           print the file       (alias: cat) [--numbers]
    tarn --help | -h
    tarn --version | -V

EDITOR KEYS:
    arrows / Home / End / PageUp / PageDown   move
    Enter  split line     Backspace / Delete  edit
    ^S     save           ^Q                  quit (twice if unsaved)

EXIT CODES:
    0 success    1 not found    2 usage error"
    );
}
