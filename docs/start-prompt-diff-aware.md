# Start prompt — execute `docs/diff-aware-and-query-expansion.md`

Paste this into a fresh session.

```
You are working in a fresh clone of BaconDroid/test-checker (default branch main):
  git clone https://github.com/BaconDroid/test-checker.git && cd test-checker

Read, in order:
- AGENTS.md (commands + test-authoring gate)
- docs/rebuild-prompt.md (how the crate mirrors comment-checker)
- docs/diff-aware-and-query-expansion.md  <- the work plan you must execute
- docs/related-projects.md (ideas/backlog from a sibling implementation)
- docs/test-signal-research.md and docs/source-exploration.md (signal evidence)

Sibling repo: BaconDroid/comment-checker (same engine/CI/npm scaffolding; see parity below).

TASK: execute the two workstreams in docs/diff-aware-and-query-expansion.md - (A) make the
presence-only rules diff-aware, and (B) expand/validate the test-body query set to >=20 languages.
Do NOT re-implement the review backlog (R1-R7) - those already landed on main (test-removed,
no-assertion, allow-added, incomplete/unawaited-assertion, broad-exception, --min-confidence,
gate-weakened). Land each workstream as its own branch -> PR -> squash-merge with CI green.

CURRENT STATE (verified): 24 rules; 15 framework languages; presence-only rules are
weakened-assertion, snapshot-regenerated, tolerance-broadened; engine in src/engine.rs (shared,
byte-identical with comment-checker); parity harness in parity/ with a CI job.

INVARIANTS (do not violate):
- Exactly four direct deps (serde, serde_json, tree-sitter, tree-sitter-language-pack); no workspace;
  single-package manifest.
- cargo fmt --all -- --check clean; cargo clippy -p test-checker --all-targets --all-features -- -D warnings
  clean (note: clippy.toml sets too-many-lines-threshold=120 - split functions rather than relaxing it);
  no unwrap/expect/panic in non-test code.
- Hook contract unchanged: stdin JSON -> stderr, exit 2 on findings else 0, fail-open, prefix [test-checker].
- PARITY: any change to a shared file (.cargo/config.toml, .gitignore, clippy.toml, deny.toml,
  rustfmt.toml, rust-toolchain.toml, .github/**, npm/**, parity/**, src/cli.rs, src/engine.rs,
  src/main.rs, tests/parity.rs) must land as a coordinated pair: push the SAME branch name to BOTH
  repos; the parity CI job compares the sibling's matching branch (PARITY_REF). New public items that
  aren't part of the shared shape must be added to check.py EXTRAS (a shared edit) or the parity job fails.
  Workstreams A and B are test-checker-only unless they touch the engine.

WORKSTREAM A - diff-aware presence rules:
1. Decide and document the Write (no-baseline) policy: recommend keep the rules at Low confidence on
   Write; fire at diff-aware confidence on Edit.
2. Add a diff context: compute the removed lines (LCS-unmatched old lines via engine::added_lines'
   complement) and thread DiffContext { removed } into classify_added_line (or add classify_diff);
   analyze_new_bodies passes an empty context.
3. Replace the marker-only checks with a declarative matcher-strength table (JS/TS: toStrictEqual >
   toEqual > toBe > toBeTruthy/toBeFalsy/toBeDefined/toBeNull/anything(); Python: assertEqual/== >
   assertIsNotNone/assertTrue/is not None; snapshot: toEqual -> toMatchSnapshot; and numeric tolerance:
   assertAlmostEqual places N->M with M<N, toBeCloseTo(x,N->M) M<N, sleep/timeout N->M M>N).
   Fire only when strength(old) > strength(new) for the same assertion subject; if alignment is
   ambiguous, do NOT fire (precision over recall).
4. Confidence: Medium for diff-detected strict->loose (High for unambiguous pairs); Low for presence-only on Write.
5. Tests: table-driven strict->loose pairs (fire) + controls (loose matcher on a brand-new test -> no
   fire under A1; unrelated edit; strict->strict; already-loose-before). A control test must fail on the
   pre-fix presence-only code.
6. Verify: fmt/clippy/test/parity + smoke (clean -> 0; weakening edit -> 2; brand-new loose test -> per A1).

WORKSTREAM B - expand/validate the test-body query set:
1. Add c, cpp (GoogleTest/Catch2/Unity: TEST, TEST_F, TEST_CASE, TEST_CASE_METHOD, SCENARIO),
   groovy (Spock def "name"() {), ocaml (alcotest/oUnit let test_... () =), elm (elm-test test "..."),
   bash (bats @test "..." {) - target >=20 framework languages. Leave css/cue/dockerfile/hcl/html/
   proto/sql/svelte/toml/yaml as non-framework.
2. Generic capture first: prefer one call_expression pattern (+ member-expression variant) and classify
   the callee in Rust; query compilation dominates cost (measured 3-14x). Set a budget (p95 < 15 ms per
   call incl. startup) and measure with benches/.
3. Per language: add a test_query arm (src/language.rs: @test + @test.name/@test.callee); extend
   is_body_candidate (src/detector.rs); VALIDATE empirically before trusting (query compiles, captures
   the body, a skip/focus marker inside yields the finding); move the language from NON_FRAMEWORK_CASES
   to FRAMEWORK_CASES + WEAKEN_CASES in tests/detector.rs, keep the table at exactly 31, clean fixture
   -> no findings.
4. Edge cases: attribute/decorator/macro lines outside the body (mirror expanded_start for Rust/Elixir/
   Python); nested bodies (dedupe already applied); add the language to line_comment_token; reuse the
   rule x language fixtures so #[tokio::test], stacked #[test] #[ignore], #[should_panic], and TS
   describe/hooks stay covered.

GOTCHAS learned (do not repeat):
- Comment lines are stripped before line-rule classification, so a marker that lives in a comment is
  invisible to rules - detect such markers from RAW content at the API layer, not in rules.rs.
- Do NOT broaden ASSERTION_MARKERS to cover Go/Swift/C#: conditional-assertion uses it and would fire on
  the idiomatic Go `if x != y { t.Errorf }` pattern. Use the separate ASSERTION_EXISTS_EXTRAS for the
  no-assertion check (this bug was already hit and fixed).
- no-assertion must skip suite bodies (nested it/test/describe) to avoid FP on describe blocks.
- Authoring: local cargo is at ~/.cargo/bin; share a target dir if you run both repos.
- CI runtime is ~3 min/job; land via branch -> PR -> squash-merge; never force-push main.

VERIFY before declaring done: cargo fmt --all -- --check; cargo clippy -p test-checker --all-targets
--all-features -- -D warnings; cargo test -p test-checker --all-targets --all-features; python3
parity/check.py (both directions); cargo bench -p test-checker --bench test_checker --no-run;
node --test npm/test-checker/*.test.js.

REPORT per step: exact commands run + results. Do not claim a language is supported until its
coverage-table row passes; do not claim a rule is diff-aware until the brand-new-test control passes.
```
