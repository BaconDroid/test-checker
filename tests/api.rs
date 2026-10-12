use serde_json::Value;
use test_checker::api::{
    check_hook_input, check_hook_input_json, check_hook_input_with_options, detect_findings_json,
};

#[test]
fn check_hook_input_given_clean_write_returns_pass() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Success"));
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
fn check_hook_input_given_file_suppression_marker_skips() {
    // given
    let input = r##"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"# test-checker: allow\ndef test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"##;

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
}
