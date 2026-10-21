# Baseline

Measured with `eval/baseline.py` on a lightly loaded dev box (load average ~2; re-record on an idle
machine for the tightest gate).

## Contract

Every exit-code case in `baseline.py` passes (`0` pass/skip, `2` findings).

## Relative perf gate

Absolute p95 drifts with machine load, so the gate is the **ratio of the analysis-case p95 to the
`--version` p95 measured in the same session**. Each gate run takes **five alternating sessions**;
session *i* measures analysis p95 and then `--version` p95, giving `ratio_i = analysis_i /
version_i`. The gate uses the **median** of the five ratios (and reports all five, the per-session
`--version` p95, and the spread across sessions).

A run is **over** when its median ratio exceeds `recorded_ratio * (1 + tolerance_percent / 100)`.
`eval/baseline.json` stores the recorded ratio, `tolerance_percent` (**35**), and `built_from` (the
commit the baseline was measured on). The gate **fails only when at least two of the last three
consecutive gate runs are over**, so a single noisy run does not fail a build. Each gate run appends
its ratios to `eval/baseline-history.json`; the harness keeps the last three entries per profile.
That file is local state and is gitignored. `--record` resets it for the recorded profile.

The 15 ms p95 value remains a **warning** target, not a failure.

## Provenance

The harness resolves the commit the binary was built from from `BASELINE_BUILT_FROM`, falling back
to `git rev-parse HEAD`. It **exits 2 and refuses to record or compare** when that commit is not
HEAD (a stale binary), when HEAD cannot be resolved, or when the baseline's `built_from` is not
HEAD. The one exception is the commit that records the baseline: a follow-up commit that changes
only `eval/baseline.json` leaves the measured source identical, so HEAD may be that metadata-only
successor.

CI or any fresh build should build from the commit under test and either rely on the HEAD fallback
or set the commit explicitly:

```bash
cargo build --release --locked
BASELINE_BUILT_FROM="$(git rev-parse HEAD)" python3 eval/baseline.py
```

The value must equal HEAD; a binary built from any other commit exits 2. CI does **not** run this
gate today, and a fresh job starts with no history, so its first run can never be a two-of-three
failure. Persist `eval/baseline-history.json` across runs (cache or artifact) if CI should fail on
repeated regressions.

## Re-record

On an idle machine, from the commit you will test:

```bash
cargo build --release
python3 eval/baseline.py --record
```

Recorded (load ~2): release ratio _TBD_ (median of five sessions); `--version` p95 spread _TBD_.
Binary size: 31 languages ≈ 53.9 MB, 35 ≈ 60.3 MB (+11.9 %).
