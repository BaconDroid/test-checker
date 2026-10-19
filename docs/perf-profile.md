# Performance profile — step 1 "profile first"

Goal: cut the per-invocation cost of the hook without changing behaviour. Budget is **p95 < 15 ms**,
startup included. This document is the *measurement* step: a per-stage breakdown, the top cost
centres, and a proposal. No rule or query was changed to produce it.

## Method

- Release binary (`cargo build --release -p test-checker`), timed **end-to-end per invocation**
  (process startup included) by piping each payload on stdin.
- Harness: 3 runs × 300 invocations per scenario, discard the first 10, report the best-p95 run
  (same shape as `eval/baseline.py`).
- `perf` is not usable on this box (`/proc/sys/kernel/perf_event_paranoid` = 2), so stage costs are
  **derived by differencing scenarios** (clearly labelled below) and corroborated by an in-process
  cold-vs-warm probe.
- The box is not isolated; best-of-3 plus an interleaved re-measurement window agreed within ±5 %.

## Scenarios (p50 / p95 ms, startup included)

| # | scenario | p50 | p95 | exit |
| - | -------- | --- | --- | ---- |
| 1 | `--version` (arg parse + print) | 2.19 | 3.14 | 0 |
| 2 | unknown tool `Read` | 2.24 | 3.43 | 0 |
| 3 | unknown extension `a.zzz` | 2.35 | 3.54 | 0 |
| 4 | non-test `src/app.py` → `skip("Non-test file")` | 2.26 | 3.51 | 0 |
| 5 | harness `pytest.ini` (no tree-sitter) | 2.33 | 3.56 | 2 |
| 6 | Python test Write (full pipeline, 2 queries) | 13.85 | 16.18 | 2 |
| 7 | large JS test Write (660 lines / 18.4 KB, 3 queries) | 93.77 | 111.87 | 0 |
| 8 | zig test Write (comment + bundled injection query) | 12.73 | 16.23 | 2 |

Supplementary, same method, to isolate the query-count axis:

| # | scenario | queries | p50 | p95 |
| - | -------- | ------- | --- | --- |
| 9 | `tests/a_test.yaml` (comment only) | 1 | 4.84 | 6.20 |
| 10 | small `tests/app.test.js` | 3 (comment+test+injection) | 31.69 | 42.08 |
| 11 | small `tests/app.test.ts` | 2 (typescript has no bundled injection) | 43.69 | 54.41 |

> Correction to an earlier assumption: **zig is not comment-only** — the grammar pack bundles a
> small injection query for zig, so it compiles comment + injection (2 queries), which is why
> scenario 8 ≈ scenario 6. The clean 1-vs-2-vs-3 query isolation is scenarios 9 / 10 / 11.

## Derived stage breakdown (p50, ms — derived by differencing)

| stage | derivation | ms |
| ----- | ---------- | -- |
| process start + arg parse + print | scenario 1 | ≈ 2.2 |
| stdin read + JSON parse + tool gate | 2 − 1 | ≈ 0.04 |
| path gates + language resolution | 4 − 2 | ≤ 0.05 (noise) |
| harness classify + block output (no tree-sitter) | 5 − 4 | ≈ 0.1 |
| **runtime build + parse + captures + classify + output** (python) | 6 − 4 | **≈ 11.6** |
| — of which **`tree_sitter::Query::new` compilation** (probe: cold detect 13.1–14.2 vs warm 0.19–0.38 ms) | | **≈ 11.2–13.8** |
| — of which parse + captures + classify + output (warm) | probe | 0.2–0.4 |
| large JS: query compilation (3 queries) | probe cold−warm | ≈ 26–35 |
| large JS: tree-sitter parse | probe | ≈ 7–12 |
| large JS: comment + test captures | probe | ≈ 7.5–11 |
| large JS: **classification / dedupe / filters** (remainder) | warm detect − (parse + captures) | **≈ 45–55** |

Marginal per-query cost (one warm process, identical sources): 1st query ≈ +2.6 ms (yaml), 2nd
≈ +8.9 ms (python), 3rd ≈ +18.0 ms (js grammar). Repeated identical `Query::new` calls cost
2.6–6.2 ms each for python, 5.5–9.4 ms for the typescript comment query, and **22.8–26.9 ms for the
typescript test query** — the cost is grammar-bound, not source-bound. `get_language` itself is
0.001–0.06 ms.

## Top cost centres

1. **Query compilation — one fresh process pays it 2–3×.** ≈ 84 % of python's ≈ 13.9 ms, ≈ 29 of
   ts's 43.7 ms, ≈ 26–35 of large-js. `src/detector.rs:200-222` (`runtime_by_name`) compiles the
   comment query, the optional test query, and the optional injection query once per language per
   process; the `RUNTIMES` cache (`detector.rs:19-20`) is per-process, so every invocation pays it.
2. **Large-write warm pipeline, dominated by per-line rule classification.** ≈ 45–55 ms of scenario 7
   (`detector.rs:705-723` runs the full `rules.rs` catalogue per body line over ~60 bodies). Small
   files are unaffected (warm detect ≈ 0.3 ms).
3. **Process start ≈ 2.2 ms** (60 MB binary with statically linked grammars) — real but minor.

**Injection query.** `runtime_by_name` compiles it unconditionally when the pack bundles one
(`detector.rs:211-212`). For 21 of 35 languages `get_injections_query` returns `None` → zero cost;
for the 14 that bundle one it is an extra `Query::new` even when the file has no injection site. It
cannot be deferred in-repo: the query is what discovers the regions (`tests/detector.rs` depends on
the html `<script>` injection finding).

## The "generic capture" premise is moot

The code already compiles **one generic test query per language** and classifies candidates in Rust
(`src/language.rs:16-54`, compiled at `src/detector.rs:210`, classification at `detector.rs:424-470`).
The remaining cost is the intrinsic grammar-bound `Query::new` cost, so restructuring the query
sources cannot recover it. The plan's *diagnosis* (query compilation dominates) is correct; its
*fix* addresses work already done.

## Proposal

**A p95 ≤ 10 ms target is not reachable in-repo without an architecture or feature trade-off.** The
in-process floor for a 2-query language is already ≈ 11–13 ms of query compilation + ≈ 2.2 ms of
process start, before parse or classification runs; python measures p95 ≈ 16 ms and JS/TS 42–54 ms
(the documented 15 ms target is already exceeded for JS/TS test writes).

Options, in order of leverage:

1. **Warm-process reuse** (a keep-alive/daemon behind the identical stdin/stdout hook contract).
   Warm small invocation is 0.19–0.38 ms, so this trivially reaches ≤ 10 ms for small files. It is
   an operational/architecture trade-off (wrapper lifecycle, upgrades, crash recovery), not a rule
   change. **Rejected by product:** the 10 ms figure was a suggestion, the budget stays 15 ms, and a
   daemon adds lifecycle, security and cross-platform complexity that contradicts the lean goal.
2. **Behaviour-preserving classification prefilter** for large writes: skip body lines that cannot
   match any rule before running the full catalogue. **Not done** — a safe prefilter would have to
   encode the triggers of all 29 rules (`src/rules.rs` is ~2.5k lines), far beyond the ~30-line
   budget set for this option.
3. **Do not** remove rules, drop the injection query, or shrink query sources: measured savings are
   small and/or change behaviour.

Rejected/unavailable: serialized query bytecode (tree-sitter 0.26 exposes only `ts_query_new`);
process-start tuning (≤ 2.2 ms at best).

## Test coverage relevant to a refactor

- **Rule × language matrix: exists** — `tests/detector.rs:455,477,499` over `FRAMEWORK_CASES` (21)
  + `NON_FRAMEWORK_CASES` (14) = 35 cases = all enabled languages.
- **Findings snapshot (identical before/after): does NOT exist.** The closest guarantees are
  `eval/corpus.json` (per-rule recall floor) and `parity/vectors.json` (exact message/exit contract).
  A refactor would need a temporary before/after findings comparison.
- `benches/test_checker.rs` runs warm in-process iterations, so it structurally cannot observe
  query-compilation or process-start costs.

## Review follow-up

Checks requested after the first version of this profile:

- **`--version` p95 vs analysis p95:** 3.14 ms vs 16.18 ms (same harness, same box). `--version` is
  ≈ 19 % of a Python analysis invocation, so the floor is the **query compilation inside the
  analysis**, not process startup.
- **Release profile:** already `opt-level = 3`, `lto = "fat"`, `codegen-units = 1` — no obvious
  profile win left.
- **Grammar registration:** lazy per language, not all 35 at startup. The pack keeps
  `static REGISTRY: LazyLock<LanguageRegistry>` and `get_language(name)` looks the language up on
  demand (pack `lib.rs:96,189-200`).
- **Large-file prefilter:** skipped (see option 2 above) — not a ~30-line, safely provable change.
- **The perf gate is relative, not absolute.** `eval/baseline.py` fails when p95 regresses beyond
  `tolerance_percent` (25 %) against `eval/baseline.json`'s recorded per-profile p95, and fails
  closed when the baseline/tolerance is missing or invalid; the 15 ms value is a warning only.
  Caveat: the gate is **not wired into CI** — it runs manually (`python3 eval/baseline.py <binary>`).
- **Absolute numbers are host-dependent.** The recorded baseline is 18.28 ms; on the reviewer's box
  p95 ≈ 14.3–14.7 ms is inside the 15 ms target, while this (un-isolated) box measured ≈ 16.2 ms.
  The relative gate is the one that must pass.
