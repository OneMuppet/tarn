<!-- Thanks for the PR! Keep it focused; small PRs get reviewed faster. -->

## What & why

<!-- What does this change, and what problem does it solve? Link the issue: Fixes #___ -->

## How I verified it

<!-- Tests added/changed, and any measurement if this is a perf claim
     (machine, input, warm vs cold). We quote honest ranges, not best runs. -->

## Checklist

- [ ] **No new crate dependency** (`[dependencies]` unchanged — std-only is a hard rule).
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` is clean.
- [ ] `cargo test` passes (added a test for any behavior change).
- [ ] `RUSTFLAGS="-D warnings" cargo build --release` is warning-clean.
- [ ] If I touched `unsafe` (mmap/SIMD), I ran it under AddressSanitizer.
- [ ] Updated docs I affected (`README.md`, `AGENTS.md`, `--help`/`help --json`, `CHANGELOG.md`).
