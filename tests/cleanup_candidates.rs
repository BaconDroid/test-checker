#![cfg(unix)]

use std::path::Path;
use std::process::Command;

#[test]
fn cleanup_candidates_never_modifies_a_file() -> Result<(), Box<dyn std::error::Error>> {
    // given: a repo-shaped directory with one clean test file
    let root = std::env::temp_dir().join(format!("tc-cleanup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("tests"))?;
    let file = root.join("tests/test_a.py");
    let content = "def test_a():\n    assert x == 1\n";
    std::fs::write(&file, content)?;
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
        std::fs::read(Path::new(&file))?,
        "cleanup-candidates must not write to any file"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
