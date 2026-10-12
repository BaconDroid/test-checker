# Prompt plan — diff-aware presence rules + test-body query expansion

> **Status: completed.** Workstreams A and B landed on `main` (#34, #35), followed by
> review-driven hardening/simplification (#36–#44). Shipped state: **25 rules** and **21 framework
> languages**. This document is retained as the historical plan; do not re-run it as pending work.

Handoff prompt for a fresh session working in `BaconDroid/test-checker`. It covers the two requested
workstreams (A, B) and a prioritized backlog distilled from the external review in
`test-checker-review.md`. Read `AGENTS.md` and `docs/rebuild-prompt.md` first.

---

## 0. Context (verified current state)

- Repo `BaconDroid/test-checker` (`main`); sibling `BaconDroid/comment-checker` (`master`).
- The domain-agnostic engine is `src/engine.rs`, **byte-identical** in both repos, enforced by
  `parity/check.py` (CI job `parity`). Shared files: `.cargo/config.toml`, `.gitignore`, `clippy.toml`,
  `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml`, `.github/**`, `npm/**`, `parity/**`,
  `src/cli.rs`, `src/engine.rs`, `src/main.rs`, `tests/parity.rs`.
- Domain layer: `src/rules.rs` (18-rule catalog), `src/detector.rs` (capture + edit diff), `src/api.rs`,
  `src/language.rs` (`test_query`, `is_test_callee`, `is_test_function_name`, `is_test_path`),
  `src/models.rs`, `src/output.rs`, `src/filters.rs`.
- **15 framework languages** today (`python`, `javascript`, `typescript`, `tsx`, `go`, `rust`, `ruby`,
  `java`, `csharp`, `php`, `kotlin`, `swift`, `scala`, `elixir`, `lua`); the other 16 parse but yield no
  body findings; `commented-out-test` works in all 31.
- Three rules are **presence-only**: `weakened-assertion`, `snapshot-regenerated`,
  `tolerance-broadened` (fire on any added line containing a marker).
- Reconciliation with the external review: the live catalog is the **18 rules** (matching the README);
  `unknown-test`/`empty-test`/`assertion-roulette`/`redundant-assertion` no longer exist. The review's
  file-wide `test-checker: allow` bypass **is** real (see Backlog R3). Everything below that cites the
  review is marked **[rev]**.

## 1. Invariants (do not violate)

- Exactly four direct deps (`serde`, `serde_json`, `tree-sitter`, `tree-sitter-language-pack`); no new
  crates, no workspace, single-package manifest.
- `cargo fmt --all -- --check` clean; `cargo clippy -p test-checker --all-targets --all-features -- -D warnings`
  clean; no `unwrap`/`expect`/`panic` in non-test code.
- Hook contract unchanged: stdin JSON → stderr message, exit 2 on findings else 0, fail-open, prefix
  `[test-checker]`.
- **Parity:** a change to any shared file must land as a coordinated pair (same branch name in both
  repos) or not at all. `test_query`/`is_body_candidate` are `pub(crate)` and not shape-checked, so A
  and B are test-checker-only unless they touch the engine.
- Test-authoring gate in `AGENTS.md`.

---

## 2. Workstream A — diff-aware presence rules

**Goal:** `weakened-assertion`, `snapshot-regenerated`, `tolerance-broadened` fire only when a stricter
form is actually replaced by a looser one, not merely because a loose marker is present.

### A1. Write (no-baseline) policy — decide and record
An `Edit` has old/new; a `Write` has only new. Pick one and justify it:
- **(recommended)** on `Write`, keep the rules at **Low** confidence; on `Edit`, fire at the diff-aware
  confidence below.
- alternative: suppress on `Write`.

### A2. Diff context
`detect_edit` already computes added-line indices with `engine::added_lines(old, new)`. Add the
**removed** lines (LCS-unmatched old lines) and thread a `DiffContext<'a> { removed: &'a [&'a str] }`
into `classify_added_line` (or add `classify_diff`). `analyze_new_bodies` passes an empty context.

### A3. Matcher-strength model **[rev]**
Replace the ad-hoc marker lists with a declarative strength table and compare strengths:
- **JS/TS:** `toStrictEqual` > `toEqual` > `toBe` > `toBeTruthy`/`toBeFalsy`/`toBeDefined`/
  `toBeUndefined`/`toBeNull`/`anything()`.
- **Python:** `assertEqual`/`assert a == b` > `assertIsNotNone`/`assertTrue`/`assertIsInstance`/
  `assert a is not None`.
- **Snapshot:** `toEqual(...)`/`assertEqual(...)` → `toMatchSnapshot()`/`toMatchInlineSnapshot()`/
  `toMatchFileSnapshot()`.
Fire only when `strength(old) > strength(new)` for the **same assertion subject**.

### A4. Tolerance is numeric, not a marker **[rev]**
Compare old/new numerically: `assertAlmostEqual(..., places=N→M)` with `M<N`; `toBeCloseTo(x, N→M)` with
`M<N`; `approx(rel/abs=…)` widened; `time.sleep(N→M)` / timeout with `M>N`.

### A5. Alignment (precision over recall)
Align removed/added lines by the assertion subject (same left operand) or same call shape at the same
position; if ambiguous, do **not** fire.

### A6. Confidence
- Diff-detected strict→loose: **Medium** (High for unambiguous pairs like `toEqual`→`toBeDefined`).
- Presence-only on `Write` (if kept): **Low**.

### A7. Tests
Table-driven strict→loose pairs per language (fire); controls: loose matcher on a brand-new test
(Write) → per A1; unrelated edit → no fire; strict→strict → no fire; already-loose-before → no fire.
Regression: a test that fails on the pre-fix presence-only behavior.

### A8. Verify
fmt / clippy / test / `python3 parity/check.py` (both directions); smoke: clean → 0, weakening edit → 2,
brand-new loose test → per A1.

---

## 3. Workstream B — expand and validate the test-body query set

**Goal:** raise the framework-language count from 15 to **≥20**, each new language validated.

### B0. Target languages
Add where a real framework exists: **c**/**cpp** (GoogleTest/Catch2/Unity: `TEST`, `TEST_F`,
`TEST_CASE`, `TEST_CASE_METHOD`, `SCENARIO`), **groovy** (Spock `def "name"() {`), **ocaml**
(alcotest/oUnit `let test_... () =`), **elm** (elm-test `test "..." ...`), **bash** (bats
`@test "..." {`). Leave `css`, `cue`, `dockerfile`, `hcl`, `html`, `proto`, `sql`, `svelte`, `toml`,
`yaml` as non-framework.

### B1. Generic capture first **[rev]**
Query **compilation** dominates cost (measured 3–14× savings for a small generic capture vs per-signal
patterns). Prefer one generic pattern per language (e.g. `(call_expression function: (identifier) @test.callee) @test`
plus the member-expression variant) and classify the callee in Rust, instead of adding many
`#match?`/`#any-of?` patterns. Set a budget (e.g. p95 < 15 ms per call including startup) and measure it
with `benches/`.

### B2. Per-language method (same shape as the existing 15)
1. Add a `test_query(name)` arm in `src/language.rs` (`@test` body + `@test.name`/`@test.callee`).
2. Extend `is_body_candidate` in `src/detector.rs` (name/annotation/macro/callee based).
3. **Validate empirically** with a fixture before trusting it (compiles, captures, marker inside →
   finding).
4. Extend the coverage table in `tests/detector.rs`: move the language from `NON_FRAMEWORK_CASES` to
   `FRAMEWORK_CASES` + `WEAKEN_CASES`; keep the table at exactly 31; clean fixture → no findings.

### B3. Edge cases
- Attribute/decorator/macro lines outside the body — mirror `expanded_start` (Rust/Elixir/Python).
- Nested bodies (dedupe already applied).
- Add the new language to `line_comment_token` so trailing comments do not trip rules.
- Rule × language matrix: reuse the existing fixtures so `#[tokio::test]`, stacked `#[test] #[ignore]`,
  `#[should_panic]`, and TS `describe`/hooks are covered (the review flagged these as past FP sources).

### B4. Verify
`cargo test --test detector` green; `python3 parity/check.py`; clippy/fmt; per-language smoke.

---

## 4. Backlog distilled from the review (prioritized, separate PRs)

Each is a distinct feature; do not bundle with A/B unless asked.

- **R1 — `test-removed` (high).** A test present in `old` but absent in `new` (by `@test` name, tolerate
  rename by body similarity). Today a deleted *assertionless* test produces nothing. Sources: S3/S5/S6/S8/S9.
- **R2 — `no-assertion` (high).** A *new* test with no assertion. Needs an **assertion-helper allowlist**
  (`assert_*`, `check*`, Go `t`-argument calls, supertest `.expect`) to avoid FPs. Sources: Sonar S2699,
  eslint `expect-expect`, LLM-test studies.
- **R3 — harden the allow marker (high).** `test-checker: allow` currently suppresses the whole file
  (`is_file_suppressed` matches any line). Make it **line-scoped** by default with a required reason
  (`test-checker: allow <rule> -- <reason>`); reserve whole-file suppression for a human config
  (`.test-checker.toml`), and emit `allow-added` when the marker is added in the same diff.
- **R4 — `unawaited`/`incomplete-assertion` (JS/TS, high).** `expect(x);`, `expect(x).toBe;`,
  `expect(p).resolves.toBe(1)` without `await`/`return`. Sources: eslint `valid-expect`,
  `valid-expect-in-promise`.
- **R5 — `broad-exception` (medium).** `pytest.raises(Exception)` without `match=`, `#[should_panic]`
  without `expected`, `assertThrows(Exception.class)`. Sources: flake8-pytest-style PT011, Sonar.
- **R6 — exit-code policy (medium).** Add `--min-confidence <level>` (default `medium`) so `low`
  findings are reported but do not exit 2.
- **R7 — harness rules (product decision).** `harness-edited`/`gate-weakened` (`|| true`,
  `--passWithNoTests`, coverage threshold lowered, `continue-on-error: true`) behind a **separate
  `harness_globs` gate** (they live outside test files). Cheap (path/regex, no tree-sitter). Sources: S6, S9.
- **R8 — non-objectives (document in README).** Production-code "test mode" branches, `__eq__`
  overrides, stack introspection, cross-file hardcoding, Bash-only signals (`git log -S`, `curl *.patch`,
  `-u`), `tests-not-run`, classic smells. All need cross-file/LLM/Bash context or are noisy — out of scope.

> Cross-repo (not this repo): the review also proposes a shared `rule-vectors.json` between the Rust
> engine and the `opencode-comments-plugin` regex fallback. That belongs to that plugin's repo.

---

## 5. Acceptance criteria

- The three rules are diff-aware (or Low-confidence on `Write` per A1), documented and tested, with a
  control test proving a brand-new test no longer fires.
- Framework language set is **≥20**, every new language validated in the coverage table.
- fmt / clippy / test / parity all green; landed via branch → PR → squash-merge; no new deps; fail-open.

## 6. Landing

- A: `src/rules.rs`, `src/detector.rs` (+ tests). B: `src/language.rs`, `src/detector.rs`,
  `tests/detector.rs`. Both test-checker-only unless the engine changes.
- Backlog items are separate PRs.
- If a change touches a shared file, push the same branch name to both repos (coordinated pair).

## 7. Reporting

Per step: exact commands + results. Do not claim a language is supported until its coverage-table row
passes; do not claim a rule is diff-aware until the brand-new-test control passes.
