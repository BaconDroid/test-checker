use test_checker::detector::{Change, TestDetector};
use test_checker::filters::{
    apply_filters, code_text, count_real_lines, is_conditional_skip, is_file_disabled,
    is_test_declaration, is_trivial_body, mock_identifier_hit, normalize_assertion, normalize_body,
    skip_focus_hit, strip_comments, strip_string_literals, tautological_hit, within_bypass,
};

// --- Comment / code text helpers -------------------------------------------

#[test]
fn strip_comments_given_python_line_comment_removes_suffix() {
    // given / when / then
    assert_eq!(strip_comments("x = 1  # note", "python"), "x = 1  ");
}

#[test]
fn strip_comments_given_javascript_block_comment_removes_it() {
    // given / when / then
    assert_eq!(strip_comments("/* block */ y", "js"), " y");
}

#[test]
fn strip_comments_given_hash_inside_string_keeps_string() {
    // given / when / then
    assert_eq!(
        strip_comments("x = \"# not a comment\"", "python"),
        "x = \"# not a comment\""
    );
}

#[test]
fn strip_comments_given_python_shebang_removes_line() {
    // given / when / then
    assert_eq!(strip_comments("#!/usr/bin/env python", "python"), "");
}

#[test]
fn code_text_given_string_literal_blanks_it() {
    // given / when / then
    assert_eq!(
        code_text("assert x == \"literal\"", "python"),
        "assert x == "
    );
}

#[test]
fn code_text_given_comment_and_string_strips_both() {
    // given / when / then
    assert_eq!(code_text("const a = \"x\"; // note", "js"), "const a = ; ");
}

#[test]
fn strip_string_literals_given_quoted_value_blanks_it() {
    // given / when / then
    assert_eq!(strip_string_literals("a \"x\" b"), "a  b");
}

#[test]
fn count_real_lines_given_blank_and_comment_lines_ignores_them() {
    // given / when / then
    assert_eq!(count_real_lines("// c\n\nx\ny", "js"), 2);
}

// --- Normalization ---------------------------------------------------------

#[test]
fn normalize_assertion_given_self_assert_equal_folds_receiver_and_name() {
    // given / when / then
    assert_eq!(
        normalize_assertion("self.assertEqual(a, b)"),
        "assertEquals(a,b)"
    );
}

#[test]
fn normalize_assertion_given_to_be_folds_to_to_equal() {
    // given / when / then
    assert_eq!(
        normalize_assertion("expect(x).toBe(y)"),
        "expect(x).toEqual(y)"
    );
}

#[test]
fn normalize_assertion_given_plain_assert_keeps_squashed_text() {
    // given / when / then
    assert_eq!(normalize_assertion("assert foo() == 1"), "assertfoo()==1");
}

#[test]
fn normalize_body_given_comment_only_lines_drops_them() {
    // given
    let lines = ["// c", "x = 1", "", "y = 2"];

    // when
    let normalized = normalize_body(lines.iter().copied(), "js");

    // then
    assert_eq!(normalized, "x = 1\ny = 2");
}

#[test]
fn normalize_body_given_code_lines_keeps_them_in_order() {
    // given
    let lines = ["x = 1", "y = 2"];

    // when
    let normalized = normalize_body(lines.iter().copied(), "js");

    // then
    assert_eq!(normalized, "x = 1\ny = 2");
}

// --- Suppression markers ---------------------------------------------------

#[test]
fn within_bypass_given_marker_within_window_returns_true() {
    // given
    let text = "line1\ntest-guard: allow\nline3";

    // when / then
    assert!(within_bypass(text, 2));
}

#[test]
fn within_bypass_given_zero_line_returns_false() {
    // given
    let text = "line1\ntest-guard: allow\nline3";

    // when / then
    assert!(!within_bypass(text, 0));
}

#[test]
fn within_bypass_given_marker_outside_window_returns_false() {
    // given
    let text = "test-guard: allow\n1\n2\n3\n4\n5";

    // when / then
    assert!(!within_bypass(text, 6));
}

#[test]
fn within_bypass_given_marker_two_lines_away_returns_true() {
    // given
    let text = "test-guard: allow\nm2\nm3\nm4\nm5";

    // when / then
    assert!(within_bypass(text, 3));
}

#[test]
fn within_bypass_given_marker_three_lines_away_returns_false() {
    // given
    let text = "test-guard: allow\nm2\nm3\nm4\nm5";

    // when / then
    assert!(!within_bypass(text, 4));
}

#[test]
fn within_bypass_given_allow_with_varied_spacing_returns_true() {
    // given / when / then
    for marker in [
        "# test-guard: allow",
        "# test-guard:  allow",
        "# test-guard:\tallow",
        "# test-guard:allow",
    ] {
        let text = format!("{marker}\ncode");
        assert!(within_bypass(&text, 1), "marker: {marker}");
    }
}

#[test]
fn within_bypass_given_allowed_or_allowance_returns_false() {
    // given / when / then
    for marker in ["# test-guard: allowed", "# test-guard: allowance"] {
        let text = format!("{marker}\ncode");
        assert!(!within_bypass(&text, 1), "marker: {marker}");
    }
}

#[test]
fn is_file_disabled_given_marker_returns_true() {
    // given / when / then
    assert!(is_file_disabled("// test-guard-disable-file\nx = 1"));
}

#[test]
fn is_file_disabled_given_plain_text_returns_false() {
    // given / when / then
    assert!(!is_file_disabled("// just a comment\nx = 1"));
}

// --- Skip / focus ----------------------------------------------------------

#[test]
fn skip_focus_given_javascript_skip_and_only_returns_true() {
    // given / when / then
    assert!(skip_focus_hit("it.skip(\"x\", () => {})", "js"));
    assert!(skip_focus_hit("describe.only(\"x\", () => {})", "ts"));
}

#[test]
fn skip_focus_given_javascript_plain_test_returns_false() {
    // given / when / then
    assert!(!skip_focus_hit("it(\"x\", () => {})", "js"));
}

#[test]
fn skip_focus_given_python_decorator_returns_true() {
    // given / when / then
    assert!(skip_focus_hit("@pytest.mark.skip", "python"));
    assert!(skip_focus_hit(
        "@unittest.skipIf(cond, \"reason\")",
        "python"
    ));
}

#[test]
fn skip_focus_given_python_plain_def_returns_false() {
    // given / when / then
    assert!(!skip_focus_hit("def test_a():", "python"));
}

#[test]
fn skip_focus_given_go_and_rust_markers_return_true() {
    // given / when / then
    assert!(skip_focus_hit("t.Skip(\"later\")", "go"));
    assert!(skip_focus_hit("#[ignore]", "rust"));
}

#[test]
fn skip_focus_given_go_and_rust_plain_lines_return_false() {
    // given / when / then
    assert!(!skip_focus_hit("t.Log(\"x\")", "go"));
    assert!(!skip_focus_hit("#[test]", "rust"));
}

#[test]
fn is_conditional_skip_given_condition_marker_returns_true() {
    // given / when / then
    assert!(is_conditional_skip("@pytest.mark.skipif(cond)"));
    assert!(is_conditional_skip("if (!ready) { skip(); }"));
}

#[test]
fn is_conditional_skip_given_plain_skip_returns_false() {
    // given / when / then
    assert!(!is_conditional_skip("it.skip(\"x\", () => {})"));
}

#[test]
fn is_conditional_skip_given_if_after_skip_returns_false() {
    // given / when / then
    assert!(!is_conditional_skip("t.Skip(\"we skip if X\")"));
}

#[test]
fn is_conditional_skip_given_if_before_skip_returns_true() {
    // given / when / then
    assert!(is_conditional_skip("if slow: t.Skip(\"x\")"));
}

// --- Assertion shapes ------------------------------------------------------

#[test]
fn tautological_given_always_true_assertions_returns_true() {
    // given / when / then
    assert!(tautological_hit("assertTrue(True)"));
    assert!(tautological_hit("assert True"));
    assert!(tautological_hit("expect(true).toBe(true)"));
    assert!(tautological_hit("assert x == x"));
}

#[test]
fn tautological_given_meaningful_assertions_returns_false() {
    // given / when / then
    assert!(!tautological_hit("assert foo() == bar()"));
    assert!(!tautological_hit("expect(value).toBe(1)"));
}

// --- Declaration / body ----------------------------------------------------

#[test]
fn mock_identifier_given_mock_names_returns_true() {
    // given / when / then
    assert!(mock_identifier_hit("const mockClient = createMock();"));
    assert!(mock_identifier_hit(
        "monkeypatch.setattr(module, \"fn\", stub)"
    ));
    assert!(mock_identifier_hit("patch(\"module.fn\")"));
}

#[test]
fn mock_identifier_given_plain_names_returns_false() {
    // given / when / then
    assert!(!mock_identifier_hit("const client = createClient();"));
}

#[test]
fn mock_identifier_given_dispatch_call_returns_false() {
    // given / when / then
    assert!(!mock_identifier_hit("const result = dispatch(event);"));
    assert!(!mock_identifier_hit("despatch(x);"));
}

#[test]
fn is_test_declaration_given_language_declarations_returns_true() {
    // given / when / then
    assert!(is_test_declaration("it(\"a\", () => {})", "js"));
    assert!(is_test_declaration("def test_a():", "python"));
    assert!(is_test_declaration("async def test_b():", "python"));
    assert!(is_test_declaration("func TestA(t *testing.T) {", "go"));
    assert!(is_test_declaration("#[test]", "rust"));
}

#[test]
fn is_test_declaration_given_helper_functions_returns_false() {
    // given / when / then
    assert!(!is_test_declaration("helper()", "js"));
    assert!(!is_test_declaration("def helper():", "python"));
    assert!(!is_test_declaration("func helper() {", "go"));
    assert!(!is_test_declaration("#[ignore]", "rust"));
}

#[test]
fn is_trivial_body_given_empty_bodies_returns_true() {
    // given / when / then
    assert!(is_trivial_body(""));
    assert!(is_trivial_body("   "));
    assert!(is_trivial_body("pass"));
    assert!(is_trivial_body("..."));
}

#[test]
fn is_trivial_body_given_executable_body_returns_false() {
    // given / when / then
    assert!(!is_trivial_body("assert x == 1"));
}

// --- Suppression at the finding boundary -----------------------------------

#[test]
fn apply_filters_given_js_finding_near_allow_marker_drops_it() {
    // given: a tautological assertion two lines below an allow marker
    let change = Change {
        file_path: "src/app.test.ts".to_owned(),
        language: "ts",
        new_text: "it(\"a\", () => {\n  // test-guard: allow\n  expect(true).toBe(true);\n});\n"
            .to_owned(),
        old_text: String::new(),
        is_new: true,
        is_test_file: true,
    };

    // when: detect captures the finding, then the suppression filter drops it
    let findings = TestDetector::new().detect(&change);
    let findings = apply_filters(&findings, &change.new_text);

    // then: the allow filter removes the finding the classifier would emit
    assert_eq!(findings, Vec::new());
}

#[test]
fn apply_filters_given_gutted_test_with_same_line_marker_keeps_gutted() {
    // given: an assertion removed, with an allow marker on the same numeric
    // line in new_text (the gutted finding reports an old-text line)
    let change = Change {
        file_path: "tests/test_a.py".to_owned(),
        language: "python",
        new_text: "def test_a():\n    # test-guard: allow\n    pass\n".to_owned(),
        old_text: "def test_a():\n    assert foo() == 1\n".to_owned(),
        is_new: false,
        is_test_file: true,
    };

    // when
    let findings = TestDetector::new().detect(&change);
    let findings = apply_filters(&findings, &change.new_text);

    // then: gutted-test is exempt from the new-text bypass window
    assert!(
        findings.iter().any(|finding| finding.rule == "gutted-test"),
        "gutted-test must survive an unrelated same-line marker"
    );
}

#[test]
fn apply_filters_given_finding_far_from_marker_keeps_it() {
    // given: a tautological assertion 5 lines above the only allow marker
    let change = Change {
        file_path: "tests/test_a.py".to_owned(),
        language: "python",
        new_text: "def test_a():\n    assert True\n\n\n\n\n# test-guard: allow\n".to_owned(),
        old_text: String::new(),
        is_new: true,
        is_test_file: true,
    };

    // when
    let findings = TestDetector::new().detect(&change);
    let findings = apply_filters(&findings, &change.new_text);

    // then: the far marker does not suppress the finding
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "tautological-assertion")
    );
}
