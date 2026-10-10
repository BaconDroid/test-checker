use std::collections::HashMap;

use crate::models::Finding;

/// Imperative message emitted when no `--prompt` is given. The `{{findings}}`
/// placeholder receives the grouped XML blocks.
pub const DEFAULT_TEMPLATE: &str =
    "TEST QUALITY DETECTED:\n{{findings}}\n\nFix the cause, do not weaken the test.";

/// Renders one file's findings as a `<findings>` block.
#[must_use]
pub fn build_findings_xml(findings: &[Finding], file_path: &str) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let mut output = String::with_capacity(32 + file_path.len() + findings.len() * 64);
    output.push_str("<findings file=\"");
    output.push_str(file_path);
    output.push_str("\">\n");
    let mut line_number = itoa::Buffer::new();
    for finding in findings {
        output.push_str("<finding line-number=\"");
        output.push_str(line_number.format(finding.line));
        output.push_str("\" rule=\"");
        output.push_str(finding.rule);
        output.push_str("\" confidence=\"");
        output.push_str(finding.confidence.as_str());
        output.push_str("\">");
        push_escaped(&mut output, &finding.message);
        output.push_str("</finding>\n");
    }
    output.push_str("</findings>");
    output
}

/// Renders the stderr message for a non-empty finding set. A non-empty
/// `custom_prompt` replaces the default; its `{{findings}}` placeholder is
/// substituted with the grouped XML.
#[must_use]
pub fn format_hook_message(findings: &[Finding], custom_prompt: &str) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let blocks = findings_xml(findings);
    let template = if custom_prompt.is_empty() {
        DEFAULT_TEMPLATE
    } else {
        custom_prompt
    };
    template.replace("{{findings}}", &blocks)
}

fn findings_xml(findings: &[Finding]) -> String {
    let mut by_file: HashMap<&str, Vec<&Finding>> = HashMap::with_capacity(findings.len());
    let mut order: Vec<&str> = Vec::new();
    for finding in findings {
        let file = finding.file.as_str();
        match by_file.entry(file) {
            std::collections::hash_map::Entry::Occupied(mut entry) => entry.get_mut().push(finding),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(vec![finding]);
                order.push(file);
            }
        }
    }
    let mut blocks = Vec::with_capacity(order.len());
    for file in order {
        let file_findings = by_file
            .get(file)
            .map(|items| {
                items
                    .iter()
                    .map(|finding| (*finding).clone())
                    .collect::<Vec<Finding>>()
            })
            .unwrap_or_default();
        blocks.push(build_findings_xml(&file_findings, file));
    }
    blocks.join("\n")
}

fn push_escaped(output: &mut String, value: &str) {
    if !value.contains(['&', '<', '>']) {
        output.push_str(value);
        return;
    }
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(ch),
        }
    }
}
