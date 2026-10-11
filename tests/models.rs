use test_checker::models::{Confidence, Edit, Finding};

#[test]
fn finding_normalized_text_given_whitespace_returns_stripped_lowercase() {
    // given
    let finding = Finding::new(
        "skip-added",
        Confidence::High,
        "m",
        1,
        "tests/test_app.py",
        "  IT.SKIP  ",
    );

    // when
    let normalized = finding.normalized_text();

    // then
    assert_eq!(normalized, "it.skip");
}

#[test]
fn edit_deserialization_given_camel_case_and_null_fields_falls_back() -> serde_json::Result<()> {
    // given
    let input = r#"{"oldString":"a","newString":null}"#;

    // when
    let edit: Edit = serde_json::from_str(input)?;

    // then
    assert_eq!(edit.old_string, "a");
    assert_eq!(edit.new_string, "");
    Ok(())
}

#[test]
fn edit_deserialization_given_snake_case_keys_parses() -> serde_json::Result<()> {
    // given
    let input = r#"{"old_string":"a","new_string":"b"}"#;

    // when
    let edit: Edit = serde_json::from_str(input)?;

    // then
    assert_eq!(edit.old_string, "a");
    assert_eq!(edit.new_string, "b");
    Ok(())
}
