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
