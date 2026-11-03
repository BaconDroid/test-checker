use test_checker::filters::KNOWN_RULES;
use test_checker::models::{Confidence, Finding};
use test_checker::output::{build_findings_xml, format_hook_message};

fn finding(message: &str, line_number: usize, rule: &str) -> Finding {
    Finding::new(
        rule,
        Confidence::High,
        message,
        line_number,
        "tests/test_app.py",
        "",
    )
}

#[test]
fn build_findings_xml_given_single_finding_returns_exact_shape() {
    // given
    let findings = vec![finding("A test skip marker was added.", 10, "skip-added")];

    // when
    let result = build_findings_xml(&findings, "tests/test_app.py");

    // then
    assert_eq!(
        result,
        "<findings file=\"tests/test_app.py\">\n\t<finding line-number=\"10\" rule=\"skip-added\" confidence=\"high\" reason=\"skip or ignore marker added\">A test skip marker was added.</finding>\n</findings>",
    );
}

#[test]
fn build_findings_xml_given_empty_findings_returns_empty_string() {
    // given
    let findings = Vec::new();

    // when
    let result = build_findings_xml(&findings, "tests/test_app.py");

    // then
    assert_eq!(result, "");
}

#[test]
fn build_findings_xml_given_special_characters_escapes_text_and_attributes() {
    // given
    let findings = vec![Finding::new(
        "tautological-assertion",
        Confidence::High,
        "assert 1 < 2 && 3 > 2",
        1,
        "tests/a&b.py",
        "",
    )];

    // when
    let result = build_findings_xml(&findings, "tests/a&b.py");

    // then
    assert_eq!(
        result,
        "<findings file=\"tests/a&amp;b.py\">\n\t<finding line-number=\"1\" rule=\"tautological-assertion\" confidence=\"high\" reason=\"tautological assertion\">assert 1 &lt; 2 &amp;&amp; 3 &gt; 2</finding>\n</findings>",
    );
}

#[test]
fn build_findings_xml_given_echo_test_returns_specific_reason() {
    // given
    let findings = vec![finding(
        "The test writes a value and reads it back unchanged through a trivial accessor.",
        3,
        "echo-test",
    )];

    // when
    let result = build_findings_xml(&findings, "tests/test_app.py");

    // then
    assert!(
        result.contains("reason=\"value echoed back unchanged\""),
        "{result}"
    );
    assert!(!result.contains("reason=\"finding\""), "{result}");
}

#[test]
fn build_findings_xml_given_mirror_implementation_test_returns_specific_reason() {
    // given
    let findings = vec![finding(
        "The expected value restates the code under test instead of pinning its behaviour.",
        3,
        "mirror-implementation-test",
    )];

    // when
    let result = build_findings_xml(&findings, "tests/test_app.py");

    // then
    assert!(
        result.contains("reason=\"expected value mirrors the implementation\""),
        "{result}"
    );
    assert!(!result.contains("reason=\"finding\""), "{result}");
}

/// The README `## Rules` paragraph is the documented rule catalog: every id it lists must be
/// suppressible via a scoped marker (`KNOWN_RULES`) and must render a specific stable reason, and
/// neither support table may list an id the README does not document.
#[test]
fn readme_rule_ids_stay_in_sync_with_the_support_tables() -> Result<(), Box<dyn std::error::Error>>
{
    // given: the id paragraph that follows the `## Rules` heading
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))?;
    let marker = "## Rules\n\n";
    let start = readme
        .find(marker)
        .ok_or("README has no `## Rules` section")?
        + marker.len();
    let rest = &readme[start..];
    let paragraph_end = rest
        .find("\n\n")
        .ok_or("the README rules paragraph never ends")?;
    let paragraph = &rest[..paragraph_end];
    let ids: Vec<&str> = paragraph.split('`').skip(1).step_by(2).collect();
    assert!(!ids.is_empty(), "no rule ids parsed from the README");

    // when: comparing the documented catalog against both support tables
    // then: every documented id is suppressible, and the table documents nothing extra
    for id in &ids {
        assert!(
            KNOWN_RULES.contains(id),
            "README rule `{id}` missing from KNOWN_RULES"
        );
    }
    for known in KNOWN_RULES {
        assert!(
            ids.contains(known),
            "KNOWN_RULES entry `{known}` missing from the README ## Rules list"
        );
    }
    // and: every documented id renders a specific reason, not the generic fallback
    for id in &ids {
        let findings = vec![Finding::new(
            *id,
            Confidence::High,
            "m",
            1,
            "tests/test_app.py",
            "",
        )];
        let xml = build_findings_xml(&findings, "tests/test_app.py");
        assert!(
            !xml.contains("reason=\"finding\""),
            "README rule `{id}` renders the generic reason"
        );
    }
    Ok(())
}

#[test]
fn format_hook_message_given_findings_contains_stop_instruction() {
    // given
    let findings = vec![finding("An assertion was removed.", 3, "assertion-removed")];

    // when
    let result = format_hook_message(&findings, "");

    // then
    assert!(result.contains("TEST-WEAKENING DETECTED"));
    assert!(result.contains("STOP: do not modify the test to make it pass."));
    assert!(result.contains("<findings file=\"tests/test_app.py\">"));
}

#[test]
fn format_hook_message_given_custom_prompt_replaces_placeholder() {
    // given
    let findings = vec![finding("A test skip marker was added.", 10, "skip-added")];
    let custom_prompt = "CUSTOM WARNING\n{{findings}}\nPlease fix.";

    // when
    let result = format_hook_message(&findings, custom_prompt);

    // then
    assert!(result.contains("CUSTOM WARNING"));
    assert!(result.contains("<findings file=\"tests/test_app.py\">"));
    assert!(result.contains("Please fix."));
    assert!(!result.contains("TEST-WEAKENING DETECTED"));
}

#[test]
fn format_hook_message_given_custom_prompt_without_placeholder_returns_custom_only() {
    // given
    let findings = vec![finding("A test skip marker was added.", 1, "skip-added")];

    // when
    let result = format_hook_message(&findings, "Simple warning.");

    // then
    assert_eq!(result, "Simple warning.");
}

#[test]
fn format_hook_message_given_empty_list_returns_empty_string() {
    // given
    let findings = Vec::new();

    // when
    let result = format_hook_message(&findings, "");

    // then
    assert_eq!(result, "");
}

#[test]
fn format_hook_message_given_agent_memo_finding_contains_memo_banner() {
    // given
    let findings = vec![finding(
        "it('now uses the new parser')",
        4,
        "agent-memo-test",
    )];

    // when
    let result = format_hook_message(&findings, "");

    // then
    assert!(result.contains("AGENT MEMO TEST DETECTED"));
    assert!(result.contains("CODE SMELL"));
    assert!(!result.contains("TEST-WEAKENING DETECTED - IMMEDIATE ACTION REQUIRED"));
}

#[test]
fn format_hook_message_given_mixed_findings_keeps_the_weakening_header() {
    // given
    let findings = vec![
        finding("A test skip marker was added.", 4, "skip-added"),
        finding("it('now uses the new parser')", 9, "agent-memo-test"),
    ];

    // when
    let result = format_hook_message(&findings, "");

    // then
    assert!(result.contains("TEST-WEAKENING DETECTED - IMMEDIATE ACTION REQUIRED"));
    assert!(result.contains("AGENT MEMO TEST DETECTED"));
}

#[test]
fn format_hook_message_given_multiple_findings_groups_into_one_block() {
    // given
    let findings = vec![
        finding("A test skip marker was added.", 5, "skip-added"),
        finding("An assertion was removed.", 15, "assertion-removed"),
    ];

    // when
    let result = format_hook_message(&findings, "");

    // then
    assert_eq!(
        result
            .matches("<findings file=\"tests/test_app.py\">")
            .count(),
        1
    );
    assert_eq!(result.matches("</findings>").count(), 1);
    assert!(result.contains("line-number=\"5\""));
    assert!(result.contains("line-number=\"15\""));
}
