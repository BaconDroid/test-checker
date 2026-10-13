# Baseline

Measured on this workstation (idle), release profile (`cargo build --release`), with
`eval/baseline.py` — 300 invocations of the trigger payload, process startup included.

| metric | value    | budget                     |
| ------ | -------- | -------------------------- |
| p50    | 11.36 ms | —                          |
| p95    | 14.69 ms | < 15 ms (met, at the edge) |

All six contract cases in `baseline.py` pass. `test-checker` is heavier than its sibling: it runs
the rule catalog and the test-body queries, so p95 sits within a hair of the 15 ms budget. (Runs
vary by ~1–2 ms; an earlier run measured p95 15.09 ms.) Treat 15 ms as soft here, or optimize the
rule layer before tightening it.
