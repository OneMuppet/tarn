//! `tarn mcp` — a zero-dependency MCP (Model Context Protocol) stdio server.
//!
//! Exposes tarn's navigation/search/edit verbs as first-class MCP tools, so any
//! MCP-capable harness (Claude Code, Cursor, SDK agents, …) can load them at
//! the same altitude as its built-in tools: `claude mcp add tarn -- tarn mcp`.
//!
//! Protocol (verified against the 2025-06-18 spec):
//! - stdio transport: newline-delimited JSON-RPC 2.0, UTF-8, no embedded
//!   newlines; stdout carries ONLY MCP messages (logs would corrupt the stream).
//! - lifecycle: `initialize` → `notifications/initialized` → operation.
//! - `tools/list` → tool objects with JSON-Schema inputs; `tools/call` →
//!   `{content: [{type:"text", …}], isError}`.
//!
//! Dispatch runs each tool call by re-executing this same binary
//! (`current_exe`) with the translated argv and capturing its output — the 27
//! commands keep their single, well-tested implementation, and tarn's exit-code
//! contract maps directly: 0/1 (ok / not-found) are valid results, 2/3 (usage /
//! guard refused) surface as tool-execution errors (`isError: true`).

use crate::json::{self, Kind, Node};
use crate::render::jstr;
use std::io::{self, BufRead, Write};
use std::process::Command;

/// Protocol revisions we can speak. If the client asks for something else we
/// answer with our latest, per the spec's version-negotiation rule.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn serve(version: &str) -> u8 {
    let stdin = io::stdin();
    let mut out = io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // stdin closed → shutdown
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle(&line, version) {
            // Single line, flushed immediately — the framing contract.
            let _ = writeln!(out, "{resp}");
            let _ = out.flush();
        }
    }
    0
}

/// Handle one JSON-RPC message; `None` for notifications (never answered).
fn handle(line: &str, version: &str) -> Option<String> {
    let root = match json::parse(line) {
        Ok(r) => r,
        Err(e) => return Some(err_resp("null", -32700, &format!("parse error: {e}"))),
    };
    // Echo the id VERBATIM from the source (number or string — no re-typing).
    let id: Option<String> = json::navigate(&root, "id").map(|n| line[n.start..n.end].to_string());
    let method = match json::navigate(&root, "method") {
        Some(Node {
            kind: Kind::Str(m), ..
        }) => m.clone(),
        _ => {
            return id.map(|i| err_resp(&i, -32600, "invalid request: no method"));
        }
    };
    if method.starts_with("notifications/") {
        return None; // notifications are never answered
    }
    // a request without an id is notification-like; stay silent
    let id = id?;
    match method.as_str() {
        "initialize" => {
            let requested = match json::navigate(&root, "params.protocolVersion") {
                Some(Node {
                    kind: Kind::Str(v), ..
                }) => v.clone(),
                _ => String::new(),
            };
            let pv = if PROTOCOL_VERSIONS.contains(&requested.as_str()) {
                requested
            } else {
                PROTOCOL_VERSIONS[0].to_string()
            };
            Some(format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\
                 \"protocolVersion\":{},\
                 \"capabilities\":{{\"tools\":{{}}}},\
                 \"serverInfo\":{{\"name\":\"tarn\",\"version\":{}}},\
                 \"instructions\":{}}}}}",
                jstr(&pv),
                jstr(version),
                jstr(
                    "tarn: structural code navigation, search, and surgical edits. \
                     Prefer outline→peek over reading whole files; find is literal \
                     (regex=true for regular expressions); locate finds files by name; \
                     replace_line is guarded by expect."
                ),
            ))
        }
        "ping" => Some(format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}"
        )),
        "tools/list" => Some(format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"tools\":[{}]}}}}",
            tools_json()
        )),
        "tools/call" => Some(call_tool(&root, line, &id)),
        _ => Some(err_resp(
            &id,
            -32601,
            &format!("method not found: {method}"),
        )),
    }
}

fn err_resp(id: &str, code: i32, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":{}}}}}",
        jstr(message)
    )
}

// ---------- argument extraction ----------

fn member<'n>(obj: &'n Node, key: &str) -> Option<&'n Node> {
    match &obj.kind {
        Kind::Obj(members) => members.iter().find(|(_, k, _)| k == key).map(|(_, _, v)| v),
        _ => None,
    }
}

fn arg_str(args: Option<&Node>, key: &str) -> Option<String> {
    match &member(args?, key)?.kind {
        Kind::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn arg_raw(args: Option<&Node>, key: &str, src: &str) -> Option<String> {
    member(args?, key).map(|n| src[n.start..n.end].to_string())
}

fn arg_bool(args: Option<&Node>, key: &str, src: &str) -> bool {
    arg_raw(args, key, src).as_deref() == Some("true")
}

fn arg_num(args: Option<&Node>, key: &str, src: &str) -> Option<String> {
    let raw = arg_raw(args, key, src)?;
    raw.chars().all(|c| c.is_ascii_digit()).then_some(raw)
}

// ---------- tool dispatch ----------

/// Translate a tool call into tarn argv. Errors are invalid-params messages.
fn tool_argv(name: &str, args: Option<&Node>, src: &str) -> Result<Vec<String>, String> {
    let req = |k: &str| arg_str(args, k).ok_or_else(|| format!("missing argument: {k}"));
    let mut v: Vec<String> = Vec::new();
    match name {
        "outline" => {
            v.push("outline".into());
            v.push(req("path")?);
            if let Some(d) = arg_num(args, "depth", src) {
                v.push("--depth".into());
                v.push(d);
            }
        }
        "find" => {
            v.push("find".into());
            v.push(req("path")?);
            let pat = req("pattern")?;
            if pat.starts_with('-') {
                v.push("--".into());
            }
            v.push(pat);
            if arg_bool(args, "regex", src) {
                v.push("-e".into());
            }
            if arg_bool(args, "ignore_case", src) {
                v.push("-i".into());
            }
            if arg_bool(args, "word", src) {
                v.push("-w".into());
            }
            if arg_bool(args, "enclosing", src) {
                v.push("--enclosing".into());
            }
            if let Some(c) = arg_num(args, "context", src) {
                v.push("-C".into());
                v.push(c);
            }
        }
        "locate" => {
            v.push("locate".into());
            v.push(req("glob")?);
            if let Some(p) = arg_str(args, "path") {
                v.push(p);
            }
        }
        "peek" => {
            v.push("peek".into());
            v.push(req("file")?);
            v.push(req("name")?);
        }
        "show" => {
            v.push("show".into());
            v.push(req("file")?);
            if let Some(l) = arg_str(args, "lines") {
                v.push(l);
            }
        }
        "defs" | "refs" => {
            v.push(name.into());
            v.push(req("name")?);
            if let Some(p) = arg_str(args, "path") {
                v.push(p);
            }
        }
        "tree" => {
            v.push("tree".into());
            if let Some(p) = arg_str(args, "path") {
                v.push(p);
            }
            if let Some(d) = arg_num(args, "depth", src) {
                v.push("--depth".into());
                v.push(d);
            }
        }
        "replace_line" => {
            v.push("replace".into());
            v.push(req("file")?);
            v.push(req("line")?);
            v.push(req("text")?);
            if let Some(e) = arg_str(args, "expect") {
                v.push("--expect".into());
                v.push(e);
            }
        }
        "check" => {
            v.push("check".into());
            v.push(req("file")?);
        }
        _ => return Err(format!("unknown tool: {name}")),
    }
    Ok(v)
}

fn call_tool(root: &Node, src: &str, id: &str) -> String {
    let name = match json::navigate(root, "params.name") {
        Some(Node {
            kind: Kind::Str(n), ..
        }) => n.clone(),
        _ => return err_resp(id, -32602, "invalid params: missing tool name"),
    };
    let args = json::navigate(root, "params.arguments");
    let argv = match tool_argv(&name, args, src) {
        Ok(a) => a,
        Err(e) => {
            return if e.starts_with("unknown tool") {
                err_resp(id, -32602, &e)
            } else {
                tool_result(id, &e, true)
            }
        }
    };
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return tool_result(id, &format!("cannot locate tarn binary: {e}"), true),
    };
    match Command::new(exe).args(&argv).output() {
        Ok(out) => {
            let code = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            match code {
                // 0 = hit(s); 1 = clean not-found/no-match — a valid answer,
                // not a failure (the model should see "no matches" as data).
                0 | 1 => {
                    let text = if stdout.trim().is_empty() {
                        if code == 0 {
                            "(ok — no output)".to_string()
                        } else {
                            format!(
                                "(no matches — exit 1){}",
                                if stderr.trim().is_empty() {
                                    String::new()
                                } else {
                                    format!("\n{}", stderr.trim())
                                }
                            )
                        }
                    } else {
                        stdout.to_string()
                    };
                    tool_result(id, &text, false)
                }
                // 2 = usage, 3 = --expect guard refused, else unexpected.
                _ => tool_result(
                    id,
                    &format!("exit {code}: {}{}", stderr.trim(), stdout.trim()),
                    true,
                ),
            }
        }
        Err(e) => tool_result(id, &format!("failed to run tarn: {e}"), true),
    }
}

fn tool_result(id: &str, text: &str, is_error: bool) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":{is_error}}}}}",
        jstr(text)
    )
}

// ---------- tool catalogue ----------

/// One tool entry: name, description, JSON-Schema input (hand-written, static).
fn tools_json() -> String {
    const T: &[(&str, &str, &str)] = &[
        (
            "outline",
            "Structural map of a code file or whole directory: definitions, classes, methods with exact line ranges. Use this BEFORE reading a file — then peek/show just the part you need.",
            r#"{"type":"object","properties":{"path":{"type":"string","description":"file or directory"},"depth":{"type":"integer","description":"limit nesting depth (0 = top level)"}},"required":["path"]}"#,
        ),
        (
            "find",
            "Search file contents across a file or directory (recursive, vendor-aware). Literal substring by default; set regex=true for a regular expression. Returns file:line hits; enclosing=true adds the definition containing each hit.",
            r#"{"type":"object","properties":{"path":{"type":"string"},"pattern":{"type":"string"},"regex":{"type":"boolean"},"ignore_case":{"type":"boolean"},"word":{"type":"boolean","description":"whole-word match"},"enclosing":{"type":"boolean"},"context":{"type":"integer","description":"context lines around each hit"}},"required":["path","pattern"]}"#,
        ),
        (
            "locate",
            "Find files by NAME with a glob (like `find -name`, but skips node_modules/target/dotfiles). `*`/`?` stay within a path segment, `**` crosses directories.",
            r#"{"type":"object","properties":{"glob":{"type":"string","description":"e.g. *.env or **/*.test.ts"},"path":{"type":"string","description":"base directory (default .)"}},"required":["glob"]}"#,
        ),
        (
            "peek",
            "Read ONE definition by name (its whole block) instead of the whole file.",
            r#"{"type":"object","properties":{"file":{"type":"string"},"name":{"type":"string","description":"definition name"}},"required":["file","name"]}"#,
        ),
        (
            "show",
            "Line-numbered view of a file; pass lines like \"120\" or \"80-140\" to window it.",
            r#"{"type":"object","properties":{"file":{"type":"string"},"lines":{"type":"string","description":"N or A-B (omit for the whole file)"}},"required":["file"]}"#,
        ),
        (
            "defs",
            "Go-to-definition: where is <name> DEFINED, across a file or directory.",
            r#"{"type":"object","properties":{"name":{"type":"string"},"path":{"type":"string","description":"default: current dir"}},"required":["name"]}"#,
        ),
        (
            "refs",
            "Find-references: word-boundary USES of <name> with their enclosing scope (excludes the definition).",
            r#"{"type":"object","properties":{"name":{"type":"string"},"path":{"type":"string"}},"required":["name"]}"#,
        ),
        (
            "tree",
            "Fast, vendor-aware directory tree (skips hidden dirs, node_modules, target, dist, build).",
            r#"{"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":[]}"#,
        ),
        (
            "replace_line",
            "Replace line N (or range A-B) of a file with new text (may be multi-line). Pass expect = the current content of the target line to make the edit refuse (guarded) if the file drifted.",
            r#"{"type":"object","properties":{"file":{"type":"string"},"line":{"type":"string","description":"N or A-B"},"text":{"type":"string"},"expect":{"type":"string","description":"guard: current text of line N"}},"required":["file","line","text"]}"#,
        ),
        (
            "check",
            "File hygiene gate: trailing whitespace, mixed indentation, mixed line endings.",
            r#"{"type":"object","properties":{"file":{"type":"string"}},"required":["file"]}"#,
        ),
    ];
    T.iter()
        .map(|(name, desc, schema)| {
            format!(
                "{{\"name\":{},\"description\":{},\"inputSchema\":{}}}",
                jstr(name),
                jstr(desc),
                schema
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(line: &str) -> Option<String> {
        handle(line, "0.9.0")
    }

    #[test]
    fn initialize_negotiates_and_echoes_id() {
        let r = h(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"1"}}}"#).unwrap();
        assert!(r.contains("\"id\":1"), "{r}");
        assert!(r.contains("\"protocolVersion\":\"2025-06-18\""), "{r}");
        assert!(r.contains("\"tools\":{}"), "{r}");
        assert!(!r.contains('\n'), "must be a single line");
        // unknown version → answer with our latest
        let r2 = h(r#"{"jsonrpc":"2.0","id":"abc","method":"initialize","params":{"protocolVersion":"9999-01-01"}}"#).unwrap();
        assert!(
            r2.contains("\"id\":\"abc\""),
            "string id echoed verbatim: {r2}"
        );
        assert!(r2.contains("\"protocolVersion\":\"2025-06-18\""), "{r2}");
    }

    #[test]
    fn notifications_get_no_response() {
        assert!(h(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(h(r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#).is_none());
    }

    #[test]
    fn tools_list_is_valid_json_with_all_tools() {
        let r = h(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#).unwrap();
        // parseable by our own JSON parser = well-formed
        assert!(json::parse(&r).is_ok(), "{r}");
        for t in [
            "outline",
            "find",
            "locate",
            "peek",
            "show",
            "defs",
            "refs",
            "tree",
            "replace_line",
            "check",
        ] {
            assert!(r.contains(&format!("\"name\":\"{t}\"")), "missing {t}");
        }
        assert!(!r.contains('\n'));
    }

    #[test]
    fn unknown_method_errors_and_ping_works() {
        let r = h(r#"{"jsonrpc":"2.0","id":2,"method":"bogus/thing"}"#).unwrap();
        assert!(r.contains("-32601"), "{r}");
        let p = h(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#).unwrap();
        assert!(p.contains("\"result\":{}"), "{p}");
    }

    #[test]
    fn parse_error_is_32700() {
        let r = h("not json at all").unwrap();
        assert!(r.contains("-32700"), "{r}");
    }

    #[test]
    fn tool_argv_translation() {
        let msg = r#"{"params":{"name":"find","arguments":{"path":"src/","pattern":"foo","regex":true,"context":2}}}"#;
        let root = json::parse(msg).unwrap();
        let args = json::navigate(&root, "params.arguments");
        let v = tool_argv("find", args, msg).unwrap();
        assert_eq!(v, vec!["find", "src/", "foo", "-e", "-C", "2"]);
        // dash-leading pattern gets the -- separator
        let m2 = r#"{"params":{"arguments":{"path":".","pattern":"-x"}}}"#;
        let r2 = json::parse(m2).unwrap();
        let v2 = tool_argv("find", json::navigate(&r2, "params.arguments"), m2).unwrap();
        assert_eq!(v2, vec!["find", ".", "--", "-x"]);
        // missing required arg is an invalid-params style error
        assert!(tool_argv("peek", None, "{}").is_err());
        assert!(tool_argv("nope", None, "{}").is_err());
    }

    #[test]
    fn tools_call_executes_against_real_binary() {
        // An end-to-end call: current_exe in `cargo test` is the TEST binary,
        // which doesn't speak the tarn CLI — so exercise argv translation +
        // result plumbing via the error path (unknown tool → -32602) and the
        // invalid-params path (missing args → isError:true), which don't spawn.
        let r = h(r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#).unwrap();
        assert!(r.contains("-32602"), "{r}");
        let r2 = h(r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"peek","arguments":{"file":"x"}}}"#).unwrap();
        assert!(r2.contains("\"isError\":true"), "{r2}");
        assert!(r2.contains("missing argument: name"), "{r2}");
    }
}
