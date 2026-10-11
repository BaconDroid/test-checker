//! The hook wire contract shared with `comment-checker`: message prefix, exit-code mapping, XML
//! shape, and fail-open skip reasons. This is the single owner of those protocol invariants.

use test_checker::api::{check_hook_input, check_hook_input_json};

#[test]
fn clean_run_is_a_prefixed_success_with_exit_zero() {
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
fn skip_reasons_are_prefixed_and_exit_zero() {
    // given / when / then
    let cases = [
        ("", "[test-checker] Skipping: No input provided\n"),
        (
            "not json",
            "[test-checker] Skipping: Invalid input format\n",
        ),
        (
            r#"{"tool_name":"Write","tool_input":{"content":"x"}}"#,
            "[test-checker] Skipping: No file path provided\n",
        ),
        (
            r#"{"tool_name":"Write","tool_input":{"file_path":"src/app.py","content":"x"}}"#,
            "[test-checker] Skipping: Non-test file\n",
        ),
    ];
    for (input, expected) in cases {
        let result = check_hook_input(input, "");
        assert_eq!(result.exit_code, 0, "input: {input}");
        assert_eq!(result.message, expected, "input: {input}");
    }
}

#[test]
fn findings_block_exits_two_and_carries_grouped_xml() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(
        result
            .message
            .contains("<findings file=\"tests/test_app.py\">")
    );
    assert!(result.message.contains("</findings>"));
    assert!(result.message.contains("rule=\"skip-added\""));
}

#[test]
fn json_api_mirrors_the_exit_and_message_fields() -> serde_json::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let value: serde_json::Value = serde_json::from_str(&check_hook_input_json(input, ""))?;

    // then
    assert_eq!(
        value.get("exitCode").and_then(serde_json::Value::as_i64),
        Some(2)
    );
    assert!(
        value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    Ok(())
}

#[test]
fn unsupported_language_fails_open_to_exit_zero() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/notes.xyz","content":"anything"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
}
