# Baseline

Measured on this workstation (idle), release profile (`cargo build --release`), with
`eval/baseline.py` — 300 invocations of the trigger payload, process startup included.

| metric | value    | budget                     |
| ------ | -------- | -------------------------- |
| p50    | 11.20 ms | —                          |
| p95    | 14.30 ms | < 15 ms (met, at the edge) |

All six contract cases in `baseline.py` pass. `test-checker` is heavier than its sibling: it runs
the rule catalog and the test-body queries, so p95 sits near the 15 ms budget. (Runs vary by
~1–2 ms; earlier runs measured p95 14.69–15.09 ms.)

`TSLP_LANGUAGES` grows the release binary: 31 languages = 53.9 MB, 34 (dart, haskell, r) = 59.4 MB,
35 (adds zig) = 60.3 MB (+11.9% over 31). Startup cost is unchanged within run-to-run noise (p95
drifts by several ms when the machine is loaded).
