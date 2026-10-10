use serde_json::Value;
use test_checker::api::{check_hook_input, check_hook_input_json};

#[test]
fn check_hook_input_given_clean_python_write_returns_success() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_clean.py","content":"def test_a():\n    assert foo() == 1\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Success"));
}

#[test]
fn check_hook_input_given_tautological_python_write_returns_block() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_tauto.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("<findings"));
}

#[test]
fn check_hook_input_given_weakening_edit_returns_block() {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"test_edit.py","old_string":"assert foo() == 1","new_string":"assert True"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("<findings"));
}

#[test]
fn check_hook_input_given_weakening_multiedit_returns_block() {
    // given
    let input = r#"{"tool_name":"MultiEdit","tool_input":{"file_path":"test_multi.py","edits":[{"old_string":"assert foo() == 1","new_string":"assert True"}]}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("<findings"));
}

#[test]
fn check_hook_input_given_invalid_json_returns_skip() {
    // given
    let input = "not json";

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Skipping: Invalid input format"));
}

#[test]
fn check_hook_input_given_unsupported_extension_returns_skip() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"foo.xyz","content":"x"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(
        result
            .message
            .contains("Skipping: Unsupported file extension")
    );
}

#[test]
fn check_hook_input_given_non_test_file_returns_skip() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"src/foo.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Skipping: Non-test file"));
}

#[test]
fn check_hook_input_given_is_test_file_override_reports_findings() {
    // given
    let input = r#"{"tool_name":"Write","is_test_file":true,"tool_input":{"file_path":"src/foo.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("<findings"));
}

#[test]
fn check_hook_input_json_given_findings_returns_camel_case_exit_code() -> serde_json::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_tauto.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let json: Value = serde_json::from_str(&check_hook_input_json(input, ""))?;

    // then
    assert_eq!(json.get("exitCode").and_then(Value::as_i64), Some(2));
    assert!(
        json.get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("<findings"))
    );
    Ok(())
}
