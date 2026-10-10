use test_checker::filters::{
    code_text, count_real_lines, expected_from_sut_hit, forced_success_hit, glob_match,
    is_conditional_skip, is_file_disabled, is_test_declaration, is_test_path, is_trivial_body,
    matches_loosening_after, matches_loosening_before, mock_identifier_hit, negative_control_hit,
    normalize_assertion, normalize_body, parse_threshold, skip_focus_hit, strip_comments,
    strip_string_literals, tautological_hit, tests_not_run_hit, weakened_config_hit, within_bypass,
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

// --- Paths -----------------------------------------------------------------

#[test]
fn glob_match_given_double_star_matches_nested_test_file() {
    // given / when / then
    assert!(glob_match("**/*.test.*", "src/a.test.ts"));
    assert!(glob_match("**/*_test.*", "pkg/a_test.go"));
}

#[test]
fn glob_match_given_double_star_rejects_plain_source_file() {
    // given / when / then
    assert!(!glob_match("**/*.test.*", "src/a.ts"));
}

#[test]
fn glob_match_given_single_star_respects_path_separator() {
    // given / when / then
    assert!(glob_match("src/*.rs", "src/main.rs"));
    assert!(!glob_match("src/*.rs", "src/deep/main.rs"));
}

#[test]
fn is_test_path_given_test_layouts_returns_true() {
    // given / when / then
    assert!(is_test_path("src/a.test.ts"));
    assert!(is_test_path("tests/test_a.py"));
    assert!(is_test_path("pkg/foo_test.go"));
}

#[test]
fn is_test_path_given_source_layout_returns_false() {
    // given / when / then
    assert!(!is_test_path("src/a.ts"));
    assert!(!is_test_path("README.md"));
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

#[test]
fn forced_success_given_forced_exit_or_or_true_returns_true() {
    // given / when / then
    assert!(forced_success_hit("process.exit(0);"));
    assert!(forced_success_hit("sys.exit(0)"));
    assert!(forced_success_hit("run_tests || true"));
    assert!(forced_success_hit("--passWithNoTests"));
}

#[test]
fn forced_success_given_failing_exit_or_false_default_returns_false() {
    // given / when / then
    assert!(!forced_success_hit("process.exit(1);"));
    assert!(!forced_success_hit("const ok = value || false;"));
}

#[test]
fn forced_success_given_malformed_zero_forms_returns_false() {
    // given / when / then
    assert!(!forced_success_hit("exit0(1);"));
    assert!(!forced_success_hit("sys.exit(01)"));
    assert!(!forced_success_hit("process.exit(0x"));
}

#[test]
fn weakened_config_given_disable_directives_returns_true() {
    // given / when / then
    assert!(weakened_config_hit("@ts-nocheck"));
    assert!(weakened_config_hit("--no-verify"));
    assert!(weakened_config_hit("fail_under = 0"));
    assert!(weakened_config_hit("# ruff: noqa"));
}

#[test]
fn weakened_config_given_nonzero_threshold_returns_false() {
    // given / when / then
    assert!(!weakened_config_hit("fail_under = 80"));
    assert!(!weakened_config_hit("const value = compute();"));
}

#[test]
fn parse_threshold_given_coverage_key_returns_value() {
    // given
    let simple = parse_threshold("fail_under = 85");
    let hyphenated = parse_threshold("cov-fail-under=90");

    // when / then
    assert!(simple.is_some_and(|value| (value - 85.0).abs() < f64::EPSILON));
    assert!(hyphenated.is_some_and(|value| (value - 90.0).abs() < f64::EPSILON));
}

#[test]
fn parse_threshold_given_unrelated_key_returns_none() {
    // given / when / then
    assert!(parse_threshold("coverage = 85").is_none());
}

#[test]
fn parse_threshold_given_prefixed_key_returns_none() {
    // given / when / then
    assert!(parse_threshold("my_fail_under = 0").is_none());
}

#[test]
fn mock_identifier_given_dispatch_call_returns_false() {
    // given / when / then
    assert!(!mock_identifier_hit("const result = dispatch(event);"));
    assert!(!mock_identifier_hit("despatch(x);"));
    assert!(mock_identifier_hit("mocker.patch('module.fn')"));
    assert!(mock_identifier_hit("patch('module.fn')"));
}

// --- Command / config rules ------------------------------------------------

#[test]
fn tests_not_run_given_exclusion_flags_returns_true() {
    // given / when / then
    assert!(tests_not_run_hit("pytest --ignore=tests/slow.py"));
    assert!(tests_not_run_hit("pytest -k \"not slow\""));
    assert!(tests_not_run_hit("pytest --deselect tests/test_a.py"));
}

#[test]
fn tests_not_run_given_plain_command_returns_false() {
    // given / when / then
    assert!(!tests_not_run_hit("pytest tests/unit"));
}

#[test]
fn negative_control_given_python_broad_exception_returns_true() {
    // given / when / then
    assert!(negative_control_hit("pytest.raises(Exception)", "python"));
    assert!(negative_control_hit(
        "assertRaises(BaseException)",
        "python"
    ));
}

#[test]
fn negative_control_given_python_specific_exception_returns_false() {
    // given / when / then
    assert!(!negative_control_hit("pytest.raises(ValueError)", "python"));
}

#[test]
fn negative_control_given_javascript_broad_throw_returns_true() {
    // given / when / then
    assert!(negative_control_hit("expect(fn).toThrow()", "js"));
    assert!(negative_control_hit("expect(fn).not.toThrow()", "js"));
}

#[test]
fn negative_control_given_javascript_specific_throw_returns_false() {
    // given / when / then
    assert!(!negative_control_hit(
        "expect(fn).toThrow(ValueError)",
        "js"
    ));
}

#[test]
fn expected_from_sut_given_repeated_call_under_assert_returns_true() {
    // given / when / then
    assert!(expected_from_sut_hit("assert compute(x) == compute(y)"));
}

#[test]
fn expected_from_sut_given_single_call_returns_false() {
    // given / when / then
    assert!(!expected_from_sut_hit("assert compute(x) == expected"));
}

#[test]
fn expected_from_sut_given_ungated_comparison_returns_false() {
    // given / when / then
    assert!(!expected_from_sut_hit("compute(x) == compute(y)"));
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

// --- Matcher loosening -----------------------------------------------------

#[test]
fn matcher_loosening_given_assert_equal_before_and_assert_true_after_splits() {
    // given
    let before = "self.assertEqual(foo, bar)";
    let after = "self.assertTrue(foo)";
    let message = "assertEqual loosened to assertTrue";

    // when / then
    assert!(matches_loosening_before(before, message));
    assert!(!matches_loosening_after(before, message));
    assert!(!matches_loosening_before(after, message));
    assert!(matches_loosening_after(after, message));
}

#[test]
fn matcher_loosening_given_to_be_variants_splits() {
    // given
    let message = "toBe loosened to toBeTruthy";

    // when / then
    assert!(matches_loosening_before("expect(x).toBe(y)", message));
    assert!(matches_loosening_after("expect(x).toBeTruthy()", message));
}

#[test]
fn matcher_loosening_given_to_throw_inversion_splits() {
    // given
    let message = "toThrow inverted to not.toThrow";

    // when / then
    assert!(matches_loosening_before("expect(fn).toThrow()", message));
    assert!(matches_loosening_after("expect(fn).not.toThrow()", message));
}

#[test]
fn matcher_loosening_given_unrelated_line_returns_false() {
    // given
    let message = "assertEqual loosened to assertTrue";

    // when / then
    assert!(!matches_loosening_before("plain_call()", message));
    assert!(!matches_loosening_after("plain_call()", message));
}
