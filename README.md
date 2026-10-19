# test-checker

A Claude Code / OpenCode hook that keeps AI-written tests lean and honest.

## Why this exists

When an agent implements or debugs something, it can read the code, run it and iterate. It
does not need a pile of tests to understand what is going on, but it tends to leave one
anyway: probes with no assertions, copies of an existing test, echoes of the
implementation, tests that grep source files, mocks of the code under test. Worse, when a
test fails it may weaken, skip or delete the test to get green.

test-checker blocks both, at write time (exit 2, so the agent revises):

- **Integrity**: weakened, hollowed-out, skipped, focused or removed tests.
- **Bloat**: duplicate, near-duplicate, tautological and other superfluous tests.

It is the building block for a plugin that keeps AI-written test suites small and
trustworthy.

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
cleanup pass driven by the plugin, on a list a human approves.

## Prevent, remove, plug in

| | Status |
|---|---|
| Prevent: block weakened and superfluous tests at write time | works today (PostToolUse hook) |
| Remove: cleanup pass over approved candidates | planned (plugin) |
| Plug in: one plugin combining this and comment-checker | planned |

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
pass. The wrapper resolves the binary, passes `0`/`2` through, and otherwise drains stdin (so a large
payload never raises SIGPIPE) and exits `1` with `did not run`. `tests/readme_hook.rs` runs this exact
command text, so weakening it (a bare command, `|| exit 0`, …) fails the build.

the wrapper assumes a POSIX shell: `command -v`, `cat`, and `>&2` must exist. On Windows without one,
wire the binary directly (`test-checker.exe`) or run it through a shell that provides them; this
command text does not claim native Windows support.

## Contract

- **stdin**: `{ "tool_name": "Write"|"Edit"|"MultiEdit", "tool_input": { "file_path"?, "content"?,
  "old_string"?, "new_string"?, "edits"? }, "is_test_file"? }`.
  Missing or `null` fields fall back to defaults; unknown fields are ignored.
- **stdout**: empty.
- **stderr**: the message. Findings are rendered as grouped XML:

  ```xml
  <findings file="tests/test_app.py">
  	<finding line-number="7" rule="skip-added" confidence="high" reason="skip or ignore marker added">A test/assertion was disabled with a skip/ignore marker.</finding>
  </findings>
  ```

- `--prompt <text>` replaces the default message; `{{findings}}` is substituted with the XML.
  `--min-confidence <low|medium|high>` (default `medium`) drops lower-confidence findings, so the
  hook only exits 2 for findings at or above that level.
- **exit**: `2` when findings remain after filtering, `0` otherwise. Never any other code, never a
  panic.

Clean runs print `[test-checker] Success: No problematic tests found`; skipped runs print
`[test-checker] Skipping: <reason>`.

`--strip` is accepted for forward compatibility but is **inert**: an inert notice is printed and the
file is never rewritten.

## Test gate

A file is checked only when its language resolves **and** it is a test file: the payload's explicit
`is_test_file` wins when present, otherwise test-path globs apply (`tests/`, `__tests__/`, `spec/`,
`test_*`, `*_test.*`, `*.test.*`, `*.spec.*`).

Harness/config files (`pytest.ini`, `tox.ini`, `setup.cfg`, `pyproject.toml`, `package.json`,
`jest.config.*`, `vitest.config.*`, `karma.conf.*`, `.nycrc*`, `Makefile`, `.github/workflows/**`,
`.gitlab-ci*`, `codecov.yml`) are gated separately: an added line that ignores a failure
(`|| true`, `continue-on-error: true`, `allow_failure: true`, `--passWithNoTests`, `set +e`) is
reported as `gate-weakened`.

## Languages

Every language compiled by `tree-sitter-language-pack` (35, listed in `.cargo/config.toml`'s
`TSLP_LANGUAGES`)
is parsed. A language yields body-based findings only when it has a real test framework and a
compiling test-body query (21): python, javascript, typescript, tsx, go, rust, ruby, java, csharp,
php, kotlin, swift, scala, elixir, lua, c, cpp, groovy, ocaml, elm, bash. Other languages
(`dockerfile`, `hcl`, `proto`, `toml`, `yaml`, `css`, `html`, `cue`, …) parse cleanly and report
nothing body-based. The `commented-out-test`
signal runs over comment captures for every language, including comments inside injected/embedded
blocks (`<script>`, markdown fences, …).

## Rules

`skip-added`, `focus-added`, `assertion-removed`, `test-removed`, `gutted-test`, `no-assertion`,
`tautological-assertion`, `constant-assertion`, `weakened-assertion`, `swallowed-error`,
`incomplete-assertion`, `unawaited-assertion`, `broad-exception`, `mock-of-sut`, `over-mocking`,
`duplicate-test`, `expected-copied-from-actual`, `snapshot-regenerated`, `tolerance-broadened`,
`conditional-assertion`, `retry-added`, `agent-memo-test`, `allow-added`, `gate-weakened`,
`commented-out-test`, `source-grep-test`, `test-volume`, `harness-edited`, `near-duplicate-test`.

Confidence policy: `source-grep-test` is `medium`; `harness-edited` is `medium` on sensitive harness
paths (CI workflows, pytest/tox/conftest, jest/vitest config) and `low` otherwise; `test-volume` and
`near-duplicate-test` are `low` telemetry, visible only with `--min-confidence low`.

Each finding carries a `confidence` of `high`/`medium`/`low`. Line rules classify the added lines of
the captured test bodies (comments stripped); diff rules compare an edit's old and new content.

Three rules — `weakened-assertion`, `snapshot-regenerated`, `tolerance-broadened` — are **diff-aware**.
They compare a declarative matcher-strength table (JS/TS `toStrictEqual` > `toEqual` > `toBe` >
truthiness/definedness; Python `assertEqual`/`==` > `assertIsNotNone`/`assertTrue`/`is not None`;
snapshot equality → `toMatchSnapshot`; numeric `places`/precision/`delta`/sleep) and fire only when a
stricter form for the **same assertion subject** was replaced by a looser one. If the subject cannot
be aligned, no finding is produced (precision over recall). On an `Edit` the finding is `medium`
(`high` for unambiguous pairs such as `toEqual` → `toBeDefined`); on a `Write` there is no baseline, so
the same patterns report at `low` confidence — below the default `--min-confidence medium` gate, so a
brand-new test is not blocked for using a loose matcher.

## Allow-list / suppression

- **BDD keywords** (`given`, `when`, `then`, `arrange`, `act`, `assert`, …) are allowed.
- **Directives** (`noqa`, `ts-ignore`, `eslint-disable`, `clippy:`, `allow`, …) are allowed.
- **Shebang lines** (`#!…`) are allowed.
- **Opt-out marker** `test-checker: allow` suppresses findings for a file. Adding the marker **in the
  same change** does not suppress it: it is reported as an `allow-added` finding instead.

`agent-memo-test` findings are additionally summarised under a dedicated banner, pointing at the test
name/assertion message that describes a change instead of the behavior.

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
the `commented-out-test` signal, and `include_body_tests` gates the body-based rules.

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
