use test_checker::api::check_hook_input;

#[test]
fn pipeline_given_clean_python_returns_success() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_clean.py","content":"def test_a():\n    assert foo() == 1\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Success"));
}

#[test]
fn pipeline_given_tautological_python_reports_findings() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_tauto.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("<findings"));
}

#[test]
fn pipeline_given_allow_marker_returns_success() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_allow.py","content":"def test_a():\n    # test-guard: allow\n    assert True\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Success"));
}

#[test]
fn pipeline_given_disable_file_marker_returns_success() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_disabled.py","content":"def test_a():\n    assert True\n# test-guard-disable-file\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 0);
    assert!(result.message.contains("Success"));
}

#[test]
fn pipeline_given_go_skipped_test_reports_findings() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"pkg/foo_test.go","content":"package foo\n\nimport \"testing\"\n\nfunc TestA(t *testing.T) {\n\tt.Skip(\"later\")\n}\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-focus-added"));
}

#[test]
fn pipeline_given_ignored_rust_test_reports_findings() {
    // given
    let input = r##"{"tool_name":"Write","tool_input":{"file_path":"src/foo_test.rs","content":"#[test]\n#[ignore]\nfn t() {\n    assert!(true);\n}\n"}}"##;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("skip-focus-added"));
}

#[test]
fn pipeline_given_tautological_javascript_reports_findings() {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"src/foo.test.ts","content":"it('a', () => {\n  expect(true).toBe(true);\n});\n"}}"#;

    // when
    let result = check_hook_input(input, "");

    // then
    assert_eq!(result.exit_code, 2);
    assert!(result.message.contains("tautological-assertion"));
}
