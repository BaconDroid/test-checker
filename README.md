# test-checker

A PostToolUse hook for Claude Code / OpenCode that catches **test-weakening** edits before they
land. It is the test-domain sibling of
[comment-checker](https://github.com/BaconDroid/comment-checker), built on the same crate layout,
tree-sitter parsing and filtering architecture.

It reads a JSON hook payload on stdin, inspects the **added** lines of the edited file, and reports
when a change weakens, disables, or hollows out a test. On findings it writes a human warning to
**stderr** and exits **2**; otherwise it exits **0** and never panics (every failure path degrades to
no findings).

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

## Contract

- **stdin**: `{ "tool_name": "Write"|"Edit"|"MultiEdit", "tool_input": { "file_path"?, "content"?,
  "old_string"?, "new_string"?, "edits"? }, "is_test_file"? }`.
  Missing or `null` fields fall back to defaults; unknown fields are ignored.
- **stdout**: empty.
- **stderr**: the message. Findings are rendered as grouped XML:

  ```xml
  <findings file="tests/test_app.py">
  	<finding line-number="7" rule="skip-added" confidence="high">A test/assertion was disabled with a skip/ignore marker.</finding>
  </findings>
  ```

- `--prompt <text>` replaces the default message; `{{findings}}` is substituted with the XML.
  `--min-confidence <low|medium|high>` (default `medium`) drops lower-confidence findings, so the
  hook only exits 2 for findings at or above that level.
- **exit**: `2` when findings remain after filtering, `0` otherwise. Never any other code, never a
  panic.

Clean runs print `[test-checker] Success: No problematic tests found`; skipped runs print
`[test-checker] Skipping: <reason>`.

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

Every language compiled by `tree-sitter-language-pack` (31, listed in `.cargo/config.toml`'s
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
`commented-out-test`.

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
