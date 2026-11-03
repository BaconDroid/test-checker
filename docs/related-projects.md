# Related projects

Other comment/test hooks worth tracking. Review each by hand and port only convention-following,
non-superseded ideas.

| Project | Relationship | Notes |
|---|---|---|
| [`code-yeongyu/go-claude-code-comment-checker`](https://github.com/code-yeongyu/go-claude-code-comment-checker) | Upstream of `BaconDroid/comment-checker` | The original Go hook, rewritten in Rust; upstream still ships its `go/` tree, `clap`/`criterion`/`tempfile`, and bindings - those were dropped only by the `BaconDroid/comment-checker` fork. |
| [`BaconDroid/comment-checker`](https://github.com/BaconDroid/comment-checker) | Sibling of this repo | Shares `src/engine.rs`, the CLI, npm/CI/release and the `parity/` harness byte-for-byte (modulo names). |
| [`systemfsoftware/comment-checker`](https://github.com/systemfsoftware/comment-checker) | Independent Rust implementation (Apache-2.0) | A more elaborate, mutation-tested classifier (workspace, 37 languages, F1 corpus, setup skill). Ideas to mine below; **no code copied** (different architecture and license). |

## Ideas from `systemfsoftware/comment-checker` (evaluated)

| # | Idea (where) | Why it helps us | Status |
|---|---|---|---|
| 1 | **F1 evaluation corpus + per-kind floors** (`eval/corpus.json`, `tests/f1.rs`) | We ship 33 rules with a coverage/unit suite; a per-kind gate stops one weak rule hiding inside an aggregate. | **Done** (landed 2026-10-12) - an `eval/corpus.json` of (fixture, expected rule) plus a threshold test with per-rule/per-kind floors. |
| 2 | **Mutation testing of the classifier** (`mutants.toml`, `mutation` workflow; `cargo mutants`) | Proves the rules are actually exercised (a surviving mutant = a missing test). | **Partial** - `.cargo/mutants.toml` now scopes `src/rules.rs` in both repos; the non-blocking job is still missing. |
| 3 | **Fragment unreliability** (`classify.rs`, `CONCEPTS.md`, plan U0) | On `Edit`/`MultiEdit` the input is a fragment: adjacent context is truncated, so context-aware verdicts must fail open there. Our diff rules (`test-removed`, `assertion-removed`) read old/new fragments and could misread a test that moved out of the edited fragment. | **Done** - context-dependent rules no longer convict when the fragment boundary could hide the counterpart: whole-test rules require a complete test node and the removal rules (`test-removed`, `assertion-removed`) require both fragments to parse cleanly (regression-tested). |
| 4 | **Justification laundering** (plan `fix-justification-laundering`) | "A marker keyword must not acquit a proven violation." We added `allow-added`, but a linter directive at the start of a line can still drop a finding on a code line (`DirectiveFilter` strips the prefix and matches). | **Partial** - a laundering probe test landed for a *trailing* directive, but a directive must not acquit a provable weakening on a *code* line: `DirectiveFilter` still applies to all findings with no comment-only gate, and no start-of-line probe exists. |
| 5 | **`CONCEPTS.md` vocabulary** — esp. **"Silent pass"** ("a hook must never claim a check it did not run") vs **fail-open** | Names the failure mode we guard against in `api.rs` (explicit `Skipping:` reasons, never a blanket success). | **Done** - both repos ship `CONCEPTS.md` (linked from both READMEs); degraded paths are covered per-path by tests asserting their explicit `Skipping:` reason (no single exhaustive test). |
| 6 | **Cited evidence on verdicts** (`RestateEvidence`) | Publishing the tokens a verdict rests on makes it checkable; ours carries only a `message`. | **Backlog** — include the matched marker(s) in the finding message/metadata. |
| 7 | **License-header / generated-file justification** (`JUSTIFIED`: `LicenseHeader`, `GeneratedFile`) | Our `comment-checker` filters (BDD/directive/shebang/agent-memo) would flag a `SPDX-License-Identifier` header or a generated file's banner. | **Done for `comment-checker`** (landed 2026-10-12) - a license-header + generated-file allow-list (`LICENSE_MARKERS`/`GENERATED_MARKERS` in its `src/filters.rs`, documented in its README). |
| 8 | **Setup/diagnostic skill + `doctor`** (`doctor.ts`; `command -v` + exit-code contract probes) | A binary that cannot resolve checks nothing; a doctor proves the wiring and the 0/2 contract end-to-end. | **Partial** - `scripts/doctor.sh` in both repos covers the resolve + exit-code probe; the npm script and setup skill remain. |
| 9 | **Ordered declarative rule tables** (`Rule { reason, matches }`, `JUSTIFIED`/`UNNECESSARY`) | Validates our `MARKER_RULES` table direction; a full ordered table with predicates is the natural next step for `src/rules.rs`. | **Direction confirmed** |

Not adopted: their workspace layout, pnpm/Effect launcher, Nix flake, changesets, and
turborepo/mutation CI — heavier tooling than these single-package repos need; take the ideas, not the
machinery.
