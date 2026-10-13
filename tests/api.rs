use serde_json::Value;
use test_checker::api::{
    check_hook_input, check_hook_input_json, check_hook_input_with_options, detect_findings_json,
};

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
fn check_hook_input_given_added_suppression_marker_reports_allow_added() {
    // given: a Write that introduces the opt-out marker
    let input = r##"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"# test-checker: allow\ndef test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"##;

    // when
    let result = check_hook_input(input, "");

    // then: the marker does not suppress the change that added it
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("allow-added"));
}

#[test]
fn check_hook_input_given_preexisting_suppression_marker_skips() {
    // given: an Edit whose marker already existed (not on an added line)
    let input = r##"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"# test-checker: allow\ndef test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n","new_string":"# test-checker: allow\ndef test_sums():\n    @pytest.mark.skip\n    assert add(2, 2) == 4\n"}}"##;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Suppressed by test-checker: allow"));
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
fn check_hook_input_given_harness_without_marker_returns_success() {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":".github/workflows/ci.yml","old_string":"      - run: cargo fmt\n","new_string":"      - run: cargo clippy\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
}

#[test]
fn check_hook_input_given_harness_comment_with_gate_marker_is_ignored() {
    // given: a comment in a workflow mentions `|| true`, not a real weakening
    let input = r##"{"tool_name":"Write","tool_input":{"file_path":".github/workflows/ci.yml","content":"# run tests with: cargo test || true\n"}}"##;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
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
