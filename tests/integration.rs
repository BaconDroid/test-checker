use test_checker::detector::TestDetector;
use test_checker::filters::apply_filters;
use test_checker::output::format_hook_message;

#[test]
fn full_pipeline_given_weakening_returns_formatted_message() {
    // given
    let detector = TestDetector::new();
    let code = "def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n";

    // when
    let findings = detector.detect(code, "tests/test_app.py", true, true);
    let filtered = apply_filters(&findings);
    let message = format_hook_message(&filtered, "");

    // then
    assert_ne!(filtered.len(), 0);
    assert!(message.contains("TEST-WEAKENING DETECTED"));
    assert!(message.contains("tests/test_app.py"));
}

#[test]
fn full_pipeline_given_suppression_marker_filters_out() {
    // given
    let detector = TestDetector::new();
    let code = "def test_sums():\n    assert x == 1\n    # assert x == 2  # test-checker: allow\n";

    // when
    let findings = detector.detect(code, "tests/test_app.py", true, true);
    let filtered = apply_filters(&findings);

    // then
    assert_eq!(filtered.len(), 0);
}
