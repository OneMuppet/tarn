# Changelog

All notable changes to **tarn** are documented here. The format loosely follows
[Keep a Changelog](https://keepachangelog.com/); this project is pre-1.0, so the
surface may still shift.

## [Unreleased]

The current feature set, ahead of the first published release.

### Navigate (no language server)
- `tree` — fast, vendor-aware directory tree (skips hidden, `target`/`node_modules`/`dist`/`build`, symlinks); `--lines`, `--depth`, `--json`.
- `outline` — structural map of a file or whole directory (defs/classes/headings + line ranges).
- `defs` — go-to-definition: where a symbol is defined, across a file or repo.
- `refs` — find-references: word-boundary uses of a symbol with their enclosing scope, excluding the definition.
- `find` — literal search with `file:line`, optional enclosing definition, context, whole-word, count/files-only.
- `peek` — read one definition by name; `show` — windowed, line-numbered view for "opening" a file in chat.

### Edit (surgical, guarded, atomic)
- `replace` / `insert` / `delete` — line-addressed edits; `--expect` guard, `--dry-run`, `--diff`, `--json`.
- `replace --match` — content-addressed line replace (survives line drift).
- `delete --def` / `replace --def` — structural: remove or swap a whole definition by name.
- `apply` — atomic multi-file batch of ops, all-or-nothing with rollback.
- `patch` — apply a unified diff (`git diff | tarn patch`); strict on content, relocates hunks whose line numbers have drifted to where their context uniquely matches; refuses ambiguous/absent context.
- `rename` — whole-word or substring rename across a file or directory; `--in <def>` scopes it.
- `write` — replace a whole file from stdin.

### Config (format-preserving)
- `json` / `toml` / `yaml` — get/set/del by path, preserving formatting, comments, and key order.
- `get` / `set` / `unset` / `keys` — `.env`-style key=value editing.

### Verify
- `check` — hygiene gate (trailing whitespace, mixed indentation, mixed line endings).
- `diff` — compact, line-numbered diff between two files.

### Agent-native
- `tarn help --json` — a single-call manifest of every command, its usage, examples, and exit codes.
- Meaningful exit codes: `0` ok, `1` not-found, `2` usage, `3` guard (`--expect`) failed (POSIX `0`/`1`/`2` for `diff`).
- Line-ending (LF/CRLF) and trailing-newline state preserved on every edit.

### Performance (std-only, no SIMD; `mmap` via libc FFI)
- `find -c` memory-maps the file (`mmap` via libc FFI, no crate) and counts matching lines across all cores (`std::thread`); on a 10-core box it's at parity with ripgrep counting a single ~380 MB file (~60 ms each), ~1.5× behind ripgrep across many small files (its SIMD scan), and ~20–30× faster than the system grep. `\n` is always a UTF-8 boundary, so each chunk validates and counts independently and the sum is exact.
- The structure parse behind `outline`/`defs`/`refs`/`peek` is allocation-free on the hot path (~6× faster than before).
- The diff renderer trims the common prefix/suffix and runs LCS only on the differing middle — a one-line change in a 40k-line file went from ~7 s / ~6 GB to ~26 ms / ~7 MB.

### Quality
- Zero crate dependencies (std only). Raw mode via `stty`.
- Heavily tested (120+ tests) and gated by adversarial review; an input fuzz across all commands found and fixed a `.env` data-loss case and tightened exit-code discipline.
