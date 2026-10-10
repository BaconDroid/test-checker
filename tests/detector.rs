use test_checker::detector::{Change, TestDetector, rule_ids};
use test_checker::models::Finding;

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
fn detect_given_allow_marker_suppresses_nearby_finding() {
    // given
    let change = make_change(
        "tests/test_a.py",
        "python",
        "",
        "def test_a():\n    # test-guard: allow\n    assert True\n",
        true,
    );

    // when
    let findings = TestDetector::new().detect(&change);

    // then
    assert_eq!(findings, Vec::<Finding>::new());
}
