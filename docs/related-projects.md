# Related projects

Other comment/test hooks worth tracking. Review each by hand and port only convention-following,
non-superseded ideas.

| Project | Relationship | Notes |
|---|---|---|
| [`code-yeongyu/go-claude-code-comment-checker`](https://github.com/code-yeongyu/go-claude-code-comment-checker) | Upstream of `BaconDroid/comment-checker` | The original Go hook; the Rust rewrite dropped its `go/` tree, `clap`/`criterion`/`tempfile`, and bindings. |
| [`BaconDroid/comment-checker`](https://github.com/BaconDroid/comment-checker) | Sibling of this repo | Shares `src/engine.rs`, the CLI, npm/CI/release and the `parity/` harness byte-for-byte (modulo names). |
| [`systemfsoftware/comment-checker`](https://github.com/systemfsoftware/comment-checker) | Independent Rust implementation (Apache-2.0) | A more elaborate, mutation-tested classifier (workspace, 37 languages, F1 corpus, setup skill). Ideas to mine below; **no code copied** (different architecture and license). |

## Ideas from `systemfsoftware/comment-checker` (evaluated)

| # | Idea (where) | Why it helps us | Status |
|---|---|---|---|
| 1 | **F1 evaluation corpus + per-kind floors** (`eval/corpus.json`, `tests/f1.rs`) | We ship 25 rules with a coverage/unit suite but no *precision* corpus; a per-kind gate stops one weak rule hiding inside an aggregate. | **Backlog** — add an `eval/corpus.json` of (fixture, expected rule) plus a threshold test with per-rule floors. |
| 2 | **Mutation testing of the classifier** (`mutants.toml`, `mutation` workflow; `cargo mutants`) | Proves the rules are actually exercised (a surviving mutant = a missing test). | **Backlog** — run `cargo mutants` over `src/rules.rs` in a non-blocking job first. |
| 3 | **Fragment unreliability** (`classify.rs`, `CONCEPTS.md`, plan U0) | On `Edit`/`MultiEdit` the input is a fragment: adjacent context is truncated, so context-aware verdicts must fail open there. Our diff rules (`test-removed`, `assertion-removed`) read old/new fragments and can misread a test that moved out of the edited fragment. | **Adopt in Workstream A** — do not convict context-dependent rules when the fragment boundary could hide the counterpart. |
| 4 | **Justification laundering** (plan `fix-justification-laundering`) | "A marker keyword must not acquit a proven violation." We added `allow-added`, but a linter directive at the start of a line can still drop a finding on a code line (`DirectiveFilter` strips the prefix and matches). | **High-value backlog** — a directive must not acquit a provable weakening on a *code* line; keep directive immunity for comment-only findings. Add a laundering probe test. |
| 5 | **`CONCEPTS.md` vocabulary** — esp. **"Silent pass"** ("a hook must never claim a check it did not run") vs **fail-open** | Names the failure mode we guard against in `api.rs` (explicit `Skipping:` reasons, never a blanket success). | **Adopt** — add a short `CONCEPTS.md` and a test that every degraded path is an explicit skip. |
| 6 | **Cited evidence on verdicts** (`RestateEvidence`) | Publishing the tokens a verdict rests on makes it checkable; ours carries only a `message`. | **Backlog** — include the matched marker(s) in the finding message/metadata. |
| 7 | **License-header / generated-file justification** (`JUSTIFIED`: `LicenseHeader`, `GeneratedFile`) | Our `comment-checker` filters (BDD/directive/shebang/agent-memo) would flag a `SPDX-License-Identifier` header or a generated file's banner. | **Backlog for `comment-checker`** — add a license-header + generated-file allow-list. |
| 8 | **Setup/diagnostic skill + `doctor`** (`doctor.ts`; `command -v` + exit-code contract probes) | A binary that cannot resolve checks nothing; a doctor proves the wiring and the 0/2 contract end-to-end. | **Backlog** — add an npm `doctor` script (resolve + exit-code probe); optional setup skill. |
| 9 | **Ordered declarative rule tables** (`Rule { reason, matches }`, `JUSTIFIED`/`UNNECESSARY`) | Validates our `MARKER_RULES` table direction; a full ordered table with predicates is the natural next step for `src/rules.rs`. | **Direction confirmed** |

Not adopted: their workspace layout, pnpm/Effect launcher, Nix flake, changesets, and
turborepo/mutation CI — heavier tooling than these single-package repos need; take the ideas, not the
machinery.
