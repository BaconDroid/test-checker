use std::io::Write;
use std::path::Path;
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

fn strip_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("tc-strip-{}-{name}", std::process::id()))
}

fn prepare_root(name: &str) -> std::io::Result<std::path::PathBuf> {
    let root = strip_root(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git"))?;
    std::fs::create_dir_all(root.join("tests"))?;
    Ok(root)
}

fn write_test_file(root: &Path, relative: &str, content: &str) -> std::io::Result<()> {
    std::fs::write(root.join(relative), content)
}

fn write_remove_config(root: &Path, entries: &str) -> std::io::Result<()> {
    std::fs::write(
        root.join(".test-checker.json"),
        format!("{{\"remove\":[{entries}]}}"),
    )
}

fn strip_payload(root: &Path, file_path: &str, content: &str) -> String {
    serde_json::json!({
        "tool_name": "Write",
        "tool_input": {"file_path": file_path, "content": content},
        "cwd": root.to_string_lossy(),
    })
    .to_string()
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
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"src/app.py","content":"def add(a, b):\n    return a + b\n"}}"#;

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

#[test]
fn cli_given_medium_diff_finding_and_min_confidence_high_exits_zero() -> std::io::Result<()> {
    // given: an edit that loosens a matcher (diff-detected, medium confidence)
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/app.test.js","old_string":"it('x', () => {\n  expect(a).toEqual(b);\n});\n","new_string":"it('x', () => {\n  expect(a).toBe(b);\n});\n"}}"#;

    // when
    let output = run_checker(input, &["--min-confidence", "high"])?;

    // then
    assert!(output.status.success());
    Ok(())
}

#[test]
fn cli_given_medium_diff_finding_and_default_confidence_exits_two() -> std::io::Result<()> {
    // given: an edit that loosens a matcher (diff-detected, medium confidence)
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/app.test.js","old_string":"it('x', () => {\n  expect(a).toEqual(b);\n});\n","new_string":"it('x', () => {\n  expect(a).toBe(b);\n});\n"}}"#;

    // when
    let output = run_checker(input, &[])?;

    // then
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

#[test]
fn cli_given_min_confidence_equals_low_keeps_medium_finding() -> std::io::Result<()> {
    // given: an edit that loosens a matcher (diff-detected, medium confidence)
    let input = r#"{"tool_name":"Edit","tool_input":{"file_path":"tests/app.test.js","old_string":"it('x', () => {\n  expect(a).toEqual(b);\n});\n","new_string":"it('x', () => {\n  expect(a).toBe(b);\n});\n"}}"#;

    // when
    let output = run_checker(input, &["--min-confidence=low"])?;

    // then
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

#[test]
fn cli_given_version_prints_name_and_version() -> std::io::Result<()> {
    // given / when
    let output = run_checker("", &["--version"])?;

    // then
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_NAME")), "stdout: {stdout}");
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "stdout: {stdout}"
    );
    Ok(())
}

#[test]
fn cli_given_prompt_version_treats_it_as_a_prompt_value() -> std::io::Result<()> {
    // given: `--version` is the value consumed by `--prompt`, not a standalone flag
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/a_test.py","content":"def test_a():\n    @pytest.mark.skip\n    assert x == 1\n"}}"#;

    // when
    let output = run_checker(input, &["--prompt", "--version"])?;

    // then
    assert!(output.stdout.is_empty(), "the version must not be printed");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "--version");
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

#[test]
fn cli_given_strip_and_missing_file_analyzes_payload_without_writing() -> std::io::Result<()> {
    // given: a duplicate candidate named by the payload, but no such file on disk
    let root = prepare_root("missing")?;
    let content =
        "def test_a():\n    assert add(1, 2) == 3\ndef test_b():\n    assert add(1, 2) == 3\n";
    let input = strip_payload(&root, "tests/test_missing.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: the payload is analyzed and reported, but nothing is created or rewritten
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(stderr.contains("duplicate-test"), "stderr: {stderr}");
    assert!(
        stderr.contains("kept awaiting approval: test_b [duplicate-test]"),
        "stderr: {stderr}"
    );
    assert!(!root.join("tests/test_missing.py").exists());
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_reads_full_file_from_disk_not_the_payload_content() -> std::io::Result<()> {
    // given: the disk file carries a skip marker that the payload content does not
    let root = prepare_root("disk")?;
    write_test_file(
        &root,
        "tests/test_a.py",
        "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n",
    )?;
    let input = strip_payload(
        &root,
        "tests/test_a.py",
        "def test_a():\n    assert x == 1\n",
    );

    // when
    let output = run_checker(&input, &["--strip"])?;

    // then: the whole file on disk is analyzed, so the skip marker is reported
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(stderr.contains("skip-added"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_without_a_remove_entry_keeps_the_candidate() -> std::io::Result<()> {
    // given: two identical tests, with the config present but no `remove` approval for them
    let root = prepare_root("unapproved")?;
    let content =
        "def test_a():\n    assert add(1, 2) == 3\ndef test_b():\n    assert add(1, 2) == 3\n";
    write_test_file(&root, "tests/test_a.py", content)?;
    std::fs::write(root.join(".test-checker.json"), "{\"allow\":[]}")?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: the candidate is reported and kept; no approval means no removal
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("kept awaiting approval: test_b [duplicate-test]"),
        "stderr: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tests/test_a.py"))?,
        content
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_with_approved_duplicate_removes_the_flagged_copy() -> std::io::Result<()> {
    // given: a human-approved removal for duplicate-test
    let root = prepare_root("approved")?;
    let content =
        "def test_a():\n    assert add(1, 2) == 3\ndef test_b():\n    assert add(1, 2) == 3\n";
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["duplicate-test"],"reason":"duplicate of an existing test"}"#,
    )?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: the flagged copy (the second body) is gone, the first body is byte-identical
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("removed: test_b [duplicate-test]"),
        "stderr: {stderr}"
    );
    let after = std::fs::read_to_string(root.join("tests/test_a.py"))?;
    assert_eq!(after, "def test_a():\n    assert add(1, 2) == 3\n\n");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_removes_the_whole_block_including_decorators() -> std::io::Result<()> {
    // given: an approved no-assertion finding on a decorated test
    let root = prepare_root("decorator")?;
    let content = concat!(
        "@pytest.mark.parametrize(\"x\", [1])\n",
        "def test_a(x):\n",
        "    print(x)\n",
        "\n",
        "\n",
        "def test_b():\n",
        "    assert x\n",
    );
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["no-assertion"],"reason":"probe with no assertion left in the suite"}"#,
    )?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: the decorator line is part of the removed span, and test_b is untouched
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("removed: test_a [no-assertion]"),
        "stderr: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tests/test_a.py"))?,
        "\n\n\ndef test_b():\n    assert x\n"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_merges_overlapping_approvals_into_one_removal() -> std::io::Result<()> {
    // given: two rules flag the same second copy, both approved
    let root = prepare_root("overlap")?;
    let content = concat!(
        "def test_a():\n",
        "    assert add(1, 2) == 3\n",
        "def test_b():\n",
        "    assert f(x) == f(x)\n",
        "def test_c():\n",
        "    assert f(x) == f(x)\n",
    );
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["duplicate-test","tautological-assertion"],"reason":"superfluous duplicate test bodies"}"#,
    )?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: test_c is removed once even though two approved rules point at it
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("cleanup pass: 2 removed, 0 kept awaiting approval."),
        "stderr: {stderr}"
    );
    assert_eq!(
        stderr.matches("removed: test_c").count(),
        1,
        "stderr: {stderr}"
    );
    let after = std::fs::read_to_string(root.join("tests/test_a.py"))?;
    assert_eq!(after, "def test_a():\n    assert add(1, 2) == 3\n\n\n");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_never_removes_an_integrity_rule() -> std::io::Result<()> {
    // given: a `remove` entry names an integrity rule, which is not a cleanup candidate
    let root = prepare_root("integrity")?;
    let content = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["skip-added"],"reason":"approved removal of a skip marker"}"#,
    )?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip"])?;

    // then: the file is unchanged and the finding still blocks
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(!stderr.contains("removed:"), "stderr: {stderr}");
    assert_eq!(
        std::fs::read_to_string(root.join("tests/test_a.py"))?,
        content
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_remove_entry_with_a_short_reason_is_dropped() -> std::io::Result<()> {
    // given: an approval whose reason is shorter than the marker policy's 12 characters
    let root = prepare_root("short-reason")?;
    let content =
        "def test_a():\n    assert add(1, 2) == 3\ndef test_b():\n    assert add(1, 2) == 3\n";
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["duplicate-test"],"reason":"too short"}"#,
    )?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: the invalid entry is dropped, so nothing is removed
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(!stderr.contains("removed:"), "stderr: {stderr}");
    assert_eq!(
        std::fs::read_to_string(root.join("tests/test_a.py"))?,
        content
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cli_given_strip_approved_removal_writes_atomically_and_leaves_no_temporary()
-> std::io::Result<()> {
    // given: a human-approved removal for duplicate-test
    let root = prepare_root("atomic")?;
    let content =
        "def test_a():\n    assert add(1, 2) == 3\ndef test_b():\n    assert add(1, 2) == 3\n";
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["duplicate-test"],"reason":"duplicate of an existing test"}"#,
    )?;
    let input = strip_payload(&root, "tests/test_a.py", content);

    // when
    let output = run_checker(&input, &["--strip", "--min-confidence", "low"])?;

    // then: the rewrite happened through the sibling temp and left none behind
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert_eq!(
        std::fs::read_to_string(root.join("tests/test_a.py"))?,
        "def test_a():\n    assert add(1, 2) == 3\n\n"
    );
    let leftovers: Vec<_> = std::fs::read_dir(root.join("tests"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".strip.tmp"))
        .collect();
    assert!(leftovers.is_empty(), "leftover temporaries: {leftovers:?}");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cleanup_pass_driver_removes_only_the_approved_candidate()
-> Result<(), Box<dyn std::error::Error>> {
    // given: a repository-shaped root with one approved duplicate and no unapproved extra
    let root = prepare_root("driver")?;
    let content =
        "def test_a():\n    assert add(1, 2) == 3\ndef test_b():\n    assert add(1, 2) == 3\n";
    write_test_file(&root, "tests/test_a.py", content)?;
    write_remove_config(
        &root,
        r#"{"glob":"tests/*.py","rules":["duplicate-test"],"reason":"duplicate of an existing test"}"#,
    )?;

    // when
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/cleanup-pass");
    let output = Command::new("python3")
        .arg(script)
        .arg(&root)
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_test-checker"))
        .output()?;

    // then: the driver reports the removal, exits 0, and the binary did the edit
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("tests/test_a.py: removed test_b [duplicate-test]"),
        "stdout: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tests/test_a.py"))?,
        "def test_a():\n    assert add(1, 2) == 3\n\n"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn cleanup_candidates_never_modifies_a_file() -> Result<(), Box<dyn std::error::Error>> {
    // given: a repo-shaped directory with one clean test file
    let root = std::env::temp_dir().join(format!("tc-cleanup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tests"))?;
    let file = root.join("tests/test_a.py");
    std::fs::write(&file, "def test_a():\n    assert x == 1\n")?;
    let before = std::fs::read(&file)?;

    // when: the read-only report runs against it
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/cleanup-candidates");
    let output = Command::new("python3")
        .arg(script)
        .arg(&root)
        .arg("--binary")
        .arg(env!("CARGO_BIN_EXE_test-checker"))
        .output()?;

    // then: it succeeds and the file is byte-identical
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        before,
        std::fs::read(std::path::Path::new(&file))?,
        "cleanup-candidates must not write to any file"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
