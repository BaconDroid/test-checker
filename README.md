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
to revise the file. The hook cannot prevent or undo the write, and it never edits the file itself.

| | Status |
|---|---|
| Detect and report weakened or superfluous tests | works today (PostToolUse, exit `2`) |
| Prevent or undo the write | not possible from `PostToolUse`; exit `2` is feedback after the write |
| Remove: cleanup pass over approved candidates | planned (plugin) |
| Plug in: one plugin combining this and comment-checker | planned |

## Philosophy: scaffolding vs guard

- **Scaffolding (flagged)**: debugging probes, duplicates, tautologies, tests with no
  assertion, source greps, mocks of the thing under test, bulk dumps of near-identical tests.
- **Guards (spared)**: tests that pin behavior, regressions of real bugs, boundary and
  contract tests, with the intent readable from the test name.
- **Never weaken a test to get green.** If the test is wrong, stop and tell the human.
- **Lean, not zero**: tests are also the oracle that keeps an agent honest. Fewer tests
  must never mean weaker checks.

## Removal policy

test-checker reports; it does not delete tests by itself. Removal is meant to happen in a
cleanup pass driven by the plugin, on a list a human approves. No cleanup or stripping feature is
implemented in this tree.

## install

This fork publishes no npm package or Homebrew formula. Build the current code from source:

```bash
git clone https://github.com/BaconDroid/test-checker
cd test-checker
cargo install --path . --locked
```

Rust 1.85+ and a C compiler are required (the tree-sitter grammars are C).

The `v0.2.0` release binaries predate the Rust rebuild of the detector and are not current. The npm
packages under `npm/` are current packaging sources, but they are not published. Build from source
unless you specifically want that older release.

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
- `--strip` is accepted for forward compatibility but is **inert**: a notice is printed to stderr and
  the file is never rewritten.

Clean runs print `[test-checker] Success: No problematic tests found`; skipped runs print
`[test-checker] Skipping: <reason>`.

## exit codes

| code | meaning |
|------|---------|
| 0 | pass, or a deliberate skip (skips print a visible `Skipping:` line) |
| 2 | at least one finding remains after filters and the confidence gate |
| 1 | **wrapper only**: the binary was missing, so nothing was checked. The binary itself never exits `1` |

Malformed, empty, unknown-tool, and non-test payloads exit `0` with a `Skipping:` line. A panic is
not caught: release builds use `panic = "abort"`, so a crash aborts with a non-zero status.

## Test gate

A test file is checked when its language resolves **and** the test gate matches. The payload's
explicit `is_test_file` wins when present (including `false`), otherwise test-path globs apply: a
path segment `tests`, `test`, `__tests__`, `spec`, or `specs`; a file starting with `test_` or
`test-`; or a file containing `_test.`, `.test.`, `.spec.`, or `_spec.`.

A path that is not a test file but is a harness/config file is checked without language resolution:
`pytest.ini`, `tox.ini`, `conftest.py`, `setup.cfg`, `pyproject.toml`, `package.json`, `Makefile`,
`codecov.yml`, `jest.config.*`, `vitest.config.*`, `karma.conf.*`, `.nycrc*`,
`.github/workflows/**`, `.gitlab-ci*`.

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
`conditional-assertion`, `retry-added`, `agent-memo-test`, `allow-added`, `gate-weakened`,
`commented-out-test`, `source-grep-test`, `test-volume`, `harness-edited`, `near-duplicate-test`,
`echo-test`, `mirror-implementation-test`.

Confidence policy: `source-grep-test` is `medium`; `harness-edited` is `medium` on sensitive harness
paths (CI workflows, `.gitlab-ci*`, pytest/tox/conftest, jest/vitest config) and `low` otherwise;
`test-volume` and
`near-duplicate-test` are `low` telemetry, so the default `medium` gate hides them. Use
`--min-confidence low` to see them and block on them.

Each finding carries a `confidence` of `high`/`medium`/`low`. Line rules classify the added lines of
the captured test bodies (comments stripped); diff rules compare an edit's old and new content.

Three rules — `weakened-assertion`, `snapshot-regenerated`, `tolerance-broadened` — are
**diff-aware**. They compare a declarative matcher-strength table (JS/TS `toStrictEqual` > `toEqual`
> `toBe` > truthiness/definedness; Python `assertEqual`/`==` > `assertIsNotNone`/`assertTrue`/`is
not None`; snapshot equality → `toMatchSnapshot`; numeric `places`/precision/`delta`/sleep) and fire
only when a stricter form for the **same assertion subject** was replaced by a looser one. If the
subject cannot be aligned, no finding is produced (precision over recall). On an `Edit` the finding
is `medium` (`high` when an exact/deep equality became a truthiness/existence check); on a `Write`
there is no baseline, so the same patterns report at `low` confidence — below the default gate, so
a brand-new test is not blocked for using a loose matcher.

## Allow-list / suppression

- **BDD keywords** (`given`, `when`, `then`, `arrange`, `act`, `assert`, …) are allowed when the
  finding's line is exactly that keyword after a comment marker.
- **Directives** (`noqa`, `ts-ignore`, `eslint-disable`, `clippy:`, `allow`, …) are allowed by
  prefix match.
- **Shebang lines** (`#!…`) are allowed.
- **Opt-out marker** `test-checker: allow` suppresses findings in the analyzed text: the whole
  `Write` content, or the edited fragment for `Edit`/`MultiEdit`. It does not read the rest of the
  file, so a marker elsewhere in the file has no effect on an edit. Adding the marker on an added
  line is reported as `allow-added` (`medium`, line 1, not suppressed), and a `Write` that contains
  the marker anywhere always produces `allow-added`.

`agent-memo-test` findings are additionally summarised under a dedicated banner, pointing at the test
name/assertion message that describes a change instead of the behavior. `--prompt` replaces that
banner as well.

## Limits and false-positive risks

- **Parse failures fail open.** If the grammar cannot load or a parse exceeds the 50 ms budget, the
  detector returns no findings and the hook prints `Success`. That is a silent pass, not proof the
  file is clean.
- **Harness edits always report.** On a sensitive harness path, an added non-blank line yields
  `harness-edited` at `medium`, so routine CI/workflow edits block until the agent justifies them.
- **Rules are textual.** Markers, matchers, and memo phrases are matched on comment-stripped lines,
  but string literals are not masked, and several markers are substrings. A test name or message
  containing a marker-like phrase can be flagged.
- **Body rules need a captured test body.** A test framework whose body is not captured by the 21
  language queries yields no body findings, only `commented-out-test`.
- **Deletion findings point at line 1.** On the edit-diff path, `test-removed`, `gutted-test`, and
  `assertion-removed` describe a line that no longer exists, so they report line `1` instead of the
  old line.
- **Suppression is fragment-scoped** for `Edit`/`MultiEdit`; see above.
- **This is a detector.** It does not fix, delete, or rewrite tests.

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

let json = detect_findings_json(content, "tests/test_app.py", true, true);
```

The two toggles mirror comment-checker's `include_docstrings`: `include_commented_out_tests` gates
the `commented-out-test` signal, and `include_body_tests` gates the body-based rules. The function
applies the language and test-path gate (returning `"[]"` for a non-test path) but **not** the
allow-list filters or the confidence gate; callers that need hook-equivalent output must apply
those themselves.

## Development

```bash
cargo fmt --all -- --check
cargo clippy -p test-checker --all-targets --all-features -- -D warnings
cargo test -p test-checker --all-targets --all-features
cargo bench -p test-checker --bench test_checker --no-run
node --test npm/test-checker/*.test.js
```

## License

MIT.
