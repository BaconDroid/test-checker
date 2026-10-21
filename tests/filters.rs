use test_checker::filters::{
    BddFilter, DirectiveFilter, ShebangFilter, SuppressionFilter, apply_filters,
    is_agent_memo_phrase, is_line_suppressed,
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
fn suppression_filter_given_valid_marker_suppresses_only_its_rule() {
    // given
    let filter = SuppressionFilter;
    let text = "@pytest.mark.skip  # test-checker: allow skip-added -- tracked upstream #12";

    // when / then
    assert!(filter.should_skip(&rule_finding("skip-added", text)));
    assert!(!filter.should_skip(&rule_finding("no-assertion", text)));
}

#[test]
fn suppression_filter_given_invalid_marker_suppresses_nothing() {
    // given
    let filter = SuppressionFilter;

    // when / then: bare, unknown rule, and short reason all suppress nothing
    for text in [
        "# test-checker: allow",
        "# test-checker: allow not-a-rule -- tracked upstream #12",
        "# test-checker: allow skip-added -- short",
    ] {
        assert!(!filter.should_skip(&rule_finding("skip-added", text)));
        assert!(!filter.should_skip(&rule_finding("commented-out-test", text)));
    }
}

#[test]
fn is_line_suppressed_given_any_marker_returns_true() {
    // then: the helper only answers "is the marker text here", not "is it valid"
    assert!(is_line_suppressed("# test-checker: allow"));
    assert!(is_line_suppressed(
        "# test-checker: allow skip-added -- tracked upstream #12"
    ));
    assert!(!is_line_suppressed("# regular comment"));
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
fn apply_filters_given_allowed_and_validly_suppressed_returns_empty() {
    // given
    let findings = vec![
        finding("# given"),
        finding("// @ts-ignore"),
        rule_finding(
            "skip-added",
            "@pytest.mark.skip  # test-checker: allow skip-added -- tracked upstream #12",
        ),
        finding("#!/usr/bin/env bats"),
    ];

    // when
    let result = apply_filters(&findings);

    // then
    assert_eq!(result.len(), 0);
}

#[test]
fn apply_filters_given_bare_marker_keeps_the_finding() {
    // given: the old file-wide marker no longer suppresses
    let findings = vec![rule_finding("skip-added", "# test-checker: allow")];

    // when
    let result = apply_filters(&findings);

    // then
    assert_eq!(result.len(), 1);
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

fn rule_finding(rule: &str, text: &str) -> Finding {
    Finding::new(rule, Confidence::High, "m", 1, "tests/test_app.py", text)
}
