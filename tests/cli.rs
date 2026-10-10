use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_checker(input: &str, args: &[&str]) -> std::io::Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_test-checker"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if !input.is_empty() {
        let Some(stdin) = child.stdin.as_mut() else {
            return Err(std::io::Error::other("checker stdin unavailable"));
        };
        stdin.write_all(input.as_bytes())?;
    }
    child.wait_with_output()
}

#[test]
fn cli_given_clean_write_exits_zero_with_success() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_clean.py","content":"def test_a():\n    assert foo() == 1\n"}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Success"));
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn cli_given_weak_test_exits_two_with_findings() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_tauto.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("<findings"));
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn cli_given_version_flag_prints_binary_name() -> std::io::Result<()> {
    // given / when
    let output = run_checker("", &["--version"])?;

    // then
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("test-checker "));
    Ok(())
}

#[test]
fn cli_given_invalid_json_exits_zero_with_skip() -> std::io::Result<()> {
    // given / when
    let output = run_checker("invalid json", &[])?;

    // then
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping"));
    Ok(())
}

#[test]
fn cli_given_custom_prompt_uses_template_and_placeholder() -> std::io::Result<()> {
    // given
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"test_tauto.py","content":"def test_a():\n    assert True\n"}}"#;

    // when
    let output = run_checker(input, &["--prompt", "CUSTOM WARNING\n{{findings}}\nEND"])?;

    // then
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("CUSTOM WARNING"));
    assert!(stderr.contains("<findings"));
    assert!(stderr.contains("END"));
    assert!(!stderr.contains("TEST QUALITY DETECTED"));
    Ok(())
}
