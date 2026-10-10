use test_checker::models::{Confidence, Payload};

#[test]
fn payload_given_write_input_deserializes_tool_fields() {
    // given
    let json = r#"{
        "tool_name": "Write",
        "tool_input": {
            "file_path": "src/app.py",
            "content": "print(1)",
            "old_string": "old",
            "new_string": "new",
            "edits": [{ "old_string": "x", "new_string": "y" }]
        }
    }"#;

    // when
    let payload: Payload = serde_json::from_str(json).unwrap();

    // then
    assert_eq!(payload.tool_name, "Write");
    assert_eq!(payload.tool_input.file_path, "src/app.py");
    assert_eq!(payload.tool_input.content, "print(1)");
    assert_eq!(payload.tool_input.old_string, "old");
    assert_eq!(payload.tool_input.new_string, "new");
    assert_eq!(payload.tool_input.edits.len(), 1);
    assert_eq!(payload.tool_input.edits[0].old_string, "x");
    assert_eq!(payload.tool_input.edits[0].new_string, "y");
}

#[test]
fn payload_given_absent_is_test_file_is_none() {
    // given
    let json = r#"{"tool_name":"Write","tool_input":{"file_path":"a.py"}}"#;

    // when
    let payload: Payload = serde_json::from_str(json).unwrap();

    // then
    assert_eq!(payload.is_test_file, None);
}

#[test]
fn payload_given_explicit_is_test_file_is_some() {
    // given
    let truthy = r#"{"tool_name":"Write","tool_input":{"file_path":"a.py"},"is_test_file":true}"#;
    let falsy = r#"{"tool_name":"Write","tool_input":{"file_path":"a.py"},"is_test_file":false}"#;

    // when / then
    assert_eq!(
        serde_json::from_str::<Payload>(truthy)
            .unwrap()
            .is_test_file,
        Some(true)
    );
    assert_eq!(
        serde_json::from_str::<Payload>(falsy).unwrap().is_test_file,
        Some(false)
    );
}

#[test]
fn payload_given_unknown_fields_ignores_them() {
    // given
    let json =
        r#"{"tool_name":"Edit","unexpected":1,"tool_input":{"file_path":"a.py","extra":true}}"#;

    // when
    let payload: Payload = serde_json::from_str(json).unwrap();

    // then
    assert_eq!(payload.tool_name, "Edit");
    assert_eq!(payload.tool_input.file_path, "a.py");
}

#[test]
fn confidence_given_variants_returns_lowercase_names() {
    // given / when / then
    assert_eq!(Confidence::High.as_str(), "high");
    assert_eq!(Confidence::Medium.as_str(), "medium");
    assert_eq!(Confidence::Low.as_str(), "low");
}
