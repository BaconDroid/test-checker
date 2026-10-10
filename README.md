# test-checker

Multi-language test-weakening detection hook for Claude Code / OpenCode.

`test-checker` reads a JSON hook payload from stdin, inspects the **added** lines
of the edited file, and reports when a change weakens a test. It is the
test-domain sibling of
[comment-checker](https://github.com/code-yeongyu/go-claude-code-comment-checker),
built on the same crate layout, tree-sitter parsing and filtering architecture.

## Contract

- **stdin**: hook payload `{tool_name, tool_input:{file_path?,content?,old_string?,new_string?,edits?}}`,
  plus an optional top-level `is_test_file: bool`.
- **stdout**: empty, except for `--version`.
- **stderr**: a human message. Findings are rendered as grouped XML:

  ```xml
  <findings file="tests/test_x.py">
  <finding line-number="2" rule="tautological-assertion" confidence="high">Assertion is tautological (always true).</finding>
  </findings>
  ```

- `--prompt <text>` replaces the default message; `{{findings}}` is replaced by
  the XML blocks.
- **exit**: `2` when findings are present, `0` otherwise (clean or any
  skipped/degraded path). Never any other code, never a panic.

Clean runs print `[test-checker] Success: No problematic tests found`; skipped
runs print `[test-checker] Skipping: <reason>`.

## Supported extensions

`.py/.pyi` → python, `.js/.jsx/.mjs/.cjs` → javascript, `.ts/.mts/.cts` →
typescript, `.tsx` → tsx, `.go` → go, `.rs` → rust. Any other extension is
ignored (exit 0).

## Rules

`skip-focus-added`, `tautological-assertion`, `empty-test`, `unknown-test`,
`gutted-test`, `swallowed-error`, `duplicate-test`, `mock-of-sut`,
`over-mocking`, `assertion-roulette`, `redundant-assertion`.

Only added lines are candidates, comments are stripped, the `is_test_file` gate
applies, and every failure path degrades to no findings (fail-open).

The library also exposes `detect_findings_json(content, file_path, is_test_file)`
(sibling of comment-checker's `detect_comments_json`), returning the findings as
a JSON array.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo deny check
```

## License

MIT
