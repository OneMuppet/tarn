<div align="center">

![tarn](assets/banner.svg)

</div>

```
  ████▓▓▒▒░░   t a r n   ░░▒▒▓▓████
        the editor your agent wishes it had
            ·  by an agent, for agents  ·
```

**tarn is the editor your agent wishes it had.** A tiny terminal text editor you can
actually understand — no modes, no manual, the help is always on screen — and a
fast, **structural command-line toolkit built for AI agents**: map a codebase (`outline`), search it with the
enclosing definition of every hit (`find`), read one function by name (`peek`),
edit it surgically with guards (`replace --expect`, `apply`), refactor
(`rename`), patch config without reflowing it (`json`), and check your work
(`check`) — all deterministic, `--json`-chainable, and quicker than the system
grep. Still zero dependencies.

> _The name comes from **tarnish** — the slow aging of metal. Polished copper
> tarnishes and, given time, becomes a **patina**; tarn is the small, sharp sibling
> to [Patina](https://github.com/OneMuppet). (It's also, neatly, a clear mountain
> lake.)_

- **Zero dependencies.** Pure Rust, std only. Raw mode is done by shelling out to
  `stty` (always present) instead of pulling in a terminal crate.
- **Readable on purpose.** A few small, commented modules — grok the whole thing
  in one sitting.
- **An editor for humans, a toolkit for agents.** A real full-screen TUI when you
  open a file in a terminal; a precise, scriptable CLI — navigate, edit, refactor,
  verify — everywhere else, with meaningful exit codes and `--json` output.
- **Fast on purpose.** Zero-allocation search, one structure-parse per file,
  binary-skipping — `find` beats the system grep with identical results.

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
tarn outline src/ --depth 0    # a whole-REPO map (recursive, one pass), top-level only
tarn peek   app.py do_GET      # show JUST one definition, by name (no line counting)
tarn show   app.py --block 27  # show the whole def at line 27 (any body line works)
tarn find   app.py 'send_'     # literal search; each hit with its line number
tarn find   src/   'send_'     # search a whole DIRECTORY (recursive), grouped by file
tarn find   app.py 'send_' --enclosing   # ...and the definition that contains it
tarn find   app.py 'send_' --json -i --limit 50
tarn find   src/   'send_' -c    # just the count (like grep -c)
tarn find   src/   'send_' -l    # just the filenames that match (like grep -l)
tarn find   app.py 'send_' -C 3  # each hit with 3 lines of context (-A/-B too)
tarn find   src/   port -w       # whole-word: matches `port`, not `import`/`use_port`
tarn find   app.py -- '--flag' # use -- to search a pattern that starts with a dash
```

**Fast on purpose.** `find` reads bytes, skips binaries, and matches without
allocating per line; `--enclosing` parses each file's structure once, not once
per hit. On a 14 MB / 1,200-file tree it's ~2.5× quicker than the system grep
with identical counts (a parallel-SIMD tool like ripgrep is still faster at raw
throughput — but it can't hand you the enclosing definition, an outline, or a
surgical edit).

`find` takes a file *or a directory* — pointed at a dir it recurses (skipping
hidden entries and `target`/`node_modules`/`dist`/`build`, and non-text files),
grouping hits by file. `--json` hits carry their `file`, so results chain
straight into `show`/edits across the repo.

After an edit, a quick hygiene gate catches the junk edits tend to leave:

```sh
tarn check app.py     # exit 0 if clean, 1 if issues (--json for details)
```

`check` flags trailing whitespace, indentation that mixes tabs and spaces, mixed
line endings, a missing final newline, a BOM, and trailing blank lines — all
reliable, parser-free checks. It deliberately does **not** balance braces/quotes
(that needs a real parser and would false-positive on strings and comments).

```
$ tarn outline server.py
  3 │ class Handler        (3–9)
  4 │   def do_GET         (4–6)
 11 │ def main             (11–14)
```

`outline` also takes a **directory** (`tarn outline src/`): it maps every file in
one recursive pass, grouped by file — orient in an unfamiliar codebase without
opening a thing. `--depth N` limits nesting (`--depth 0` = top-level only).

Structure is **heuristic, not semantic** — tarn has no language parser (zero
deps). It uses extension-aware keyword patterns (`def`/`class`/`fn`/`func`/
`function`/`struct`/… and Markdown `#` headings); nesting depth comes from
indentation/heading level, and a def's extent from indentation. That nails the
common case; a def whose body holds an unindented multi-line string may report a
short end range. `find` is literal substring (`-i` = ASCII case-insensitive),
not regex. Everything takes `--json` so results chain into `show`/edits.

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

**Across files, too.** A `file <path>` line switches the target, so one batch can
edit many files — and it stays **all-or-nothing across all of them**: every file's
ops are validated and computed *before any write*, so a failed `expect`/range/
conflict anywhere aborts the whole transaction with nothing written (exit 3 for
`expect`). If a write itself fails mid-batch (read-only target, disk full), the
files already written are rolled back (best effort) and the error says so.

```sh
tarn apply --diff <<'OPS'
file src/config.rs
expect 12 const PORT: u16 = 8000;
replace 12 const PORT: u16 = 9090;
file src/main.rs
insert 0 //! updated port
OPS
```

### Rename across a file or directory

`tarn rename` does a **whole-word** rename by default, so `port` never touches
`import` or `use_port`. Point it at a file or a directory (recursive), preview
with `--dry-run`, then run for real — line endings are preserved untouched.

```sh
tarn rename src/ oldName newName --dry-run   # preview: per-file counts, nothing written
tarn rename src/ oldName newName             # apply (computes all, then writes)
tarn rename config.ini old new --substring   # match anywhere, not just whole words
tarn rename app.py x n --in alpha             # only within the definition named `alpha`
```

`--in <def>` scopes the rename to a single named definition's body (per file),
so a local rename never touches a same-named identifier elsewhere.

Exit 1 if there were no occurrences. `--json` reports `{from,to,word,total,files:[…]}`.

### Edit JSON config by path (format-preserving)

Agents edit config constantly and usually clobber it — a full reparse/reserialize
reorders keys and reflows the file. `tarn json` instead edits **surgically**: it
locates just the target value's byte span and splices it, leaving every other
byte (whitespace, key order, layout) identical.

```sh
tarn json get config.json server.port        # 8000   (strings come back decoded)
tarn json get config.json tags.0             # array/object indices work: a.b.0.c
tarn json set config.json server.port 9090   # number stays a number
tarn json set config.json name prod          # a bare word is auto-quoted -> "prod"
tarn json set config.json tags '["x","y"]' --diff   # valid JSON is used verbatim
```

`get` exits 1 if the path is absent; `set` never creates paths (exit 1 if absent)
and takes `--dry-run`/`--diff`. It's hand-rolled, zero-dep, and JSON-only (a key
that literally contains `.` isn't addressable).

### …and TOML, the same way

`tarn toml get/set` does the same surgical, format-preserving edit for TOML —
ideal for `Cargo.toml`, `pyproject.toml`, and friends. Paths are dotted across
table headers and keys; comments, key order, and layout are untouched.

```sh
tarn toml get Cargo.toml package.version          # "0.1.0"  (strings decoded)
tarn toml set Cargo.toml package.version 0.2.0    # → version = "0.2.0"  (auto-quoted)
tarn toml set Cargo.toml profile.release.opt-level 2 --diff
tarn toml set pyproject.toml tool.ruff.line-length 100
```

Genuine bare values (numbers, bools, dates) stay bare; anything else (e.g. a
semver) is quoted so the result is always valid TOML. It handles `[table]`/
`[table.sub]` headers, dotted keys, and single-line values; multiline strings,
multiline arrays, and arrays-of-tables (`[[x]]`) are tracked so parsing never
breaks, but `set` on them errors rather than risk a bad edit (it never corrupts).

### …and YAML, for the config world agents live in

`tarn yaml get/set` brings the same surgical, format-preserving edit to YAML —
the format behind Kubernetes, GitHub Actions, docker-compose, and Ansible. Paths
are dotted across nested block mappings; comments, indentation, and key order are
untouched.

```sh
tarn yaml get deploy.yaml spec.replicas        # 3
tarn yaml set deploy.yaml spec.replicas 5 --diff
tarn yaml set .github/workflows/ci.yml jobs.build.timeout-minutes 30
```

It edits **block-mapping scalar values** (the overwhelming majority of config
keys); a value is quoted only when a plain scalar would be unsafe, so the result
is always valid YAML. Sequences (`- item`), flow collections (`[..]`/`{..}`),
block scalars (`|`/`>`), anchors/aliases, and multi-document streams are tracked
so parsing never misreads them — but `set` on them **errors rather than corrupt**.

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
src/toml.rs       surgical TOML get/set by path
src/yaml.rs       surgical YAML get/set by path (block mappings)
src/help.rs       agent-native manifest (`help --json`) + per-command help
```

## License

MIT.
