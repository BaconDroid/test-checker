use test_checker::detector::TestDetector;
use test_checker::language::language_for_path;
use test_checker::models::Confidence;

const FRAMEWORK_CASES: [(&str, &str); 21] = [
    (
        "tests/test_app.py",
        "def test_sums():\n    assert add(1, 2) == 3\n",
    ),
    (
        "tests/app.test.js",
        "it('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
    ),
    (
        "tests/app.test.ts",
        "it('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
    ),
    (
        "tests/app.test.tsx",
        "it('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
    ),
    (
        "tests/app_test.go",
        "package app\n\nfunc TestSums(t *testing.T) {\n\tif got := add(1, 2); got != 3 {\n\t\tt.Errorf(\"got %d\", got)\n\t}\n}\n",
    ),
    (
        "src/lib_test.rs",
        "#[test]\nfn sums() {\n    assert_eq!(add(1, 2), 3);\n}\n",
    ),
    (
        "test/app_test.rb",
        "def test_sums\n  assert_equal 3, add(1, 2)\nend\n",
    ),
    (
        "src/test/java/AppTest.java",
        "class AppTest {\n  @Test\n  void sums() {\n    assertEquals(3, add(1, 2));\n  }\n}\n",
    ),
    (
        "tests/AppTest.cs",
        "class AppTest {\n  [Fact]\n  public void Sums() {\n    Assert.Equal(3, Add(1, 2));\n  }\n}\n",
    ),
    (
        "tests/AppTest.php",
        "<?php\nclass AppTest {\n  public function testSums() {\n    $this->assertEquals(3, add(1, 2));\n  }\n}\n",
    ),
    (
        "src/test/kotlin/AppTest.kt",
        "class AppTest {\n  @Test\n  fun sums() {\n    assertEquals(3, add(1, 2))\n  }\n}\n",
    ),
    (
        "Tests/AppTests.swift",
        "func testSums() {\n  XCTAssertEqual(add(1, 2), 3)\n}\n",
    ),
    (
        "src/test/scala/AppSpec.scala",
        "class AppSpec {\n  it(\"sums\") {\n    assert(add(1, 2) == 3)\n  }\n}\n",
    ),
    (
        "test/sums_test.exs",
        "defmodule SumsTest do\n  test \"sums\" do\n    assert add(1, 2) == 3\n  end\nend\n",
    ),
    (
        "spec/sums_spec.lua",
        "describe(\"sums\", function()\n  it(\"sums\", function()\n    assert.are.equal(3, add(1, 2))\n  end)\nend)\n",
    ),
    (
        "tests/math_test.c",
        "TEST(Math, Adds) {\n  EXPECT_EQ(add(1, 2), 3);\n}\n",
    ),
    (
        "tests/math_test.cpp",
        "TEST(Math, Adds) {\n  EXPECT_EQ(add(1, 2), 3);\n}\n",
    ),
    (
        "test/Spec.groovy",
        "class Spec extends Specification {\n  def \"adds\"() {\n    expect:\n    assert add(1, 2) == 3\n  }\n}\n",
    ),
    (
        "test/test_math.ml",
        "let test_adds () =\n  Alcotest.(check int) \"adds\" 3 (add 1 2)\n",
    ),
    (
        "tests/MathTest.elm",
        "module MathTest exposing (..)\n\nimport Expect\n\ntest_adds : Test\ntest_adds =\n    test \"adds\" <|\n        \\_ -> Expect.equal (add 1 2) 3\n",
    ),
    (
        "tests/math.bats",
        "@test \"adds\" {\n  run add 1 2\n  assert_success\n}\n",
    ),
];

const WEAKEN_CASES: [(&str, &str); 21] = [
    (
        "tests/test_app.py",
        "@pytest.mark.skip(reason=\"later\")\ndef test_sums():\n    assert add(1, 2) == 3\n",
    ),
    (
        "tests/app.test.js",
        "it.skip('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
    ),
    (
        "tests/app.test.ts",
        "it.skip('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
    ),
    (
        "tests/app.test.tsx",
        "it.skip('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
    ),
    (
        "tests/app_test.go",
        "package app\n\nfunc TestSums(t *testing.T) {\n\tt.Skip(\"later\")\n\tif got := add(1, 2); got != 3 {\n\t\tt.Errorf(\"got %d\", got)\n\t}\n}\n",
    ),
    (
        "src/lib_test.rs",
        "#[test]\n#[ignore]\nfn sums() {\n    assert_eq!(add(1, 2), 3);\n}\n",
    ),
    (
        "test/app_test.rb",
        "xit(\"sums\") do\n  expect(add(1, 2)).to eq(3)\nend\n",
    ),
    (
        "src/test/java/AppTest.java",
        "class AppTest {\n  @Test\n  @Disabled\n  void sums() {\n    assertEquals(3, add(1, 2));\n  }\n}\n",
    ),
    (
        "tests/AppTest.cs",
        "class AppTest {\n  [Fact]\n  [Ignore]\n  public void Sums() {\n    Assert.Equal(3, Add(1, 2));\n  }\n}\n",
    ),
    (
        "tests/AppTest.php",
        "<?php\nclass AppTest {\n  public function testSums() {\n    $this->markTestSkipped('later');\n    $this->assertEquals(3, add(1, 2));\n  }\n}\n",
    ),
    (
        "src/test/kotlin/AppTest.kt",
        "class AppTest {\n  @Test\n  @Ignore\n  fun sums() {\n    assertEquals(3, add(1, 2))\n  }\n}\n",
    ),
    (
        "Tests/AppTests.swift",
        "func testSums() throws {\n  throw XCTSkip(\"later\")\n  XCTAssertEqual(add(1, 2), 3)\n}\n",
    ),
    (
        "src/test/scala/AppSpec.scala",
        "class AppSpec {\n  it(\"sums\") {\n    cancel(\"later\")\n    assert(add(1, 2) == 3)\n  }\n}\n",
    ),
    (
        "test/sums_test.exs",
        "defmodule SumsTest do\n  @tag :skip\n  test \"sums\" do\n    assert add(1, 2) == 3\n  end\nend\n",
    ),
    (
        "spec/sums_spec.lua",
        "describe(\"sums\", function()\n  it(\"sums\", function()\n    pending()\n    assert.are.equal(3, add(1, 2))\n  end)\nend)\n",
    ),
    (
        "tests/math_test.c",
        "TEST(Math, Adds) {\n  GTEST_SKIP();\n  EXPECT_EQ(add(1, 2), 3);\n}\n",
    ),
    (
        "tests/math_test.cpp",
        "TEST(Math, Adds) {\n  GTEST_SKIP();\n  EXPECT_EQ(add(1, 2), 3);\n}\n",
    ),
    (
        "test/Spec.groovy",
        "class Spec extends Specification {\n  @Ignore\n  def \"adds\"() {\n    expect:\n    assert add(1, 2) == 3\n  }\n}\n",
    ),
    (
        "test/test_math.ml",
        "let test_adds () =\n  skip \"later\";\n  Alcotest.(check int) \"adds\" 3 (add 1 2)\n",
    ),
    (
        "tests/MathTest.elm",
        "module MathTest exposing (..)\n\nimport Expect\n\ntest_adds : Test\ntest_adds =\n    skip <| test \"adds\" <|\n        \\_ -> Expect.equal (add 1 2) 3\n",
    ),
    (
        "tests/math.bats",
        "@test \"adds\" {\n  skip \"later\"\n  run add 1 2\n  assert_success\n}\n",
    ),
];

const NON_FRAMEWORK_CASES: [(&str, &str); 14] = [
    ("tests/clean.css", "body { color: red; }\n"),
    ("tests/clean.cue", "package x\n"),
    ("Dockerfile", "FROM debian\n"),
    ("tests/clean.hcl", "resource \"x\" \"y\" {}\n"),
    ("tests/clean.html", "<html><body></body></html>\n"),
    ("tests/clean.proto", "message X {}\n"),
    ("tests/clean.sql", "SELECT 1;\n"),
    ("tests/clean.svelte", "<div></div>\n"),
    ("tests/clean.toml", "x = 1\n"),
    ("tests/clean.yaml", "x: 1\n"),
    ("tests/clean.dart", "void main() {}\n"),
    ("tests/clean.hs", "main = pure ()\n"),
    ("tests/clean.r", "x <- 1\n"),
    ("tests/clean.zig", "pub fn main() void {}\n"),
];

#[test]
fn detect_given_brand_new_loose_assertion_does_not_weaken_at_medium_or_higher() {
    // given: a brand-new test whose only assertion is a loose matcher (no baseline).
    let detector = TestDetector::new();
    let source = "it('sums', () => {\n  expect(add(1, 2)).toBeDefined();\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then: presence-only, so it must not reach the default medium gate.
    assert!(
        !findings.iter().any(|finding| {
            finding.rule == "weakened-assertion" && finding.confidence != Confidence::Low
        }),
        "brand-new loose test reported at medium or higher: {findings:?}"
    );
}

#[test]
fn detect_edit_given_loosened_matcher_reports_weakened_assertion() {
    // given: an exact matcher replaced by a loose one for the same subject.
    let detector = TestDetector::new();
    let old = "it('sums', () => {\n  expect(add(1, 2)).toEqual(3);\n});\n";
    let new = "it('sums', () => {\n  expect(add(1, 2)).toBeDefined();\n});\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then
    assert!(
        findings.iter().any(|finding| {
            finding.rule == "weakened-assertion" && finding.confidence == Confidence::High
        }),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_brand_new_loose_test_does_not_weaken_at_medium_or_higher() {
    // given: an edit that appends a brand-new loose test with no removed counterpart.
    let detector = TestDetector::new();
    let old = "it('a', () => {\n  expect(one()).toEqual(1);\n});\n";
    let new = concat!(
        "it('a', () => {\n  expect(one()).toEqual(1);\n});\n",
        "it('b', () => {\n  expect(two()).toBeDefined();\n});\n"
    );

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings.iter().any(|finding| {
            finding.rule == "weakened-assertion" && finding.confidence != Confidence::Low
        }),
        "brand-new loose test in an edit reported at medium or higher: {findings:?}"
    );
}

#[test]
fn detect_given_commented_out_test_reports_commented_out_test() {
    // given
    let detector = TestDetector::new();
    let source = "def test_sums():\n    x = 1\n    # assert x == 2\n";

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "commented-out-test")
    );
}

#[test]
fn detect_given_commented_out_tests_disabled_ignores_comments() {
    // given
    let detector = TestDetector::new();
    let source = "def test_sums():\n    x = 1\n    # assert x == 2\n";

    // when
    let findings = detector.detect(source, "tests/test_app.py", false, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "commented-out-test")
    );
}

#[test]
fn detect_given_body_tests_disabled_ignores_weakening() {
    // given
    let detector = TestDetector::new();
    let source = "def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n";

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, false);

    // then
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn detect_edit_given_removed_assertion_reports_assertion_removed() {
    // given
    let detector = TestDetector::new();
    let old = "def test_sums():\n    assert add(1, 2) == 3\n    assert add(2, 2) == 4\n";
    let new = "def test_sums():\n    assert add(1, 2) == 3\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/test_app.py", true, true);

    // then
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "assertion-removed")
    );
}

#[test]
fn detect_edit_given_unchanged_content_returns_no_findings() {
    // given
    let detector = TestDetector::new();
    let source = "def test_sums():\n    assert add(1, 2) == 3\n";

    // when
    let findings = detector.detect_edit(source, source, "tests/test_app.py", true, true);

    // then
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn detect_given_commented_out_test_inside_html_script_reports_it() {
    // given
    let detector = TestDetector::new();
    let source = "<html><body>\n<script>\n// it('sums', () => { expect(add(1, 2)).toBe(3); });\n</script>\n</body></html>";

    // when
    let findings = detector.detect(source, "tests/page.html", true, true);

    // then
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "commented-out-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_python_decorator_skip_reports_skip_added() {
    // given
    let detector = TestDetector::new();
    let source = "@pytest.mark.skip\ndef test_sums():\n    assert add(1, 2) == 3\n";

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then
    assert!(findings.iter().any(|finding| finding.rule == "skip-added"));
}

#[test]
fn detect_edit_given_same_line_added_to_two_tests_does_not_report_duplicate() {
    // given
    let detector = TestDetector::new();
    let old = "def test_a():\n    a()\ndef test_b():\n    b()\n";
    let new = "def test_a():\n    a()\n    bump()\ndef test_b():\n    b()\n    bump()\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/test_app.py", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "duplicate-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_block_commented_assertion_in_body_is_not_classified() {
    // given: a block-commented assertion inside a test body must be invisible to the line rules
    let detector = TestDetector::new();
    let source = "it('x', () => {\n  /* expect(1).toBe(1) */\n  expect(add(1, 2)).toBe(3);\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "constant-assertion"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_multiline_block_commented_assertion_in_body_is_not_classified() {
    // given: an unadorned inner line of a multi-line block comment is still a comment
    let detector = TestDetector::new();
    let source =
        "it('x', () => {\n  /*\n  expect(1).toBe(1);\n  */\n  expect(add(1, 2)).toBe(3);\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "constant-assertion"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_code_after_block_comment_closer_is_kept() {
    // given: a real assertion after the `*/` on the closing line must not be discarded
    let detector = TestDetector::new();
    let source = "it('x', () => {\n  /*\n  setup\n  */ expect(add(1, 2)).toBe(3);\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "no-assertion"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_standalone_suite_skip_reports_skip_added() {
    // given: a top-level mocha-legacy `suite.skip(...)` is captured and reported
    let detector = TestDetector::new();
    let source = "suite.skip('a', () => {\n  expect(add(1, 2)).toBe(3);\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        findings.iter().any(|finding| finding.rule == "skip-added"),
        "findings: {findings:?}"
    );
}

#[test]
fn pipeline_coverage_given_every_language_parses_and_clean_fixtures_report_nothing() {
    // given
    let detector = TestDetector::new();
    let mut failures = Vec::new();

    // when
    for (path, source) in FRAMEWORK_CASES.iter().chain(NON_FRAMEWORK_CASES.iter()) {
        if language_for_path(path).is_none() {
            failures.push(format!("{path}: grammar did not resolve"));
            continue;
        }
        let findings = detector.detect(source, path, true, true);
        if !findings.is_empty() {
            failures.push(format!("{path}: clean fixture produced {findings:?}"));
        }
    }

    // then
    assert!(failures.is_empty(), "coverage failures: {failures:#?}");
}

#[test]
fn pipeline_coverage_given_framework_languages_the_test_body_query_compiles_and_captures() {
    // given
    let detector = TestDetector::new();
    let mut failures = Vec::new();

    // when
    for index in 0..FRAMEWORK_CASES.len() {
        let path = FRAMEWORK_CASES[index].0;
        let weakening = WEAKEN_CASES[index].1;
        let findings = detector.detect(weakening, path, true, true);
        if !findings.iter().any(|finding| finding.rule == "skip-added") {
            failures.push(format!(
                "{path}: captured body did not yield skip-added: {findings:?}"
            ));
        }
    }

    // then
    assert!(failures.is_empty(), "coverage failures: {failures:#?}");
}

#[test]
fn pipeline_coverage_given_non_framework_languages_capture_no_test_bodies() {
    // given
    let detector = TestDetector::new();
    let mut failures = Vec::new();

    // when: a skip marker in a language with no test framework must not be captured
    for (path, source) in NON_FRAMEWORK_CASES {
        let weakened = format!("{source}@pytest.mark.skip\n");
        let findings = detector.detect(&weakened, path, true, true);
        if !findings.is_empty() {
            failures.push(format!(
                "{path}: non-framework fixture produced {findings:?}"
            ));
        }
    }

    // then
    assert!(failures.is_empty(), "coverage failures: {failures:#?}");
}

#[test]
fn detect_edit_given_deleted_test_reports_test_removed() {
    // given
    let detector = TestDetector::new();
    let old = "def test_a():\n    assert one() == 1\ndef test_b():\n    assert two() == 2\n";
    let new = "def test_a():\n    assert one() == 1\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/test_app.py", true, true);

    // then
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "test-removed"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_renamed_test_does_not_report_test_removed() {
    // given: same body, different name
    let detector = TestDetector::new();
    let old = "def test_a():\n    assert one() == 1\n";
    let new = "def test_b():\n    assert one() == 1\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/test_app.py", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "test-removed"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_edited_test_does_not_report_test_removed() {
    // given: same name, changed body
    let detector = TestDetector::new();
    let old = "def test_a():\n    assert one() == 1\n";
    let new = "def test_a():\n    assert one() == 2\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/test_app.py", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "test-removed"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_case_only_literal_difference_does_not_report_duplicate() {
    // given: two tests whose bodies differ only by a literal's case (a real distinction)
    let detector = TestDetector::new();
    let source = "it('upper', () => {\n  expect(x).toBe('A');\n});\nit('lower', () => {\n  expect(x).toBe('a');\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "duplicate-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_commented_out_baseline_does_not_weaken_at_medium_or_higher() {
    // given: the only removed line was a commented-out assertion, which is not a real baseline
    let detector = TestDetector::new();
    let old = "it('x', () => {\n  // expect(a).toEqual(3);\n});\n";
    let new = "it('x', () => {\n  expect(a).toBeTruthy();\n});\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "weakened-assertion"
                && finding.confidence != Confidence::Low),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_test_reading_a_source_file_reports_source_grep_test() {
    // given: a test body that reads a source file and asserts on its text
    let detector = TestDetector::new();
    let source = "def test_reads_source():\n    text = Path(\"src/app.py\").read_text()\n    assert \"def \" in text\n";

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then: the rule fires at medium confidence (policy D29: it is meant to change behaviour), so
    // it reaches the default `--min-confidence medium` gate
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "source-grep-test"
                && finding.confidence == Confidence::Medium),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_test_reading_a_fixture_does_not_report_source_grep_test() {
    // given: the read targets a non-source fixture, and a second body reads a source file but
    // makes no assertion
    let detector = TestDetector::new();
    let fixture = "it('reads', () => {\n  const text = fs.readFileSync('tests/fixtures/data.json', 'utf8');\n  expect(text).toContain('x');\n});\n";
    let no_assertion = "def test_reads_source():\n    text = Path(\"src/app.py\").read_text()\n";

    // when
    let fixture_findings = detector.detect(fixture, "tests/app.test.js", true, true);
    let no_assertion_findings = detector.detect(no_assertion, "tests/test_app.py", true, true);

    // then
    assert!(
        !fixture_findings
            .iter()
            .any(|finding| finding.rule == "source-grep-test"),
        "findings: {fixture_findings:?}"
    );
    assert!(
        !no_assertion_findings
            .iter()
            .any(|finding| finding.rule == "source-grep-test"),
        "findings: {no_assertion_findings:?}"
    );
}

/// Build `count` Python test functions, each carrying one assertion.
fn python_tests(count: usize) -> String {
    use std::fmt::Write as _;
    let mut source = String::new();
    for index in 0..count {
        let _ = write!(
            source,
            "def test_{index}():\n    assert {index} == {index}\n"
        );
    }
    source
}

#[test]
fn detect_given_more_than_ten_added_tests_reports_test_volume() {
    // given: eleven new test functions in one payload
    let detector = TestDetector::new();
    let source = python_tests(11);

    // when
    let findings = detector.detect(&source, "tests/test_app.py", true, true);

    // then: one low-confidence volume warning
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "test-volume" && finding.confidence == Confidence::Low),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_ten_added_tests_does_not_report_test_volume() {
    // given: exactly ten new test functions, the threshold edge
    let detector = TestDetector::new();
    let source = python_tests(10);

    // when
    let findings = detector.detect(&source, "tests/test_app.py", true, true);

    // then
    assert!(
        !findings.iter().any(|finding| finding.rule == "test-volume"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_comment_only_body_difference_reports_near_duplicate() {
    // given: two tests with the same body except one carries an inner comment line
    let detector = TestDetector::new();
    let source = concat!(
        "def test_with_comment():\n",
        "    # explains the check\n",
        "    assert add(1, 2) == 3\n",
        "def test_without_comment():\n",
        "    assert add(1, 2) == 3\n",
    );

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then: the comment-only difference is reported once as a low-confidence near-duplicate, and
    // never as an exact duplicate
    let near: Vec<_> = findings
        .iter()
        .filter(|finding| finding.rule == "near-duplicate-test")
        .collect();
    assert_eq!(near.len(), 1, "findings: {findings:?}");
    assert_eq!(near[0].confidence, Confidence::Low);
    assert_eq!(
        near[0].message,
        "This test is identical to the test at line 1 except for comments."
    );
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "duplicate-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_comment_only_body_difference_with_different_assertions_reports_neither() {
    // given: identical comments but different assertions (a real behavioral difference)
    let detector = TestDetector::new();
    let source = concat!(
        "def test_one():\n",
        "    # same comment\n",
        "    assert add(1, 2) == 3\n",
        "def test_two():\n",
        "    # same comment\n",
        "    assert add(1, 2) == 4\n",
    );

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then: neither duplicate rule fires
    assert!(
        !findings.iter().any(
            |finding| finding.rule == "duplicate-test" || finding.rule == "near-duplicate-test"
        ),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_exact_duplicate_reports_duplicate_not_near_duplicate() {
    // given: two tests identical including their comment
    let detector = TestDetector::new();
    let source = concat!(
        "def test_a():\n",
        "    # same comment\n",
        "    assert add(1, 2) == 3\n",
        "def test_b():\n",
        "    # same comment\n",
        "    assert add(1, 2) == 3\n",
    );

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then: the exact signature still wins, so only duplicate-test fires
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "duplicate-test"),
        "findings: {findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "near-duplicate-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_added_comment_only_variant_of_unchanged_test_reports_near_duplicate() {
    // given: an edit appends a test that matches an existing, untouched test except for a comment;
    // the unchanged test must still act as the near-duplicate baseline (duplicate-test contract).
    let detector = TestDetector::new();
    let old = "def test_existing():\n    assert add(1, 2) == 3\n";
    let new = concat!(
        "def test_existing():\n",
        "    assert add(1, 2) == 3\n",
        "def test_added():\n",
        "    # explains the check\n",
        "    assert add(1, 2) == 3\n",
    );

    // when
    let findings = detector.detect_edit(old, new, "tests/test_app.py", true, true);

    // then: the touched variant is a low-confidence near-duplicate of the untouched test at line 1,
    // and is never reported as an exact duplicate
    let near: Vec<_> = findings
        .iter()
        .filter(|finding| finding.rule == "near-duplicate-test")
        .collect();
    assert_eq!(near.len(), 1, "findings: {findings:?}");
    assert_eq!(near[0].confidence, Confidence::Low);
    assert_eq!(
        near[0].message,
        "This test is identical to the test at line 1 except for comments."
    );
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "duplicate-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_field_round_trip_reports_echo_test() {
    // given: a DTO is constructed with a value and read back unchanged
    let detector = TestDetector::new();
    let source = "def test_dto():\n    dto = Dto(name)\n    assert dto.name == name\n";

    // when
    let findings = detector.detect(source, "tests/test_dto.py", true, true);

    // then
    assert!(
        findings.iter().any(|finding| finding.rule == "echo-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_mirrored_expression_reports_mirror_implementation_test() {
    // given: the expected value repeats the call's own arithmetic
    let detector = TestDetector::new();
    let source = "it('adds', () => {\n  expect(add(a, b)).toBe(a + b);\n});\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "mirror-implementation-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_incomplete_fragment_does_not_report_whole_test_rules() {
    // given: the new fragment has a captured test but no closing delimiter
    let detector = TestDetector::new();
    let old = "it('x', () => {\n  expect(add(1, 2)).toBe(3);\n});\n";
    let new = "it('x', () => {\n  dto.name = name;\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then: no whole-test rule may fire on an unreliable fragment
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "no-assertion" || finding.rule == "echo-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_added_echo_line_reports_echo_test() {
    // given: the edit introduces the write whose value is read back unchanged
    let detector = TestDetector::new();
    let old =
        "it('stores', () => {\n  dto.set_name(other);\n  expect(dto.get_name()).toBe(name);\n});\n";
    let new =
        "it('stores', () => {\n  dto.set_name(name);\n  expect(dto.get_name()).toBe(name);\n});\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then
    assert!(
        findings.iter().any(|finding| finding.rule == "echo-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_unchanged_echo_test_does_not_report_echo_test() {
    // given: an echo pattern exists, but the edit touches an unrelated assertion
    let detector = TestDetector::new();
    let old = "it('stores', () => {\n  dto.set_name(name);\n  expect(dto.get_name()).toBe(name);\n  expect(other).toBe(1);\n});\n";
    let new = "it('stores', () => {\n  dto.set_name(name);\n  expect(dto.get_name()).toBe(name);\n  expect(other).toBe(2);\n});\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then: the edit did not introduce the echo pattern
    assert!(
        !findings.iter().any(|finding| finding.rule == "echo-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_given_incomplete_test_fragment_does_not_report_no_assertion() {
    // given: a Write whose test node is incomplete (missing closing delimiters)
    let detector = TestDetector::new();
    let source = "it('x', () => {\n  dto.name = name;\n";

    // when
    let findings = detector.detect(source, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "no-assertion"),
        "findings: {findings:?}"
    );
}

#[test]
fn detect_edit_given_incomplete_new_fragment_does_not_report_test_removed() {
    // given: the new fragment is a partial test, so absence of the old test proves nothing
    let detector = TestDetector::new();
    let old = "it('old', () => {\n  expect(1).toBe(1);\n});\n";
    let new = "it('new', () => {\n";

    // when
    let findings = detector.detect_edit(old, new, "tests/app.test.js", true, true);

    // then
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule == "test-removed"),
        "findings: {findings:?}"
    );
}

// ---- masked-view precision fixtures ----

fn detect(path: &str, content: &str) -> Vec<String> {
    TestDetector::new()
        .detect(content, path, true, true)
        .into_iter()
        .map(|finding| finding.rule)
        .collect()
}

fn has(rules: &[String], rule: &str) -> bool {
    rules.iter().any(|candidate| candidate == rule)
}

#[test]
fn typescript_marker_inside_string_is_not_a_finding() {
    // then: `it.only` inside a string literal is data, not a focus marker
    assert!(!has(
        &detect(
            "tests/app.test.ts",
            "it('sums', () => {\n  const label = \"it.only('x', () => {})\";\n  expect(add(1, 2)).toBe(3);\n});\n"
        ),
        "focus-added"
    ));
    // but the same marker in code still fires
    assert!(has(
        &detect(
            "tests/app.test.ts",
            "it.only('sums', () => {\n  expect(add(1, 2)).toBe(3);\n});\n"
        ),
        "focus-added"
    ));
    // a commented-out test is still a finding
    assert!(has(
        &detect(
            "tests/app.test.ts",
            "// it('old', () => {\n//   expect(1).toBe(1);\n// });\n"
        ),
        "commented-out-test"
    ));
    // a JSDoc block carrying a focus marker is documentation
    assert!(!has(
        &detect(
            "tests/app.test.ts",
            "it('sums', () => {\n  /** it.only('x') */\n  expect(add(1, 2)).toBe(3);\n});\n"
        ),
        "focus-added"
    ));
}

#[test]
fn python_marker_inside_string_is_not_a_finding() {
    // then: the decorator text inside a string is data
    assert!(!has(
        &detect(
            "tests/test_app.py",
            "def test_sums():\n    label = \"@pytest.mark.skip\"\n    assert add(1, 2) == 3\n"
        ),
        "skip-added"
    ));
    // the same decorator in code still fires
    assert!(has(
        &detect(
            "tests/test_app.py",
            "@pytest.mark.skip\ndef test_sums():\n    assert add(1, 2) == 3\n"
        ),
        "skip-added"
    ));
    // a commented-out test is still a finding
    assert!(has(
        &detect(
            "tests/test_app.py",
            "# def test_old():\n#     assert x == 1\n"
        ),
        "commented-out-test"
    ));
    // a docstring carrying a skip marker is documentation
    assert!(!has(
        &detect(
            "tests/test_app.py",
            "def test_sums():\n    \"\"\"@pytest.mark.skip\"\"\"\n    assert add(1, 2) == 3\n"
        ),
        "skip-added"
    ));
}

#[test]
fn go_marker_inside_string_is_not_a_finding() {
    // then: `t.Skip(` inside a string literal is data
    assert!(!has(
        &detect(
            "tests/app_test.go",
            "package app\n\nfunc TestSums(t *testing.T) {\n\tlabel := \"t.Skip(\\\"later\\\")\"\n\tif got := add(1, 2); got != 3 {\n\t\tt.Errorf(\"got %d\", got)\n\t}\n}\n"
        ),
        "skip-added"
    ));
    // the same call in code still fires
    assert!(has(
        &detect(
            "tests/app_test.go",
            "package app\n\nfunc TestSums(t *testing.T) {\n\tt.Skip(\"later\")\n}\n"
        ),
        "skip-added"
    ));
    // a commented-out test is still a finding
    assert!(has(
        &detect(
            "tests/app_test.go",
            "// func TestOld(t *testing.T) {\n// \tt.Skip(\"later\")\n// }\n"
        ),
        "commented-out-test"
    ));
    // a doc comment carrying a skip call is documentation
    assert!(!has(
        &detect(
            "tests/app_test.go",
            "// t.Skip(\"documented\")\nfunc TestSums(t *testing.T) {\n\tif got := add(1, 2); got != 3 {\n\t\tt.Errorf(\"got %d\", got)\n\t}\n}\n"
        ),
        "skip-added"
    ));
}

#[test]
fn rust_marker_inside_string_is_not_a_finding() {
    // then: an attribute inside a string literal is data
    assert!(!has(
        &detect(
            "tests/app_test.rs",
            "#[test]\nfn sums() {\n    let label = \"#[ignore]\";\n    assert_eq!(add(1, 2), 3);\n}\n"
        ),
        "skip-added"
    ));
    // the same attribute in code still fires
    assert!(has(
        &detect(
            "tests/app_test.rs",
            "#[test]\n#[ignore]\nfn sums() {\n    assert_eq!(add(1, 2), 3);\n}\n"
        ),
        "skip-added"
    ));
    // a commented-out test is still a finding
    assert!(has(
        &detect(
            "tests/app_test.rs",
            "// #[test]\n// fn old() {\n//     assert_eq!(1, 1);\n// }\n"
        ),
        "commented-out-test"
    ));
    // a doc comment carrying an attribute is documentation
    assert!(!has(
        &detect(
            "tests/app_test.rs",
            "/// #[ignore]\n#[test]\nfn sums() {\n    assert_eq!(add(1, 2), 3);\n}\n"
        ),
        "skip-added"
    ));
}

#[test]
fn java_marker_inside_string_is_not_a_finding() {
    // then: an annotation inside a string literal is data
    assert!(!has(
        &detect(
            "src/test/java/AppTest.java",
            "class AppTest {\n  @Test\n  void sums() {\n    String label = \"@Disabled\";\n    assertEquals(3, add(1, 2));\n  }\n}\n"
        ),
        "skip-added"
    ));
    // the same annotation in code still fires
    assert!(has(
        &detect(
            "src/test/java/AppTest.java",
            "class AppTest {\n  @Test\n  @Disabled\n  void sums() {\n    assertEquals(3, add(1, 2));\n  }\n}\n"
        ),
        "skip-added"
    ));
    // a commented-out test is still a finding
    assert!(has(
        &detect(
            "src/test/java/AppTest.java",
            "// @Test\n// void old() {\n//   assertEquals(1, 1);\n// }\n"
        ),
        "commented-out-test"
    ));
    // a Javadoc block carrying an annotation is documentation
    assert!(!has(
        &detect(
            "src/test/java/AppTest.java",
            "class AppTest {\n  /** @Disabled */\n  @Test\n  void sums() {\n    assertEquals(3, add(1, 2));\n  }\n}\n"
        ),
        "skip-added"
    ));
}

#[test]
fn masked_view_preserves_line_numbers() {
    // given: a multi-line string whose content carries a marker, followed by a real skip
    let content = "def test_sums():\n    text = \"\"\"@pytest.mark.skip\n    assert x == 1\n    \"\"\"\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n";

    // when
    let findings = TestDetector::new().detect(content, "tests/test_app.py", true, true);

    // then: the real marker is still reported on its original line
    let skip = findings
        .iter()
        .find(|finding| finding.rule == "skip-added" && finding.line_number == 5);
    assert!(skip.is_some(), "findings: {findings:?}");
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.rule == "skip-added")
            .count(),
        1,
        "the marker inside the docstring must not surface"
    );
}

#[test]
fn agent_memo_reads_the_original_text() {
    // given: a memo phrase in a test name (a string literal, masked for line rules)
    let findings = TestDetector::new().detect(
        "it('now we parse twice', () => {\n  expect(add(1, 2)).toBe(3);\n});\n",
        "tests/app.test.js",
        true,
        true,
    );

    // then: the comment/text rule still reads the original string
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "agent-memo-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn source_grep_reads_the_original_literals() {
    // given: the read target is only visible in the string literal
    let findings = TestDetector::new().detect(
        "def test_reads_source():\n    text = Path(\"src/app.py\").read_text()\n    assert \"def \" in text\n",
        "tests/test_app.py",
        true,
        true,
    );

    // then: masking the view must not hide the source-grep signal
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "source-grep-test"),
        "findings: {findings:?}"
    );
}

#[test]
fn template_raw_and_heredoc_content_is_masked() {
    // then: a focus marker in a template string is data
    assert!(!has(
        &detect(
            "tests/app.test.ts",
            "it('sums', () => {\n  const label = `it.only('x')`;\n  expect(add(1, 2)).toBe(3);\n});\n"
        ),
        "focus-added"
    ));
    // a raw string
    assert!(!has(
        &detect(
            "tests/app_test.rs",
            "#[test]\nfn sums() {\n    let label = r#\"#[ignore]\"#;\n    assert_eq!(add(1, 2), 3);\n}\n"
        ),
        "skip-added"
    ));
    // a raw Go string
    assert!(!has(
        &detect(
            "tests/app_test.go",
            "package app\n\nfunc TestSums(t *testing.T) {\n\tlabel := `t.Skip(\"later\")`\n\tif got := add(1, 2); got != 3 {\n\t\tt.Errorf(\"got %d\", got)\n\t}\n}\n"
        ),
        "skip-added"
    ));
    // a Ruby heredoc
    assert!(!has(
        &detect(
            "test/app_test.rb",
            "def test_sums\n  label = <<~EOT\n    it.only('x')\n  EOT\n  assert_equal 3, add(1, 2)\nend\n"
        ),
        "focus-added"
    ));
}

#[test]
fn detect_given_payload_markers_in_rust_string_literals_reports_no_line_findings() {
    // given: the shapes that produced the known false positives in `tests/readme_hook.rs` and
    // `tests/api.rs` — framework markers and assertion snippets carried as Rust string data
    let source = r##"#[test]
fn reads_payloads() {
    let python = "def test_a():\n    @pytest.mark.skip\n    assert x == 1\n";
    let js = r#"it.only('x', () => { expect(1).toBeDefined(); });"#;
    let java = "assertEquals(1, 1)";
    let json = b"{\"tool_name\":\"Write\",\"tool_input\":{\"content\":\"@pytest.mark.skip\"}}";
    assert!(python.contains("skip"));
}
"##;

    // when
    let findings = TestDetector::new().detect(source, "tests/readme_hook.rs", true, true);

    // then: the string content is data; only the real assertion on the last line is code
    for rule in [
        "skip-added",
        "focus-added",
        "weakened-assertion",
        "constant-assertion",
        "conditional-assertion",
        "incomplete-assertion",
    ] {
        assert!(
            !findings.iter().any(|finding| finding.rule == rule),
            "{rule} leaked from a string literal: {findings:?}"
        );
    }
}

#[test]
fn detect_given_distinct_literal_operands_does_not_report_tautology() {
    // given: masking the string content collapses both operands' text
    let detector = TestDetector::new();
    let distinct = "def test_a():\n    assert f(\"a\") == f(\"b\")\n";
    let same = "def test_a():\n    assert f(\"a\") == f(\"a\")\n";

    // when
    let distinct_findings = detector.detect(distinct, "tests/test_app.py", true, true);
    let same_findings = detector.detect(same, "tests/test_app.py", true, true);

    // then: only the expression compared with itself is a tautology
    assert!(
        !distinct_findings
            .iter()
            .any(|finding| finding.rule == "tautological-assertion"),
        "findings: {distinct_findings:?}"
    );
    assert!(
        same_findings
            .iter()
            .any(|finding| finding.rule == "tautological-assertion"),
        "findings: {same_findings:?}"
    );
}
