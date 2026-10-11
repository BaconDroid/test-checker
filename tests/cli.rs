use std::io::Write;
use std::process::{Command, Stdio};

fn run_checker(input: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_test-checker"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(stdin) = child.stdin.as_mut() else {
        return Err(std::io::Error::other("checker stdin unavailable"));
    };
    stdin.write_all(input.as_bytes())?;
    child.wait_with_output()
}

#[test]
fn cli_given_clean_test_exits_zero() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Success"));
    Ok(())
}

#[test]
fn cli_given_weakening_edit_exits_two() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/test_app.py","old_string":"assert add(1, 2) == 3","new_string":"assert add(1, 2) == add(1, 2)"}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("TEST-WEAKENING DETECTED"));
    Ok(())
}

#[test]
fn cli_given_non_test_file_exits_zero_with_skip() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"src/app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping: Non-test file"));
    Ok(())
}

#[test]
fn cli_given_invalid_json_exits_zero_with_skip() -> std::io::Result<()> {
    // when
    let output = run_checker("invalid json", &[])?;

    // then
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping: Invalid input format"));
    Ok(())
}

#[test]
fn cli_given_custom_prompt_replaces_placeholder() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}"#;

    // when
    let output = run_checker(input, &["--prompt", "CUSTOM {{findings}}"])?;

    // then
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("CUSTOM"));
    assert!(stderr.contains("<findings"));
    Ok(())
}

#[test]
fn cli_given_multiedit_weakening_exits_two() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"MultiEdit","tool_input":{"file_path":"tests/test_app.py","edits":[{"old_string":"assert add(1, 2) == 3","new_string":"assert add(1, 2) == add(1, 2)"}]}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}
