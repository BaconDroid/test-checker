# Baseline

Measured with `eval/baseline.py` on a lightly loaded dev box (load average ~2; re-record on an idle
machine for the tightest gate).

## Contract

All six exit-code cases in `baseline.py` pass.

## Relative perf gate

Each run is 210 invocations of the trigger payload (the first 10 are discarded); the harness takes the
**lowest p95 of 3 runs** and **fails** when p95 regresses by more than **25 %** against
`eval/baseline.json` (`profiles.<profile>.p95_ms`). It fails closed on a missing, non-positive, or
non-finite baseline/tolerance. The 15 ms budget is a documented **target** — it prints a warning, not
a failure.

Re-record on an idle machine:

```bash
python3 eval/baseline.py target/release/test-checker --record
```

Recorded (load ~2): release p95 18.28 ms. Binary size: 31 languages ≈ 53.9 MB, 35 ≈ 60.3 MB (+11.9 %).
