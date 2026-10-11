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

    let agent_memo: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.rule == "agent-memo-test")
        .collect();

    let mut output = String::new();
    if findings
        .iter()
        .any(|finding| finding.rule != "agent-memo-test")
    {
        output.push_str("TEST-WEAKENING DETECTED - IMMEDIATE ACTION REQUIRED\n\n");
    }
    if !agent_memo.is_empty() {
        output.push_str("🚨 AGENT MEMO TEST DETECTED - CODE SMELL ALERT 🚨\n\n");
        output.push_str("⚠️  A test name or assertion message describes WHAT changed instead of WHAT the behavior is.  ⚠️\n\n");
        output.push_str("Examples of agent memo patterns detected:\n");
        output.push_str("  - \"Changed from X to Y\", \"Updated from...\", \"Migrated to...\"\n");
        output.push_str("  - \"Now this uses...\", \"Previously...\", \"Note:\"\n");
        output.push_str("  - \"This implements/adds/removes/changes/fixes...\"\n");
        output.push_str("  - \"oldValue -> newValue\"\n");
        output.push_str("WHY THIS IS BAD:\n");
        output.push_str("  1. A test name should describe the behavior it verifies, not the edit that produced it\n");
        output
            .push_str("  2. These names become outdated immediately and mislead future readers\n");
        output.push_str("  3. Git history already tracks what changed\n\n");
        output.push_str("ACTION REQUIRED:\n");
        output.push_str("  -> RENAME the test (or its assertion message) to describe the behavior under test\n\n");
        output.push_str("Detected agent memo findings:\n");
        for memo in agent_memo {
            output.push_str("  - Line ");
            output.push_str(&memo.line_number.to_string());
            output.push_str(": ");
            output.push_str(memo.text.trim());
            output.push('\n');
        }
        output.push_str("\n---\n\n");
    }
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
