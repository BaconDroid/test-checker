use test_checker::detector::TestDetector;
use test_checker::language::language_for_path;

const FRAMEWORK_CASES: [(&str, &str); 15] = [
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
];

const WEAKEN_CASES: [(&str, &str); 15] = [
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
];

const NON_FRAMEWORK_CASES: [(&str, &str); 16] = [
    ("tests/clean.sh", "echo hello\n"),
    ("tests/clean.c", "int main(void) { return 0; }\n"),
    ("tests/clean.cpp", "int main() { return 0; }\n"),
    ("tests/clean.css", "body { color: red; }\n"),
    ("tests/clean.cue", "package x\n"),
    ("Dockerfile", "FROM debian\n"),
    ("tests/clean.elm", "module Main exposing (..)\n"),
    ("tests/clean.groovy", "def x = 1\n"),
    ("tests/clean.hcl", "resource \"x\" \"y\" {}\n"),
    ("tests/clean.html", "<html><body></body></html>\n"),
    ("tests/clean.ml", "let x = 1\n"),
    ("tests/clean.proto", "message X {}\n"),
    ("tests/clean.sql", "SELECT 1;\n"),
    ("tests/clean.svelte", "<div></div>\n"),
    ("tests/clean.toml", "x = 1\n"),
    ("tests/clean.yaml", "x: 1\n"),
];

#[test]
fn detect_given_python_clean_test_returns_no_findings() {
    // given
    let detector = TestDetector::new();

    // when
    let findings = detector.detect(FRAMEWORK_CASES[0].1, "tests/test_app.py", true, true);

    // then
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
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
fn detect_given_two_identical_tests_reports_duplicate_test() {
    // given
    let detector = TestDetector::new();
    let source =
        "def test_a():\n    assert compute() == 1\ndef test_b():\n    assert compute() == 1\n";

    // when
    let findings = detector.detect(source, "tests/test_app.py", true, true);

    // then
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "duplicate-test"),
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
