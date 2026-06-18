<div align="center">

![tarn](assets/banner.svg)

</div>

```
  ████▓▓▒▒░░   t a r n   ░░▒▒▓▓████
               a tiny, understandable
               terminal editor
```

**tarn** is a tiny terminal text editor you can actually understand — no modes, no
manual, the help is always on screen. It's also a scriptable `.env` tool, so an AI
harness (or a shell script) can read and edit key=value files deterministically
without ever driving a TUI.

> _The name comes from **tarnish** — the slow aging of metal. Polished copper
> tarnishes and, given time, becomes a **patina**; tarn is the small, sharp sibling
> to [Patina](https://github.com/OneMuppet). (It's also, neatly, a clear mountain
> lake.)_

- **Zero dependencies.** Pure Rust, std only. Raw mode is done by shelling out to
  `stty` (always present) instead of pulling in a terminal crate.
- **Readable on purpose.** A few small, commented modules — grok the whole thing
  in one sitting.
- **Two modes, one binary.** A human gets a friendly editor; a harness gets a
  precise key=value CLI with meaningful exit codes.

<div align="center">
<br>
<img src="assets/mascot.svg" alt="Cu, the tarn mascot" width="120">
<br>
<sub>meet <b>Cu</b> — the cursor come alive, copper slowly tarnishing to patina.<br>
(Cu, as in the element symbol for copper. yes, really.)</sub>
</div>

---

## Install

```sh
cargo build --release
# binary at ./target/release/tarn  — copy it onto your PATH
cp target/release/tarn ~/.local/bin/
```

## The editor

```sh
tarn notes.md        # opens the full-screen editor
```

| Key | Action |
| --- | --- |
| arrows, `Home`/`End`, `PageUp`/`PageDown` | move |
| printable keys | insert |
| `Enter` | split line |
| `Backspace` / `Delete` | delete |
| `Tab` | insert 4 spaces |
| `^S` | save |
| `^Q` | quit — if there are unsaved changes, press it twice |

The status bar always shows the filename (with a `*` when unsaved), your line:col,
and the save/quit hints. Discoverability is the point. A warm **copper** accent
echoes the Patina family; otherwise it stays in your terminal's default colors.

The editor only starts when stdout is a real terminal. If you pipe into it or run
it under a harness, `tarn` won't try to be a TUI — it prints the file and points
you at the subcommands below.

## Navigate a file without reading it

For an agent, reading a whole file just to find one thing burns context. These
two give you structure cheaply — see the shape, then jump to the part you need.

```sh
tarn outline app.py            # a map of defs / classes / headings + line ranges
tarn find   app.py 'send_'     # literal search; each hit with its line number
tarn find   src/   'send_'     # search a whole DIRECTORY (recursive), grouped by file
tarn find   app.py 'send_' --enclosing   # ...and the definition that contains it
tarn find   app.py 'send_' --json -i --limit 50
tarn find   app.py -- '--flag' # use -- to search a pattern that starts with a dash
```

`find` takes a file *or a directory* — pointed at a dir it recurses (skipping
hidden entries and `target`/`node_modules`/`dist`/`build`, and non-text files),
grouping hits by file. `--json` hits carry their `file`, so results chain
straight into `show`/edits across the repo.

```
$ tarn outline server.py
  3 │ class Handler        (3–9)
  4 │   def do_GET         (4–6)
 11 │ def main             (11–14)
```

Structure is **heuristic, not semantic** — tarn has no language parser (zero
deps). It uses extension-aware keyword patterns (`def`/`class`/`fn`/`func`/
`function`/`struct`/… and Markdown `#` headings) plus indentation for extent.
That nails the common case; a function with a multi-line signature may report a
slightly short end range. `find` is literal substring (`-i` for case-insensitive),
not regex. Both take `--json` so results chain straight into `show`/edits.

## Opening & editing documents (for AI harnesses)

The interactive editor needs a real terminal, which an agent like Claude Code
doesn't have — its commands' stdout just lands in the chat. So tarn gives an
agent a way to **open a document right in the conversation** and edit it
precisely, entirely through stdout and exit codes.

**Open a document** — an editor-style, windowed snapshot:

```sh
tarn show app.py                       # auto window (whole file if short)
tarn show app.py --lines 20-40         # a specific range
tarn show app.py --around 27 --context 4   # 4 lines either side of line 27
tarn show app.py --head 30   # or --tail 30, or --all
tarn show app.py --around 27 --highlight 27   # mark the line of interest
```

```
┌─ app.py ─ 7 lines ────────────────
  1 │ def greet(name):
▸ 2 │     print("hi " + name)
  3 │
└─ lines 1–3 · 4 below ─────────────
```

Color is on when stdout is a TTY and off otherwise (honoring `NO_COLOR`), so
harness-captured output stays clean; force it with `--plain` / `--color`.

**Edit by line** — surgical, like the `.env` commands but for any text. Add
`--diff` to print a line-numbered diff so the change is reviewable inline:

```sh
tarn replace app.py 2 '    print(f"hi {name}!")' --diff
tarn insert  app.py 0 '#!/usr/bin/env python3'   # 0 = insert at the top
tarn delete  app.py 5-6                           # (alias: del)
some-generator | tarn write app.py --diff         # replace whole file from stdin
```

```
    1   def greet(name):
-   2       print("hi " + name)
+   2       print(f"hi {name}!")
    3
  ⋯
```

Line numbers are 1-based and match `show`'s gutter, so an agent reads a number
off the view and edits exactly that line. Untouched lines are preserved
**byte-for-byte**: the file's line ending (LF or CRLF) and its trailing-newline
state are detected and kept, so an edit never reflows or normalizes a line it
didn't touch.

Two more flags make the edit commands agent-friendly:

- `--dry-run` computes the edit and previews the diff **without writing** — safe
  to propose a change and look before committing.
- `--json` (on `show` and the edits) emits machine-readable output instead of the
  rendered view, so an agent reasons on structure rather than scraping text:

```sh
tarn show app.py --around 27 --json
# {"path":"app.py","total":120,"window":[24,30],"highlight":null,"lines":[{"n":24,"text":"..."}, …]}
tarn replace app.py 27 'PORT = 9090' --json
# {"ok":true,"path":"app.py","op":"replace","before":120,"after":120,"dry_run":false}
```

### Edit safely: guards and atomic batches

`--expect <text>` turns a blind edit into a checked one: the edit only happens if
the target currently matches, otherwise tarn refuses and exits **3** without
touching the file. No more clobbering the wrong line after numbers drift — and no
defensive re-read first.

```sh
tarn replace app.py 27 'PORT=9090' --expect 'PORT=8000'   # applies only if line 27 == PORT=8000
```

`tarn apply` runs a **batch of edits atomically** from stdin. Every op is resolved
against the *original* line numbers (so order doesn't matter and numbers never
drift between ops), conflicts are rejected, and any failed `expect` aborts the
*whole* batch — all-or-nothing.

```sh
tarn apply app.py --diff <<'OPS'
expect 1 import os          # precondition for the whole batch
insert 0 #!/usr/bin/env python3
replace 4 DEBUG = False
delete  5-5
OPS
```

Ops: `expect <N> <text>`, `replace <N> <text>`, `insert <after-N> <text>`,
`delete <A-B>`. Blank lines and `#` comments are ignored. Combine with
`--dry-run`/`--json` like any edit.

## The scriptable side (for AI harnesses & scripts)

These are non-interactive and deterministic. Edits are **surgical**: comments,
blank lines, and ordering are always preserved — only the target key's line is
touched.

```sh
tarn get   .env DATABASE_URL      # print the value (exit 1 if missing)
tarn set   .env PORT=8080         # add or update PORT
tarn set   .env PORT 8080         # same thing, space form
tarn unset .env OLD_KEY           # remove it          (alias: rm)
tarn keys  .env                   # list keys, one per line (alias: list)
tarn view  .env                   # print the file     (alias: cat)
tarn view  .env --numbers         # ...with line numbers
```

**Exit codes** (so scripts can branch reliably):

| code | meaning |
| --- | --- |
| `0` | success |
| `1` | key / file not found |
| `2` | usage error |
| `3` | guard (`--expect`) failed |

### key=value semantics

A *key line* is `[whitespace][export ]KEY[whitespace]=value`, where `KEY` is made
of `A–Z a–z 0–9 _ .`. Everything else (comments, blanks) is opaque and untouched.

- **get** — value is the text after the first `=`, trimmed, with one layer of
  matching surrounding quotes (`'` or `"`) removed. If a key appears more than
  once, the last occurrence wins.
- **set** — if the key exists, its line is rewritten in place (an `export ` prefix
  is kept); otherwise `KEY=value` is appended. The value is written **verbatim** —
  you supply any quoting. The file always ends with a single trailing newline.
- **unset** — removes every line assigning that key.
- **keys** — unique keys in first-seen order.

Example — note how the comment and blank line survive an edit:

```sh
$ printf '# db\nHOST=localhost\n\nPORT=5432\n' > .env
$ tarn set .env PORT 8080
$ tarn view .env
# db
HOST=localhost

PORT=8080
```

## Design notes

- **Why `stty` instead of a crate?** It's always there, it's tiny, and it keeps
  the dependency list genuinely empty. `tarn` captures your settings with
  `stty -g`, switches to raw mode + the alternate screen, and restores everything
  on exit — including on panic (the binary deliberately avoids `panic = "abort"`
  so the terminal is never left broken).
- **The document model** is a `Vec<Vec<char>>`. A char-indexed cursor keeps UTF-8
  editing correct and the code simple.
- **Wide-character caveat:** v1 assumes one column per character. Wide CJK glyphs
  and emoji edit correctly but may render one column narrow in the on-screen
  cursor position. A future version can measure display width.

## Layout

```
src/main.rs       arg parsing → TUI or subcommand dispatch
src/terminal.rs   RawMode guard, size(), read_key(), Key
src/editor.rs     the full-screen editor
src/envfile.rs    surgical get/set/unset/keys
src/textfile.rs   line-addressable replace/insert/delete
src/render.rs     the `show` snapshot view + `--diff` renderer
src/structure.rs  heuristic outline / enclosing-scope detection
```

## License

MIT.
