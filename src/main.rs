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
mod patch;
mod regex;
mod render;
mod structure;
mod terminal;
mod textfile;
mod toml;
mod yaml;

use render::Window;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// A read-only memory map of a file via libc `mmap` (no crate — the same
/// std+FFI approach tarn already uses for `stty`/signals). Lets `find -c` scan a
/// large file straight from the page cache instead of `fs::read` copying the
/// whole thing into a `Vec` first, which is the single biggest cost on a
/// multi-hundred-MB file. Unix only; callers fall back to `fs::read`.
///
/// Tradeoff: a `MAP_PRIVATE` read-only map means that if another process
/// TRUNCATES the file while we're scanning it, touching the now-missing pages
/// raises SIGBUS and kills the process (the `fs::read` path copies up front and
/// avoids this). Same accepted tradeoff ripgrep makes — files are agent-local
/// and the map lives only for the few milliseconds of a single scan.
#[cfg(unix)]
mod mmap {
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    // PROT_READ / MAP_PRIVATE have these values on both macOS and Linux.
    const PROT_READ: i32 = 0x1;
    const MAP_PRIVATE: i32 = 0x2;

    extern "C" {
        fn mmap(
            addr: *mut core::ffi::c_void,
            len: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut core::ffi::c_void;
        fn munmap(addr: *mut core::ffi::c_void, len: usize) -> i32;
    }

    pub struct Mapped {
        ptr: *mut core::ffi::c_void,
        len: usize,
        _file: File, // keep the file (and its fd's mapping) alive for our lifetime
    }

    impl Mapped {
        /// Map `path` read-only. `None` for an empty file (mmap rejects a zero
        /// length) or any failure — the caller falls back to `fs::read`.
        pub fn open(path: &std::path::Path) -> Option<Mapped> {
            let file = File::open(path).ok()?;
            let len = file.metadata().ok()?.len() as usize;
            if len == 0 {
                return None;
            }
            // SAFETY: fd is valid for the duration (we own `file`); len > 0; a
            // null addr lets the kernel place it; the region is only ever read.
            let ptr = unsafe {
                mmap(
                    core::ptr::null_mut(),
                    len,
                    PROT_READ,
                    MAP_PRIVATE,
                    file.as_raw_fd(),
                    0,
                )
            };
            if ptr as isize == -1 {
                return None; // MAP_FAILED
            }
            Some(Mapped {
                ptr,
                len,
                _file: file,
            })
        }

        pub fn bytes(&self) -> &[u8] {
            // SAFETY: ptr/len come from a successful mmap and stay valid until
            // Drop munmaps them; the returned borrow is tied to `&self`.
            unsafe { core::slice::from_raw_parts(self.ptr as *const u8, self.len) }
        }
    }

    impl Drop for Mapped {
        fn drop(&mut self) {
            // SAFETY: ptr/len are exactly what mmap handed back.
            unsafe {
                munmap(self.ptr, self.len);
            }
        }
    }
}

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
        "defs" => cmd_defs(&args[1..]),
        "refs" => cmd_refs(&args[1..]),
        "tree" => cmd_tree(&args[1..]),
        "find" => cmd_find(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "diff" => cmd_diff(&args[1..]),
        "replace" => cmd_replace(&args[1..]),
        "insert" => cmd_insert(&args[1..]),
        "delete" | "del" => cmd_delete(&args[1..]),
        "write" => cmd_write(&args[1..]),
        "apply" => cmd_apply(&args[1..]),
        "patch" => cmd_patch(&args[1..]),
        "batch" => cmd_batch(&args[1..]),
        "rename" => cmd_rename(&args[1..]),
        "json" => cmd_json(&args[1..]),
        "toml" => cmd_toml(&args[1..]),
        "yaml" => cmd_yaml(&args[1..]),
        // A lone argument is treated as a file to open (the TUI entry point).
        // But a bare token with trailing arguments that isn't a known command
        // is almost certainly a mistyped subcommand (e.g. `tarn relace f 3 X`):
        // report that as a usage error rather than "file not found".
        _ if args.len() > 1 && !PathBuf::from(first).exists() => {
            eprintln!("tarn: unknown command '{first}' — run `tarn help` for the list");
            EXIT_USAGE
        }
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
    let content = match read_or_empty(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match envfile::get(&content, key) {
        Some(val) => {
            println!("{val}");
            EXIT_OK
        }
        None => EXIT_NOT_FOUND,
    }
}

fn cmd_set(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let file = match flags.rest.first() {
        Some(f) => f.as_str(),
        None => return usage_err("set <file> <KEY=VAL>"),
    };

    // Accept either `set file KEY=VAL` or `set file KEY VAL`.
    let (key, value) = match flags.rest.get(1) {
        Some(second) => {
            if let Some((k, v)) = second.split_once('=') {
                (k.to_string(), v.to_string())
            } else if let Some(v) = flags.rest.get(2) {
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

    let old = match read_or_empty(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    if let Err(e) = check_expect(&flags.expect, envfile::get(&old, &key)) {
        eprintln!("tarn: {e}");
        return EXIT_GUARD;
    }
    let new = envfile::set(&old, &key, &value);
    commit(file, "set", &flags, &old, &new)
}

fn cmd_unset(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, key) = match (flags.rest.first(), flags.rest.get(1)) {
        (Some(f), Some(k)) => (f.as_str(), k.as_str()),
        _ => return usage_err("unset <file> <KEY>"),
    };
    let old = match read_or_empty(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    if let Err(e) = check_expect(&flags.expect, envfile::get(&old, key)) {
        eprintln!("tarn: {e}");
        return EXIT_GUARD;
    }
    let new = envfile::unset(&old, key);
    commit(file, "unset", &flags, &old, &new)
}

fn cmd_keys(args: &[String]) -> u8 {
    let file = match args.first() {
        Some(f) => f,
        None => return usage_err("keys <file>"),
    };
    let content = match read_or_empty(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
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
                Some((a, b)) if a <= b => lines = Some((a, b)),
                _ => return usage_err("show <file> --lines A-B   (A must be ≤ B)"),
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
        None => {
            return usage_err("show <file> [--lines A-B | --around N | --head | --tail | --all]")
        }
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
        print!(
            "{}",
            render::show_json(&base_name(file), &content, &win, highlight)
        );
    } else {
        let color = color_pref.unwrap_or_else(use_color);
        print!(
            "{}",
            render::show(&base_name(file), &content, &win, highlight, color)
        );
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
            print!(
                "{}",
                render::outline_dir_view(path, &per_file, total, color)
            );
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
    let mut diff = false;
    let mut color_pref: Option<bool> = None;
    let mut in_def: Option<&str> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--substring" => substring = true,
            "--dry-run" => dry_run = true,
            "--json" => json = true,
            "--diff" => diff = true,
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
        _ => {
            return usage_err(
                "rename <path> <old> <new> [--in <def>] [--substring] [--dry-run] [--json]",
            )
        }
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
    let mut changes: Vec<(String, String, String, usize)> = Vec::new(); // (file, old, new, count)
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
                    (
                        format!("{}{}{}", &content[..a], slice, &content[b..]),
                        count,
                    )
                }
                None => continue, // def not in this file
            }
        } else {
            textfile::rename(&content, old, new, word)
        };
        if count > 0 {
            total += count;
            changes.push((fname, content, updated, count));
        }
    }

    if total == 0 {
        eprintln!("tarn: no occurrences of '{old}'");
        return EXIT_NOT_FOUND;
    }

    if !dry_run {
        for (f, _old, new_content, _) in &changes {
            if let Err(e) = fs::write(f, new_content) {
                eprintln!("tarn: cannot write {f}: {e}");
                return EXIT_USAGE;
            }
        }
    }

    let color = color_pref.unwrap_or_else(use_color);
    if json {
        let summary: Vec<(String, usize)> =
            changes.iter().map(|(f, _, _, c)| (f.clone(), *c)).collect();
        print!(
            "{}",
            render::rename_json(old, new, &summary, total, word, dry_run)
        );
    } else if diff {
        let multi = changes.len() > 1;
        for (f, old_c, new_c, _) in &changes {
            if multi {
                let header = format!("── {f} ──");
                print!(
                    "{}",
                    if color {
                        format!("\x1b[38;2;199;117;46m{header}\x1b[0m\n")
                    } else {
                        format!("{header}\n")
                    }
                );
            }
            print!("{}", render::diff(old_c, new_c, color));
        }
    } else {
        let summary: Vec<(String, usize)> =
            changes.iter().map(|(f, _, _, c)| (f.clone(), *c)).collect();
        print!(
            "{}",
            render::rename_view(old, new, &summary, total, word, dry_run, color)
        );
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `json get|set <file> <path> [value]` — surgical, format-preserving JSON.
fn cmd_json(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("get") => json_get(&args[1..]),
        Some("set") => json_set(&args[1..]),
        Some("del") => json_del(&args[1..]),
        _ => usage_err("json get|set|del <file> <path> [value]"),
    }
}

fn json_del(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, path) = match (flags.rest.first(), flags.rest.get(1)) {
        (Some(f), Some(p)) => (f.as_str(), p.as_str()),
        _ => return usage_err("json del <file> <path> [--dry-run|--diff]"),
    };
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match json::del(&old, path) {
        Ok(Some(new)) => commit(file, "json del", &flags, &old, &new),
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
        Some("del") => toml_del(&args[1..]),
        _ => usage_err("toml get|set|del <file> <path> [value]"),
    }
}

fn toml_del(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, path) = match (flags.rest.first(), flags.rest.get(1)) {
        (Some(f), Some(p)) => (f.as_str(), p.as_str()),
        _ => return usage_err("toml del <file> <path> [--dry-run|--diff]"),
    };
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match toml::del(&old, path) {
        Ok(Some(new)) => commit(file, "toml del", &flags, &old, &new),
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
        Some("del") => yaml_del(&args[1..]),
        _ => usage_err("yaml get|set|del <file> <path> [value]"),
    }
}

fn yaml_del(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, path) = match (flags.rest.first(), flags.rest.get(1)) {
        (Some(f), Some(p)) => (f.as_str(), p.as_str()),
        _ => return usage_err("yaml del <file> <path> [--dry-run|--diff]"),
    };
    let old = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("tarn: cannot read {file}");
            return EXIT_NOT_FOUND;
        }
    };
    match yaml::del(&old, path) {
        Ok(Some(new)) => commit(file, "yaml del", &flags, &old, &new),
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

/// `diff <a> <b> [--plain|--color]` — show how file `a` differs from file `b`.
/// POSIX-ish exit: 0 identical, 1 differ, 2 trouble (unreadable).
fn cmd_diff(args: &[String]) -> u8 {
    let color_pref = color_flag(args);
    let unified = args.iter().any(|s| s == "-u" || s == "--unified");
    let stat = args.iter().any(|s| s == "--stat");
    let pos: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.starts_with('-'))
        .collect();
    let (a, b) = match (pos.first(), pos.get(1)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => return usage_err("diff <a> <b>"),
    };
    let (ca, cb) = match (fs::read_to_string(a), fs::read_to_string(b)) {
        (Ok(ca), Ok(cb)) => (ca, cb),
        _ => {
            eprintln!("tarn: cannot read {a} or {b}");
            return EXIT_USAGE; // 2 = trouble
        }
    };
    if ca == cb {
        return EXIT_OK; // identical
    }
    if stat {
        let (ins, del) = render::diff_stat(&ca, &cb);
        println!("{b}: +{ins} -{del} ({} change(s))", ins + del);
        let _ = io::stdout().flush();
        return EXIT_NOT_FOUND;
    }
    if unified {
        print!(
            "{}",
            render::diff_unified(&ca, &cb, &format!("a/{a}"), &format!("b/{b}"))
        );
    } else {
        print!(
            "{}",
            render::diff(&ca, &cb, color_pref.unwrap_or_else(use_color))
        );
    }
    let _ = io::stdout().flush();
    EXIT_NOT_FOUND // 1 = differences found (POSIX diff convention)
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
        print!(
            "{}",
            render::check_view(&name, &issues, color_pref.unwrap_or_else(use_color))
        );
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
/// `defs <name> [path] [--json]` — find where `name` is DEFINED (not every
/// occurrence) across a file or directory: go-to-definition via the structure
/// heuristics. `path` defaults to the current directory. Exit 1 if none found.
fn cmd_defs(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let color_pref = color_flag(args);
    // Positionals: everything after a bare `--` is literal; before it, skip flags.
    let mut pos: Vec<&str> = Vec::new();
    let mut literal = false;
    for a in args {
        if literal {
            pos.push(a);
        } else if a == "--" {
            literal = true;
        } else if !a.starts_with('-') {
            pos.push(a);
        }
    }
    let name = match pos.first() {
        Some(n) => *n,
        None => return usage_err("defs <name> [path] [--json]"),
    };
    let path = pos.get(1).copied().unwrap_or(".");

    let mut found: Vec<(String, structure::Def)> = Vec::new();
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
        for d in structure::outline(&fname, content) {
            if d.name == name {
                found.push((fname.clone(), d));
            }
        }
    }
    if found.is_empty() {
        eprintln!("tarn: no definition named '{name}' under {path}");
        return EXIT_NOT_FOUND;
    }
    if json {
        print!("{}", render::defs_json(name, &found));
    } else {
        print!(
            "{}",
            render::defs_view(name, &found, color_pref.unwrap_or_else(use_color))
        );
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `refs <name> [path] [--json] [--limit N]` — find-references: word-boundary
/// USES of `name` across a file or directory, each with its enclosing scope,
/// EXCLUDING the definition site(s) themselves. The symbol-aware complement to
/// `defs`: "who uses this", not "where is it". Exit 1 if no uses found.
fn cmd_refs(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let color_pref = color_flag(args);
    // Positionals: everything after a bare `--` is literal; before it, skip flags.
    let mut pos: Vec<&str> = Vec::new();
    let mut literal = false;
    let mut limit = 200usize;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if literal {
            pos.push(a);
        } else if a == "--" {
            literal = true;
        } else if a == "--limit" {
            match next_usize(args, &mut i) {
                Some(n) => limit = n,
                None => return usage_err("refs <name> [path] --limit N"),
            }
        } else if !a.starts_with('-') {
            pos.push(a);
        }
        i += 1;
    }
    let name = match pos.first() {
        Some(n) => *n,
        None => return usage_err("refs <name> [path] [--json] [--limit N]"),
    };
    let path = pos.get(1).copied().unwrap_or(".");
    let needle = name.as_bytes();

    let files = collect_files(path);
    let multi = files.len() > 1;
    let mut matches: Vec<render::FindMatch> = Vec::new();
    let mut total = 0usize;
    let mut hit_files = 0usize;
    for f in &files {
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
        let fname = f.to_string_lossy().to_string();
        let defs = structure::outline(&fname, content);
        // Lines that START a definition of `name` are the declaration sites —
        // exclude them so we report uses, not the definition itself.
        let decl_lines: Vec<usize> = defs
            .iter()
            .filter(|d| d.name == name)
            .map(|d| d.line)
            .collect();
        let mut file_hit = false;
        for (idx, line) in content.lines().enumerate() {
            // Cheap pre-filter: std's Two-Way `contains` skips the ~99% of lines
            // that don't hold the substring at all, so the per-line word-boundary
            // count only runs where it can find something. A word-bounded match
            // always contains the substring, so this never drops a real use.
            if !line.contains(name) {
                continue;
            }
            let lineno = idx + 1;
            let occ = word_occurrences(line.as_bytes(), needle);
            if occ == 0 {
                continue;
            }
            // On a declaration line, one occurrence IS the declaration token —
            // skip the line only if that's all it has. A self-recursive call or a
            // single-line body that also uses the name (occ ≥ 2) is a real use.
            if decl_lines.contains(&lineno) && occ < 2 {
                continue;
            }
            total += 1;
            file_hit = true;
            if matches.len() < limit {
                matches.push(render::FindMatch {
                    file: fname.clone(),
                    line: lineno,
                    text: line.to_string(),
                    scope: structure::qualified_in(&defs, lineno),
                    before: Vec::new(),
                    after: Vec::new(),
                });
            }
        }
        if file_hit {
            hit_files += 1;
        }
    }

    if total == 0 {
        eprintln!("tarn: no uses of '{name}' under {path}");
        return EXIT_NOT_FOUND;
    }
    if json {
        print!("{}", render::refs_json(name, &matches, total));
    } else {
        let color = color_pref.unwrap_or_else(use_color);
        let shown_files = if multi { hit_files } else { 1 };
        print!("{}", render::refs_view(name, &matches, shown_files, color));
        if total > matches.len() {
            println!("… {} more use(s) (raise --limit)", total - matches.len());
        }
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `tree [path] [--json] [--depth N] [--lines]` — repo orientation: a fast,
/// vendor-aware directory tree (same skip rules as the rest of tarn: hidden
/// entries, target/node_modules/dist/build, symlinks). `--lines` annotates each
/// file with its line count; `--depth N` limits recursion. Path defaults to ".".
fn cmd_tree(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let with_lines = args.iter().any(|a| a == "--lines");
    let color_pref = color_flag(args);
    let mut depth: Option<usize> = None;
    let mut path = ".";
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" | "--lines" | "--plain" | "--color" => {}
            "--depth" => match next_usize(args, &mut i) {
                Some(n) => depth = Some(n),
                None => return usage_err("tree [path] --depth N"),
            },
            s if s.starts_with("--") => {
                eprintln!("tarn: unknown flag {s}");
                return EXIT_USAGE;
            }
            s => path = s,
        }
        i += 1;
    }

    let p = PathBuf::from(path);
    if !p.exists() {
        eprintln!("tarn: no such path: {path}");
        return EXIT_NOT_FOUND;
    }
    let is_dir = p.is_dir();
    let root = render::TreeEntry {
        name: path.to_string(),
        is_dir,
        lines: if is_dir || !with_lines {
            None
        } else {
            count_lines(&p)
        },
        children: if is_dir {
            tree_children(&p, depth, with_lines)
        } else {
            Vec::new()
        },
    };

    if json {
        print!("{}", render::tree_json(&root));
    } else {
        print!(
            "{}",
            render::tree_view(&root, color_pref.unwrap_or_else(use_color))
        );
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// Recursively build a directory's children, applying the shared vendor/hidden/
/// symlink skip rules. `depth` is the remaining levels (None = unlimited).
fn tree_children(dir: &PathBuf, depth: Option<usize>, with_lines: bool) -> Vec<render::TreeEntry> {
    if depth == Some(0) {
        return Vec::new();
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    let next_depth = depth.map(|d| d - 1);
    let mut out = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            continue;
        }
        if path.is_dir() {
            if is_vendor_dir(&name) {
                continue;
            }
            out.push(render::TreeEntry {
                name,
                is_dir: true,
                lines: None,
                children: tree_children(&path, next_depth, with_lines),
            });
        } else {
            out.push(render::TreeEntry {
                name,
                is_dir: false,
                lines: if with_lines { count_lines(&path) } else { None },
                children: Vec::new(),
            });
        }
    }
    // Directories first, then files; each group already sorted by `paths.sort()`.
    out.sort_by_key(|e| !e.is_dir);
    out
}

/// Line count for `--lines`, or None for binary/unreadable files (so a binary
/// shows as a plain entry rather than a bogus count).
fn count_lines(path: &PathBuf) -> Option<usize> {
    let bytes = fs::read(path).ok()?;
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    Some(text.lines().count())
}

fn cmd_peek(args: &[String]) -> u8 {
    let json = args.iter().any(|a| a == "--json");
    let color_pref = color_flag(args);
    let positional: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.starts_with("--"))
        .collect();
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
        print!(
            "{}",
            render::show(
                &base,
                &content,
                &win,
                hl,
                color_pref.unwrap_or_else(use_color)
            )
        );
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
    let mut exts: Vec<String> = Vec::new(); // --ext rs,toml → only these extensions
    let mut end_flags = false; // set by `--`: everything after is positional
    let mut regex_mode = false; // -e/--regex: treat the pattern as a regular expression

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
                "-e" | "--regex" => regex_mode = true,
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
                "--ext" | "-t" => {
                    i += 1;
                    match args.get(i) {
                        Some(v) => {
                            exts = v
                                .split(',')
                                .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                                .filter(|e| !e.is_empty())
                                .collect()
                        }
                        None => return usage_err("find <dir> <pattern> --ext rs,toml"),
                    }
                }
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

    let re = if regex_mode {
        match regex::Regex::new(pattern, ignore_case) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("tarn: invalid regex: {e}");
                return EXIT_USAGE;
            }
        }
    } else {
        None
    };
    let mut files = collect_files(path);
    if !exts.is_empty() {
        files.retain(|f| {
            f.extension()
                .map(|e| {
                    exts.iter()
                        .any(|x| x == &e.to_string_lossy().to_lowercase())
                })
                .unwrap_or(false)
        });
    }
    if files.is_empty() {
        if exts.is_empty() {
            eprintln!("tarn: no readable files at {path}");
        } else {
            eprintln!("tarn: no files matching --ext under {path}");
        }
        return EXIT_NOT_FOUND;
    }
    let multi = files.len() > 1;

    // Pure-count fast path, handled up front so it can parallelize: memory-map
    // each file and count distinct matching lines, fanning ACROSS files over
    // cores for a directory, or splitting a single big file WITHIN itself.
    // Binary / non-UTF-8 files are skipped, same as the read path below.
    if count_only && !regex_mode && can_fast_count(pattern, ignore_case, word) {
        let total = if files.len() == 1 {
            scan_count(&files[0], pattern, true).unwrap_or(0)
        } else {
            count_files_parallel(&files, pattern)
        };
        if total == 0 {
            return EXIT_NOT_FOUND;
        }
        if json {
            println!(
                "{{\"pattern\":{},\"total\":{}}}",
                render::jstr(pattern),
                total
            );
        } else {
            println!("{total}");
        }
        return EXIT_OK;
    }

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
        // Only materialize the line vector when we need random access to
        // neighbours (context lines). The common path iterates lazily, which
        // skips a Vec<&str> allocation of one pointer per line.
        let lines: Vec<&str> = if want_ctx {
            content.lines().collect()
        } else {
            Vec::new()
        };
        let mut file_hit = false;
        for (idx, line) in content.lines().enumerate() {
            let hit = match &re {
                Some(r) => r.is_match(line),
                None => line_matches(line, pattern, ignore_case, word),
            };
            if !hit {
                continue;
            }
            total += 1;
            file_hit = true;
            if files_only {
                break; // one hit is enough to name the file
            }
            if !count_only && matches.len() < limit {
                let scope = defs
                    .as_ref()
                    .and_then(|d| structure::qualified_in(d, idx + 1));
                let (before, after) = if want_ctx {
                    let (lo, hi) = context_bounds(lines.len(), idx, ctx_before, ctx_after);
                    (
                        (lo..idx).map(|j| (j + 1, lines[j].to_string())).collect(),
                        ((idx + 1)..hi)
                            .map(|j| (j + 1, lines[j].to_string()))
                            .collect(),
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
            println!(
                "{{\"pattern\":{},\"total\":{}}}",
                render::jstr(pattern),
                total
            );
        } else {
            println!("{total}");
        }
        return EXIT_OK;
    }
    // -l / --files: just the names of files that matched.
    if files_only {
        if json {
            let items: Vec<String> = matched_files.iter().map(|f| render::jstr(f)).collect();
            println!(
                "{{\"pattern\":{},\"files\":[{}]}}",
                render::jstr(pattern),
                items.join(",")
            );
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
        print!(
            "{}",
            render::find_view(pattern, &matches, shown_files, color)
        );
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
    let b = if end < starts.len() {
        starts[end]
    } else {
        content.len()
    };
    Some((a, b))
}

/// Zero-allocation substring search over bytes. `ci` = ASCII case-insensitive;
/// `word` = whole-word only (boundaries are any non-`[A-Za-z0-9_]` byte, or the
/// line edge). A UTF-8 needle matches the same byte sequence; case-folding is
/// ASCII-only (the documented behavior of `-i`). Callers search line-by-line
/// with the terminator stripped, so CRLF is normalized — a literal `\r` won't match.
/// Does `line` match `pat`? The hot path for `find`. The common case
/// (case-sensitive substring) defers to std's `str::contains`, which uses the
/// Two-Way algorithm with a word-at-a-time skip — far faster than a naive
/// byte-by-byte scan when the first byte is common (e.g. searching `function`).
/// Case-insensitive and whole-word keep the explicit, proven `find_in` scan.
/// Whether `find -c` may use the whole-buffer fast count path. It must match the
/// per-line semantics of every other `find` mode (which scan `content.lines()`,
/// i.e. `\r`/`\n`-stripped lines), so it's only safe for a plain, case-sensitive,
/// non-empty pattern that itself contains no `\r`/`\n` — a pattern with an
/// embedded line terminator can't match any single stripped line, and the raw
/// buffer would count it differently. Everything else falls back to the line scan.
fn can_fast_count(pat: &str, ci: bool, word: bool) -> bool {
    !ci && !word && !pat.is_empty() && !pat.contains(['\r', '\n'])
}

/// Count distinct matching lines straight off raw bytes, splitting a large file
/// across CPU cores. Returns `None` if the bytes aren't valid UTF-8 (the file
/// should be skipped, exactly as the serial `from_utf8` path would).
///
/// Chunks start right after a `\n`, which is always a UTF-8 boundary (LF is a
/// 1-byte char, never part of a multibyte sequence) — so the whole buffer is
/// valid UTF-8 iff every chunk is, and a line never straddles a boundary. Each
/// thread therefore validates AND counts its own chunk independently, and the
/// sum is the exact serial answer. This parallelizes both the validation and the
/// scan, which is where std-only tarn can pull level with a single-file ripgrep.
/// Below the threshold (or on one core) it runs serially.
/// Count distinct matching lines in one file for `find -c`, memory-mapping it to
/// avoid a full-file copy and falling back to `fs::read` when mapping isn't
/// available (non-Unix, empty file, map failure). Returns `None` to skip the
/// file: a binary (NUL in the first 8 KiB) or anything not valid UTF-8 — exactly
/// what the regular read+`from_utf8` path skips.
/// `parallel` splits ONE file's scan across cores (for a single big file). When
/// the caller is already fanning out across many files (a directory), it passes
/// `false` so each file is counted serially on its worker thread — no thread
/// oversubscription.
fn scan_count(path: &Path, pat: &str, parallel: bool) -> Option<usize> {
    let count = |bytes: &[u8]| -> Option<usize> {
        if bytes.iter().take(8192).any(|&x| x == 0) {
            return None; // binary → skip, like the read path
        }
        if parallel {
            count_matching_lines_par_bytes(bytes, pat)
        } else {
            Some(count_matching_lines(std::str::from_utf8(bytes).ok()?, pat))
        }
    };
    // `mmap` only pays off on big files: the map/unmap syscalls cost more than
    // just reading a small file, which dominates when scanning a tree of many
    // small files. So map only above a threshold; otherwise read. (ripgrep makes
    // the same large-file-only choice.)
    #[cfg(unix)]
    const MMAP_MIN: u64 = 256 * 1024;
    #[cfg(unix)]
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) >= MMAP_MIN {
        if let Some(map) = mmap::Mapped::open(path) {
            return count(map.bytes());
        }
    }
    count(&fs::read(path).ok()?)
}

/// Count one file for the directory fast path: read it (no `mmap`/stat — these
/// are the many-small-files most of a tree is, where the map/unmap syscalls
/// don't pay off), skip a binary (NUL in first 8 KiB) or non-UTF-8 file, and
/// SIMD-count the matching lines. `None` = skipped, contributing nothing.
fn count_file_read(path: &Path, pat: &str) -> Option<usize> {
    let bytes = fs::read(path).ok()?;
    if bytes.iter().take(8192).any(|&x| x == 0) {
        return None;
    }
    std::str::from_utf8(&bytes).ok()?; // skip non-UTF-8 files, like the read path
    Some(count_lines_bytes(&bytes, pat.as_bytes()))
}

/// Sum `find -c` across many files, fanning the files out over CPU cores with
/// `std::thread` — the part of a recursive search ripgrep parallelizes and tarn
/// previously did serially. Each worker counts its files serially (the count is
/// order-independent, so the threads just sum). Skipped files (binary/non-UTF-8)
/// contribute nothing, exactly as in the serial walk.
fn count_files_parallel(files: &[PathBuf], pat: &str) -> usize {
    let nthreads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(files.len())
        .max(1);
    let chunk = files.len().div_ceil(nthreads);
    std::thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|group| {
                s.spawn(move || {
                    group
                        .iter()
                        .filter_map(|f| count_file_read(f, pat))
                        .sum::<usize>()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or(0)).sum()
    })
}

fn count_matching_lines_par_bytes(bytes: &[u8], pat: &str) -> Option<usize> {
    const PAR_THRESHOLD: usize = 4 * 1024 * 1024; // 4 MiB
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if threads <= 1 || bytes.len() < PAR_THRESHOLD {
        let content = std::str::from_utf8(bytes).ok()?;
        return Some(count_matching_lines(content, pat));
    }

    // Cut the buffer into `threads` slices, each ending just after a newline.
    let approx = bytes.len() / threads;
    let mut bounds = vec![0usize];
    for k in 1..threads {
        let mut p = (approx * k).min(bytes.len());
        while p < bytes.len() && bytes[p] != b'\n' {
            p += 1;
        }
        if p < bytes.len() {
            p += 1;
        }
        bounds.push(p);
    }
    bounds.push(bytes.len());
    bounds.dedup();

    std::thread::scope(|s| {
        let handles: Vec<_> = bounds
            .windows(2)
            .map(|w| {
                let chunk = &bytes[w[0]..w[1]];
                s.spawn(move || {
                    std::str::from_utf8(chunk)
                        .ok()
                        .map(|c| count_matching_lines(c, pat))
                })
            })
            .collect();
        // Any chunk that isn't valid UTF-8 (or a panicked thread) → the whole
        // file is treated as non-text and skipped, matching the serial path.
        let mut total = 0;
        for h in handles {
            total += h.join().ok().flatten()?;
        }
        Some(total)
    })
}

/// Find the next byte `b` in `hay` at or after `from`. The hot primitive of the
/// search: on aarch64 it scans 16 bytes per step with a NEON compare (still
/// std-only — `core::arch` intrinsics, no crate); everywhere else it's a scalar
/// scan.
#[inline]
fn memchr_from(hay: &[u8], b: u8, from: usize) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is baseline on aarch64; only in-bounds reads are made.
        unsafe { memchr_neon(hay, b, from) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        hay.get(from..)?
            .iter()
            .position(|&x| x == b)
            .map(|p| from + p)
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
unsafe fn memchr_neon(hay: &[u8], b: u8, from: usize) -> Option<usize> {
    use std::arch::aarch64::*;
    let n = hay.len();
    if from >= n {
        return None;
    }
    let ptr = hay.as_ptr();
    let target = vdupq_n_u8(b);
    let mut i = from;
    // 16 bytes per iteration: compare-equal, then a horizontal max tells us if
    // any lane matched; only then do we locate the exact byte.
    while i + 16 <= n {
        let chunk = vld1q_u8(ptr.add(i));
        if vmaxvq_u8(vceqq_u8(chunk, target)) != 0 {
            for k in 0..16 {
                if *ptr.add(i + k) == b {
                    return Some(i + k);
                }
            }
        }
        i += 16;
    }
    while i < n {
        if *ptr.add(i) == b {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Count the distinct lines of `hay` that contain `needle` (case-sensitive
/// substring). The hot path for `find -c`: it scans for the needle's first byte
/// with a SIMD `memchr`, verifies full matches (cheap last-byte reject first),
/// and counts a new line whenever a `\n` falls between consecutive matches — so
/// several matches on one line count once. Callers gate this via
/// [`can_fast_count`]: `needle` is non-empty and free of `\r`/`\n`.
fn count_lines_bytes(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0; // guarded by can_fast_count; avoids the empty-needle degenerate
    }
    let llen = needle.len();
    let first = needle[0];
    let last = needle[llen - 1];
    let mut count = 0usize;
    let mut prev: Option<usize> = None;
    let mut i = 0usize;
    while i + llen <= hay.len() {
        let p = match memchr_from(hay, first, i) {
            Some(p) if p + llen <= hay.len() => p,
            _ => break,
        };
        // Leftmost, non-overlapping matches (like str::match_indices).
        if hay[p + llen - 1] == last && &hay[p..p + llen] == needle {
            let new_line = match prev {
                Some(pp) => hay[pp..p].contains(&b'\n'),
                None => true,
            };
            if new_line {
                count += 1;
            }
            prev = Some(p);
            i = p + llen;
        } else {
            i = p + 1;
        }
    }
    count
}

/// Count the distinct lines containing `pat` (the `&str` entry point; delegates
/// to the byte scanner so the same SIMD path is exercised everywhere).
fn count_matching_lines(content: &str, pat: &str) -> usize {
    count_lines_bytes(content.as_bytes(), pat.as_bytes())
}

#[inline]
fn line_matches(line: &str, pat: &str, ci: bool, word: bool) -> bool {
    if !ci && !word {
        return pat.is_empty() || line.contains(pat);
    }
    if !ci && word {
        // A whole-word match requires the substring to be present at all, so use
        // the fast Two-Way `contains` to skip the (majority) lines that don't
        // contain it, and only run the boundary scan where it could match.
        return pat.is_empty()
            || (line.contains(pat) && find_in(line.as_bytes(), pat.as_bytes(), false, true));
    }
    find_in(line.as_bytes(), pat.as_bytes(), ci, word)
}

fn find_in(hay: &[u8], needle: &[u8], ci: bool, word: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let eq = |a: u8, b: u8| {
        if ci {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    };
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

/// Count non-overlapping, word-boundary, case-sensitive occurrences of `needle`
/// in `hay`. Used by `refs` to tell a bare declaration line (one occurrence: the
/// name itself) from a line that ALSO uses the symbol (self-recursion, a
/// single-line `impl Body { ... Name ... }`), so the latter isn't dropped.
fn word_occurrences(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || needle.len() > hay.len() {
        return 0;
    }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut count = 0;
    let mut i = 0;
    'outer: while i + needle.len() <= hay.len() {
        for k in 0..needle.len() {
            if hay[i + k] != needle[k] {
                i += 1;
                continue 'outer;
            }
        }
        let end = i + needle.len();
        let before_ok = i == 0 || !is_word(hay[i - 1]);
        let after_ok = end == hay.len() || !is_word(hay[end]);
        if before_ok && after_ok {
            count += 1;
            i = end; // non-overlapping
        } else {
            i += 1;
        }
    }
    count
}

/// Files to search: a single file as-is, or every readable file under a
/// directory (skipping hidden entries and common build/vendor dirs).
/// Build/vendor directories skipped by every recursive walk, so output is
/// project code, not dependencies. Shared by `walk` (find/refs/rename/…) and
/// `tree` so they never drift.
fn is_vendor_dir(name: &str) -> bool {
    matches!(name, "target" | "node_modules" | "dist" | "build")
}

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
    // Use the directory entry's own file type — the `d_type` from `readdir`,
    // which costs no extra syscall on the common filesystems — instead of
    // `symlink_metadata` + `is_dir`, which stat every entry TWICE and dominated
    // the walk's cost on a big tree. Sort by path for deterministic output.
    let mut items: Vec<(std::fs::FileType, PathBuf)> = entries
        .flatten()
        .filter_map(|e| e.file_type().ok().map(|ft| (ft, e.path())))
        .collect();
    items.sort_by(|a, b| a.1.cmp(&b.1));
    for (ft, path) in items {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue; // .git, .venv, dotfiles
        }
        if ft.is_symlink() {
            continue; // don't follow symlinks — avoids loops and duplicate hits
        }
        if ft.is_dir() {
            if !is_vendor_dir(&name) {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// `replace <file> <N> <text> [--diff|--json] [--dry-run]`
fn cmd_replace(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);

    // Structural mode: `replace <file> --def <name>` — swap a whole definition
    // block for new text read from stdin. The symmetric write to `delete --def`.
    if let Some(name) = flags.def.clone() {
        let file = match flags.rest.first() {
            Some(f) => f.as_str(),
            None => return usage_err("replace <file> --def <name>   (new definition on stdin)"),
        };
        let old = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("tarn: cannot read {file}");
                return EXIT_NOT_FOUND;
            }
        };
        let (a, b) = match def_block_range(file, &old, &name) {
            Ok(r) => r,
            Err((code, msg)) => {
                eprintln!("tarn: {msg}");
                return code;
            }
        };
        let mut input = String::new();
        if io::stdin().read_to_string(&mut input).is_err() {
            eprintln!("tarn: failed to read stdin");
            return EXIT_USAGE;
        }
        // Drop one trailing line terminator (\n, \r\n, or \r) so the piped block
        // doesn't leave a blank line after the spliced definition.
        let mut repl = input.as_str();
        if let Some(s) = repl.strip_suffix('\n') {
            repl = s;
        }
        if let Some(s) = repl.strip_suffix('\r') {
            repl = s;
        }
        if repl.is_empty() {
            eprintln!(
                "tarn: empty replacement — use `tarn delete {file} --def {name}` to remove a definition"
            );
            return EXIT_USAGE;
        }
        let repl = repl.to_string();
        let exp = flags.expect.clone();
        return apply_edit(
            file,
            "replace",
            &flags,
            |c| check_expect(&exp, textfile::range_text(c, a, b)),
            move |c| textfile::replace_range(c, a, b, &repl),
        );
    }

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

    // Line-number mode: a single line `N`, or a line range `A-B` whose lines are
    // replaced by `text` — which may itself span multiple lines.
    let (file, spec, text) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(s), Some(t)) => (f.as_str(), s.as_str(), t.as_str()),
        _ => return usage_err("replace <file> <N|A-B> <text>   (or: --match <anchor> <new-line>)"),
    };
    let (a, b) = match parse_range(spec) {
        Some(r) => r,
        None => {
            return usage_err("replace <file> <N|A-B> <text>   (or: --match <anchor> <new-line>)")
        }
    };
    let exp = flags.expect.clone();
    if a == b {
        apply_edit(
            file,
            "replace",
            &flags,
            |c| check_expect(&exp, textfile::line_at(c, a)),
            |c| textfile::replace(c, a, text),
        )
    } else {
        apply_edit(
            file,
            "replace",
            &flags,
            |c| check_expect(&exp, textfile::range_text(c, a, b)),
            |c| textfile::replace_range(c, a, b, text),
        )
    }
}

/// `insert <file> <after-N> <text> [...]`  (after-N = 0 inserts at the top)
fn cmd_insert(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let (file, after, text) = match (flags.rest.first(), flags.rest.get(1), flags.rest.get(2)) {
        (Some(f), Some(n), Some(t)) => match n.parse::<usize>() {
            Ok(n) => (f.as_str(), n, t.as_str()),
            Err(_) => {
                return usage_err("insert <file> <after-N> <text> [--diff|--json] [--dry-run]")
            }
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
    let exp = flags.expect.clone();

    // Resolve the target line range, either structurally (`--def <name>`,
    // whole-definition delete) or from an explicit `<A-B>`. Doing it up front
    // lets a missing/ambiguous def report the right exit code (1/2) instead of
    // routing through the edit guard (3).
    let (file, range): (&str, (usize, usize)) = if let Some(name) = flags.def.clone() {
        let file = match flags.rest.first() {
            Some(f) => f.as_str(),
            None => return usage_err("delete <file> --def <name> [--diff|--json] [--dry-run]"),
        };
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("tarn: cannot read {file}");
                return EXIT_NOT_FOUND;
            }
        };
        match def_block_range(file, &content, &name) {
            Ok(r) => (file, r),
            Err((code, msg)) => {
                eprintln!("tarn: {msg}");
                return code;
            }
        }
    } else {
        match (flags.rest.first(), flags.rest.get(1)) {
            (Some(f), Some(r)) => match parse_range(r) {
                Some(r) => (f.as_str(), r),
                None => return usage_err("delete <file> <A-B> [--diff|--json] [--dry-run]"),
            },
            _ => {
                return usage_err("delete <file> <A-B> | --def <name>  [--diff|--json] [--dry-run]")
            }
        }
    };

    apply_edit(
        file,
        "delete",
        &flags,
        |c| check_expect(&exp, textfile::range_text(c, range.0, range.1)),
        |c| textfile::delete(c, range.0, range.1),
    )
}

/// Resolve a definition NAME to its inclusive (start, end) line range for
/// structural edits. Returns an exit code alongside the message: not-found is
/// EXIT_NOT_FOUND, an ambiguous name (more than one definition) is EXIT_USAGE —
/// the caller should disambiguate with an explicit line range rather than have
/// tarn guess which to touch.
fn def_block_range(path: &str, content: &str, name: &str) -> Result<(usize, usize), (u8, String)> {
    let defs = structure::outline(path, content);
    let hits: Vec<&structure::Def> = defs.iter().filter(|d| d.name == name).collect();
    match hits.len() {
        0 => Err((EXIT_NOT_FOUND, format!("no definition named '{name}'"))),
        1 => Ok((hits[0].line, hits[0].end)),
        n => Err((
            EXIT_USAGE,
            format!("ambiguous: {n} definitions named '{name}' — use an explicit line range (A-B)"),
        )),
    }
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

/// `batch` — run a stream of tarn commands from stdin in ONE process, one
/// command per line, so an agent doing many operations pays the OS
/// process-spawn cost (the real bottleneck — tarn's own work is sub-millisecond)
/// ONCE instead of per command. Blank lines and `#` comments are skipped; a
/// `==> <command>` header is written to STDERR before each command so results
/// stay attributable while stdout remains pure command output (so a batched
/// `find --json` is cleanly machine-parseable). Exit code is the last non-zero
/// command's (0 if all succeed).
///
/// Example (one spawn, not five):
///     printf 'outline src/\ndefs Config src/\nreplace a.rs 3 X --expect Y\n' | tarn batch
fn cmd_batch(args: &[String]) -> u8 {
    if !args.is_empty() {
        return usage_err("batch   (commands on stdin, one per line)");
    }
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("tarn: failed to read stdin");
        return EXIT_USAGE;
    }
    let mut worst = EXIT_OK;
    let mut ran = 0usize;
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let argv = tokenize(trimmed);
        if argv.is_empty() {
            continue;
        }
        // Guard against `batch` inside `batch` (stdin is already consumed).
        if argv[0] == "batch" {
            eprintln!("tarn: batch: nested `batch` ignored");
            continue;
        }
        ran += 1;
        // Framing → stderr so stdout stays pure command output (clean --json).
        eprintln!("==> {trimmed}");
        let code = run(&argv);
        let _ = io::stdout().flush();
        if code != EXIT_OK {
            worst = code;
        }
    }
    eprintln!("tarn: batch ran {ran} command(s)");
    worst
}

/// Split a command line into argv, honouring single and double quotes so an
/// argument may contain spaces (e.g. `replace f 3 'a b c'`). No escape
/// sequences — quotes are the only grouping, which is all an agent needs.
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false; // distinguishes an empty quoted arg "" from no arg
    for c in line.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                c if c.is_whitespace() => {
                    if started {
                        out.push(std::mem::take(&mut cur));
                        started = false;
                    }
                }
                _ => {
                    cur.push(c);
                    started = true;
                }
            },
        }
    }
    if started {
        out.push(cur);
    }
    out
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
                // A failed `expect` is a guard failure (3); a malformed batch is
                // a usage error (2). Derived from the error's type, not its text.
                let code = if e.is_guard() { EXIT_GUARD } else { EXIT_USAGE };
                eprintln!("tarn: {path}: {}", e.message());
                return code;
            }
        }
    }

    write_and_report("apply", &changes, &flags)
}

/// Write a validated set of `(path, old, new)` changes atomically and report the
/// result (per-file diff, or a JSON summary; `op` names the operation in JSON).
/// Shared by `apply` and `patch`: validation is all-or-nothing, so the WRITE
/// phase is too — if a write fails mid-batch, the files already written are
/// restored (best effort) so the transaction never leaves a partial result.
/// Honours `--dry-run`.
fn write_and_report(op: &str, changes: &[(String, String, String)], flags: &EditFlags) -> u8 {
    if !flags.dry_run {
        let mut written: Vec<&(String, String, String)> = Vec::new();
        for change in changes {
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

    if flags.json {
        let files: Vec<(String, usize, usize)> = changes
            .iter()
            .map(|(p, o, n)| (p.clone(), o.lines().count(), n.lines().count()))
            .collect();
        print!("{}", render::apply_json(op, &files, flags.dry_run));
    } else {
        let multi = changes.len() > 1;
        for (path, old, new) in changes {
            if multi {
                let header = format!("── {path} ──");
                print!(
                    "{}",
                    if flags.color {
                        format!("\x1b[38;2;199;117;46m{header}\x1b[0m\n")
                    } else {
                        format!("{header}\n")
                    }
                );
            }
            if flags.unified {
                print!(
                    "{}",
                    render::diff_unified(old, new, &format!("a/{path}"), &format!("b/{path}"))
                );
            } else {
                print!("{}", render::diff(old, new, flags.color));
            }
        }
    }
    let _ = io::stdout().flush();
    EXIT_OK
}

/// `patch [--dry-run] [--diff] [--json]` — apply a unified diff from stdin.
/// Strict: a hunk applies only if its context/removed lines match the file
/// exactly (else a guard failure, exit 3). Multi-file diffs are atomic. File
/// creation (`--- /dev/null`) is supported; deletion is refused (tarn won't
/// remove files for you).
fn cmd_patch(args: &[String]) -> u8 {
    let flags = parse_edit_flags(args);
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("tarn: failed to read stdin");
        return EXIT_USAGE;
    }
    let files = match patch::parse(&input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("tarn: patch parse error: {e}");
            return EXIT_USAGE;
        }
    };

    // Validate + compute every file first; write nothing until all succeed.
    let mut changes: Vec<(String, String, String)> = Vec::new();
    for fp in &files {
        if fp.delete {
            eprintln!(
                "tarn: patch deletes {} — tarn won't remove files; delete it yourself",
                fp.path
            );
            return EXIT_USAGE;
        }
        let old = match (fp.create, fs::read_to_string(&fp.path)) {
            // A creation patch onto a file that already has content is a mistake.
            (true, Ok(c)) if !c.is_empty() => {
                eprintln!("tarn: patch creates {} but it already exists", fp.path);
                return EXIT_USAGE;
            }
            (true, _) => String::new(),
            (false, Ok(c)) => c,
            (false, Err(_)) => {
                eprintln!("tarn: cannot read {}", fp.path);
                return EXIT_NOT_FOUND;
            }
        };
        let (body, final_nl) = match patch::apply(&old, fp.hunks()) {
            Ok(n) => n,
            Err(e) => {
                // File drift (the file isn't what the diff expects) is a guard
                // failure; a structurally broken diff is a usage error.
                let code = if e.is_drift() { EXIT_GUARD } else { EXIT_USAGE };
                eprintln!("tarn: {}: {}", fp.path, e.message());
                return code;
            }
        };
        changes.push((
            fp.path.clone(),
            old.clone(),
            reframe_like(&old, body, final_nl),
        ));
    }
    if changes.is_empty() {
        return usage_err("patch [--dry-run|--diff|--json]   (unified diff on stdin)");
    }
    write_and_report("patch", &changes, &flags)
}

/// Re-apply `original`'s line ending and trailing-newline state to `lf_body`
/// (lines joined by `\n`, no trailing newline — what `patch::apply` returns), so
/// patching preserves CRLF and final-newline exactly like every other edit. A
/// newly created file (empty original) defaults to LF with a trailing newline.
fn reframe_like(original: &str, lf_body: String, final_nl_override: Option<bool>) -> String {
    let crlf = original
        .find('\n')
        .map(|i| i > 0 && original.as_bytes()[i - 1] == b'\r')
        .unwrap_or(false);
    // The patch dictates the trailing newline when it reaches EOF; otherwise the
    // untouched tail means we keep the original's state.
    let final_nl =
        final_nl_override.unwrap_or_else(|| original.is_empty() || original.ends_with('\n'));
    let ending = if crlf { "\r\n" } else { "\n" };
    let mut body = if crlf {
        lf_body.replace('\n', "\r\n")
    } else {
        lf_body
    };
    if final_nl && !body.is_empty() {
        body.push_str(ending);
    }
    body
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
    unified: bool,
    json: bool,
    dry_run: bool,
    color: bool,
    all: bool,
    expect: Option<String>,
    match_anchor: Option<String>,
    def: Option<String>,
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
    // `write` may create the file and fully replaces it from stdin, so the old
    // text is only the diff base — best-effort is fine. The line ops require a
    // readable existing file.
    let old = if op == "write" {
        read_lossy(file)
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
            render::edit_json(
                file,
                op,
                old.lines().count(),
                new.lines().count(),
                flags.dry_run
            )
        );
    } else if flags.diff || flags.dry_run {
        // A dry-run with no explicit output still previews via diff.
        if flags.unified {
            print!(
                "{}",
                render::diff_unified(old, new, &format!("a/{file}"), &format!("b/{file}"))
            );
        } else {
            print!("{}", render::diff(old, new, flags.color));
        }
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
    let mut unified = false;
    let mut color_pref: Option<bool> = None;
    let mut expect: Option<String> = None;
    let mut match_anchor: Option<String> = None;
    let mut def: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            // end of flags: everything after `--` is a positional value verbatim
            // (so a value like `--diff` can be passed as text)
            "--" => {
                rest.extend(args[i + 1..].iter().cloned());
                break;
            }
            "--diff" => diff = true,
            "--json" => json = true,
            "--dry-run" => dry_run = true,
            "--all" => all = true,
            "-u" | "--unified" => unified = true,
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
            "--def" => {
                i += 1;
                def = args.get(i).cloned();
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
        unified,
        expect,
        match_anchor,
        def,
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
/// Read a file for an edit that may also CREATE it: a missing file is an empty
/// string, but any OTHER error (not valid UTF-8, permission denied, …) is
/// propagated — so a caller that's about to rewrite the file (`set`/`unset`)
/// aborts instead of silently treating unreadable existing content as empty and
/// destroying it.
fn read_or_empty(path: &str) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// `read_or_empty` for callers that just want best-effort content and never
/// write back based on it (e.g. `write`, which fully replaces the file from
/// stdin and only uses the old text for the diff preview).
fn read_lossy(path: &str) -> String {
    read_or_empty(path).unwrap_or_default()
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
    tarn defs    <name> [path]   where <name> is DEFINED (go-to-definition) [--json]
    tarn refs    <name> [path]   USES of <name> w/ scope, excl. the def [--json]
    tarn tree    [path]          vendor-aware directory tree [--depth N --lines --json]
    tarn find    <path> <text>  search a file OR directory; hits with file+line
        -i (ignore case) | -w (whole word) | --enclosing | --limit N | --json
        -c/--count (just the number) | -l/--files (just filenames) | --ext rs,toml
        -C/-B/-A N  context lines (around / before / after each hit)
        --  <text>  search a pattern that starts with a dash
    tarn check   <file>         file-hygiene gate (0 clean / 1 issues)  [--json]
    tarn diff    <a> <b>        show how file a differs from b (0 same/1 differ)

  documents (non-interactive — for scripts & AI harnesses):
    tarn show    <file>         editor-style snapshot to stdout
        --lines A-B | --around N [--context K] | --head [K] | --tail [K] | --all
        --block N (the whole def at line N) | --highlight A-B | --json | --plain | --color
    tarn replace <file> <N> <text>        replace line N, or --def <name> to swap a whole def (stdin)
        --match <anchor> <new-line>       ...or the whole line containing <anchor> [--all]
    tarn insert  <file> <after-N> <text>  insert after line N (0=top)
    tarn delete  <file> <A-B>             delete line range, or --def <name> for a whole def  (alias: del)
    tarn write   <file>                   replace file from stdin
    tarn apply   [file]                   batch ops from stdin, atomically
    tarn batch                           run many tarn commands in one process (stdin)
    tarn patch                           apply a unified diff from stdin (atomic, strict)
        across files too: a `file <path>` line in the ops switches target
    tarn rename  <path> <old> <new>       whole-word rename in a file/dir
        --in <def> (within that def; first if names repeat) | --substring | --dry-run
    tarn json get <file> <path>           read a JSON value by path (a.b.0.c)
    tarn json set <file> <path> <value>   set it, preserving file formatting
    tarn json del <file> <path>           delete a member/element (comma-aware)
    tarn toml get <file> <path>           read a TOML value by path (a.b.c)
    tarn toml set <file> <path> <value>   set it, preserving comments + layout
    tarn toml del <file> <path>           delete a key (json/toml/yaml)
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

    #[test]
    fn line_matches_fast_path_agrees_with_find_in() {
        // The plain case routes through str::contains; must match find_in.
        assert!(line_matches("a function here", "function", false, false));
        assert!(!line_matches("nope", "function", false, false));
        assert!(line_matches("anything", "", false, false)); // empty matches
                                                             // ci / word still go through find_in
        assert!(line_matches("The PORT", "port", true, false));
        assert!(!line_matches("import socket", "port", false, true));
        // word fast path agrees with find_in across boundary cases
        for (line, pat) in [
            ("the port is", "port"),
            ("import socket", "port"),
            ("use_port(x)", "port"),
            ("port2 = 1", "port"),
            ("import port", "port"),
            ("nomatch", "port"),
            ("anything", ""),
        ] {
            assert_eq!(
                line_matches(line, pat, false, true),
                find_in(line.as_bytes(), pat.as_bytes(), false, true),
                "word mismatch for {line:?} / {pat:?}"
            );
        }
    }

    #[test]
    fn tokenize_handles_quotes_and_spaces() {
        assert_eq!(
            tokenize("find src/ TODO -c"),
            vec!["find", "src/", "TODO", "-c"]
        );
        assert_eq!(
            tokenize("replace f 3 'a b c'"),
            vec!["replace", "f", "3", "a b c"]
        );
        assert_eq!(tokenize("x \"two words\" y"), vec!["x", "two words", "y"]);
        assert_eq!(tokenize("  spaced   out  "), vec!["spaced", "out"]);
        assert_eq!(tokenize("replace f 3 ''"), vec!["replace", "f", "3", ""]); // empty quoted arg kept
        assert!(tokenize("   ").is_empty());
    }

    #[test]
    fn fast_count_gated_to_plain_line_safe_patterns() {
        // The fast count path only runs for plain, case-sensitive, non-empty
        // patterns with no embedded line terminator — otherwise it could disagree
        // with the per-line (content.lines()) semantics every other mode uses.
        assert!(can_fast_count("function", false, false));
        assert!(!can_fast_count("", false, false)); // empty → per-line path
        assert!(!can_fast_count("port", true, false)); // -i
        assert!(!can_fast_count("port", false, true)); // -w
        assert!(!can_fast_count("o\r", false, false)); // CR in pattern
        assert!(!can_fast_count("c\nd", false, false)); // LF in pattern (spans lines)
    }

    #[test]
    fn count_lines_bytes_matches_naive_oracle() {
        // The SIMD scanner must equal "lines containing the substring" for any
        // input. Deterministic pseudo-random fuzz over a tiny alphabet with lots
        // of newlines and needle near-misses; needles of length 1..4.
        fn naive(hay: &str, needle: &str) -> usize {
            hay.lines().filter(|l| l.contains(needle)).count()
        }
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut rng = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let alpha = [b'a', b'b', b'\n', b'a', b' ']; // newline-heavy, repetitive
        for _ in 0..3000 {
            let len = (rng() % 40) as usize;
            let hay: Vec<u8> = (0..len)
                .map(|_| alpha[(rng() % alpha.len() as u64) as usize])
                .collect();
            let hay = String::from_utf8(hay).unwrap();
            for nlen in 1..=4usize {
                let needle: String = (0..nlen)
                    .map(|_| [b'a', b'b', b' '][(rng() % 3) as usize] as char)
                    .collect();
                assert_eq!(
                    count_lines_bytes(hay.as_bytes(), needle.as_bytes()),
                    naive(&hay, &needle),
                    "hay={hay:?} needle={needle:?}"
                );
            }
        }
        // A multibyte haystack with an ASCII needle (NEON byte scan must be
        // UTF-8-agnostic but still correct on byte boundaries).
        assert_eq!(
            count_lines_bytes("café\nrésumé x\nplain".as_bytes(), b"x"),
            1
        );
    }

    #[test]
    fn par_bytes_count_matches_serial_and_skips_non_utf8() {
        // Serial path (below the parallel threshold).
        assert_eq!(
            count_matching_lines_par_bytes(b"a hit\nno\nhit\n", "hit"),
            Some(2)
        );
        // Not valid UTF-8 → None (file is skipped, like the serial from_utf8 path).
        assert_eq!(
            count_matching_lines_par_bytes(&[0x68, 0x69, 0xff, 0xfe, 0x0a], "hi"),
            None
        );
        // Parallel path: a >4 MiB buffer must equal the serial count exactly,
        // proving the per-chunk split (at newline boundaries) is faithful.
        let mut big = String::new();
        for i in 0..400_000 {
            if i % 10 == 0 {
                big.push_str("needle in this line of text\n");
            } else {
                big.push_str("just some filler text here padding\n");
            }
        }
        assert!(
            big.len() > 4 * 1024 * 1024,
            "must exceed the parallel threshold"
        );
        let expected = count_matching_lines(&big, "needle");
        assert_eq!(expected, 40_000);
        assert_eq!(
            count_matching_lines_par_bytes(big.as_bytes(), "needle"),
            Some(expected)
        );
    }

    #[test]
    fn count_matching_lines_counts_distinct_lines() {
        // Several matches on one line count once; matches on N lines count N.
        assert_eq!(count_matching_lines("aa aa aa\nbb\naa\n", "aa"), 2);
        assert_eq!(count_matching_lines("x\ny\nz\n", "q"), 0);
        assert_eq!(count_matching_lines("hit\nhit\nhit\n", "hit"), 3);
        // CRLF: \n still delimits, \r doesn't interfere.
        assert_eq!(count_matching_lines("a hit\r\nb\r\nhit two\r\n", "hit"), 2);
        // No trailing newline, match on the last line.
        assert_eq!(count_matching_lines("nope\nlast hit", "hit"), 1);
    }

    #[test]
    fn read_or_empty_allows_missing_but_refuses_unreadable() {
        // Missing file → empty (so `set` can create it).
        let missing = std::env::temp_dir().join("tarn_test_missing_zzz_42");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(read_or_empty(missing.to_str().unwrap()).unwrap(), "");
        // Existing but NOT valid UTF-8 → Err, so set/unset abort instead of
        // silently treating it as empty and overwriting (data-loss regression).
        let bad = std::env::temp_dir().join("tarn_test_nonutf8_42.bin");
        std::fs::write(&bad, [0x6b, 0x3d, 0xff, 0xfe, 0x0a]).unwrap(); // "k=" + bad bytes
        assert!(read_or_empty(bad.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn word_occurrences_counts_uses() {
        // Bare declaration: a single occurrence → refs skips the line.
        assert_eq!(word_occurrences(b"fn foo() {", b"foo"), 1);
        // Self-recursion on the def line: two occurrences → it's a real use.
        assert_eq!(word_occurrences(b"fn foo() { foo(); }", b"foo"), 2);
        // Single-line impl body using its own type repeatedly.
        assert_eq!(
            word_occurrences(b"impl Config { fn new() -> Config { Config } }", b"Config"),
            3
        );
        // Word-boundary: substrings don't count.
        assert_eq!(word_occurrences(b"import use_port port2 port", b"port"), 1);
        // Non-overlapping.
        assert_eq!(word_occurrences(b"aa aa aa", b"aa"), 3);
        assert_eq!(word_occurrences(b"nothing here", b"foo"), 0);
    }

    #[test]
    fn replace_routes_a_line_range_to_replace_range() {
        // The new `replace <file> A-B <text>` wiring: parse_range + replace_range.
        let (a, b) = parse_range("2-4").unwrap();
        let out = textfile::replace_range("one\ntwo\nthree\nfour\nfive\n", a, b, "A\nB").unwrap();
        assert_eq!(out, "one\nA\nB\nfive\n");
        // a bare number still routes to a single-line (n, n).
        assert_eq!(parse_range("3"), Some((3, 3)));
    }
}
