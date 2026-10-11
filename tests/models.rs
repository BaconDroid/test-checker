use test_checker::models::{Confidence, Finding};

#[test]
fn normalized_text_given_whitespace_returns_stripped_lowercase() {
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
fn identity_given_same_rule_and_text_is_equal_regardless_of_line() {
    // given
    let a = Finding::new("skip-added", Confidence::High, "m", 1, "a.py", "  IT.SKIP ");
    let b = Finding::new("skip-added", Confidence::High, "m", 9, "b.py", "it.skip");

    // when / then
    assert_eq!(a.identity(), b.identity());
}
