use std::path::Path;

use test_checker::input::{read_file, read_string};
use test_checker::models::{Confidence, Finding};

#[test]
fn finding_given_populated_value_serializes_wire_fields() {
    // given
    let finding = Finding::new(
        "tests/test_a.py",
        3,
        "tautological-assertion",
        "Assertion is tautological (always true).",
        Confidence::High,
    );

    // when
    let json = serde_json::to_value(&finding).unwrap();

    // then
    assert_eq!(json["file"], "tests/test_a.py");
    assert_eq!(json["line"], 3);
    assert_eq!(json["rule"], "tautological-assertion");
    assert_eq!(json["message"], "Assertion is tautological (always true).");
    assert_eq!(json["confidence"], "high");
}

#[test]
fn confidence_given_variants_returns_lowercase_names() {
    // given / when / then
    assert_eq!(Confidence::High.as_str(), "high");
    assert_eq!(Confidence::Medium.as_str(), "medium");
    assert_eq!(Confidence::Low.as_str(), "low");
}

#[test]
fn confidence_given_variants_serializes_lowercase() {
    // given / when / then
    assert_eq!(serde_json::to_value(Confidence::High).unwrap(), "high");
    assert_eq!(serde_json::to_value(Confidence::Medium).unwrap(), "medium");
    assert_eq!(serde_json::to_value(Confidence::Low).unwrap(), "low");
}

#[test]
fn read_string_given_content_returns_owned_copy() {
    // given / when / then
    assert_eq!(
        read_string("def test_a():\n    pass\n"),
        "def test_a():\n    pass\n"
    );
}

#[test]
fn read_file_given_existing_path_returns_utf8_content() {
    // given
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_a.py");
    std::fs::write(&path, "def test_a():\n    pass\n").unwrap();

    // when / then
    assert_eq!(read_file(&path), "def test_a():\n    pass\n");
}

#[test]
fn read_file_given_missing_path_returns_empty() {
    // given / when / then
    assert_eq!(
        read_file(Path::new("/nonexistent/test-checker/missing.py")),
        ""
    );
}
