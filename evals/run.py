#!/usr/bin/env python3
"""tarn accuracy eval — measure structural-nav precision/recall against a golden corpus.

Drives the REAL `tarn` binary through `--json` (the exact surface an agent uses), so
this harness adds zero crate dependencies (Python stdlib only). It scores three
commands against hand-labeled ground truth:

  outline  — does it find the right defs, at the right line ranges?
  defs     — go-to-definition: does the query land on the right line?
  refs     — find-references: are the use-sites complete and clean (P/R)?

Corpus: evals/corpus/<lang>/<case>.expected.json  (+ the source file it names).
Run:    python3 evals/run.py            # scorecard + regression gate vs baseline.json
        python3 evals/run.py --update   # rewrite baseline.json from this run
        python3 evals/run.py --json     # machine-readable scorecard

Exit: 0 = at/above baseline · 1 = a metric regressed · 2 = harness/setup error.
"""
from __future__ import annotations
import json, os, subprocess, sys, glob

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(ROOT)
BASELINE = os.path.join(ROOT, "baseline.json")
# how far below baseline a metric may drift before it's a regression (float noise guard)
EPS = 0.005


def find_binary() -> str:
    cand = os.path.join(REPO, "target", "release", "tarn")
    if os.access(cand, os.X_OK):
        return cand
    cand_dbg = os.path.join(REPO, "target", "debug", "tarn")
    if os.access(cand_dbg, os.X_OK):
        return cand_dbg
    from shutil import which
    p = which("tarn")
    if p:
        return p
    sys.exit("error: no tarn binary — run `cargo build --release` first")


def run_json(binary: str, *args: str) -> dict:
    """Invoke tarn with --json and parse stdout. {} on empty/exit-1 (not-found)."""
    r = subprocess.run([binary, *args, "--json"], capture_output=True, text=True)
    out = r.stdout.strip()
    if not out:
        return {}
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        raise SystemExit(f"error: non-JSON from `tarn {' '.join(args)} --json`:\n{out[:400]}")


def f1(p: float, rc: float) -> float:
    return 0.0 if (p + rc) == 0 else 2 * p * rc / (p + rc)


def score_outline(expected: list, got: list) -> dict:
    """Match returned defs to expected by (name,kind) then name; measure name P/R and
    line-range exactness among matched pairs."""
    exp = list(expected)
    rem = list(got)
    matched = 0
    kind_ok = start_ok = end_ok = 0
    for e in exp:
        hit = None
        for g in rem:  # prefer exact name+kind
            if g["name"] == e["name"] and g.get("kind") == e.get("kind"):
                hit = g
                break
        if hit is None:
            for g in rem:  # fall back to name-only (kind wrong but located)
                if g["name"] == e["name"]:
                    hit = g
                    break
        if hit is not None:
            rem.remove(hit)
            matched += 1
            if hit.get("kind") == e.get("kind"):
                kind_ok += 1
            if hit.get("line") == e.get("line"):
                start_ok += 1
            if hit.get("end") == e.get("end"):
                end_ok += 1
    n_exp, n_got = len(exp), len(got)
    recall = matched / n_exp if n_exp else 1.0
    precision = matched / n_got if n_got else (1.0 if n_exp == 0 else 0.0)
    return {
        "name_recall": recall,
        "name_precision": precision,
        "name_f1": f1(precision, recall),
        "kind_acc": kind_ok / matched if matched else 1.0,
        "start_exact": start_ok / matched if matched else 1.0,
        "end_exact": end_ok / matched if matched else 1.0,
        "_w": n_exp,  # weight for aggregation = number of expected defs
    }


def score_defs(binary: str, src: str, queries: list) -> dict:
    """Each query expects a def at a specific line; hit if that line is among returned defs."""
    hits = 0
    for q in queries:
        got = run_json(binary, "defs", q["q"], src)
        lines = {d["line"] for d in got.get("defs", [])}
        if q["line"] in lines:
            hits += 1
    n = len(queries)
    return {"defs_acc": hits / n if n else 1.0, "_w": n}


def score_refs(binary: str, src: str, queries: list) -> dict:
    """Set precision/recall over expected use-line numbers per query (def excluded)."""
    tot_p = tot_r = 0.0
    for q in queries:
        got = run_json(binary, "refs", q["q"], src)
        got_lines = {u["line"] for u in got.get("uses", [])}
        exp_lines = set(q["lines"])
        tp = len(got_lines & exp_lines)
        p = tp / len(got_lines) if got_lines else (1.0 if not exp_lines else 0.0)
        r = tp / len(exp_lines) if exp_lines else 1.0
        tot_p += p
        tot_r += r
    n = len(queries)
    p = tot_p / n if n else 1.0
    r = tot_r / n if n else 1.0
    return {"refs_precision": p, "refs_recall": r, "refs_f1": f1(p, r), "_w": n}


def wmean(rows: list, key: str) -> float:
    num = sum(r[key] * r["_w"] for r in rows if key in r and r["_w"])
    den = sum(r["_w"] for r in rows if key in r and r["_w"])
    return num / den if den else 1.0


METRICS = [
    ("outline", "name_f1"), ("outline", "kind_acc"),
    ("outline", "start_exact"), ("outline", "end_exact"),
    ("defs", "defs_acc"),
    ("refs", "refs_f1"),
]


def main() -> int:
    update = "--update" in sys.argv
    as_json = "--json" in sys.argv
    binary = find_binary()

    cases = sorted(glob.glob(os.path.join(ROOT, "corpus", "*", "*.expected.json")))
    if not cases:
        sys.exit("error: no corpus cases under evals/corpus/<lang>/*.expected.json")

    per_lang: dict[str, dict[str, list]] = {}
    case_counts: dict[str, int] = {}
    for cpath in cases:
        spec = json.load(open(cpath))
        lang = spec.get("lang") or os.path.basename(os.path.dirname(cpath))
        case_counts[lang] = case_counts.get(lang, 0) + 1
        src = os.path.join(os.path.dirname(cpath), spec["file"])
        if not os.path.exists(src):
            sys.exit(f"error: corpus source missing: {src}")
        buckets = per_lang.setdefault(lang, {"outline": [], "defs": [], "refs": []})
        if "outline" in spec:
            got = run_json(binary, "outline", src).get("defs", [])
            buckets["outline"].append(score_outline(spec["outline"], got))
        if spec.get("defs"):
            buckets["defs"].append(score_defs(binary, src, spec["defs"]))
        if spec.get("refs"):
            buckets["refs"].append(score_refs(binary, src, spec["refs"]))

    # aggregate per language, then overall (macro-avg across languages)
    report: dict[str, dict[str, float]] = {}
    for lang, b in sorted(per_lang.items()):
        agg = {}
        for cmd, key in METRICS:
            if b[cmd]:
                agg[f"{cmd}.{key}"] = round(wmean(b[cmd], key), 4)
        report[lang] = agg
    overall = {}
    for cmd, key in METRICS:
        vals = [report[l][f"{cmd}.{key}"] for l in report if f"{cmd}.{key}" in report[l]]
        if vals:
            overall[f"{cmd}.{key}"] = round(sum(vals) / len(vals), 4)
    report["OVERALL"] = overall

    if as_json:
        print(json.dumps(report, indent=2))
    else:
        print_scorecard(report)

    if update:
        out = dict(report)
        # _meta pins the corpus size so a later run that DROPS a case (which could
        # silently raise OVERALL by removing a weak case) trips the gate.
        out["_meta"] = {"cases": case_counts, "total": sum(case_counts.values())}
        json.dump(out, open(BASELINE, "w"), indent=2, sort_keys=True)
        open(BASELINE, "a").write("\n")
        print(f"\n✓ baseline written → {os.path.relpath(BASELINE, REPO)}")
        return 0

    if not os.path.exists(BASELINE):
        print("\n! no baseline.json yet — run `python3 evals/run.py --update` to set one.")
        return 0

    base = json.load(open(BASELINE))
    regressions = []
    for scope, metrics in report.items():
        for k, v in metrics.items():
            b = base.get(scope, {}).get(k)
            if b is not None and v < b - EPS:
                regressions.append(f"  {scope} {k}: {v:.4f} < baseline {b:.4f}")
    # corpus must not shrink — a dropped case can't silently lift OVERALL
    base_cases = base.get("_meta", {}).get("cases", {})
    for lang, n in base_cases.items():
        cur = case_counts.get(lang, 0)
        if cur < n:
            regressions.append(f"  corpus shrank: '{lang}' has {cur} case(s) < baseline {n}")
    if regressions:
        print("\n✗ REGRESSION vs baseline:")
        print("\n".join(regressions))
        return 1
    print("\n✓ at or above baseline.")
    return 0


def print_scorecard(report: dict) -> None:
    cols = [k for _, k in [(c, f"{c}.{m}") for c, m in METRICS]]
    hdr = [f"{c}.{m}" for c, m in METRICS]
    w = max(12, max(len(h) for h in hdr))
    print(f"{'language':<12} " + " ".join(f"{h:>{w}}" for h in hdr))
    print("-" * (13 + (w + 1) * len(hdr)))
    for scope in [s for s in report if s != "OVERALL"] + ["OVERALL"]:
        row = report[scope]
        cells = []
        for h in hdr:
            cells.append(f"{row[h]:>{w}.3f}" if h in row else f"{'—':>{w}}")
        bold = "*" if scope == "OVERALL" else " "
        print(f"{bold}{scope:<11} " + " ".join(cells))


if __name__ == "__main__":
    sys.exit(main())
