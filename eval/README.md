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
A stale `baseline.json` makes the gate refuse by design; re-record only in the release block below.

## Recording policy

The gate is authoritative for the tagged release tree. A pull request does not re-record the
baseline, and a source change that leaves `built_from` stale is not a reason to edit
`eval/baseline.json`.

Re-recording happens in the release block (`docs/roadmap.md`) immediately before tagging, on the
merge commit; the tree measured is the tree that ships. `--record` writes that commit to
`built_from`.

If the gate refuses for provenance, do not re-record on the branch. Rebuild from the release tag's
merge commit, re-record once, and commit `eval/baseline.json` in the release PR.

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
Payload selection reads the committed `eval/path-predicates.json` test-path table (generated from
`src/language.rs`), so the audit corpus classifies files exactly as the binary does.

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
`--repos` and `--corpus` are CLI arguments with checkout-relative defaults: the default repos are
this checkout plus a sibling `../comment-checker` checkout when it exists, and the default corpus is
`<checkout>/eval/corpus.json`. Pass `--repos /path/to/test-checker /path/to/comment-checker` when the
sibling lives elsewhere; a missing default sibling is skipped with a warning instead of aborting.
Per payload it reports `old_lines`, `removed_lines`, `removed_tests`, `added_tests`, and the
pre-demotion `test_removed`/`assertion_removed` counts; the script applies
`(old_lines >= 20 and ratio > threshold) or (removed_tests > 0 and added_tests >= removed_tests)`.
`--truth` lists the replay payloads verified as genuine deletions (`cases[].key` =
`sha256(kind|path|old|new)[:16]`); without it, replay TP recall prints `n/a`. Recall is
**payload-level**: every truth payload in the replay corpus counts in the denominator, including a
truth payload whose removal finding is missing entirely, so a false negative cannot disappear from
the ratio. The recall-corpus part uses the expected rule directly, so it always reports 12/12 while
the guard keeps every genuine deletion High.

## Secondary corpus

`eval/build_secondary_corpus.py` samples real test-file changes from three OSS applications so a
line-rule change can be measured on code this project does not own. It shares `replay.py`'s payload
shape: `Write` carries the committed content, and `Edit` carries the parent content as `old_string`
and the committed content as `new_string`. There are no truth labels, so the hit counts below are a
precision sample, not TP/FP rates.

Only `eval/secondary-manifest.json` is tracked. It records each repository's URL, license, clone
mode, HEAD, the commit SHA and path of every payload, the payload kind, the skip counts, and the
expected counts. The payloads embed third-party source, so they are written to
`$TMPDIR/opencode/corpus-src` (or `--scratch`), never to this checkout; `build` refuses a `--dump`
path inside the checkout.

### Sources

| repo                    | language   | license           | clone              | HEAD         | commits | payloads |  target |
| :---------------------- | :--------- | :---------------- | :----------------- | :----------- | ------: | -------: | ------: |
| sharkdp/fd              | Rust       | MIT OR Apache-2.0 | full               | 24d90b28d254 |      56 |      120 |   >=100 |
| netbox-community/netbox | Python     | Apache-2.0        | full               | b34cbecc45c5 |      41 |      120 | 100-160 |
| calcom/cal.diy          | TypeScript | MIT               | --filter=blob:none | 2eb48eccd8ac |      44 |      110 | 100-120 |

`select` takes the newest qualifying commits at the clone HEAD and caps each commit at two test
files (`max_files_per_commit`) so one bulk commit cannot dominate a repository's sample. It keeps
modified files only (parent and commit blobs must both decode), so added and deleted test files are
skipped and counted in the manifest. `build` and `measure` reproduce the manifest exactly.

### Regenerate

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
python3 eval/build_secondary_corpus.py build --manifest eval/secondary-manifest.json
python3 eval/build_secondary_corpus.py measure --binary target/release/test-checker \
  --json /tmp/opencode/secondary-measure.json
```

`select` rewrites the manifest from whatever each upstream HEAD is now; run it only when refreshing
the corpus. `build` clones a missing source into the scratch directory with the recorded clone mode
(full for fd and netbox, `--filter=blob:none` for cal.diy) and writes `<repo>.jsonl` records of
`{repo, kind, commit, path, payload}`.

### Measured distributions

Release binary built from `485e149`, measured 2026-11-09 with `--min-confidence low`. Exit `1` never
occurred. The `Write`/`Edit` split matters: `Write` scans the whole committed file, so every
pre-existing pattern counts; `Edit` only sees the parent -> commit diff.

Four full runs gave identical fd and cal.diy exit codes and rule totals. Netbox varied because its
largest files sit at the 50 ms parse budget: exit 2 totalled 74-75 (Write 57-58, Edit 17-18),
`weakened-assertion` 890-943, and `duplicate-test` 1,295-1,296. Two netbox `Edit` payloads (305 KB
and 385 KB of new content) flipped exit code between identical invocations. The table records one
run; treat netbox rule counts as +/-5 %. p95 is host-dependent (netbox measured 348-413 ms across
runs).

| repo   | payloads | exit 0 | exit 2 | exit 2 Write | exit 2 Edit | p50 ms | p95 ms |
| :----- | -------: | -----: | -----: | -----------: | ----------: | -----: | -----: |
| fd     |      120 |     65 |     55 |           54 |           1 |  94.14 | 179.62 |
| netbox |      120 |     45 |     75 |           58 |          17 |  85.59 | 347.71 |
| caldiy |      110 |     52 |     58 |           45 |          13 |  38.12 | 127.73 |

fd (`sharkdp/fd`):

| rule                  | hits | Write | Edit | test-adding |
| :-------------------- | ---: | ----: | ---: | ----------: |
| incomplete-assertion  |  177 |   175 |    2 |          81 |
| conditional-assertion |  108 |   108 |    0 |          44 |
| no-assertion          |   54 |    54 |    0 |          22 |
| test-volume           |   54 |    54 |    0 |          22 |

netbox (`netbox-community/netbox`):

| rule                  | hits | Write | Edit | test-adding |
| :-------------------- | ---: | ----: | ---: | ----------: |
| duplicate-test        | 1295 |  1294 |    1 |         711 |
| weakened-assertion    |  937 |   920 |   17 |         875 |
| no-assertion          |  144 |   144 |    0 |         139 |
| mock-of-sut           |   70 |    61 |    9 |          69 |
| test-volume           |   58 |    58 |    0 |          52 |
| conditional-assertion |   20 |    19 |    1 |          18 |
| swallowed-error       |    6 |     6 |    0 |           6 |
| assertion-removed     |    2 |     0 |    2 |           2 |
| test-removed          |    2 |     0 |    2 |           2 |

cal.diy (`calcom/cal.diy`):

| rule                  | hits | Write | Edit | test-adding |
| :-------------------- | ---: | ----: | ---: | ----------: |
| weakened-assertion    |  168 |   163 |    5 |          45 |
| test-volume           |   40 |    37 |    3 |          25 |
| conditional-assertion |   39 |    38 |    1 |           3 |
| no-assertion          |    8 |     8 |    0 |           1 |
| incomplete-assertion  |    7 |     7 |    0 |           0 |
| test-removed          |    4 |     0 |    4 |           1 |
| snapshot-regenerated  |    3 |     3 |    0 |           3 |
| skip-added            |    3 |     3 |    0 |           1 |
| swallowed-error       |    3 |     2 |    1 |           3 |
| commented-out-test    |    2 |     2 |    0 |           0 |
| duplicate-test        |    2 |     2 |    0 |           0 |
| gutted-test           |    2 |     0 |    2 |           0 |

### FP suspects

No truth labels exist. These are the rules whose hit volume on test-adding payloads is high enough
to check by hand first; they are suspects, not labelled false positives. The `test-adding` tag is
itself a heuristic: a commit is tagged when the committed file has more test declarations than its
parent (`def test_`, `fn test`, `test(`/`it(`/`describe(`).

- **`incomplete-assertion` (fd: 177 hits, 175 on Write)** - Rust `Result::expect("...")` matches the
  JavaScript `expect(` matcher. Sample: `tests/tests.rs:1132`
  `.expect("Failed to create broken symlink.")`. This is a Rust/JS matcher collision, not a
  weakened assertion, and it accounts for most fd `Write` exit-2 results.
- **`duplicate-test` (netbox: 1,295 hits, 1,294 on Write)** - the signature is the whole test body
  keyed per file with no class scope, so identical bodies repeated across `TestCase` classes count.
  Sample: `test_export_objects_anonymous` in `netbox/extras/tests/test_views.py` at lines 484 and
  1323.
- **`weakened-assertion` (netbox: 937, cal.diy: 168; almost all on Write)** - a `Write` has no
  baseline, so every loose matcher reports at Low confidence. Netbox `assertTrue`/`assertIsNotNone`
  (`netbox/users/tests/test_views.py:273`) and cal.diy `toBeTruthy`/`toBeDefined`
  (`packages/features/bookings/lib/handleNewBooking/test/fresh-booking.test.ts:3680`) dominate. The
  `Edit` column is the change-scoped number: 17 and 5.
- **`no-assertion` (netbox: 144, all on Write)** - netbox disables inherited tests with methods that
  only `return` (`netbox/extras/tests/test_views.py:481-488`). They are honestly assertion-free but
  unchanged in the replay, so the full-file scan reports them.

### Caveats

- **fd** - low diversity: `tests/tests.rs` supplies 54 of 60 changed files. The corpus is Write-heavy
  noise from JS-oriented matchers applied to Rust (`incomplete-assertion`, `conditional-assertion`).
  Only one fd `Edit` payload exits 2.
- **netbox** - only `test_*.py` is sampled; 120 payloads span 41 commits and 35 files. Two test files
  added in their commit are skipped because an `Edit` needs both sides. Large files push p95 to
  347.71 ms, over the 15 ms warning target.
- **cal.diy** - blobless clone; git fetches blobs on demand. Playwright reports and
  `packages/prisma/` artifacts are excluded. 110 payloads span 44 commits and 49 files; 42 added and
  14 deleted test files are skipped because an `Edit` needs both sides.
