use test_checker::filters::{
    BddFilter, DirectiveFilter, ShebangFilter, SuppressionFilter, apply_filters,
    is_agent_memo_phrase,
};
use test_checker::models::{Confidence, Finding};

fn finding(text: &str) -> Finding {
    Finding::new(
        "commented-out-test",
        Confidence::High,
        "m",
        1,
        "tests/test_app.py",
        text,
    )
}

#[test]
fn bdd_filter_given_bdd_keyword_returns_true() {
    // given
    let filter = BddFilter;

    // when / then
    assert!(filter.should_skip(&finding("# given")));
    assert!(filter.should_skip(&finding("// when & then")));
    assert!(filter.should_skip(&finding("-- arrange")));
    assert!(!filter.should_skip(&finding("# regular comment")));
}

#[test]
fn directive_filter_given_directive_returns_true() {
    // given
    let filter = DirectiveFilter;

    // when / then
    assert!(filter.should_skip(&finding("# noqa: F401")));
    assert!(filter.should_skip(&finding("// @ts-ignore")));
    assert!(filter.should_skip(&finding("# type: ignore")));
    assert!(!filter.should_skip(&finding("# regular comment")));
}

#[test]
fn suppression_filter_given_marker_returns_true() {
    // given
    let filter = SuppressionFilter;

    // when / then
    assert!(filter.should_skip(&finding("# test-checker: allow")));
    assert!(filter.is_file_suppressed("x = 1  # test-checker: allow"));
    assert!(!filter.is_file_suppressed("x = 1"));
}

fn rule_finding(rule: &str, text: &str) -> Finding {
    Finding::new(rule, Confidence::High, "m", 1, "tests/test_app.py", text)
}

#[test]
fn suppression_filter_given_scoped_marker_suppresses_only_its_rule() {
    // given
    let filter = SuppressionFilter;
    let text = "@pytest.mark.skip  # test-checker: allow skip-added -- legacy";

    // when / then
    assert!(filter.should_skip(&rule_finding("skip-added", text)));
    assert!(!filter.should_skip(&rule_finding("no-assertion", text)));
}

#[test]
fn suppression_filter_given_scoped_marker_does_not_suppress_the_file() {
    // given
    let filter = SuppressionFilter;

    // when / then
    assert!(!filter.is_file_suppressed("x = 1  # test-checker: allow skip-added -- legacy"));
    assert!(filter.is_file_suppressed("x = 1  # test-checker: allow"));
}

#[test]
fn shebang_filter_given_shebang_returns_true() {
    // given
    let filter = ShebangFilter;

    // when / then
    assert!(filter.should_skip(&finding("#!/usr/bin/env bats")));
    assert!(filter.should_skip(&finding("#! /usr/bin/env bash")));
    assert!(!filter.should_skip(&finding("# regular comment")));
}

#[test]
fn apply_filters_given_allowed_and_suppressed_returns_empty() {
    // given
    let findings = vec![
        finding("# given"),
        finding("// @ts-ignore"),
        finding("# test-checker: allow"),
        finding("#!/usr/bin/env bats"),
    ];

    // when
    let result = apply_filters(&findings);

    // then
    assert_eq!(result.len(), 0);
}

#[test]
fn apply_filters_given_regular_finding_keeps_it() {
    // given
    let findings = vec![finding("assert x == y")];

    // when
    let result = apply_filters(&findings);

    // then
    assert_eq!(result.len(), 1);
}

#[test]
fn agent_memo_phrase_given_memo_text_returns_true() {
    // then
    assert!(is_agent_memo_phrase("Changed from old to new"));
    assert!(is_agent_memo_phrase("now this uses the new parser"));
    assert!(is_agent_memo_phrase("oldValue -> newValue"));
    assert!(is_agent_memo_phrase("Modified to use new implementation"));
}

#[test]
fn agent_memo_phrase_given_behavioral_text_returns_false() {
    // then
    assert!(!is_agent_memo_phrase("adds numbers"));
    assert!(!is_agent_memo_phrase("returns the sum of two numbers"));
    assert!(!is_agent_memo_phrase("rejects invalid input"));
}
