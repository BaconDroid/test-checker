use test_checker::detector::{Change, TestDetector};
use test_checker::models::Finding;
use test_checker::rules::rule_ids;

fn make_change(
    path: &str,
    language: &'static str,
    before: &str,
    after: &str,
    is_new: bool,
) -> Change {
    Change {
        file_path: path.to_owned(),
        language,
        new_text: after.to_owned(),
        old_text: before.to_owned(),
        is_new,
        is_test_file: true,
    }
}

fn rule_present(findings: &[Finding], rule: &str) -> bool {
    findings.iter().any(|finding| finding.rule == rule)
}

// --- Catalog ---------------------------------------------------------------

#[test]
fn rule_ids_given_catalog_returns_eleven_ordered_ids() {
    // given / when
    let ids = rule_ids();

    // then
    assert_eq!(ids.len(), 11);
    assert_eq!(
        ids,
        vec![
            "skip-focus-added",
            "tautological-assertion",
            "empty-test",
            "unknown-test",
            "gutted-test",
            "swallowed-error",
            "duplicate-test",
            "mock-of-sut",
            "over-mocking",
            "assertion-roulette",
            "redundant-assertion",
        ]
    );
}

// --- Traversal robustness --------------------------------------------------

#[test]
fn detect_given_deeply_nested_parentheses_returns_without_aborting() {
    // given: deeply nested input that previously overflowed the recursive walk
    let depth = 30_000;
    let payload = format!(
        "def test_deep():\n    assert {}{}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    let change = make_change("tests/test_deep.py", "python", "", &payload, true);

    // when: iterative traversal must not abort the process
    let findings: Vec<Finding> = TestDetector::new().detect(&change);

    // then: the call returns a finding list rather than crashing
    assert!(!rule_present(&findings, "swallowed-error"));
}

// --- skip-focus-added ------------------------------------------------------

#[test]
fn skip_focus_added_given_skipped_test_reports_rule() {
    // given
    let change = make_change(
        "src/app.test.ts",
        "ts",
        "",
        "it.skip(\"a\", () => {});",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "skip-focus-added"));
}

#[test]
fn skip_focus_added_given_plain_test_skips_rule() {
    // given
    let change = make_change(
        "src/app.test.ts",
        "ts",
        "",
        "it(\"a\", () => { expect(x).toBe(1); });",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "skip-focus-added"));
}

// --- tautological-assertion ------------------------------------------------

#[test]
fn tautological_assertion_given_always_true_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert True\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "tautological-assertion"));
}

#[test]
fn tautological_assertion_given_comparison_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == bar()\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "tautological-assertion"));
}

#[test]
fn tautological_assertion_given_two_identical_added_lines_reports_each_line() {
    // given: the same tautology added in two separate tests
    let change = make_change(
        "tests/test_two.py",
        "python",
        "",
        "def test_a():\n    assert True\n\ndef test_b():\n    assert True\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then: each added occurrence keeps its own line, so neither collapses
    let lines: Vec<usize> = findings
        .iter()
        .filter(|finding| finding.rule == "tautological-assertion")
        .map(|finding| finding.line)
        .collect();
    assert_eq!(lines, vec![2, 5]);
}

// --- empty-test ------------------------------------------------------------

#[test]
fn empty_test_given_pass_body_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    pass\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "empty-test"));
}

#[test]
fn empty_test_given_assertion_body_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == bar()\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "empty-test"));
}

// --- Block extraction for every grammar ------------------------------------

#[test]
fn empty_test_given_go_empty_body_reports_rule() {
    // given
    let change = make_change(
        "pkg/foo_test.go",
        "go",
        "",
        "package foo\n\nfunc TestA(t *testing.T) {\n}\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "empty-test"));
}

#[test]
fn unknown_test_given_go_assertion_free_body_reports_rule() {
    // given
    let change = make_change(
        "pkg/foo_test.go",
        "go",
        "",
        "package foo\n\nfunc TestA(t *testing.T) {\n\tdowork()\n}\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "unknown-test"));
}

#[test]
fn empty_test_given_rust_empty_body_reports_rule() {
    // given
    let change = make_change(
        "src/foo_test.rs",
        "rust",
        "",
        "#[test]\nfn t() {\n}\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "empty-test"));
}

#[test]
fn unknown_test_given_rust_assertion_free_body_reports_rule() {
    // given
    let change = make_change(
        "src/foo_test.rs",
        "rust",
        "",
        "#[test]\nfn t() {\n    do_work();\n}\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "unknown-test"));
}

#[test]
fn empty_test_given_js_empty_body_reports_rule() {
    // given
    let change = make_change(
        "src/foo.test.ts",
        "ts",
        "",
        "it(\"a\", () => {\n});\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "empty-test"));
}

#[test]
fn unknown_test_given_js_assertion_free_body_reports_rule() {
    // given
    let change = make_change(
        "src/foo.test.ts",
        "ts",
        "",
        "it(\"a\", () => {\n  doWork();\n});\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "unknown-test"));
}

// --- unknown-test ----------------------------------------------------------

#[test]
fn unknown_test_given_assertion_free_body_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    do_work()\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "unknown-test"));
}

#[test]
fn unknown_test_given_assertion_body_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == bar()\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "unknown-test"));
}

// --- gutted-test -----------------------------------------------------------

#[test]
fn gutted_test_given_removed_only_assertion_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "def test_a():\n    assert foo() == 1\n",
        "def test_a():\n    pass\n",
        false,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "gutted-test"));
}

#[test]
fn gutted_test_given_replaced_assertion_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "def test_a():\n    assert foo() == 1\n",
        "def test_a():\n    assert bar() == 2\n",
        false,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "gutted-test"));
}

#[test]
fn gutted_test_given_duplicate_removed_assertion_reports_removed_occurrence_line() {
    // given: one of two identical assertions is removed
    let change = make_change(
        "tests/test_a.py",
        "python",
        "def test_a():\n    assert foo() == 1\n    assert foo() == 1\n",
        "def test_a():\n    assert foo() == 1\n",
        false,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then: the finding points at the removed (second) occurrence at line 3,
    // not the surviving first match at line 2
    let gutted: Vec<usize> = findings
        .iter()
        .filter(|finding| finding.rule == "gutted-test")
        .map(|finding| finding.line)
        .collect();
    assert_eq!(gutted, vec![3]);
}

// --- swallowed-error -------------------------------------------------------

#[test]
fn swallowed_error_given_python_except_pass_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    try:\n        do_work()\n    except Exception:\n        pass\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "swallowed-error"));
}

#[test]
fn swallowed_error_given_python_bare_except_pass_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    try:\n        do_work()\n    except:\n        pass\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "swallowed-error"));
}

#[test]
fn swallowed_error_given_named_exception_with_pass_does_not_fire() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    try:\n        do_work()\n    except ValidationError:\n        password = x\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "swallowed-error"));
}

#[test]
fn swallowed_error_given_python_except_reraise_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    try:\n        do_work()\n    except Exception:\n        raise\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "swallowed-error"));
}

// --- duplicate-test --------------------------------------------------------

#[test]
fn duplicate_test_given_identical_bodies_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == 1\n\ndef test_b():\n    assert foo() == 1\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "duplicate-test"));
}

#[test]
fn duplicate_test_given_distinct_bodies_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == 1\n\ndef test_b():\n    assert foo() == 2\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "duplicate-test"));
}

// --- mock-of-sut -----------------------------------------------------------

#[test]
fn mock_of_sut_given_mocked_module_stem_reports_rule() {
    // given
    let change = make_change(
        "src/foo.test.ts",
        "ts",
        "",
        "vi.mock('./foo', () => ({}));",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "mock-of-sut"));
}

#[test]
fn mock_of_sut_given_unrelated_module_skips_rule() {
    // given
    let change = make_change(
        "src/foo.test.ts",
        "ts",
        "",
        "vi.mock('./bar', () => ({}));",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "mock-of-sut"));
}

#[test]
fn mock_of_sut_given_spy_on_imported_name_reports_rule() {
    // given
    let change = make_change(
        "src/foo.test.ts",
        "ts",
        "",
        "import { helper } from './foo';\nvi.spyOn(helper, 'run');\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "mock-of-sut"));
}

#[test]
fn mock_of_sut_given_spy_on_unimported_name_skips_rule() {
    // given
    let change = make_change(
        "src/foo.test.ts",
        "ts",
        "",
        "import { helper } from './foo';\nvi.spyOn(other, 'run');\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "mock-of-sut"));
}

// --- over-mocking ----------------------------------------------------------

#[test]
fn over_mocking_given_added_mock_identifier_reports_rule() {
    // given
    let change = make_change(
        "src/service.test.ts",
        "ts",
        "",
        "const mockClient = createMock();",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "over-mocking"));
}

#[test]
fn over_mocking_given_added_plain_identifier_skips_rule() {
    // given
    let change = make_change(
        "src/service.test.ts",
        "ts",
        "",
        "const client = createClient();",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "over-mocking"));
}

#[test]
fn over_mocking_given_dispatch_call_skips_rule() {
    // given: `dispatch` merely contains `patch`; it is not a `patch(` call
    let change = make_change(
        "src/service.test.ts",
        "ts",
        "",
        "const result = dispatch(event);",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "over-mocking"));
}

// --- assertion-roulette ----------------------------------------------------

#[test]
fn assertion_roulette_given_unlabeled_assertions_reports_rule() {
    // given
    let change = make_change(
        "src/app.test.ts",
        "ts",
        "",
        "it(\"a\", () => {\n  expect(foo()).toBe(1);\n  expect(bar()).toBe(2);\n});",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "assertion-roulette"));
}

#[test]
fn assertion_roulette_given_labeled_assertions_skips_rule() {
    // given
    let change = make_change(
        "src/app.test.ts",
        "ts",
        "",
        "it(\"a\", () => {\n  expect(foo(), \"one\").toBe(1);\n  expect(bar(), \"two\").toBe(2);\n});",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "assertion-roulette"));
}

// --- redundant-assertion ---------------------------------------------------

#[test]
fn redundant_assertion_given_repeated_line_reports_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == 1\n    assert foo() == 1\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "redundant-assertion"));
}

#[test]
fn redundant_assertion_given_distinct_lines_skips_rule() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert foo() == 1\n    assert bar() == 2\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "redundant-assertion"));
}

// --- Regression: assertion / capture correctness ---------------------------

#[test]
fn unknown_test_given_rust_assert_macros_skips_rule() {
    // given: every Rust assertion macro form must count as an assertion
    let bodies = [
        "#[test]\nfn t() {\n    assert!(x);\n}\n",
        "#[test]\nfn t() {\n    assert_eq!(2 + 2, 4);\n}\n",
        "#[test]\nfn t() {\n    assert_ne!(a, b);\n}\n",
        "#[test]\nfn t() {\n    debug_assert!(x);\n}\n",
    ];
    for body in bodies {
        let change = make_change("src/foo_test.rs", "rust", "", body, true);

        // when
        let findings = TestDetector::new().detect(&change);

        // then
        assert!(!rule_present(&findings, "unknown-test"), "body: {body}");
    }
}

#[test]
fn unknown_test_given_tokio_and_rstest_attributes_reports_rule() {
    // given: attribute macros beyond `#[test]` must still capture the body,
    // while `#[cfg(test)]` stays excluded
    let captured = [
        "#[tokio::test]\nasync fn t() {\n    do_work();\n}\n",
        "#[rstest]\nfn t() {\n    do_work();\n}\n",
    ];
    for body in captured {
        let change = make_change("src/foo_test.rs", "rust", "", body, true);

        // when
        let findings = TestDetector::new().detect(&change);

        // then
        assert!(rule_present(&findings, "unknown-test"), "body: {body}");
    }
    let cfg = "#[cfg(test)]\nfn t() {\n    do_work();\n}\n";
    let change = make_change("src/foo_test.rs", "rust", "", cfg, true);

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "unknown-test"), "body: {cfg}");
}

#[test]
fn unknown_test_given_edited_block_with_trailing_comment_reports_rule() {
    // given: an edit adds a body line whose trailing comment is stripped when
    // the added-line multiset is keyed
    let change = make_change(
        "tests/test_a.py",
        "python",
        "def test_a():\n    pass\n",
        "def test_a():\n    do_work()  # wip\n",
        false,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "unknown-test"));
}

#[test]
fn unknown_test_given_trivial_body_skips_rule() {
    // given: a `pass` body is empty-test, not also unknown-test
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    pass\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(rule_present(&findings, "empty-test"));
    assert!(!rule_present(&findings, "unknown-test"));
}

#[test]
fn unknown_test_given_describe_container_skips_container_block() {
    // given: a `describe` wraps an `it`; only the inner block is a test body
    let change = make_change(
        "src/app.test.ts",
        "ts",
        "",
        "describe(\"group\", () => {\n  it(\"a\", () => {\n    doWork();\n  });\n});\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    let unknown: Vec<_> = findings
        .iter()
        .filter(|finding| finding.rule == "unknown-test")
        .collect();
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].line, 2);
}

#[test]
fn assertion_roulette_given_python_assert_messages_skips_rule() {
    // given: two Python asserts, each carrying a trailing message
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert a == b, \"first\"\n    assert c == d, \"second\"\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "assertion-roulette"));
}

#[test]
fn mock_of_sut_given_python_dispatch_call_skips_rule() {
    // given: `dispatch(` only contains `patch(`; it is not a patch call
    let change = make_change(
        "tests/test_evt.py",
        "python",
        "",
        "def test_evt():\n    dispatch(\"evt\")\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert!(!rule_present(&findings, "mock-of-sut"));
}

#[test]
fn detect_pipeline_given_allow_marker_near_one_identical_tautology_keeps_the_other() {
    // given: two identical tautologies, an allow marker only near the first
    let change = make_change(
        "tests/test_two.py",
        "python",
        "",
        "def test_a():\n    # test-guard: allow\n    assert True\n\ndef test_b():\n    assert True\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);
    let findings = test_checker::filters::apply_filters(&findings, &change.new_text);

    // then: the marker suppresses only the first tautology; the second at line
    // 6 survives instead of being hidden by the collapsed first-match finding
    let lines: Vec<usize> = findings
        .iter()
        .filter(|finding| finding.rule == "tautological-assertion")
        .map(|finding| finding.line)
        .collect();
    assert_eq!(lines, vec![6]);
}

// --- Cross-cutting gates ---------------------------------------------------

#[test]
fn detect_given_non_test_file_yields_no_findings() {
    // given
    let mut change = make_change(
        "src/app.py",
        "python",
        "",
        "def test_a():\n    assert True\n",
        true,
    );
    change.is_test_file = false;

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert_eq!(findings, Vec::<Finding>::new());
}

#[test]
fn detect_given_disable_file_marker_suppresses_all_rules() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert True\n# test-guard-disable-file\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert_eq!(findings, Vec::<Finding>::new());
}

#[test]
fn detect_pipeline_given_allow_marker_suppresses_nearby_finding() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    # test-guard: allow\n    assert True\n",
        true,
    );

    // when: detect captures the finding, then the suppression filter drops it
    let findings = TestDetector::new().detect(&change);
    let findings = test_checker::filters::apply_filters(&findings, &change.new_text);

    // then
    assert_eq!(findings, Vec::<Finding>::new());
}

#[test]
fn detect_pipeline_given_finding_far_from_marker_keeps_it() {
    // given: the tautology is 5 lines above the only allow marker
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    assert True\n\n\n\n\n# test-guard: allow\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);
    let findings = test_checker::filters::apply_filters(&findings, &change.new_text);

    // then
    assert!(rule_present(&findings, "tautological-assertion"));
}
