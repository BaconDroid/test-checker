# Agent notes: test-checker

The Rust crate at the repository root (`src/`, `tests/`, `benches/`, `npm/`) is the current
implementation.

## Commands (the same ones CI runs, `.github/workflows/ci.yml`)
- Format: `cargo fmt --all -- --check`
- Lint: `cargo clippy -p test-checker --all-targets --all-features -- -D warnings`
- Full test: `cargo test -p test-checker --all-targets --all-features`
- Focused test: `cargo test -p test-checker --test <name>`, where `<name>` is one of the files in `tests/*.rs`
- npm wrapper: `node --test npm/test-checker/*.test.js`
- Dependency audit: `cargo deny check`

## Module map
`src/api.rs` owns the hook models, the test gate, `evaluate`, and the JSON API. `src/detector.rs`
owns tree-sitter parsing (per-language `Runtime` + per-thread `Parser` caches, a ~50 ms parse
budget), test-body capture, comment capture (host + injected), and the edit diff. `src/rules.rs`
owns the rule catalog: one classifier per rule over the added lines / diff. `src/language.rs` owns
the extension→grammar map, the per-language test-body query, and the test-path globs.
`src/filters.rs` owns the BDD/directive/shebang allow-lists, the `test-checker: allow` suppression
marker, and the agent-memo phrase catalog. `src/output.rs` renders the grouped `<findings>` XML and
the dedicated agent-memo banner. `tests/contract.rs` is the single owner of the shared hook wire
contract (message prefix, exit-code mapping, XML shape, fail-open skip reasons).

## Dependency convention
Direct dependencies are exactly `serde` (data model + serialization), `serde_json` (wire I/O only),
`tree-sitter` (parsing and matching, via queries), and `tree-sitter-language-pack` (language,
grammar, and identification only). Do not add dynamic `serde_json::Value` modeling, `TreeCursor`
walks, or the pack's `get_parser`/`process`/runtime download. Keep direct dependencies to this set
unless a change is justified and convention-following.

Comments are collected with `tree-sitter` queries, including the pack's injection machinery, so a
commented-out test inside an embedded block is caught.

## Release platform touchpoints
Each supported platform (`darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`, `win32-x64`) is
named in six places — keep them in sync: `npm/test-checker/index.js` `SUPPORTED_PLATFORMS`,
`npm/test-checker/postinstall.js` `PLATFORM_MAP`, `npm/test-checker/package.json`
`optionalDependencies`, `.github/scripts/prepare-npm-packages.sh`,
`.github/scripts/verify-npm-packages.sh`, and the `.github/workflows/release.yml` build matrix.
`packages.test.js` ties the first three together.

## Test authoring gate
Before adding or changing a test, answer all four; a missing answer means do not add it yet:
1. What observable behavior, invariant, or independent contract does it protect?
2. What credible regression makes it fail?
3. Why does existing coverage not already catch that failure? Each contract has one primary test owner at the strongest boundary; extend a table case or shared fixture instead of adding a near-duplicate.
4. Does it need a production seam (export, flag, wrapper, injection hook) no production caller needs? If yes, test at the real boundary instead.
Junk patterns (reject unless the retention bar below applies): assertion-free probes; self-comparisons and identity copiers; copied fixtures, inventories, manifests or export lists; exact source, import or string greps; private call-shape tests duplicated at a real boundary; duplicate invocations of one contract; replays of a shared helper through a wrapper; tests that exist to keep a test-only export or wrapper alive; production code whose only callers are tests; expected values produced by the code under test; mocks that implement the asserted behavior; fixture-supplied receipts or postconditions the code under test should produce; assertions against a store nothing writes; capability or flag restatements without a delivery proof; negative controls that pass for an unrelated reason; names that promise more than the input exercises.
Retention bar: keep a pattern match only when it independently guards a public API, protocol, config, migration, storage, security, platform, default, prompt-byte, generated or cross-language, package, release, or architecture contract, and say which one in the test. Static or slow is never a deletion reason. A retained test that fails on the base is a product bug to fix at its owner, never a test to delete.
A bug regression test must fail on the pre-fix code for the intended reason; an existing owner test that already fails may serve as that proof.
Method source: openclaw test-audit skill.

## Coverage contract
`tests/detector.rs` holds the 31-language pipeline coverage table: every language must resolve its
grammar, clean fixtures must produce no findings, and the languages with a test-body query must
compile that query and capture a body. Only languages with a framework produce body findings;
`commented-out-test` works everywhere.

## Parity harness
`parity/` is shared byte-for-byte with `BaconDroid/comment-checker`: `check.py` is the cross-repo
drift check, `vectors.json` is the shared hook-contract corpus, and `tests/parity.rs` runs it. CI
runs `python3 parity/check.py` (the `parity` job). When a shared file changes, change the sibling in
the same wave.

## Related projects
See `docs/related-projects.md`. Base/upstream: [`BaconDroid/comment-checker`](https://github.com/BaconDroid/comment-checker)
(shared engine + scaffolding) and its upstream [`code-yeongyu/go-claude-code-comment-checker`](https://github.com/code-yeongyu/go-claude-code-comment-checker).
An independent Rust implementation, [`systemfsoftware/comment-checker`](https://github.com/systemfsoftware/comment-checker),
is tracked there for evaluated ideas (F1 corpus, mutation gate, fragment-unreliability, justification
laundering, license-header/generated-file justification, setup doctor).
