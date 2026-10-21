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
    let code = "def test_sums():\n    assert x == 1\n    # assert x == 2  # test-checker: allow commented-out-test -- tracked upstream #12\n";

    // when
    let findings = detector.detect(code, "tests/test_app.py", true, true);
    let filtered = apply_filters(&findings);

    // then
    assert_eq!(filtered.len(), 0);
}

use std::io::Write;
use std::process::{Command, Stdio};

#[derive(serde::Deserialize)]
struct Case {
    path: String,
    content: String,
    expected: Option<String>,
}

fn corpus() -> Result<Vec<Case>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/eval/corpus.json"))?;
    Ok(serde_json::from_str(&raw)?)
}

/// Run the real hook binary and collect the `rule="..."` ids from its stderr. The corpus spans
/// every confidence band, so the binary runs at `--min-confidence low`; a clean case must then
/// still produce no findings.
fn detected(case: &Case) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let payload = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": case.path, "content": case.content },
    })
    .to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_test-checker"))
        .arg("--min-confidence")
        .arg("low")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .as_mut()
        .ok_or("stdin handle unavailable")?
        .write_all(payload.as_bytes())?;
    let output = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&output.stderr);
    Ok(text
        .split("rule=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect())
}

/// The per-rule floor: every rule with ground-truth cases must be recalled, and a clean case must
/// stay clean. Mirrors the per-kind floor described in CONCEPTS.md.
#[allow(clippy::cast_precision_loss)]
fn floor_violations(cases: &[Case], got: &[Vec<String>]) -> Vec<String> {
    const MIN_BUCKET: usize = 1;
    const FLOOR: f64 = 0.5;

    let mut truth: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut hit: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut violations = Vec::new();
    for (case, rules) in cases.iter().zip(got) {
        match &case.expected {
            Some(expected) => {
                *truth.entry(expected.clone()).or_default() += 1;
                if rules.iter().any(|rule| rule == expected) {
                    *hit.entry(expected.clone()).or_default() += 1;
                }
            }
            None if !rules.is_empty() => {
                violations.push(format!("{}: unexpected findings {rules:?}", case.path));
            }
            None => {}
        }
    }
    for (rule, total) in &truth {
        if *total < MIN_BUCKET {
            continue;
        }
        let found = hit.get(rule).copied().unwrap_or(0);
        let recall = found as f64 / *total as f64;
        if recall < FLOOR {
            violations.push(format!("{rule}: recall {recall:.2}"));
        }
    }
    violations
}

#[test]
fn corpus_clears_the_per_rule_floor() -> Result<(), Box<dyn std::error::Error>> {
    // given
    let cases = corpus()?;

    // when
    let mut got = Vec::with_capacity(cases.len());
    for case in &cases {
        got.push(detected(case)?);
    }
    let violations = floor_violations(&cases, &got);

    // then
    assert!(
        violations.is_empty(),
        "per-rule floor violated: {violations:?}"
    );
    Ok(())
}

#[test]
fn per_rule_floor_trips_on_a_missed_rule() {
    // given: a case whose expected rule is not among the findings
    let cases = vec![Case {
        path: "tests/a_test.py".to_owned(),
        content: "def test_a():\n    assert x == 1\n".to_owned(),
        expected: Some("skip-added".to_owned()),
    }];
    let got = vec![Vec::new()];

    // when
    let violations = floor_violations(&cases, &got);

    // then
    assert!(!violations.is_empty(), "the floor must catch a missed rule");
}
