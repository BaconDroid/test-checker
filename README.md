# test-checker

A PostToolUse hook for Claude Code / OpenCode that reports tests an AI agent has weakened,
hollowed out, or left behind as scaffolding.

## Why this exists

When an agent implements or debugs something, it can read the code, run it and iterate. It
does not need a pile of tests to understand what is going on, but it tends to leave one
anyway: probes with no assertions, copies of an existing test, echoes of the
implementation, tests that grep source files, mocks of the code under test. Worse, when a
test fails it may weaken, skip or delete the test to get green.

test-checker reports both kinds of change:

- **Integrity**: weakened, hollowed-out, skipped, focused or removed tests.
- **Bloat**: duplicate, near-duplicate, tautological and other superfluous tests.

It is the building block for a plugin that keeps AI-written test suites small and
trustworthy.

## What it does (and does not do)

`PostToolUse` runs **after** the tool has written the file. On a finding the binary exits `2` and
writes its message to stderr; Claude Code feeds that message back to the agent, which is expected
to revise the file. The default hook cannot prevent or undo the write, and it never edits the file
itself. The explicit `--strip` cleanup pass is the one exception: it removes only candidates a human
approved in `.test-checker.json` (see [Removal policy](#removal-policy)).

This is a **speed bump, not a security boundary**. The agent has a shell: it can edit files
directly, disarm or remove the hook, or write its own suppression, and any of those bypasses the
check. The hook's job is to make weakening visible in the transcript and ask the agent to stop; it
does not and cannot constrain a determined agent. An intended removal is approved by a **human**
through a committed `.test-checker.json` entry with a reason — the agent must stop and ask for that,
never grant it to itself.

| | Status |
|---|---|
| Detect and report weakened or superfluous tests | works today (PostToolUse, exit `2`) |
| Prevent or undo the write | not possible from `PostToolUse`; exit `2` is feedback after the write |
| Remove: cleanup pass over approved candidates | works today (`--strip` + a committed `.test-checker.json` `remove` entry) |
| Plug in: one plugin combining this and comment-checker | works today (BaconDroid/opencode-comments-plugin) |

## Philosophy: scaffolding vs guard

- **Scaffolding (flagged)**: debugging probes, duplicates, tautologies, tests with no
  assertion, source greps, mocks of the thing under test, bulk dumps of near-identical tests.
- **Guards (spared)**: tests that pin behavior, regressions of real bugs, boundary and
  contract tests, with the intent readable from the test name.
- **Never weaken a test to get green.** If the test is wrong, stop and tell the human.
- **One test per behavior, not per incident.** Regressions of the same invariant pile into
  a table of cases, not into sibling test functions; merge the bodies, never the assertions.
  `near-duplicate-test`, `echo-test` and `test-volume` are merge mandates, not telemetry to
  leave hidden behind the default confidence gate.
- **Lean, not zero**: tests are also the oracle that keeps an agent honest. Fewer tests
  must never mean weaker checks: lean shrinks the lines around the checks, never the checks.

## Removal policy

The hook reports; it does not delete tests by itself. Removal writes happen only in the explicit
`--strip` cleanup pass, and only for candidates a **human** approved in a committed
`.test-checker.json` entry with a reason at least 12 characters long. An agent can never grant
removal to itself: it must stop and ask for the entry, and editing the config is itself reported as
a high `config-edited` finding.

- **Candidates**: only the file-level superfluity rules `duplicate-test`,
  `near-duplicate-test`, `tautological-assertion`, `no-assertion`, `source-grep-test`,
  `gutted-test`, `echo-test`, and `mirror-implementation-test`. Integrity rules (`skip-added`,
  `weakened-assertion`, `assertion-removed`, `test-removed`, ...) are never removable.
- **Approval**: a candidate is removable only when its file matches the glob and its rule is listed
  in a valid `remove` entry. No matching entry means no removal, always. For a duplicate pair, the
  flagged copy (the later body) is the one removed.
- **Unit**: the whole enclosing test block, including its attribute/marker lines (Rust `#[test]`,
  Python decorators, Elixir tags, Groovy decorators). Overlapping removals collapse into one span;
  every other byte of the file stays identical.
- **Write**: the new content is built in memory and written once. On an ambiguous span or a write
  error nothing is touched, and the message says why.
- **Output**: the normal report is followed by a cleanup summary listing removed items
  (`test name [rule]`) and candidates kept awaiting approval, plus the `.test-checker.json`
  `remove` pointer. Exit codes are unchanged: `0` clean, `2` findings, `1` did not run.

```json
{
  "remove": [
    {
      "glob": "tests/**",
      "rules": ["duplicate-test", "near-duplicate-test"],
      "reason": "legacy suite merged into the table-driven cases in #123"
    }
  ]
}
```

## Cleanup candidates

`scripts/cleanup-candidates [root]` feeds each test file to the real binary as a whole-file `Write`
and aggregates the **file-level** superfluity findings (`duplicate-test`, `near-duplicate-test`,
`tautological-assertion`, `no-assertion`, `source-grep-test`, `gutted-test`, `echo-test`,
`mirror-implementation-test`). It prints a JSON report envelope - `root`, `files_scanned`,
`total_candidates`, `candidates_by_rule`, and a `candidates` array whose entries are shaped
`{file, test_name, line, rule, confidence, reason}` - sorted by confidence, then a summary on
stderr. It always exits 0 unless the arguments are invalid: a root that is not a directory exits 2.

It never modifies files. Diff-based integrity rules are not part of the report.

`scripts/cleanup-pass --binary path/to/test-checker [root]` is the driver for the removal step. It
walks the same test files, feeds each one to the binary with `--strip --min-confidence low`, and
prints what was removed and what was kept awaiting approval. It never edits files itself; only the
binary writes, and only for approved candidates. Like `cleanup-candidates`, it exits 0 unless the
arguments are invalid.

The workflow keeps the human in the loop:

1. Report: run `scripts/cleanup-candidates --binary path/to/test-checker`.
2. Approve: a human adds a `remove` entry (`glob`, `rules`, `reason`) to `.test-checker.json` and
   commits it. The reason must be at least 12 characters.
3. Remove: run `scripts/cleanup-pass --binary path/to/test-checker`. Anything without a matching
   `remove` entry is reported as kept and left in place.

## install

This fork publishes no npm package or Homebrew formula. Build the current code from source:

```bash
git clone https://github.com/BaconDroid/test-checker
cd test-checker
cargo install --path . --locked
```

Rust 1.85+ and a C compiler are required (the tree-sitter grammars are C).

GitHub Releases carry prebuilt binaries for the five platforms (`v0.3.1` at the time of writing);
they can lag the source tree, so build from source when you want the latest behavior. The older
`v0.2.0` binaries predate the Rust rebuild of the detector, and the `v0.3.0` release failed to
build assets - neither is current. The npm packages under `npm/` are current packaging sources,
but they are not published.

## Setup

register it as a `PostToolUse` hook for `Write|Edit|MultiEdit`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "if command -v test-checker >/dev/null 2>&1; then exec test-checker; fi; cat >/dev/null; echo '[test-checker] did not run - nothing checked this write.' >&2; exit 1"
          }
        ]
      }
    ]
  }
}
```

use the wrapper above, **not** a bare `test-checker`. A bare command exits `127` without reading
stdin when the binary is not on `PATH`, so the edit goes unchecked and you are never told — a silent
pass. The wrapper resolves the binary, passes its exit code through, and only when the binary is
missing drains stdin (so a large payload never raises SIGPIPE) and exits `1` with `did not run`.
`tests/readme_hook.rs` runs this exact command text, so weakening it (a bare command,
`|| exit 0`, …) fails the build.

the wrapper assumes a POSIX shell: `command -v`, `cat`, and `>&2` must exist. On Windows without one,
wire the binary directly (`test-checker.exe`) or run it through a shell that provides them; this
command text does not claim native Windows support.

## Contract

- **stdin**: `{ "tool_name": "Write"|"Edit"|"MultiEdit", "tool_input": { "file_path"?, "content"?,
  "old_string"?, "new_string"?, "edits"? }, "is_test_file"? }`.
  Missing or `null` fields fall back to defaults; unknown fields are ignored. The camelCase aliases
  `toolName`, `toolInput`, `filePath`, `oldString`, `newString`, and `isTestFile` are accepted.
  `apply_patch` is treated as an `Edit`.
- **stdout**: empty on a hook run. `--version` prints the name and version to stdout.
- **stderr**: the message. Findings are rendered as grouped XML:

  ```xml
  <findings file="tests/test_app.py">
  	<finding line-number="7" rule="skip-added" confidence="high" reason="skip or ignore marker added">A test/assertion was disabled with a skip/ignore marker.</finding>
  </findings>
  ```

  `reason` is a short stable phrase derived from `rule`.

- `--prompt <text>` replaces the default message; `{{findings}}` is substituted with the XML. Without
  the placeholder the custom text replaces the whole message, including the agent-memo banner.
- `--min-confidence <low|medium|high>` (default `medium`) drops lower-confidence findings, so the
  hook only exits `2` for findings at or above that level. An unrecognised value silently falls back
  to `medium`.
- `--version` prints `test-checker <version>` and exits `0`.
- `--strip` runs the cleanup pass. It resolves `tool_input.file_path`/`filePath` against the payload
  `cwd` (the process cwd when absent). If the file exists, the whole file on disk is analyzed - the
  payload only names it; if the file is missing, the payload content is analyzed and never rewritten.
  Only candidates approved by a `remove` entry in `<git root>/.test-checker.json` are removed; the
  message appends a cleanup summary, and exit codes stay `0`/`2`/`1` as below.
- **Write baseline (opt-in, off by default)**: set `TEST_CHECKER_WRITE_BASELINE=1` in the `env` block
  of `.claude/settings.json` (or the hook's environment) to let a whole-file `Write` diff against
  `git show HEAD:<path>`. Enabling it adds about **7 ms p95** to a `Write`; nothing is spawned when it
  is off.

Clean runs print `[test-checker] Success: No problematic tests found`; skipped runs print
`[test-checker] Skipping: <reason>`.

## exit codes

| code | meaning |
|------|---------|
| 0 | pass, or a deliberate skip (skips print a visible `Skipping:` line) |
| 2 | at least one finding remains after filters and the confidence gate |
| 1 | did not run - an internal failure, or the wrapper could not run the binary; nothing was checked |

Malformed, empty, unknown-tool, and non-test payloads exit `0` with a `Skipping:` line. A panic is
not caught: release builds use `panic = "abort"`, so a crash aborts with a non-zero status.

## Test gate

A test file is checked when its language resolves **and** the test gate admits it. The payload's
explicit `is_test_file` wins when present in either direction. Without it, a test-path match admits
the file: a path segment `tests`, `test`, `__tests__`, `spec`, or `specs`; a file starting with
`test_` or `test-`; a file containing `_test.`, `.test.`, `.spec.`, or `_spec.`; or a Java/Kotlin
class name starting or ending with `Test`. A file outside every one of those patterns is still
admitted when the change contains at least one detected test body (for example the tests inside an
inline `#[cfg(test)] mod tests`, or a `describe`/`it` block in a production JavaScript file). The
semantic probe runs only after the hint and the path table both decline, so a normal test-path edit
pays no parse; a probe that exceeds the 50 ms parse budget is a visible skip, not a block.

A file whose extension maps to a grammar this build does not ship is skipped as `Non-code file`
before the test gate, even if the payload hint says it is a test file. A path that is not a test file
but is a harness/config file is checked without language resolution:
`pytest.ini`, `tox.ini`, `conftest.py`, `setup.cfg`, `pyproject.toml`, `package.json`, `Makefile`,
`codecov.yml`, `jest.config.*`, `vitest.config.*`, `karma.conf.*`, `.nycrc*`,
`.github/workflows/**`, `.gitlab-ci*`.

The test-path and harness-path tables are the constants in `src/language.rs`.
`eval/path-predicates.json` is generated from them and committed;
`scripts/generate-path-predicates` rewrites it, and the
`path_predicates_contract_matches_language_tables` unit test fails on any drift. The cleanup
scripts and `eval/replay.py` read that committed JSON, so their file selection follows the binary
instead of keeping a private regex copy.

An added non-blank line of a harness file yields one `harness-edited` finding on the first such line
(`medium` on CI workflows, `.gitlab-ci*`, `pytest.ini`, `tox.ini`, `conftest.py`, `jest.config.*`,
and `vitest.config.*`; `low` on the other harness files). A non-comment added line that matches a
gate marker (`|| true`, `|| exit 0`, `continue-on-error: true`, `allow_failure: true`,
`--passWithNoTests`, `set +e`) also yields `gate-weakened` (`medium`).

## Languages

Every language compiled by `tree-sitter-language-pack` (35, listed in `.cargo/config.toml`'s
`TSLP_LANGUAGES`) is parsed. A language yields body-based findings only when it has a real test
framework and a compiling test-body query (21): python, javascript, typescript, tsx, go, rust,
ruby, java, csharp, php, kotlin, swift, scala, elixir, lua, c, cpp, groovy, ocaml, elm, bash.
`.bats` files use the bash grammar. Other languages (`dockerfile`, `hcl`, `proto`, `toml`, `yaml`,
`css`, `html`, `cue`, …) parse cleanly and report nothing body-based. The `commented-out-test`
signal runs over comment captures for every language, including comments inside injected regions
that the compiled host grammar exposes (for example, a `<script>` block in HTML).

## Rules

`skip-added`, `focus-added`, `assertion-removed`, `test-removed`, `gutted-test`, `no-assertion`,
`tautological-assertion`, `constant-assertion`, `weakened-assertion`, `swallowed-error`,
`incomplete-assertion`, `unawaited-assertion`, `broad-exception`, `mock-of-sut`, `over-mocking`,
`duplicate-test`, `expected-copied-from-actual`, `snapshot-regenerated`, `tolerance-broadened`,
`conditional-assertion`, `retry-added`, `agent-memo-test`, `allow-added`, `allow-invalid`,
`config-edited`, `gate-weakened`, `commented-out-test`, `source-grep-test`, `test-volume`,
`harness-edited`, `near-duplicate-test`, `echo-test`, `mirror-implementation-test`.

Confidence policy: `source-grep-test` is `low` telemetry and fires only when an assertion inspects
the **text** of a read source file (substring/`includes`, regex/`toMatch`, or snapshot); reading a
missing file or asserting a parsed property does not fire. `harness-edited` is `medium` on sensitive
harness paths (CI workflows, `.gitlab-ci*`, pytest/tox/conftest, jest/vitest config) and `low`
otherwise; `test-volume`, `near-duplicate-test`, `echo-test` and `mirror-implementation-test` are
`low` telemetry, so the default `medium` gate hides them. Use `--min-confidence low` to see them and
block on them.

Each finding carries a `confidence` of `high`/`medium`/`low`. Line rules classify the added lines of
the captured test bodies (comments stripped) against a **masked view**: the content of string
literals, docstrings, template strings, raw strings and heredocs is replaced by spaces, preserving
newlines and byte offsets so line numbers do not move. A `@pytest.mark.skip` or `.only` that appears
only inside a string is therefore data, not a disabled or focused test. The per-language string node
kinds live in one table (`src/language.rs`), with a fixture per language. Comment-based rules
(`commented-out-test`, `agent-memo-test`) keep reading the original text; the rules that read literal
targets (`source-grep-test`, `mock-of-sut`) locate the call in the view and resolve the file or module
literal from the original line. Diff rules compare an edit's old and new content.

On an `Edit`/`MultiEdit` fragment, whole-test rules (`gutted-test`, `over-mocking`, `no-assertion`,
`duplicate-test`, `near-duplicate-test`, `echo-test`) fire only when the fragment contains the
complete test; line-local rules still apply.

Three rules — `weakened-assertion`, `snapshot-regenerated`, `tolerance-broadened` — are
**diff-aware**. They compare a declarative matcher-strength table (JS/TS `toStrictEqual` > `toEqual`
> `toBe` > truthiness/definedness; Python `assertEqual`/`==` > `assertIsNotNone`/`assertTrue`/`is
not None`; snapshot equality → `toMatchSnapshot`; numeric `places`/precision/`delta`/sleep) and fire
only when a stricter form for the **same assertion subject** was replaced by a looser one. If the
subject cannot be aligned, no finding is produced (precision over recall). On an `Edit` each rule
reports at its own confidence: `weakened-assertion` is `medium` (`high` when an exact/deep equality
became a truthiness/existence check), `snapshot-regenerated` is always `high`, and
`tolerance-broadened` is `medium`. On a `Write` **without** the `TEST_CHECKER_WRITE_BASELINE=1`
opt-in (see Contract) there is no baseline, so the same patterns report at `low` confidence - below
the default gate, so a brand-new test is not blocked for using a loose matcher. With the opt-in on,
the `Write` diffs against `git show HEAD:<path>` and reports at the same `medium`/`high` confidences
as an `Edit`.

## Allow-list / suppression

- **BDD keywords** (`given`, `when`, `then`, `arrange`, `act`, `assert`, …) are allowed when the
  finding's line is exactly that keyword after a comment marker.
- **Directives** (`noqa`, `ts-ignore`, `eslint-disable`, `clippy:`, `allow`, …) are allowed by
  prefix match.
- **Shebang lines** (`#!…`) are allowed.
- **Scoped opt-out marker.** `test-checker: allow <rule> -- <reason>` suppresses that rule's finding
  on the marker's line or the line above. The marker is valid only when `<rule>` is a known rule id and
  the reason after `--` has at least 12 characters. A bare marker, an unknown rule, or a short reason
  suppresses nothing and is itself reported as `allow-invalid`. A marker added **in the same change**
  does not suppress it and is reported as `allow-added`.
- **Markers are inert on a whole-file `Write`.** On a `Write` every line counts as added, so a
  marker in the written content neither suppresses any finding nor is exempt from reporting: it is
  itself reported as `allow-added`, and its rule still blocks. Marker suppression only ever applies
  to `Edit`/`MultiEdit` fragments (above).
- **File-wide suppression.** Commit a `.test-checker.json` at the repo root. The hook walks up from
  the payload `cwd` to the first directory containing `.git` and reads the config only there, so a
  config in a subdirectory is ignored. Only entries with a glob, at least one rule id, and a non-empty
  reason suppress anything:

  ```json
  { "allow": [ { "glob": "tests/**", "rules": ["skip-added"], "reason": "legacy suite, tracked in #123" } ] }
  ```

  A file whose path matches a glob and whose rule is listed is suppressed. Editing `.test-checker.json`
  itself is reported as a **high** `config-edited` finding. The reason is the human's approval of an
  intended removal; the agent must ask for the entry instead of adding one.

  A second top-level array, `"remove"`, is the human approval for the `--strip` cleanup pass. Its
  entries have the same `{glob, rules, reason}` shape and the same git-root-only discovery, but the
  reason must be at least **12 characters**; an entry that fails validation is dropped. A cleanup
  candidate is removed only when its file matches the glob and its rule is listed there. See
  [Removal policy](#removal-policy).
- `scripts/doctor.sh` lists old bare `test-checker: allow` markers that no longer suppress.

`agent-memo-test` findings are additionally summarised under a dedicated banner, pointing at the test
name/assertion message that describes a change instead of the behavior. `--prompt` replaces that
banner as well.

## Limits and false-positive risks

- **Parse failures never print `Success`.** If the grammar or a query cannot load, the detector
  fails: the hook exits `1` with `did not run` and nothing was checked (per the exit table above).
  A parse that exceeds the 50 ms budget aborts that file and exits `0` with a visible
  `Skipping: Parse budget exceeded` line - a skipped file is not proof it is clean. Neither path is
  a silent pass, and only a genuinely clean run prints `Success`.
- **Harness edits always report.** On a sensitive harness path, an added non-blank line yields
  `harness-edited` at `medium`, so routine CI/workflow edits block until the agent justifies them.
- **Rules are textual.** Body line rules run on comment-stripped lines against the masked view
  (string literals blanked - see above), but comment-based rules (`commented-out-test`,
  `agent-memo-test`) read the original text, harness-file lines are matched raw (no parser runs
  there), and a language with no body query is never masked because no body rule can fire in it.
  Separately, several markers are substrings: a test name or message containing a marker-like
  phrase can be flagged.
- **Body rules need a captured test body.** A test framework whose body is not captured by the 21
  language queries yields no body findings, only `commented-out-test`.
- **Deletion findings point at line 1.** On the edit-diff path, `test-removed`, `gutted-test`, and
  `assertion-removed` describe a line that no longer exists, so they report line `1` instead of the
  old line.
- **Suppression is fragment-scoped** for `Edit`/`MultiEdit` and does not apply to a whole-file
  `Write` (every line counts as added there); see above.
- **This is a detector by default.** It does not fix or rewrite tests unless invoked explicitly with
  `--strip`, which only removes candidates a human approved in `.test-checker.json`.

## Scope and non-goals

`test-checker` targets **test weakening** (a change that disables, hollows out, or loosens a test
while still passing) and **bad or surplus tests** (no assertion, tautological/constant assertions,
swallowed errors, over-mocking, duplicates). It is deliberately **not**:

- **production code written to pass the tests** (a test-mode branch, a constant `__eq__`, stack
  introspection, cross-file hardcoding). Detecting that needs cross-file heuristics or a judge, not
  a per-write hook; a separate `prod-checker` would own it.
- **rules over Bash commands** (`git log -S`, `curl *.patch`, `-u` snapshot regeneration): a
  different matcher, a different binary.
- **`tests-not-run`**: not observable from a `Write`/`Edit`.
- **classic test smells** (Assertion Roulette, Magic Number, Eager, Long): noisy on generated tests.

The shared vocabulary (verdict, reason, evidence, unreliable, floor, and the difference between
fail-open and a silent pass) is in [`CONCEPTS.md`](CONCEPTS.md).

## Library

```rust
use test_checker::api::detect_findings_json;

let json = detect_findings_json(content, "tests/test_app.py", None, true, true);
```

The two toggles mirror comment-checker's `include_docstrings`: `include_commented_out_tests` gates
the `commented-out-test` signal, and `include_body_tests` gates the body-based rules. The third
argument is the payload's `is_test_file` hint (`None` when the caller has none); admission then
follows the same rule as the hook, accepting a hint, a test-path match, or a detected test body in
`content`. The function applies the language and test-admission gate (returning `"[]"` for a
non-analyzable or non-admitted path) but **not** the allow-list filters or the confidence gate;
callers that need hook-equivalent output must apply those themselves.

## Development

```bash
cargo fmt --all -- --check
cargo clippy -p test-checker --all-targets --all-features -- -D warnings
cargo test -p test-checker --all-targets --all-features
cargo bench -p test-checker --bench test_checker --no-run
node --test npm/test-checker/*.test.js
```

`eval/baseline.py` is a manual perf gate; it is not run in CI. See `eval/README.md`.

## License

MIT.
