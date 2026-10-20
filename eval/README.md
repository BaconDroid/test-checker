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
