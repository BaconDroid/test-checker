# Baseline

Measured on this workstation (idle), release profile (`cargo build --release`), with
`eval/baseline.py` — 300 invocations of the trigger payload, process startup included.

| metric | value    | budget                          |
| ------ | -------- | ------------------------------- |
| p50    | 11.05 ms | —                               |
| p95    | 15.09 ms | < 15 ms (**marginally over**)   |

All six contract cases in `baseline.py` pass. `test-checker` is heavier than its sibling: it runs
the rule catalog and the test-body queries. p95 sits within a hair of the 15 ms budget, so treat
15 ms as soft here, or optimize the rule layer before tightening it.
