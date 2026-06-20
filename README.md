<div align="center">

![tarn](assets/banner.svg)

[![CI](https://github.com/OneMuppet/tarn/actions/workflows/ci.yml/badge.svg)](https://github.com/OneMuppet/tarn/actions/workflows/ci.yml)
&nbsp;zero dependencies&nbsp;·&nbsp;124 tests

</div>

```
  ████▓▓▒▒░░   t a r n   ░░▒▒▓▓████
        the editor your agent wishes it had
            ·  by an agent, for agents  ·
```

**tarn is the editor your agent wishes it had.** A tiny terminal text editor you can
actually understand — no modes, no manual, the help is always on screen — and a
fast, **structural command-line toolkit built for AI agents**: get your bearings in a
repo (`tree`), map a file (`outline`), jump to where a symbol is defined (`defs`) or
used (`refs`), search with the enclosing definition of every hit (`find`), and read one
function by name (`peek`) — LSP-style navigation with no language server. Then edit by
*unit of meaning*: swap or delete a whole definition (`replace --def`, `delete --def`),
edit surgically with guards (`replace --expect`, `apply`), refactor (`rename`), patch
config without reflowing it (`json`/`toml`/`yaml`), and check your work (`check`) — all
deterministic, `--json`-chainable, and quicker than the system grep. Still zero
dependencies.

> _The name comes from **tarnish** — the slow aging of metal. Polished copper
> tarnishes and, given time, becomes a **patina**; tarn is the small, sharp sibling
> to [Patina](https://github.com/OneMuppet). (It's also, neatly, a clear mountain
> lake.)_

- **Zero dependencies.** Pure Rust, std only. Raw mode is done by shelling out to
  `stty` (always present) instead of pulling in a terminal crate.
- **Small and readable, with a fast path where it earns it.** Mostly a handful of
  small, commented modules. The search hot path adds real performance machinery —
  `mmap`, `std::thread` fan-out, and NEON SIMD — to go toe-to-toe with ripgrep;
  it's commented and isolated, not sprinkled everywhere.
- **An editor for humans, a toolkit for agents.** A real full-screen TUI when you
  open a file in a terminal; a precise, scriptable CLI — navigate, edit, refactor,
  verify — everywhere else, with meaningful exit codes and `--json` output.
- **Fast on purpose — and still zero dependencies.** `tarn find -c` memory-maps
  the file, scans with NEON SIMD (`core::arch`, not a crate), and counts across
  every core (`std::thread`). On a 10-core box it's ~1.3× **faster than ripgrep**
  on a single ~380 MB file (~42 vs ~57 ms), at parity across many small files, and
  ~45× the system grep. And `tarn batch` runs a whole session in one process —
  ~10,500 edits/sec (~34× over per-call spawn). (Reproducible — see
  [Performance](#performance).)

<div align="center">
<br>
<img src="assets/mascot.svg" alt="Cu, the tarn mascot" width="120">
<br>
<sub>meet <b>Cu</b> — the cursor come alive, copper slowly tarnishing to patina.<br>
(Cu, as in the element symbol for copper. yes, really.)</sub>
</div>

---

## For agents

Any agent can learn tarn's whole surface in one call:

```sh
tarn help --json        # machine-readable manifest: every command, usage, examples, exit codes
tarn help find          # focused help for one command
```

Drop [`AGENTS.md`](AGENTS.md) into your harness's context — it's a one-screen guide
to why and how to use tarn over `grep`/`sed`/`cat`, the everyday loop, and the
exit-code/`--json` conventions.

## Install

From a clone (works today, zero setup):

```sh
cargo install --path .      # builds + installs `tarn` to ~/.cargo/bin
```

From crates.io — the crate is named `tarn-cli` (the name `tarn` was taken), but
the command it installs is `tarn`:

```sh
cargo install tarn-cli      # once published
```

Or build and place it yourself:

```sh
cargo build --release       # binary at ./target/release/tarn
cp target/release/tarn ~/.local/bin/
```

Needs only a Rust toolchain — no other dependencies.

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

## Navigate a repo without reading it

For an agent, reading whole files just to find one thing burns context. This
suite gives you structure cheaply — orient in the repo, see a file's shape, then
jump straight to the symbol you need. LSP-style navigation, no language server.

```sh
tarn tree   src/ --lines       # vendor-aware file tree (skips node_modules/target/…)
tarn outline app.py            # a map of defs / classes / headings + line ranges
tarn outline src/ --depth 0    # a whole-REPO map (recursive, one pass), top-level only
tarn peek   app.py do_GET      # show JUST one definition, by name (no line counting)
tarn defs   handle_request src/ # go-to-definition: WHERE a symbol is defined, repo-wide
tarn refs   handle_request src/ # find-references: WHO uses it (excludes the def itself)
tarn show   app.py --block 27  # show the whole def at line 27 (any body line works)
tarn find   app.py 'send_'     # literal search; each hit with its line number
tarn find   src/   'send_'     # search a whole DIRECTORY (recursive), grouped by file
tarn find   app.py 'send_' --enclosing   # ...and the definition that contains it
tarn find   app.py 'send_' --json -i --limit 50
tarn find   src/   'send_' -c    # just the count (like grep -c)
tarn find   src/   'send_' -l    # just the filenames that match (like grep -l)
tarn find   app.py 'send_' -C 3  # each hit with 3 lines of context (-A/-B too)
tarn find   src/   port -w       # whole-word: matches `port`, not `import`/`use_port`
tarn find   src/   TODO --ext rs,toml   # only search .rs and .toml files (-t alias)
tarn find   app.py -- '--flag' # use -- to search a pattern that starts with a dash
```

**Fast on purpose.** `find` matches without allocating per line, skips binaries,
and parses each file's structure once (only when `--enclosing` needs it). For
counting (`-c`) it memory-maps, scans with NEON SIMD, and counts across all
cores — beating ripgrep on a single large file and at parity across many small
files, and far ahead of the system grep (see [Performance](#performance)).
And it hands you the enclosing definition, an outline, or a surgical edit, which
a raw scanner won't.

`find` takes a file *or a directory* — pointed at a dir it recurses (skipping
hidden entries and `target`/`node_modules`/`dist`/`build`, and non-text files),
grouping hits by file. `--json` hits carry their `file`, so results chain
straight into `show`/edits across the repo.

After an edit, a quick hygiene gate catches the junk edits tend to leave:

```sh
tarn check app.py     # exit 0 if clean, 1 if issues (--json for details)
tarn diff  a.py b.py  # compare two files (exit 0 same / 1 differ / 2 error)
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
deps). It uses extension-aware keyword patterns — Python, Rust, JS/TS, Go, Ruby,
PHP, Swift, Kotlin (incl. `data`/`suspend` modifiers), and class/type-level for
Java/C#/C/C++, plus Markdown `#` headings, with a keyword union as the fallback;
nesting depth comes from
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
tarn replace app.py --match 'PORT = 8000' 'PORT = 9090'   # by CONTENT, not line number
tarn insert  app.py 0 '#!/usr/bin/env python3'   # 0 = insert at the top
tarn delete  app.py 5-6                           # (alias: del)
tarn delete  src/main.rs --def old_helper         # delete a WHOLE definition by name
cat new_fn.rs | tarn replace src/main.rs --def old_helper --diff   # swap a whole def from stdin
some-generator | tarn write app.py --diff         # replace whole file from stdin
git diff | tarn patch --dry-run                   # apply a unified diff (context-matched, atomic, multi-file)
printf 'replace a 3 X\nfind b TODO -c\n' | tarn batch   # many commands in ONE process (no per-call spawn; ~34×)
```

```
    1   def greet(name):
-   2       print("hi " + name)
+   2       print(f"hi {name}!")
    3
  ⋯
```

`replace --match <anchor> <new-line>` edits **by content instead of line number**:
it rewrites the whole line containing `<anchor>`, which must be unique (otherwise
it exits 2 and lists the matching line numbers — pass `--all` to change them all,
or exit 1 if none match). No `find` first, and it survives line drift.

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
and takes `--dry-run`/`--diff`. `tarn json del <file> <path>` removes a member or
array element with comma-aware splicing so the result stays valid JSON. It's
hand-rolled, zero-dep, and JSON-only (a key that literally contains `.` isn't
addressable).

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

**Deleting keys.** `tarn toml del <file> <path>` and `tarn yaml del <file> <path>`
remove a key's whole line, preserving everything else (comments, order, siblings)
— the delete half of surgical config CRUD. They refuse (and leave the file
untouched) on the same unsupported targets as `set`.

```sh
tarn toml del Cargo.toml dependencies.unused
tarn yaml del deploy.yaml spec.replicas --diff
```

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

## Performance

tarn `find -c` is built to go toe-to-toe with ripgrep while keeping **zero crate
dependencies**. Three ideas, all std-only:

- **`mmap`** the file (libc FFI, no crate) so the scan reads straight from the
  page cache — no `fs::read` copy of the whole file.
- **`std::thread`** to count across every core. ripgrep searches a single file on
  one thread; tarn splits it. `\n` is always a UTF-8 boundary, so each chunk
  counts independently and the sum is exact — and a directory fans its files out
  across cores too.
- **SIMD** substring scan. On aarch64 the hot `memchr` uses NEON intrinsics
  (`core::arch` — std, not a crate), comparing 16 bytes per step; scalar fallback
  elsewhere.

Measured on a 10-core Apple Silicon laptop, counting lines containing `function`,
warm cache, median of 15 runs (figures bounce ±~15% run to run, so these are
representative, not cherry-picked):

| workload | `tarn find -c` | `ripgrep -c` | `ugrep -c` (system grep) |
| --- | --- | --- | --- |
| one ~380 MB file | **~42 ms** | ~57 ms | ~2.2 s |
| ~380 MB across 3,000 files | ~52 ms | ~48 ms | ~2.3 s |

On a **single large file** tarn is ~1.3× **faster than ripgrep** (mmap + NEON +
all cores). Across **many small files** it's at **parity** — ripgrep's tuned
per-file walk edges it ~1.1×. Both are **~45× faster than the system grep**, with
identical counts and **zero dependencies**. Reproduce it: build `--release` and
point `tarn find -c` and `rg -c` at the same target.

**Editing throughput.** An agent's edit-heavy session is bottlenecked by OS
process spawn (~3.3 ms/call), not tarn — its edit work is ~0.1–0.2 ms. `tarn
batch` runs a whole command stream in one process: **1000 edits in ~95 ms
(~10,500 edits/sec), ~34× faster** than 1000 separate invocations.

The structure pass behind `outline`/`defs`/`refs`/`peek` is separately fast:
allocation-free on the hot line scan (a byte-prefix keyword test per line, not a
`format!` per keyword per line) — parsing a 289 MB file dropped from ~10 s to
~1.5 s. And the diff renderer trims the common prefix/suffix so a one-line change
in a 40k-line file diffs in ~26 ms instead of ~7 s.

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
