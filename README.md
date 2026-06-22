<div align="center">

![tarn](assets/banner.svg)

[![CI](https://github.com/OneMuppet/tarn/actions/workflows/ci.yml/badge.svg)](https://github.com/OneMuppet/tarn/actions/workflows/ci.yml)
&nbsp;zero dependencies&nbsp;·&nbsp;165 tests

</div>

```
  ████▓▓▒▒░░   t a r n   ░░▒▒▓▓████
        the editor your agent wishes it had
            ·  by an agent, for agents  ·
```

**tarn is the CLI built for AI agents.** A zero-dependency Rust binary for fast,
structural, surgical code **navigation, search, and editing** — the part of an
agent's toolchain that replaces the awkward grep/sed/cat/awk dance. It returns
**exact line numbers** and **meaningful exit codes**, every read and edit speaks
`--json` so commands chain, and edits are **guarded and atomic** so they're safe
to script. Orient in a repo (`tree`), map a file (`outline`), jump to where a
symbol is defined (`defs`) or used (`refs`), read one function by name (`peek`),
search with the enclosing definition of every hit (`find`) — LSP-style
navigation with no language server — then edit by *unit of meaning* with a guard
that refuses if the target moved (`replace --expect`, exit 3). Still zero crate
dependencies.

> _The name comes from **tarnish** — the slow aging of metal. Polished copper
> tarnishes and, given time, becomes a **patina**; tarn is the small, sharp sibling
> to [Patina](https://github.com/OneMuppet). (It's also, neatly, a clear mountain
> lake.)_

- **An agent's toolchain, not a human's.** Structural where grep gives flat
  lines, surgical where sed reflows the file, self-describing where the others
  answer in prose. Built so an agent prefers it over `grep`/`sed`/`cat`/`awk`.
- **Self-describing in one call.** `tarn help --json` emits a machine-readable
  manifest of every command — usage, examples, exit codes — so an agent learns
  the whole surface without docs. Drop [`AGENTS.md`](AGENTS.md) into your harness.
- **Safe to script.** Exact 1-based line numbers, a stable exit-code contract
  (`0`/`1`/`2`/`3`), guarded edits (`--expect`), and atomic multi-file batches
  with rollback. An agent can act without a defensive re-read.
- **Fast on purpose, zero dependencies.** Pure Rust std — `mmap`, `std::thread`
  fan-out, and NEON SIMD via `core::arch`/libc FFI, no crates. `tarn find -c`
  goes toe-to-toe with ripgrep on a single large file; `tarn batch` runs a whole
  edit session in one process (~10,500 edits/sec). (See [Performance](#performance).)

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

Drop [`AGENTS.md`](AGENTS.md) into your harness's context — a one-screen guide to
**why and how to use tarn over `grep`/`sed`/`cat`/`awk`**, the everyday loop, and
the exit-code/`--json` conventions. For the design and the benchmarks behind it,
see the [whitepaper](whitepaper/tarn-whitepaper.html)
([PDF](whitepaper/tarn-whitepaper.pdf)).

### The everyday loop

```sh
tarn tree   src/                                  # 0. see the shape of the codebase
tarn outline src/ --depth 0                       # 1. map the repo without reading it
tarn defs   handle_request src/                   # 2. jump to where a symbol is defined
tarn refs   handle_request src/                   # 3. …and find everyone who uses it
tarn peek   src/server.rs handle_request          # 4. read one definition by name
tarn show   src/server.rs --around 42 --highlight 42   # 5. open a region in chat
tarn replace src/server.rs 42 'new line' --expect 'old line' --diff   # 6. edit, guarded
tarn check  src/server.rs                         # 7. verify you left no junk
```

Reading whole files just to find one thing burns context. This loop gives you
structure cheaply — orient, see a file's shape, jump straight to the symbol,
edit the one line — all deterministic and `--json`-chainable.

### Command quick-reference (26 commands)

| Task | Command |
| --- | --- |
| Orient in a repo | `tarn tree [path] [--depth N] [--lines] [--json]` |
| Map a file or repo | `tarn outline <path> [--depth N] [--json]` |
| Search (file or dir) | `tarn find <path> <pat> [-i -w -e/--regex -c -l --enclosing -A/-B/-C N --json]` (literal by default; `-e` = regex) |
| Read one definition | `tarn peek <file> <name>` |
| Go-to-definition | `tarn defs <name> [path] [--json]` |
| Find-references | `tarn refs <name> [path] [--json] [--limit N]` |
| Open a region | `tarn show <file> [--around N \| --block N \| --lines A-B] [--highlight A-B]` |
| Replace a line/range | `tarn replace <file> <N\|A-B> <text> [--expect T] [--diff\|--dry-run]` (range replaces lines A–B; text may be multi-line) |
| Replace by content | `tarn replace <file> --match <anchor> <new-line> [--all]` |
| Regex find/replace | `tarn replace <file> --regex <pat> <repl> [--all]` (per line; `$1`/`${1}` capture backrefs, `$$` = literal `$`) |
| Insert / delete | `tarn insert <file> <after-N> <text>` · `tarn delete <file> <A-B>` |
| Edit a whole def | `tarn delete <file> --def <name>` · `… \| tarn replace <file> --def <name>` |
| Rewrite a file | `… \| tarn write <file>` |
| Batch / cross-file edit | `… \| tarn apply [file]` (use `file <path>` lines; atomic) |
| Run a whole session | `… \| tarn batch` (many commands in one process — ~34× over per-call spawn) |
| Apply a unified diff | `git diff \| tarn patch [--dry-run\|--diff]` (context-matched, relocates drifted hunks, atomic) |
| Rename (whole-word) | `tarn rename <path> <old> <new> [--in <def>] [--dry-run]` |
| Read/set/del JSON config | `tarn json get\|set\|del <file> <path> [value]` |
| Read/set/del TOML config | `tarn toml get\|set\|del <file> <path> [value]` |
| Read/set/del YAML config | `tarn yaml get\|set\|del <file> <path> [value]` |
| `.env` key=value | `tarn get\|set\|unset\|keys <file> [KEY[=val]]` |
| Hygiene gate | `tarn check <file>` |
| Diff two files | `tarn diff <a> <b> [-u] [--stat]` (0 same / 1 differ / 2 error; `-u` = unified patch, `--stat` = magnitude) |
| Inspect / view | `tarn view <file> [--numbers]` |
| Print the agent guide | `tarn agents` (alias `guide`) — bundled in the binary, always matches the installed version |

### Exit codes (branch on these)

| code | meaning |
| --- | --- |
| `0` | success |
| `1` | key / file not found, or no matches |
| `2` | usage error |
| `3` | guard (`--expect`) failed — nothing written |

`diff` uses POSIX `0`/`1`/`2` (same / differ / error). `--json` on read commands
returns structured data; on edits it returns a result object you can chain.

## Install

These are live as of **v0.7.2**.

**Homebrew:**

```sh
brew install onemuppet/tap/tarn
```

**Prebuilt binary (no Rust toolchain needed)** — from the
[v0.7.2 release](https://github.com/OneMuppet/tarn/releases/tag/v0.7.2):

```sh
# macOS (Apple Silicon)
curl -L https://github.com/OneMuppet/tarn/releases/download/v0.7.2/tarn-v0.7.2-aarch64-apple-darwin.tar.gz | tar xz

# Linux (x86_64)
curl -L https://github.com/OneMuppet/tarn/releases/download/v0.7.2/tarn-v0.7.2-x86_64-unknown-linux-gnu.tar.gz | tar xz
```

Then put the extracted `tarn` on your `PATH`.

**From crates.io** — the crate is named `tarn-cli` (the name `tarn` was taken),
but the command it installs is `tarn`:

```sh
cargo install tarn-cli      # once published (crates.io publish pending)
```

**From source** — needs only a Rust toolchain, no other dependencies:

```sh
cargo install --path .      # builds + installs `tarn` to ~/.cargo/bin
cargo build --release       # or just build: binary at ./target/release/tarn
```

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
cores (see [Performance](#performance)). And it hands you the enclosing
definition, an outline, or a surgical edit, which a raw scanner won't.

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

`outline` also takes a **directory** (`tarn outline src/`): it maps every file in
one recursive pass, grouped by file — orient in an unfamiliar codebase without
opening a thing. `--depth N` limits nesting (`--depth 0` = top-level only).

> **Honest limit — structure is heuristic, not a parser.** tarn has no language
> parser (zero deps). It uses extension-aware keyword + brace/indentation patterns —
> Python, Rust, JS/TS, Go, Ruby, PHP, Swift, Kotlin, and Java/C#/C/C++ (including
> their keyword-less `returnType name(...)` methods, constructors, and C++
> destructors), plus Markdown `#` headings, with a keyword union fallback. For
> braced languages it tracks `{`/`}` balance (ignoring braces in strings, char
> literals, comments, and Rust raw strings), so multi-line signatures report their
> full body range. Residual gaps: C# `@"..."` verbatim strings, and Python/Ruby
> bodies with unindented multi-line content. And `find` is **literal substring**
> (`-i` = ASCII case-insensitive); pass `-e`/`--regex` for a regular expression.

## Open & edit a document (for AI harnesses)

The interactive editor needs a real terminal, which an agent like Claude Code
doesn't have. So tarn gives an agent a way to **open a document right in the
conversation** and edit it precisely, entirely through stdout and exit codes.

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

**Edit by line** — surgical. Add `--diff` to print a line-numbered diff so the
change is reviewable inline:

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

`tarn batch` runs a whole **command stream** (not just edit ops) in one process,
so an edit-heavy session isn't paying OS process-spawn per call — ~10,500
edits/sec, ~34× over per-call spawn (see [Performance](#performance)). That figure is *small-file* throughput — each op is a full file read+write, so it scales inversely with file size; for many edits to one large file use `apply`/`patch`, which apply all ops in a single pass.

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
so a local rename never touches a same-named identifier elsewhere. Exit 1 if
there were no occurrences. `--json` reports `{from,to,word,total,files:[…]}`.

### Verify your work

```sh
tarn check app.py     # exit 0 if clean, 1 if issues (--json for details)
tarn diff  a.py b.py       # compare two files (exit 0 same / 1 differ / 2 error)
tarn diff  a.py b.py -u    # ...as a standard unified patch (git apply / tarn patch ready)
```

`check` flags trailing whitespace, indentation that mixes tabs and spaces, mixed
line endings, a missing final newline, a BOM, and trailing blank lines — all
reliable, parser-free checks. It deliberately does **not** balance braces/quotes
(that needs a real parser and would false-positive on strings and comments).

## Edit config by path (format-preserving)

Agents edit config constantly and usually clobber it — a full reparse/reserialize
reorders keys and reflows the file. tarn instead edits **surgically**: it locates
just the target value's byte span and splices it, leaving every other byte
(whitespace, key order, layout) identical. Same model for **JSON, TOML, and YAML**.

```sh
tarn json get config.json server.port        # 8000   (strings come back decoded)
tarn json set config.json server.port 9090   # number stays a number
tarn json set config.json tags '["x","y"]' --diff   # valid JSON is used verbatim
tarn json del config.json deprecated         # comma-aware splice keeps it valid

tarn toml get Cargo.toml package.version          # "0.1.0"  (strings decoded)
tarn toml set Cargo.toml package.version 0.2.0    # → version = "0.2.0"  (auto-quoted)
tarn toml set pyproject.toml tool.ruff.line-length 100
tarn toml del Cargo.toml dependencies.unused

tarn yaml get deploy.yaml spec.replicas        # 3
tarn yaml set deploy.yaml spec.replicas 5 --diff
tarn yaml set .github/workflows/ci.yml jobs.build.timeout-minutes 30
tarn yaml del deploy.yaml spec.replicas --diff
```

All three are hand-rolled and zero-dep. Paths are dotted across nested
tables/mappings; comments, key order, and layout are untouched. Genuine bare
values stay bare and anything else is quoted, so the result is always valid.
Unsupported targets (multiline strings, arrays-of-tables, YAML sequences/flow
collections/block scalars/anchors, multi-document streams) are **tracked so
parsing never misreads them — but `set`/`del` on them errors rather than risk a
bad edit.** It never corrupts.

For `.env`-style files, the same surgical guarantee applies via `get`/`set`/
`unset`/`keys` — comments and blank lines survive every edit:

```sh
tarn get   .env DATABASE_URL      # print the value (exit 1 if missing)
tarn set   .env PORT=8080         # add or update PORT (space form works too)
tarn unset .env OLD_KEY           # remove it          (alias: rm)
tarn keys  .env                   # list keys, one per line (alias: list)
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
representative, not cherry-picked — identical counts across all three tools):

| workload | `tarn find -c` | `ripgrep -c` | `ugrep -c` (system grep) |
| --- | --- | --- | --- |
| one ~380 MB file | **~42 ms** | ~57 ms | ~2.2 s |
| ~380 MB across 3,000 files | ~52 ms | ~48 ms | ~2.3 s |

On a **single large file** tarn is ~1.3× faster than ripgrep (mmap + NEON + all
cores), and ~45× faster than the system grep. Across **many small files** it's at
**parity** — ripgrep's tuned per-file walk edges it slightly. Reproduce it: build
`--release` and point `tarn find -c` and `rg -c` at the same target.

**We don't claim "10× on one scan."** A single count is at the
memory-bandwidth ceiling — counting a 380 MB file takes about as long as merely
`cat`-ing it reads the bytes. There's no honest 10× to be had on one pass. The
real multiplier for an agent is **repeated navigation + batched edits**:

- **`tarn batch`** runs a whole command session in one process — ~10,500
  edits/sec, **~34× over per-call process spawn**. An agent's edit loop is
  bottlenecked by OS spawn (~3.3 ms/call), not by tarn's edit work (~0.1–0.2 ms).
  Caveat: that is *small-file* throughput. Each `batch` op is a full read+write, so it scales inversely with file size (measured: ~1,980 edits/s at 5k lines, ~178/s at 100k). For many edits to **one large file**, `tarn apply`/`patch` apply every op in a single pass — ~9,000 edits/s even at 100k lines.
- The structure pass behind `outline`/`defs`/`refs`/`peek` is allocation-free on
  the hot line scan — parsing a 289 MB file dropped from ~10 s to ~1.5 s.
- The diff renderer trims the common prefix/suffix so a one-line change in a
  40k-line file diffs in ~26 ms instead of ~7 s.

**Quality.** 26 commands, **165 tests**, gated by adversarial review on every
feature. The unsafe NEON path is **AddressSanitizer-clean** and the SIMD counter
is **differential-tested** against a scalar oracle (900+ fuzz cases); its counts
also match `rg`/`grep` on the benchmark corpus. Zero crate dependencies — std
only, with `mmap`, NEON SIMD, and threads via `core::arch`/libc FFI.

## Interactive editor (you can actually exit)

tarn does ship a real full-screen terminal editor — open a file in a terminal and
you get one. It's a secondary mode, not the headline: the headline is the agent
CLI above. But it exists, it's tiny, and — yes — you can quit it (`^Q`).

```sh
tarn notes.md        # opens the full-screen editor when stdout is a real terminal
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

No modes, no manual — the help is always on screen. The status bar shows the
filename (with a `*` when unsaved), your line:col, and the save/quit hints. A warm
**copper** accent echoes the Patina family; otherwise it stays in your terminal's
default colors. The editor only starts when stdout is a real terminal — pipe into
it or run it under a harness and `tarn` won't try to be a TUI; it prints the file
and points you at the subcommands.

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

## More

- [`AGENTS.md`](AGENTS.md) — the one-screen drop-in guide for your harness.
- [whitepaper](whitepaper/tarn-whitepaper.html) ([PDF](whitepaper/tarn-whitepaper.pdf)) — the design, the benchmarks, and an honest negative result.
- [`CHANGELOG.md`](CHANGELOG.md) — what's shipped.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to build, test, and contribute.

## License

MIT. See [`LICENSE`](LICENSE).
