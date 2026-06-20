# Changelog

All notable changes to **tarn** are documented here. The format loosely follows
[Keep a Changelog](https://keepachangelog.com/); this project is pre-1.0, so the
surface may still shift.

## [0.2.0]

### Diff output (emit, not just apply)
- `tarn diff -u` / `--unified`, and `--diff -u` on edits (`replace`/`insert`/`delete`/`write`/`apply`) — emit a standard unified diff. CRLF- and no-final-newline-faithful; accepted by `git apply`, `patch`, and `tarn patch`.
- `tarn patch` now honors `\ No newline at end of file`, so `tarn diff -u | tarn patch` round-trips byte-for-byte (incl. trailing-newline flips).
- `tarn diff --stat` — change magnitude (`+ins -del`).

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
- `replace` / `insert` / `delete` — line-addressed edits; `--expect` guard, `--dry-run`, `--diff` (add `-u`/`--unified` to emit an applyable patch), `--json`.
- `replace --match` — content-addressed line replace (survives line drift).
- `delete --def` / `replace --def` — structural: remove or swap a whole definition by name.
- `apply` — atomic multi-file batch of ops, all-or-nothing with rollback.
- `patch` — apply a unified diff (`git diff | tarn patch`); strict on content, relocates hunks whose line numbers have drifted to where their context uniquely matches; refuses ambiguous/absent context. Honors `\ No newline at end of file`, so it round-trips `tarn diff -u` / `--diff -u` byte-faithfully.
- `rename` — whole-word or substring rename across a file or directory; `--in <def>` scopes it.
- `write` — replace a whole file from stdin.

### Config (format-preserving)
- `json` / `toml` / `yaml` — get/set/del by path, preserving formatting, comments, and key order.
- `get` / `set` / `unset` / `keys` — `.env`-style key=value editing.

### Verify
- `check` — hygiene gate (trailing whitespace, mixed indentation, mixed line endings).
- `diff` — compact, line-numbered diff between two files; `-u`/`--unified` emits a standard unified diff (CRLF- and no-final-newline-faithful) that `git apply`, `patch`, and `tarn patch` accept.

### Agent-native
- `tarn help --json` — a single-call manifest of every command, its usage, examples, and exit codes.
- Meaningful exit codes: `0` ok, `1` not-found, `2` usage, `3` guard (`--expect`) failed (POSIX `0`/`1`/`2` for `diff`).
- Line-ending (LF/CRLF) and trailing-newline state preserved on every edit.

### Performance (std-only: `core::arch` SIMD + `mmap` via libc FFI, zero crates)
- `find -c` memory-maps the file (`mmap`, libc FFI), scans with NEON SIMD on aarch64 (`core::arch` intrinsics, scalar fallback elsewhere), and counts across all cores (`std::thread`). On a 10-core box it is **~1.3× faster than ripgrep** counting a single ~380 MB file (~42 ms vs ~57 ms; figures vary ±~15%), is at parity across many small files (the directory walk now reads each entry's type from readdir instead of stat-ing twice), and is far ahead of the system grep. All zero crate dependencies. `\n` is always a UTF-8 boundary, so chunks count independently and the sum is exact.
- The structure parse behind `outline`/`defs`/`refs`/`peek` is allocation-free on the hot path (~6× faster than before).
- The diff renderer trims the common prefix/suffix and runs LCS only on the differing middle — a one-line change in a 40k-line file went from ~7 s / ~6 GB to ~26 ms / ~7 MB.

### Quality
- Zero crate dependencies (std only). Raw mode via `stty`.
- Heavily tested (124 tests) and gated by adversarial review; an input fuzz across all commands found and fixed a `.env` data-loss case and tightened exit-code discipline.
