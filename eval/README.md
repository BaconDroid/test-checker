# eval

Off-contract evaluation assets. `parity/` locks the cross-repo wire contract; `eval/` is
deliberately **outside** that lock (the parity harness only compares `SHARED` entries), so it may
grow per-repo fixtures, regression cases, and measured baselines without a coordinated wave.

`eval/baseline.py` is a **manual** tool: CI does not run it (`ci.yml` runs fmt/clippy/test/bench/npm
and `parity/check.py`). Run it locally before/after a detector or classifier change.

`baseline.py` is the D0 regression guard: it drives the built binary over a fixed set of hook
payloads, asserts each one's exit code, and measures per-invocation wall time (process startup
included — that is what a PostToolUse hook pays). The gate compares five alternating
analysis/`--version` p95 ratios, uses their median, and fails only when two of the last three runs
exceed the recorded ratio by 35 %. Run history is kept in `eval/baseline-history.json` (gitignored);
see `BASELINE.md` for the provenance rule and how a fresh build sets `BASELINE_BUILT_FROM`.
A stale `baseline.json` makes the gate refuse; re-record on an idle machine or in CI after any change to the binary.

```bash
cargo build --release
python3 eval/baseline.py                      # asserts + timing on target/release/test-checker
python3 eval/baseline.py path/to/binary
```

Exit codes are the contract: `0` clean or deliberate skip, `2` findings, `1` did not run (internal
failure, or the hook wrapper could not run the binary).

## Precision replay

`replay.py` measures a line-rule change on real history instead of hand-written cases:

```bash
python3 eval/replay.py --before /tmp/test-checker-before --after target/debug/test-checker
```

It builds 320 payloads from the two repositories' git history (80 test-file changes per repo, each a
`Write` of the committed content and an `Edit` of parent -> commit), plus a 58-file audit of every
tracked `.rs`/`.js`/`.py` source file in both repos (`eval/baseline.py` excluded), all at
`--min-confidence low`. It prints exit-2 and per-rule finding counts for each binary. Read-only.

## Refactor-guard thresholds

`guard_thresholds.py` recomputes the removal-rule refactor-guard table (20/40/60 %) from the shipped
binary; no instrumented build is needed. The binary prints one `GUARD_METRICS {json}` line per edit
when `TEST_CHECKER_GUARD_METRICS=1` is set (the flag is off by default and never changes a finding).

```bash
cargo build --release
python3 eval/guard_thresholds.py --binary target/release/test-checker \
  --truth eval/replay_removal_truth.json
```

It uses the same 320-payload `replay.py` corpus plus the `old_content` cases in `eval/corpus.json`.
Per payload it reports `old_lines`, `removed_lines`, `removed_tests`, `added_tests`, and the
pre-demotion `test_removed`/`assertion_removed` counts; the script applies
`(old_lines >= 20 and ratio > threshold) or (removed_tests > 0 and added_tests >= removed_tests)`.
`--truth` lists the replay payloads verified as genuine deletions (`cases[].key` =
`sha256(kind|path|old|new)[:16]`); without it, replay TP recall prints `n/a`. The recall-corpus part
uses the expected rule directly, so it always reports 12/12 while the guard keeps every genuine
deletion High.
