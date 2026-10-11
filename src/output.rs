use crate::models::Finding;

#[must_use]
pub fn build_findings_xml(findings: &[Finding], file_path: &str) -> String {
    if findings.is_empty() {
        return String::new();
    }

    let mut output = String::with_capacity(24 + file_path.len() + findings.len() * 96);
    push_findings_xml_header(&mut output, file_path);
    for finding in findings {
        push_finding_xml(&mut output, finding);
    }
    output.push_str("</findings>");
    output
}

fn push_escaped_text(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}

fn push_escaped_attribute(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            _ => output.push(character),
        }
    }
}

fn push_findings_xml_header(output: &mut String, file_path: &str) {
    output.push_str("<findings file=\"");
    push_escaped_attribute(output, file_path);
    output.push_str("\">\n");
}

fn push_finding_xml(output: &mut String, finding: &Finding) {
    output.push_str("\t<finding line-number=\"");
    output.push_str(&finding.line_number.to_string());
    output.push_str("\" rule=\"");
    push_escaped_attribute(output, &finding.rule);
    output.push_str("\" confidence=\"");
    output.push_str(finding.confidence.as_str());
    output.push_str("\">");
    push_escaped_text(output, &finding.message);
    output.push_str("</finding>\n");
}

#[must_use]
pub fn format_hook_message(findings: &[Finding], custom_prompt: &str) -> String {
    if findings.is_empty() {
        return String::new();
    }

    let file_path = findings
        .first()
        .map_or("", |finding| finding.file_path.as_str());
    let mut findings_xml = build_findings_xml(findings, file_path);
    findings_xml.push('\n');

    if !custom_prompt.is_empty() {
        return custom_prompt.replace("{{findings}}", &findings_xml);
    }

    let mut output = String::new();
    output.push_str("TEST-WEAKENING DETECTED - IMMEDIATE ACTION REQUIRED\n\n");
    output.push_str("Your recent change weakens a test. This triggered the hook and requires immediate action.\n");
    output.push_str("STOP: do not modify the test to make it pass. If the test is wrong, stop and tell the user instead of weakening it.\n\n");
    output.push_str("CRITICAL WARNING: This hook message MUST NEVER be ignored, even if you receive it multiple times.\n");
    output.push_str(
        "You MUST provide a corresponding explanation or action for EACH reported finding.\n\n",
    );
    output.push_str("PRIORITY-BASED ACTION GUIDELINES:\n\n");
    output.push_str(
        "1. The finding is a false positive (the flagged line is not a real weakening).\n",
    );
    output.push_str(
        "\t-> Explain to the user why it is a false positive and proceed (justify it)\n\n",
    );
    output.push_str("2. The test genuinely needs to change because the behavior under test changed intentionally.\n");
    output.push_str("\t-> Explain the intended behavior change and update the test honestly, do not delete assertions\n\n");
    output.push_str("3. The test is failing for a real reason in the code under test.\n");
    output.push_str("\t-> STOP. Do not weaken, skip, or delete the test. Fix the code under test, or tell the user the test is wrong\n\n");
    output.push_str("4. The change disables or hides a test (skip/ignore/focus/commented-out).\n");
    output.push_str("\t-> Remove the suppression and fix the underlying failure, or justify it with a tracked reason\n\n");
    output.push_str("MANDATORY REQUIREMENT: You must acknowledge this hook message and take one of the above actions.\n");
    output.push_str("REMINDER: These rules apply to ALL your future test edits, not just this specific edit.\n\n");
    output.push_str("Detected findings:\n");
    output.push_str(&findings_xml);
    output
}

#[cfg(test)]
mod tests {
    use super::{build_findings_xml, format_hook_message};
    use crate::models::{Confidence, Finding};

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
            "<findings file=\"tests/test_app.py\">\n\t<finding line-number=\"10\" rule=\"skip-added\" confidence=\"high\">A test skip marker was added.</finding>\n</findings>",
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
            "<findings file=\"tests/a&amp;b.py\">\n\t<finding line-number=\"1\" rule=\"tautological-assertion\" confidence=\"high\">assert 1 &lt; 2 &amp;&amp; 3 &gt; 2</finding>\n</findings>",
        );
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
    fn format_hook_message_given_findings_contains_stop_instruction() {
        // given
        let findings = vec![finding("An assertion was removed.", 3, "assertion-removed")];

        // when
        let result = format_hook_message(&findings, "");

        // then
        assert!(result.contains("TEST-WEAKENING DETECTED"));
        assert!(result.contains("STOP: do not modify the test to make it pass."));
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
}
