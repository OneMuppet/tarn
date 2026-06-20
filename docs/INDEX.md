# Design note: an optional navigation index

Status: **proposed, not built.** Captures a four-agent design discussion plus a
Phase-0 measurement, so the decision and the plan are on record. Building it adds
the first persistent state tarn has ever had, so it's gated on an explicit go.

## The problem it solves

`find -c` already scans at memory bandwidth (mmap + NEON SIMD + all cores) — a
single scan can't get faster; it's physics. But `defs` / `refs` / `outline` work
differently: each call walks the tree and **re-reads and re-parses every file**.
Across a session an agent issues dozens of these against a tree that barely
changes, re-doing the same parse every time.

### Phase-0 measurement (frost-oak, 1,688 files, warm, median of 15)

| query | time | what it is |
| --- | --- | --- |
| `tree` (walk only — `d_type` stat, no read) | ~15 ms | the floor an index query hits |
| `defs` / `refs` / `outline` (walk + read + parse every file) | ~156 ms | today |

**~91% of a nav query (~142 ms) is re-reading and re-parsing files that didn't
change.** That is exactly what an index caches. An index-backed query is ~15 ms
(cheap walk + stat-verify + cached lookup) — **~10× per repeated query**.

Across a session with no edits between queries: 10 queries ~1,560 ms → ~290 ms
(5.4×); 20 queries → ~7.2×. (`find`, already at ~51 ms via SIMD, barely benefits
— the win is squarely navigation.)

The "10× faster" an agent actually feels is hiding in the *repetition*, not the
scan.

## Non-negotiable: accelerator, never authority

The index must never be trusted over the file. Every query stat-verifies each
file in the same walk that serves it; any drift → live re-parse of just that
file; a missing/corrupt/version-skewed index → today's full-scan behavior. This
makes the freshness guarantee statable in one sentence:

> Every entry served was `stat`-confirmed current in the same walk that served
> it; any file that drifted is re-read live.

A stale-but-confident result (a moved/deleted symbol) is worse than no index — it
breaks the trust that earns the tool its slot. So correctness dominates: the
index turns "read+parse all files" into "stat all (cheap) + read+parse only the
few that changed" — O(drift), not O(repo) — without ever risking a wrong answer.

## Storage & lifecycle

- **Central store**, not in-repo: `$XDG_CACHE_HOME/tarn` (→ `~/.cache/tarn`).
  tarn's own walker skips dotfiles, so an in-repo `.tarn/` would give zero
  locality benefit but invite accidental commits and pollute read-only/CI
  checkouts. Central also means parallel subagents share one warm index.
  `rm -rf ~/.cache/tarn` is always safe and total.
- **Keyed by the canonicalized absolute repo root** (resolve symlinks, so
  worktrees/`/tmp` checkouts don't alias). Not by git HEAD/tree — agents work on
  dirty, often non-git trees; git state is the wrong correctness primitive.
- **Per-file shards** so one file's edit never rewrites another's entry and
  parallel invocations don't contend. Atomic write-temp-then-`rename`; lock-free
  reads.
- A per-file manifest of `(size, mtime_ns, content_hash)`. `(size, mtime)` is the
  cheap hot-path check; the content hash is the tiebreaker only on a suspicious
  signal (coarse mtime, fast in-tick edits, `git checkout` resetting mtime).

## Updates: editor-as-indexer (hyper-efficient CRUD)

tarn has a single write chokepoint (`commit()`), and right after every edit it
already holds the new content in memory. So:

- **Write-through on tarn's own edits** (`replace`/`insert`/`delete`/`apply`/
  `patch`/`write`/`rename`/`--def`): re-run `structure::outline` on the in-memory
  new content (sub-ms for one file) and overwrite that shard, stamped with the
  post-write stat. No rescan — tarn's edits keep the index exact for free.
- **Lazy verify-or-refresh on external edits** (git pull, another editor): caught
  by the stat during the query walk; re-index just the drifted files.

## What to index (and what not to)

- **Phase 1 — structure cache only** (the big, proven win): per-file
  `Vec<Def>` from the existing `outline`, plus a `name → [(file,line,kind)]`
  symbol map. Makes `defs`/`outline` lookups and `refs`' enclosing-scope work
  cached. Zero new heuristics — reuses `structure::outline`.
- **Phase 2 — file-granular trigram postings** for `find`/`refs` candidate
  pruning, *only if* huge-repo `find` proves a bottleneck. `find` is already at
  bandwidth, so this is secondary. Avoid byte-offset/positional indexes — they
  bloat past the source size for no gain (the SIMD scanner locates the line).

## Zero-config UX

No `tarn index build`. Queries consult the store if present and writable, build
it opportunistically on first query over a tree above a size threshold (below it,
the live scan already wins), and degrade silently to today's scan when the store
is absent/unwritable/corrupt. Worst case is exactly today's behavior.

## Risks (from the skeptic's seat)

- **Silently-stale results** — mitigated by accelerator-never-authority +
  stat-verify + content-hash tiebreaker; degrade to scan on any doubt.
- **New failure surface** — corruption, disk-full, version skew, partial
  write-through. Each must fail *loud or safe* (re-parse from disk), never
  serve a wrong answer.
- **Secrets** — derived structure/postings in a cache dir are a smaller surface
  than an indexed file in the repo, but the index should avoid caching raw
  contents of obviously-sensitive files.
- **It may not be worth it for small/one-shot work** — hence the size threshold
  and transparent fallback; the index earns its keep only on repeated nav over a
  large, mostly-stable tree.

## Decision

Phase-0 says the win is real (~10× on repeated nav, ~91% of query time is
cacheable, never-stale). Recommendation: build **Phase 1** (structure cache)
behind the accelerator-not-authority guarantee; defer trigrams. Gate: explicit
go, because it's tarn's first persistent state.
