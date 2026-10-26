use serde_json::Value;
use std::process::Command;
use test_checker::api::{
    check_hook_input, check_hook_input_from_reader, check_hook_input_json,
    check_hook_input_with_options, detect_findings_json,
};
use test_checker::engine::Options;

#[test]
fn check_hook_input_given_brand_new_loose_assertion_write_returns_pass() {
    // given: a new test whose only assertion is a loose matcher; on Write there is no baseline,
    // so the presence-only rule reports Low and must not reach the default medium gate.
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/app.test.js","content":"it('sums', () => {\n  expect(add(1, 2)).toBeDefined();\n});\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
}

#[test]
fn check_hook_input_given_diff_aware_weakening_edit_exits_two() {
    // given: an edit that replaces an exact matcher with a loose one for the same subject.
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/app.test.js","old_string":"it('sums', () => {\n  expect(add(1, 2)).toEqual(3);\n});\n","new_string":"it('sums', () => {\n  expect(add(1, 2)).toBeDefined();\n});\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("weakened-assertion"));
}

#[test]
fn check_hook_input_given_weakening_write_exits_two() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-added"));
}

#[test]
fn check_hook_input_given_non_test_file_skips() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"src/app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Non-test file"));
}

#[test]
fn check_hook_input_given_is_test_file_override_true_checks_nonstandard_path() {
    // given
    let input = r#"{"tool_name":"Write","is_test_file":true,"tool_input":{"file_path":"src/app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
}

#[test]
fn check_hook_input_given_is_test_file_override_false_skips_test_path() {
    // given
    let input = r#"{"tool_name":"Edit","is_test_file":false,"tool_input":{"file_path":"tests/test_app.py","old_string":"assert x == 1","new_string":"assert x == x"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
}

#[test]
fn check_hook_input_json_given_weakening_returns_camel_case_block() -> serde_json::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let json: Value = serde_json::from_str(&check_hook_input_json(input, ""))?;

    // then
    assert_eq!(json.get("exitCode").and_then(Value::as_i64), Some(2));
    assert!(
        json.get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("<findings file=\"tests/test_app.py\">"))
    );
    Ok(())
}

#[test]
fn detect_findings_json_given_python_weakening_returns_finding_payload() -> serde_json::Result<()> {
    // given
    let content = "def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n";

    // when
    let json: Value = serde_json::from_str(&detect_findings_json(
        content,
        "tests/test_app.py",
        true,
        true,
    ))?;

    // then
    let findings = json.as_array().cloned().unwrap_or_default();
    assert!(
        findings
            .iter()
            .any(|finding| { finding.get("rule").and_then(Value::as_str) == Some("skip-added") })
    );
    Ok(())
}

#[test]
fn detect_findings_json_given_non_test_path_returns_empty_array() -> serde_json::Result<()> {
    // given
    let content = "def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n";

    // when
    let json: Value =
        serde_json::from_str(&detect_findings_json(content, "src/app.py", true, true))?;

    // then
    assert_eq!(json.as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn detect_findings_json_given_body_tests_disabled_returns_empty_array() -> serde_json::Result<()> {
    // given
    let content = "def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n";

    // when
    let json: Value = serde_json::from_str(&detect_findings_json(
        content,
        "tests/test_app.py",
        true,
        false,
    ))?;

    // then
    assert_eq!(json.as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn check_hook_input_given_camel_case_keys_and_null_fields_parses() {
    // given
    let input = r#"{"toolName":"Write","toolInput":{"filePath":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n","edits":null}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
}

#[test]
fn check_hook_input_given_invalid_json_skips() {
    // when
    let result = check_hook_input("not json", "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Skipping: Invalid input format"));
}

#[test]
fn check_hook_input_with_options_given_body_disabled_returns_pass() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input_with_options(input, "", false, false);

    // then
    assert_eq!(result.exit_code, 0);
}

#[test]
fn check_hook_input_given_deletion_edit_exits_two() {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_sums():\n    assert add(1, 2) == 3\n","new_string":""}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("gutted-test"));
}

#[test]
fn check_hook_input_given_multiedit_deletion_edit_exits_two() {
    // given: a MultiEdit whose only edit deletes an assertion
    let input = r#"{"tool_name":"MultiEdit","tool_input":{"file_path":"tests/test_app.py","edits":[{"old_string":"def test_sums():\n    assert add(1, 2) == 3\n","new_string":""}]}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("gutted-test"));
}

#[test]
fn check_hook_input_given_added_marker_does_not_suppress_the_finding_it_was_added_with() {
    // given: a Write that introduces a valid scoped marker on the same line as the skip
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip  # test-checker: allow skip-added -- tracked upstream #12\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then: the marker is inert for the change that added it, and is itself reported
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("allow-added"), "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
}

#[test]
fn check_hook_input_given_preexisting_bare_marker_no_longer_suppresses() {
    // given: an Edit that keeps the old file-wide marker while adding a skip below it
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_sums():\n    # test-checker: allow\n    assert add(1, 2) == 3\n","new_string":"def test_sums():\n    # test-checker: allow\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then: the old marker suppresses nothing, is reported, and names the replacement config once
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("allow-invalid"),
        "{}",
        result.message
    );
    assert!(result.message.contains("skip-added"), "{}", result.message);
    assert_eq!(result.message.matches(".test-checker.json").count(), 1);
}

#[test]
fn check_hook_input_given_valid_marker_on_the_same_line_suppresses() {
    // given: a duplicate-test finding is anchored at the marked test's declaration line, and the
    // edit touches the body with a blank line (so the body is re-analyzed but the marker is not new)
    let old = "def helper():\n    return 1\n\ndef test_one():\n    assert helper() == 1\n\ndef test_two():  # test-checker: allow duplicate-test -- duplicates a shared fixture test #42\n    assert helper() == 1\n";
    let new = "def helper():\n    return 1\n\ndef test_one():\n    assert helper() == 1\n\ndef test_two():  # test-checker: allow duplicate-test -- duplicates a shared fixture test #42\n    \n    assert helper() == 1\n";
    let input = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": { "file_path": "tests/test_app.py", "old_string": old, "new_string": new },
    })
    .to_string();

    // when
    let result = check_hook_input(&input, "");

    // then
    assert_eq!(result.exit_code, 0, "{}", result.message);
    assert!(
        !result.message.contains("duplicate-test"),
        "{}",
        result.message
    );
}

#[test]
fn check_hook_input_given_valid_marker_on_the_line_above_suppresses() {
    // given: an Edit that keeps the marker and adds a skip on the line below it
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_sums():\n    # test-checker: allow skip-added -- tracked upstream #12\n    assert add(1, 2) == 3\n","new_string":"def test_sums():\n    # test-checker: allow skip-added -- tracked upstream #12\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0, "{}", result.message);
}

#[test]
fn check_hook_input_given_unknown_rule_marker_suppresses_nothing_and_is_reported() {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_sums():\n    # test-checker: allow not-a-rule -- tracked upstream #12\n    assert add(1, 2) == 3\n","new_string":"def test_sums():\n    # test-checker: allow not-a-rule -- tracked upstream #12\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("allow-invalid"),
        "{}",
        result.message
    );
    assert!(
        result.message.contains("unknown rule"),
        "{}",
        result.message
    );
    assert!(result.message.contains("skip-added"), "{}", result.message);
}

#[test]
fn check_hook_input_given_short_reason_marker_suppresses_nothing_and_is_reported() {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_sums():\n    # test-checker: allow skip-added -- short\n    assert add(1, 2) == 3\n","new_string":"def test_sums():\n    # test-checker: allow skip-added -- short\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("allow-invalid"),
        "{}",
        result.message
    );
    assert!(result.message.contains("at least 12"), "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
}

#[test]
fn check_hook_input_given_clean_run_returns_the_exact_success_line() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.message,
        "[test-checker] Success: No problematic tests found\n"
    );
}

#[test]
fn check_hook_input_given_empty_input_skips_with_the_exact_reason() {
    // when
    let result = check_hook_input("", "");

    // then
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.message,
        "[test-checker] Skipping: No input provided\n"
    );
}

#[test]
fn check_hook_input_given_no_file_path_skips_with_the_exact_reason() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"content":"x"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.message,
        "[test-checker] Skipping: No file path provided\n"
    );
}

#[test]
fn check_hook_input_given_oversize_write_skips_as_file_too_large() {
    // given: a test file whose content exceeds the 2 MiB parse limit
    let content = "assert x == 1\n".repeat(150_000);
    let payload = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": "tests/test_app.py", "content": content },
    })
    .to_string();

    // when
    let result = check_hook_input(&payload, "");

    // then: the file is skipped visibly rather than reported clean
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.message, "[test-checker] Skipping: File too large\n");
}

#[test]
fn check_hook_input_given_unsupported_language_fails_open() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/notes.xyz","content":"anything"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(
        result.message.contains("Non-code file"),
        "{}",
        result.message
    );
}

#[test]
fn check_hook_input_given_harness_gate_weakened_exits_two() {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":".github/workflows/ci.yml","old_string":"      - run: cargo test\n","new_string":"      - run: cargo test || true\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("gate-weakened"));
}

#[test]
fn check_hook_input_given_non_sensitive_harness_edit_returns_success() {
    // given: an edit to a harness path outside the sensitive set, with no gate marker
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"makefile","old_string":"\tgo test ./...\n","new_string":"\tgo test ./... -count=1\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then: harness-edited stays low on a non-sensitive path, so it does not block by default
    assert_eq!(result.exit_code, 0);
}

#[test]
fn check_hook_input_given_harness_comment_with_gate_marker_is_ignored() {
    // given: a comment in a workflow mentions `|| true`, not a real weakening
    let input = r##"{"tool_name":"Write","tool_input":{"file_path":".github/workflows/ci.yml","content":"# run tests with: cargo test || true\n"}}"##;

    // when
    let result = check_hook_input(input, "");

    // then: the comment is not read as a gate weakening; the sensitive-path harness-edited is the
    // only finding, and it is expected under the confidence policy
    assert!(
        !result.message.contains("gate-weakened"),
        "message: {}",
        result.message
    );
    assert!(result.message.contains("harness-edited"));
}

#[test]
fn check_hook_input_given_unknown_tool_skips() {
    // given: a non-edit tool, which the hook matcher (Write|Edit|MultiEdit) never sends
    let input = r#"{"tool_name":"Read","tool_input":{"file_path":"tests/a_test.py","content":"def test_a():\n    @pytest.mark.skip\n    assert x == 1\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
}

#[test]
fn check_hook_input_given_apply_patch_weakening_edit_exits_two() {
    // given: the OpenCode plugin forwards `apply_patch`, which must be checked as an edit and not
    // silently skipped.
    let input = r#"{"tool_name":"apply_patch","tool_input":{"file_path":"tests/app.test.js","old_string":"it('sums', () => {\n  expect(add(1, 2)).toEqual(3);\n});\n","new_string":"it('sums', () => {\n  expect(add(1, 2)).toBeDefined();\n});\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("weakened-assertion"));
}

#[test]
fn check_hook_input_given_lowercase_write_exits_two() {
    // given: the OpenCode plugin forwards the tool name lowercased.
    let input = r#"{"tool_name":"write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-added"));
}

#[test]
fn check_hook_input_given_edit_with_edits_array_analyzes_deletion() {
    // given: an `Edit` carrying the plugin's `edits[]` array, whose only edit deletes an assertion
    // (the top-level `new_string` is empty).
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","edits":[{"old_string":"def test_sums():\n    assert add(1, 2) == 3\n","new_string":""}]}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("gutted-test"));
}

fn run_binary(payload: &str, baseline: bool) -> std::io::Result<std::process::Output> {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_test-checker"))
        .env(
            "TEST_CHECKER_WRITE_BASELINE",
            if baseline { "1" } else { "0" },
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let Some(stdin) = child.stdin.as_mut() else {
        return Err(std::io::Error::other("checker stdin unavailable"));
    };
    stdin.write_all(payload.as_bytes())?;
    child.wait_with_output()
}

fn git(dir: &std::path::Path, args: &[&str]) -> std::io::Result<()> {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()?;
    assert!(status.success(), "git {args:?}");
    Ok(())
}

fn baseline_dir(label: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("tc-baseline-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests"))?;
    Ok(dir)
}

#[test]
fn check_hook_input_given_write_with_head_baseline_reports_removed_assertion() -> std::io::Result<()>
{
    // given: a committed test file, then a Write that removes the assertion
    let dir = baseline_dir("head")?;
    std::fs::write(
        dir.join("tests/test_app.py"),
        "def test_a():\n    assert x == 1\n",
    )?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "-q", "-m", "init"])?;
    let input = format!(
        r#"{{"tool_name":"Write","cwd":{:?},"tool_input":{{"file_path":"tests/test_app.py","content":"def test_a():\n    pass\n"}}}}"#,
        dir.to_string_lossy()
    );

    // when: the opt-in baseline is enabled for this invocation
    let output = run_binary(&input, true)?;

    // then: the diff rules ran against HEAD
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("assertion-removed") || stderr.contains("gutted-test"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn check_hook_input_given_write_with_uncommitted_file_shows_visible_skip() -> std::io::Result<()> {
    // given: a git repo whose test file is not committed
    let dir = baseline_dir("untracked")?;
    git(&dir, &["init", "-q"])?;
    let input = format!(
        r#"{{"tool_name":"Write","cwd":{:?},"tool_input":{{"file_path":"tests/test_app.py","content":"def test_a():\n    assert x == 1\n"}}}}"#,
        dir.to_string_lossy()
    );

    // when: the opt-in baseline is enabled for this invocation
    let output = run_binary(&input, true)?;

    // then: no diff rules, but the skip is visible
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(stderr.contains("Skipping:"), "stderr: {stderr}");
    assert!(stderr.contains("not committed"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
#[test]
fn check_hook_input_given_write_baseline_ignores_pre_existing_conditional() -> std::io::Result<()> {
    // given: a committed test with a pre-existing conditional, then a Write that only adds a line
    let dir = baseline_dir("conditional-added-lines")?;
    std::fs::write(
        dir.join("tests/test_app.py"),
        "def test_a():\n    if flag:\n        assert x == 1\n",
    )?;
    git(&dir, &["init", "-q"])?;
    git(&dir, &["add", "."])?;
    git(&dir, &["commit", "-q", "-m", "init"])?;
    let input = serde_json::json!({
        "tool_name": "Write",
        "cwd": dir.to_string_lossy(),
        "tool_input": {
            "file_path": "tests/test_app.py",
            "content": "def test_a():\n    if flag:\n        assert x == 1\n    assert y == 2\n",
        },
    })
    .to_string();

    // when: the HEAD baseline is enabled
    let with_baseline = run_binary(&input, true)?;
    let stderr = String::from_utf8_lossy(&with_baseline.stderr);
    let without_baseline = run_binary(&input, false)?;

    // then: only the added line is examined, while a whole-file Write still flags the guard
    assert_eq!(with_baseline.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        !stderr.contains("conditional-assertion"),
        "stderr: {stderr}"
    );
    let whole_file = String::from_utf8_lossy(&without_baseline.stderr);
    assert_eq!(
        without_baseline.status.code(),
        Some(2),
        "stderr: {whole_file}"
    );
    assert!(
        whole_file.contains("conditional-assertion"),
        "stderr: {whole_file}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn check_hook_input_given_linter_directive_next_to_skip_still_reports_skip() {
    // given: a skip marker and a linter directive on the same line
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_a():\n    @pytest.mark.skip  # noqa\n    assert x == 1\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then: the directive does not silence the skip finding
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-added"));
}

#[test]
fn check_hook_input_given_scoped_marker_added_reports_allow_added() {
    // given: a valid scoped marker added in the same Write
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_a():\n    @pytest.mark.skip  # test-checker: allow skip-added -- tracked upstream #12\n    assert x == 1\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then: the marker itself is reported and does not suppress the finding it accompanied
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("allow-added"), "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
}

/// Options that keep low-confidence findings, so the harness/config rules are observable.
fn low_confidence_options() -> Options {
    Options {
        min_confidence: Some("low".to_owned()),
        ..Options::default()
    }
}

#[test]
fn check_hook_input_from_reader_given_sensitive_harness_edit_reports_harness_edited() {
    // given: an edit to a sensitive harness path (a CI workflow) with no gate marker
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":".github/workflows/ci.yml","old_string":"      - run: cargo fmt\n","new_string":"      - run: cargo clippy\n"}}"#;

    // when: checked at the default medium gate
    let result = check_hook_input_from_reader(input.as_bytes(), &Options::default());

    // then: harness-edited is medium on a sensitive path, so it blocks at the default gate
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("harness-edited"),
        "message: {}",
        result.message
    );
}

#[test]
fn check_hook_input_given_source_grep_test_is_low_telemetry() {
    // given: a test that reads a source file and asserts on its text
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_reads_source():\n    text = Path(\"src/app.py\").read_text()\n    assert \"def \" in text\n"}}"#;

    // when: the default medium gate keeps only medium+ findings; the low band keeps all
    let default = check_hook_input(input, "");
    let low = check_hook_input_from_reader(input.as_bytes(), &low_confidence_options());

    // then: source-grep-test is low telemetry and does not block at the default gate
    assert_eq!(default.exit_code, 0);
    assert_eq!(low.exit_code, 2);
    assert!(
        low.message.contains("source-grep-test"),
        "message: {}",
        low.message
    );
}

#[test]
fn check_hook_input_given_non_sensitive_harness_edit_is_low_telemetry() {
    // given: a harness edit outside the sensitive set (package.json), with no gate marker
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"package.json","content":"{\n  \"scripts\": {\n    \"test\": \"jest\"\n  }\n}\n"}}"#;

    // when: the default medium gate keeps only medium+ findings; the low band keeps all
    let default = check_hook_input(input, "");
    let low = check_hook_input_from_reader(input.as_bytes(), &low_confidence_options());

    // then: a non-sensitive harness path stays low, so it is telemetry that does not block
    assert_eq!(default.exit_code, 0);
    assert_eq!(low.exit_code, 2);
    assert!(
        low.message.contains("harness-edited"),
        "message: {}",
        low.message
    );
}

#[test]
fn check_hook_input_from_reader_given_non_harness_edit_does_not_report_harness_edited() {
    // given: a plain test-file edit, checked with low findings kept
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_a():\n    assert x == 1\n","new_string":"def test_a():\n    assert x == 2\n"}}"#;

    // when
    let result = check_hook_input_from_reader(input.as_bytes(), &low_confidence_options());

    // then
    assert!(
        !result.message.contains("harness-edited"),
        "message: {}",
        result.message
    );
}

/// A unique temporary repository directory: a directory with a `.git` entry, so the config
/// lookup treats it as the repository root.
fn temp_repo(label: &str) -> std::io::Result<std::path::PathBuf> {
    let path = temp_dir(label)?;
    std::fs::create_dir_all(path.join(".git"))?;
    Ok(path)
}

/// A unique temporary directory with no `.git` anywhere in its ancestry.
fn temp_dir(label: &str) -> std::io::Result<std::path::PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "test-checker-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn write_payload(cwd: &std::path::Path, file_path: &str, content: &str) -> String {
    serde_json::json!({
        "tool_name": "Write",
        "cwd": cwd.to_string_lossy(),
        "tool_input": { "file_path": file_path, "content": content },
    })
    .to_string()
}

#[test]
fn check_hook_input_given_committed_config_suppresses_the_listed_rule() -> std::io::Result<()> {
    // given: a committed config whose glob and rules cover this file
    let repo = temp_repo("config-suppresses")?;
    std::fs::write(
        repo.join(".test-checker.json"),
        r#"{"allow":[{"glob":"tests/**","rules":["skip-added"],"reason":"legacy suite migration tracked in #42"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&repo, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then
    assert_eq!(result.exit_code, 0, "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_config_entry_without_a_reason_suppresses_nothing() -> std::io::Result<()>
{
    // given: the same entry, but with an empty reason
    let repo = temp_repo("config-no-reason")?;
    std::fs::write(
        repo.join(".test-checker.json"),
        r#"{"allow":[{"glob":"tests/**","rules":["skip-added"],"reason":""}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&repo, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_config_glob_or_rule_mismatch_suppresses_nothing() -> std::io::Result<()> {
    // given: a config that matches neither the path nor the rule under test
    let repo = temp_repo("config-mismatch")?;
    std::fs::write(
        repo.join(".test-checker.json"),
        r#"{"allow":[{"glob":"other/**","rules":["focus-added"],"reason":"another suite tracked in #42"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&repo, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_config_edit_returns_a_high_finding() {
    // given: an edit to the committed suppression config itself
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":".test-checker.json","content":"{\"allow\":[]}\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("config-edited"),
        "{}",
        result.message
    );
    assert!(
        result.message.contains("confidence=\"high\""),
        "{}",
        result.message
    );
}

#[cfg(unix)]
#[test]
fn doctor_lists_old_file_wide_markers() -> Result<(), Box<dyn std::error::Error>> {
    // given: a repo with the old bare marker and the real binary on PATH
    let repo = temp_repo("doctor")?;
    std::fs::write(
        repo.join("old_test.py"),
        "def test_a():\n    # test-checker: allow\n    assert x == 1\n",
    )?;

    let binary = std::env::var("CARGO_BIN_EXE_test-checker")?;
    let binary_dir = std::path::Path::new(&binary)
        .parent()
        .ok_or("binary has no parent directory")?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let combined = format!(
        "{}:{}",
        binary_dir.to_string_lossy(),
        path.to_string_lossy()
    );
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/doctor.sh");

    // when
    let output = Command::new("bash")
        .arg(script)
        .current_dir(&repo)
        .env("PATH", combined)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_dir_all(&repo);

    // then: the old marker is listed and the doctor still succeeds
    assert!(
        output.status.success(),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("old file-wide marker") && stdout.contains("old_test.py"),
        "stdout: {stdout}"
    );
    Ok(())
}

#[test]
fn check_hook_input_given_write_of_api_test_file_reports_no_allow_findings() -> std::io::Result<()>
{
    // given: the real tests/api.rs, whose raw-string payloads carry opt-out marker text. Its
    // skip payloads are data too, so a root config suppresses only that unrelated rule.
    let repo = temp_repo("api-file")?;
    std::fs::write(
        repo.join(".test-checker.json"),
        r#"{"allow":[{"glob":"tests/api.rs","rules":["skip-added","source-grep-test"],"reason":"payload text is string data in this file"}]}"#,
    )?;

    // when: a default Write of the whole file
    let result = check_hook_input(
        &write_payload(&repo, "tests/api.rs", include_str!("api.rs")),
        "",
    );
    let _ = std::fs::remove_dir_all(&repo);

    // then: markers inside Rust string literals are invisible
    assert_eq!(result.exit_code, 0, "{}", result.message);
    assert!(!result.message.contains("allow-"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_marker_inside_a_rust_string_literal_is_invisible() {
    // given: a Write of a Rust source whose raw string merely contains the marker text
    let content = "fn payload() {\n    let input = r#\"{\"content\":\"# test-checker: allow \"}\"#;\n    let skip = \"@pytest.mark.skip\";\n}\n";
    let input = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": "tests/api.rs", "content": content },
    })
    .to_string();

    // when
    let result = check_hook_input(&input, "");

    // then
    assert_eq!(result.exit_code, 0, "{}", result.message);
    assert!(!result.message.contains("allow-"), "{}", result.message);
}

#[test]
fn check_hook_input_given_nested_config_does_not_suppress() -> std::io::Result<()> {
    // given: a config below the root, which is not the committed file the hook reads
    let repo = temp_repo("nested-config")?;
    std::fs::create_dir_all(repo.join("tests/nested"))?;
    std::fs::write(
        repo.join("tests/nested/.test-checker.json"),
        r#"{"allow":[{"glob":"tests/**","rules":["skip-added"],"reason":"nested config must not apply"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&repo, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_nested_config_edit_returns_a_high_finding() {
    // given: a config at depth is still the suppression config
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/nested/.test-checker.json","content":"{\"allow\":[]}\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("config-edited"),
        "{}",
        result.message
    );
    assert!(
        result.message.contains("confidence=\"high\""),
        "{}",
        result.message
    );
}

#[test]
fn check_hook_input_given_config_at_git_root_found_from_subdirectory_cwd() -> std::io::Result<()> {
    // given: the config lives at the repository root, but the payload's cwd is a subdirectory
    let repo = temp_repo("config-from-subdir")?;
    std::fs::create_dir_all(repo.join("tests/nested"))?;
    std::fs::write(
        repo.join(".test-checker.json"),
        r#"{"allow":[{"glob":"tests/**","rules":["skip-added"],"reason":"legacy suite migration tracked in #42"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when: the cwd is below the root that owns the config
    let result = check_hook_input(
        &write_payload(&repo.join("tests/nested"), "tests/test_app.py", content),
        "",
    );
    let _ = std::fs::remove_dir_all(&repo);

    // then: the walk reached the git root and honoured its config
    assert_eq!(result.exit_code, 0, "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_config_in_the_cwd_subdirectory_is_ignored() -> std::io::Result<()> {
    // given: the cwd itself is a subdirectory carrying its own config; the git root has none
    let repo = temp_repo("config-in-cwd")?;
    let cwd = repo.join("tests/nested");
    std::fs::create_dir_all(&cwd)?;
    std::fs::write(
        cwd.join(".test-checker.json"),
        r#"{"allow":[{"glob":"**","rules":["skip-added"],"reason":"subdirectory config must not apply"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&cwd, "test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then: only the repository-root config is read
    assert_eq!(result.exit_code, 2, "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_config_in_the_file_directory_is_ignored() -> std::io::Result<()> {
    // given: the edited file's own directory carries a config, but the git root does not
    let repo = temp_repo("config-in-file-dir")?;
    std::fs::create_dir_all(repo.join("tests"))?;
    std::fs::write(
        repo.join("tests/.test-checker.json"),
        r#"{"allow":[{"glob":"tests/**","rules":["skip-added"],"reason":"file-directory config must not apply"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&repo, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then
    assert_eq!(result.exit_code, 2, "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_no_git_ancestor_reads_no_config() -> std::io::Result<()> {
    // given: a config in a directory that is not inside any git work tree
    let dir = temp_dir("no-git")?;
    std::fs::write(
        dir.join(".test-checker.json"),
        r#"{"allow":[{"glob":"**","rules":["skip-added"],"reason":"config outside a git work tree must not apply"}]}"#,
    )?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&dir, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&dir);

    // then: no `.git` ancestor means no config, even though the file exists in the cwd
    assert_eq!(result.exit_code, 2, "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_git_root_without_a_config_file_makes_only_stats() -> std::io::Result<()> {
    // given: a git root whose `.test-checker.json` path is a directory, not a readable file.
    // The lookup must stat the path and skip it; it must not open it as a config.
    let repo = temp_repo("config-is-directory")?;
    std::fs::create_dir_all(repo.join(".test-checker.json"))?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";

    // when
    let result = check_hook_input(&write_payload(&repo, "tests/test_app.py", content), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then: a non-file config path suppresses nothing
    assert_eq!(result.exit_code, 2, "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

/// A config entry that lists only the two diff rules and whose reason is long enough.
const fn diff_rule_config() -> &'static str {
    r#"{"allow":[
        {"glob":"npm/test-checker/*.test.js","rules":["assertion-removed","test-removed"],"reason":"payload fixtures intentionally drop assertions"},
        {"glob":"tests/detector.rs","rules":["assertion-removed","test-removed"],"reason":"fixture text embeds assertion samples"},
        {"glob":"tests/output.rs","rules":["assertion-removed","test-removed"],"reason":"fixture text embeds assertion samples"}
    ]}"#
}

fn edit_payload(cwd: &std::path::Path, file_path: &str, old: &str, new: &str) -> String {
    serde_json::json!({
        "tool_name": "Edit",
        "cwd": cwd.to_string_lossy(),
        "tool_input": { "file_path": file_path, "old_string": old, "new_string": new },
    })
    .to_string()
}

#[test]
fn check_hook_input_given_config_diff_rules_suppress_the_three_known_shapes() -> std::io::Result<()>
{
    // given: the three files whose fixture text makes the diff rules fire on a benign edit
    let repo = temp_repo("diff-rule-suppression")?;
    std::fs::write(repo.join(".test-checker.json"), diff_rule_config())?;
    let cases = [
        (
            "npm/test-checker/index.test.js",
            "it('a', () => {\n  assert.equal(add(1, 2), 3);\n  assert.equal(add(2, 2), 4);\n});\n",
            "it('a', () => {\n  assert.equal(add(1, 2), 3);\n});\n",
        ),
        (
            "tests/detector.rs",
            "#[test]\nfn a() {\n    assert_eq!(add(1, 2), 3);\n    assert_eq!(add(2, 2), 4);\n}\n",
            "#[test]\nfn a() {\n    assert_eq!(add(1, 2), 3);\n}\n",
        ),
        (
            "tests/output.rs",
            "#[test]\nfn a() {\n    assert_eq!(add(1, 2), 3);\n    assert_eq!(add(2, 2), 4);\n}\n",
            "#[test]\nfn a() {\n    assert_eq!(add(1, 2), 3);\n}\n",
        ),
    ];

    for (path, old, new) in cases {
        // when
        let result = check_hook_input(&edit_payload(&repo, path, old, new), "");

        // then: the committed reason suppresses the diff rules for this edit
        assert_eq!(result.exit_code, 0, "{path}: {}", result.message);
    }
    let _ = std::fs::remove_dir_all(&repo);
    Ok(())
}

#[test]
fn check_hook_input_given_config_diff_entry_does_not_suppress_other_rules() -> std::io::Result<()> {
    // given: a config that lists only the diff rules, on an edit that adds a skip marker
    let repo = temp_repo("diff-rule-only")?;
    std::fs::write(repo.join(".test-checker.json"), diff_rule_config())?;
    let old = "#[test]\nfn a() {\n    assert_eq!(add(1, 2), 3);\n}\n";
    let new = "#[test]\n#[ignore]\nfn a() {\n    assert_eq!(add(1, 2), 3);\n}\n";

    // when
    let result = check_hook_input(&edit_payload(&repo, "tests/detector.rs", old, new), "");
    let _ = std::fs::remove_dir_all(&repo);

    // then: skip-added is not listed, so it still blocks
    assert_eq!(result.exit_code, 2, "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    Ok(())
}

#[test]
fn check_hook_input_given_removed_assertion_asks_the_human_for_a_config_entry() {
    // given: an edit that drops one assertion
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_a():\n    assert add(1, 1) == 2\n    assert add(2, 2) == 4\n","new_string":"def test_a():\n    assert add(1, 1) == 2\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then: the human is told how to approve an intended removal
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("assertion-removed"));
    assert!(
        result.message.contains(
            "STOP and ask the human to add a .test-checker.json entry with a reason if the removal is intended"
        ),
        "{}",
        result.message
    );
}

#[test]
fn check_hook_input_given_removed_test_asks_the_human_for_a_config_entry() {
    // given: an edit that drops a whole test while keeping another
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"def test_a():\n    assert x == 1\n\ndef test_b():\n    assert y == 2\n","new_string":"def test_b():\n    assert y == 2\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(
        result.message.contains("test-removed"),
        "{}",
        result.message
    );
    assert!(
        result.message.contains(
            "STOP and ask the human to add a .test-checker.json entry with a reason if the removal is intended"
        ),
        "{}",
        result.message
    );
}

#[test]
fn check_hook_input_given_marker_inside_a_python_string_is_invisible() {
    // given: an edit that keeps a string literal containing the marker text and adds a skip
    let old = "def test_a():\n    note = \"test-checker: allow skip-added -- tracked upstream #12\"\n    assert x == 1\n";
    let new = "def test_a():\n    note = \"test-checker: allow skip-added -- tracked upstream #12\"\n    @pytest.mark.skip\n    assert x == 1\n";
    let input = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": { "file_path": "tests/test_app.py", "old_string": old, "new_string": new },
    })
    .to_string();

    // when
    let result = check_hook_input(&input, "");

    // then: text inside a string is not a comment, so it neither suppresses nor reports itself
    assert_eq!(result.exit_code, 2, "{}", result.message);
    assert!(result.message.contains("skip-added"), "{}", result.message);
    assert!(!result.message.contains("allow-"), "{}", result.message);
}

#[test]
fn check_hook_input_given_marker_in_a_block_comment_suppresses() {
    // given: a marker inside a Rust block comment on the line above an added `#[ignore]`
    let old = "#[test]\nfn a() {\n    /* test-checker: allow skip-added -- tracked upstream #12 */\n    assert_eq!(x, 1);\n}\n";
    let new = "#[test]\nfn a() {\n    /* test-checker: allow skip-added -- tracked upstream #12 */\n    #[ignore]\n    assert_eq!(x, 1);\n}\n";
    let input = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": { "file_path": "tests/app_test.rs", "old_string": old, "new_string": new },
    })
    .to_string();

    // when
    let result = check_hook_input(&input, "");

    // then: the comment node proves the marker is a comment, so it suppresses the skip finding
    assert_eq!(result.exit_code, 0, "{}", result.message);
}
