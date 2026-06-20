# Contributing to tarn

Thanks for wanting to make tarn better. tarn is **the editor your agent wishes it
had** — fast, deterministic, and zero-dependency. Contributions are welcome; this
guide keeps them smooth.

## The one hard rule: zero crate dependencies

tarn depends on **nothing but the Rust standard library** (plus a tiny bit of
`libc`/`core::arch` FFI for mmap and SIMD, which ship with the toolchain). This is
deliberate — it's what makes tarn install anywhere, build in seconds, and stay
auditable. **A PR that adds a crate to `[dependencies]` will not be merged.** If you
think you need one, open an issue first and let's talk about doing it with `std`.

## Getting set up

```sh
git clone https://github.com/OneMuppet/tarn
cd tarn
cargo build --release      # binary at target/release/tarn
cargo test                 # run the suite
```

You need a recent stable Rust toolchain. No other system dependencies.

## Before you open a PR — run the gates locally

CI runs exactly these, and treats warnings as errors. Run them before pushing so the
green check is a formality:

```sh
cargo fmt --check                              # formatting
cargo clippy --all-targets -- -D warnings      # lints (warnings = errors)
cargo test                                     # the full suite
RUSTFLAGS="-D warnings" cargo build --release  # release build, warning-clean
```

If you touched `unsafe` (mmap, SIMD), build and test under AddressSanitizer too:

```sh
RUSTFLAGS="-Zsanitizer=address" cargo +nightly test --target aarch64-apple-darwin
```

## What makes a good change

- **Add a test.** Behavior changes need a test that fails before and passes after.
  tarn is differential-tested against the obvious tools — if you change `find`,
  `outline`, an edit command, etc., cover the new behavior and the edge cases.
- **Keep edits surgical.** tarn never reflows untouched lines and preserves CRLF;
  hold new code to the same standard.
- **Performance matters, but correctness first.** If a change claims to be faster,
  back it with a measurement in the PR description (what machine, what input, warm
  vs cold). We quote honest ranges, never cherry-picked best runs.
- **Match the surrounding style.** Read the neighboring code and mirror its naming,
  comment density, and idioms.
- **Update the docs you touched.** New command or flag → update `README.md`,
  `AGENTS.md`, the `--help`/`help --json` text, and `CHANGELOG.md`.

## Commit & PR conventions

- Small, focused commits with a clear subject line (imperative mood:
  "Add `--enclosing` to find", not "added stuff").
- Reference the issue you're closing (`Fixes #123`).
- The PR template will ask you to confirm the gates pass and no crate was added —
  please actually check the boxes.

## Reporting bugs & requesting features

Use the issue templates (Bug report / Feature request). For bugs, the single most
useful thing you can give us is a **minimal reproduction**: the exact `tarn` command,
the input file (or a tiny snippet of it), what you expected, and what happened —
including the **exit code** (`echo $?`). For features, tell us the *task* you're
trying to do, not just the flag you want; often tarn can already do it a different way.

## Questions

Open a [Discussion](https://github.com/OneMuppet/tarn/discussions) or a low-priority
issue. We'd rather talk early than review a big surprise PR.

By contributing, you agree your contributions are licensed under the MIT License
(see [LICENSE](LICENSE)).
