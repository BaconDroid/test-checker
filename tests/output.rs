use test_checker::models::{Confidence, Finding};
use test_checker::output::{build_findings_xml, format_hook_message};

#[test]
fn build_findings_xml_given_single_finding_returns_exact_shape() {
    // given
    let findings = [Finding::new(
        "src/app.py",
        10,
        "tautological-assertion",
        "Assertion is tautological (always true).",
        Confidence::High,
    )];

    // when
    let xml = build_findings_xml(&findings, "src/app.py");

    // then
    assert_eq!(
        xml,
        "<findings file=\"src/app.py\">\n\
         <finding line-number=\"10\" rule=\"tautological-assertion\" \
         confidence=\"high\">Assertion is tautological (always true).</finding>\n\
         </findings>"
    );
}

#[test]
fn build_findings_xml_given_special_characters_escapes_message() {
    // given
    let findings = [Finding::new(
        "src/app.py",
        3,
        "empty-test",
        "a & b < c > d",
        Confidence::Medium,
    )];

    // when
    let xml = build_findings_xml(&findings, "src/app.py");

    // then
    assert!(xml.contains("a &amp; b &lt; c &gt; d"));
    assert!(!xml.contains("a & b < c > d"));
}

#[test]
fn build_findings_xml_given_empty_findings_returns_empty_string() {
    // given / when
    let xml = build_findings_xml(&[], "src/app.py");

    // then
    assert_eq!(xml, "");
}

#[test]
fn format_hook_message_given_default_template_contains_warning_and_findings() {
    // given
    let findings = [Finding::new(
        "src/app.py",
        10,
        "tautological-assertion",
        "Assertion is tautological (always true).",
        Confidence::High,
    )];

    // when
    let message = format_hook_message(&findings, "");

    // then
    assert!(message.contains("TEST QUALITY DETECTED"));
    assert!(!message.contains("{{findings}}"));
    assert!(message.contains("<findings file=\"src/app.py\">"));
    assert!(message.contains("line-number=\"10\""));
    assert!(message.contains("rule=\"tautological-assertion\""));
    assert!(message.contains("confidence=\"high\""));
}

#[test]
fn format_hook_message_given_custom_prompt_replaces_placeholder() {
    // given
    let findings = [Finding::new(
        "src/app.py",
        10,
        "tautological-assertion",
        "Assertion is tautological (always true).",
        Confidence::High,
    )];
    let custom_prompt = "CUSTOM WARNING\n{{findings}}\nPlease fix.";

    // when
    let message = format_hook_message(&findings, custom_prompt);

    // then
    assert!(message.contains("CUSTOM WARNING"));
    assert!(message.contains("<findings file=\"src/app.py\">"));
    assert!(message.contains("Please fix."));
    assert!(!message.contains("{{findings}}"));
    assert!(!message.contains("TEST QUALITY DETECTED"));
}

#[test]
fn format_hook_message_given_custom_prompt_without_placeholder_returns_prompt() {
    // given
    let findings = [Finding::new(
        "src/app.py",
        1,
        "empty-test",
        "no body",
        Confidence::Low,
    )];

    // when
    let message = format_hook_message(&findings, "Simple warning without placeholder.");

    // then
    assert_eq!(message, "Simple warning without placeholder.");
}

#[test]
fn format_hook_message_given_multiple_files_groups_one_block_each_in_first_seen_order() {
    // given
    let findings = [
        Finding::new(
            "src/a.ts",
            1,
            "tautological-assertion",
            "first",
            Confidence::High,
        ),
        Finding::new("src/b.ts", 2, "empty-test", "second", Confidence::Medium),
        Finding::new("src/a.ts", 3, "unknown-test", "third", Confidence::Low),
    ];

    // when
    let message = format_hook_message(&findings, "");

    // then
    assert_eq!(message.matches("<findings file=\"src/a.ts\">").count(), 1);
    assert_eq!(message.matches("<findings file=\"src/b.ts\">").count(), 1);
    assert_eq!(message.matches("</findings>").count(), 2);
    assert!(message.contains("line-number=\"1\""));
    assert!(message.contains("line-number=\"3\""));
    let first = message.find("<findings file=\"src/a.ts\">").unwrap();
    let second = message.find("<findings file=\"src/b.ts\">").unwrap();
    assert!(first < second);
}

#[test]
fn format_hook_message_given_empty_findings_returns_empty_string() {
    // given / when
    let message = format_hook_message(&[], "");

    // then
    assert_eq!(message, "");
}
