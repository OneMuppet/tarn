# tarn accuracy evals

tarn's structure layer is **heuristic, not a parser** (extension-keyed keywords +
indentation — see `src/structure.rs`). That trade buys speed and zero dependencies,
but it means `outline`/`defs`/`refs` can miss or mis-range definitions. This harness
turns that accuracy from a *vibe* into a **tracked number**, and gates CI so a change
can't silently regress it.

It is deliberately **dependency-free**: a single Python-stdlib script that drives the
real `tarn` binary through `--json` — i.e. it measures the exact surface an agent uses,
and adds nothing to the crate.

## Run

```sh
cargo build --release
python3 evals/run.py            # scorecard + regression gate vs baseline.json
python3 evals/run.py --json     # machine-readable
python3 evals/run.py --update   # accept current scores as the new baseline
```

Exit `0` = at/above baseline · `1` = a metric regressed **or the corpus shrank** ·
`2` = setup error. CI runs `python3 evals/run.py` after the release build
(`.github/workflows/ci.yml`).

> `--update` is a **human** action, run only after reviewing *why* the numbers moved.
> An agent must not "fix" a regression by rebaselining it away — that defeats the gate.
> The baseline also pins the per-language case count (`_meta.cases`), so deleting a
> weak case (which could silently lift `OVERALL`) trips the gate instead.

## What it measures

Per language, weighted across cases:

| Metric | Meaning |
| --- | --- |
| `outline.name_f1` | Did `outline` find the right definitions? (name precision/recall) |
| `outline.kind_acc` | Right kind (`fn`/`class`/`interface`…) among matched defs |
| `outline.start_exact` | Exact start line among matched defs |
| `outline.end_exact` | Exact **end** line — i.e. is the def's block range correct? |
| `defs.defs_acc` | `defs <name>` lands on the expected definition line |
| `refs.refs_f1` | `refs <name>` use-sites: precision/recall (definition excluded) |

`OVERALL` is the macro-average across languages.

## Add a case

Drop a source file under `corpus/<lang>/` and a sibling `<case>.expected.json`:

```json
{
  "lang": "rust",
  "file": "basic.rs",
  "outline": [{"name": "start", "kind": "fn", "line": 11, "end": 13}],
  "defs":    [{"q": "start", "line": 11}],
  "refs":    [{"q": "handle", "lines": [12]}]
}
```

Keep fixtures **small and unambiguous** — the `.expected.json` is the ground truth, so
a sloppy label measures the harness, not tarn. (`outline` lists every def incl. nested;
`refs.lines` are 1-based use-sites with the definition line omitted.) Then
`python3 evals/run.py --update` to fold it into the baseline.

## Known gaps this currently surfaces

The seed corpus (rust/python/js) already pins three real, reproducible weaknesses —
fix targets for the structure heuristic, now guarded against regression:

1. **Multi-line signatures truncate the block range** (`outline.end_exact`). A def
   whose params span lines (`fn long_signature(\n …\n) -> T {`) gets an `end` at the
   last param line, not the closing brace. (rust `long_signature`, python `fetch`.)
2. **Keyword-less methods are missed** (`outline`/`defs`). A TS/JS class method
   `find(id): User {` has no leading `function`, so it isn't detected. (js `find`.)
3. **Multi-line arrow bindings are missed** (`outline`/`defs`/`refs`). `const f = (\n …\n) => {`
   isn't recognized as a def — and its def line then leaks into `refs` as a false
   use. (js `fetchData`.)
