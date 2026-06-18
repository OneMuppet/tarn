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
```

## License

MIT.
