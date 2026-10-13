use serde::{Deserialize, Serialize};

use crate::detector::TestDetector;
use crate::engine::{self, null_default};
use crate::filters::{SuppressionFilter, apply_filters, is_line_suppressed};
use crate::language::{is_harness_path, is_test_path, language_for_path};
use crate::models::{Confidence, Edit, Finding};
use crate::output::format_hook_message;
use crate::rules::{self, AddedLine};

const EXIT_PASS: i32 = 0;
const EXIT_BLOCK: i32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookCheckResult {
    pub exit_code: i32,
    pub message: String,
}

impl HookCheckResult {
    #[must_use]
    pub fn pass(message: impl Into<String>) -> Self {
        Self {
            exit_code: EXIT_PASS,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn block(message: impl Into<String>) -> Self {
        Self {
            exit_code: EXIT_BLOCK,
            message: message.into(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ToolInput {
    #[serde(default, alias = "filePath", deserialize_with = "null_default")]
    file_path: String,
    #[serde(default, deserialize_with = "null_default")]
    content: String,
    #[serde(default, alias = "newString", deserialize_with = "null_default")]
    new_string: String,
    #[serde(default, alias = "oldString", deserialize_with = "null_default")]
    old_string: String,
    #[serde(default, deserialize_with = "null_default")]
    edits: Vec<Edit>,
}

#[derive(Debug, Deserialize)]
struct HookInput {
    #[serde(default, alias = "toolName", deserialize_with = "null_default")]
    tool_name: String,
    #[serde(default, alias = "toolInput", deserialize_with = "null_default")]
    tool_input: ToolInput,
    #[serde(default, alias = "isTestFile")]
    is_test_file: Option<bool>,
}

#[must_use]
pub fn check_hook_input(input: &str, custom_prompt: &str) -> HookCheckResult {
    check_hook_input_with_options(input, custom_prompt, true, true)
}

/// Same as [`check_hook_input`], with the include toggles threaded through, mirroring
/// `comment-checker`'s `include_docstrings`.
#[must_use]
pub fn check_hook_input_with_options(
    input: &str,
    custom_prompt: &str,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> HookCheckResult {
    if input.is_empty() {
        return skip("No input provided");
    }

    serde_json::from_str::<HookInput>(input).map_or_else(
        |_| skip("Invalid input format"),
        |hook_input| {
            evaluate(
                &hook_input,
                custom_prompt,
                Confidence::Medium,
                include_commented_out_tests,
                include_body_tests,
            )
        },
    )
}

#[must_use]
pub fn check_hook_input_from_reader<R: std::io::Read>(
    reader: R,
    options: &crate::engine::Options,
) -> HookCheckResult {
    serde_json::from_reader::<R, HookInput>(reader).map_or_else(
        |_| skip("Invalid input format"),
        |hook_input| {
            evaluate(
                &hook_input,
                &options.prompt,
                parse_min_confidence(options.min_confidence.as_deref()),
                true,
                true,
            )
        },
    )
}

fn evaluate(
    hook_input: &HookInput,
    custom_prompt: &str,
    min_confidence: Confidence,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> HookCheckResult {
    if !matches!(
        hook_input.tool_name.as_str(),
        "Write" | "Edit" | "MultiEdit"
    ) {
        return skip("Unsupported tool");
    }
    let file_path = hook_input.tool_input.file_path.as_str();
    if file_path.is_empty() {
        return skip("No file path provided");
    }
    let is_test = hook_input
        .is_test_file
        .unwrap_or_else(|| is_test_path(file_path));
    if !is_test {
        if is_harness_path(file_path) {
            return harness_result(hook_input, file_path, custom_prompt, min_confidence);
        }
        if language_for_path(file_path).is_none() {
            return skip("Non-code file");
        }
        return skip("Non-test file");
    }
    if language_for_path(file_path).is_none() {
        return skip("Non-code file");
    }

    let marker_added = marker_added_in_change(hook_input);
    if !marker_added && file_suppressed(hook_input) {
        return skip("Suppressed by test-checker: allow");
    }

    let detector = TestDetector::new();
    let mut findings = match hook_input.tool_name.as_str() {
        "Edit" => {
            let old_string = hook_input.tool_input.old_string.as_str();
            let new_string = hook_input.tool_input.new_string.as_str();
            if new_string.is_empty() {
                if old_string.is_empty() {
                    return skip("No content to check");
                }
                // A deletion edit has no new content to parse, but it can still remove assertions.
                if include_body_tests {
                    rules::diff_findings(old_string, "", file_path)
                } else {
                    Vec::new()
                }
            } else {
                detector.detect_edit(
                    old_string,
                    new_string,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                )
            }
        }
        "MultiEdit" => {
            if hook_input.tool_input.edits.is_empty() {
                return skip("No content to check");
            }
            let mut findings = Vec::new();
            for edit in &hook_input.tool_input.edits {
                if edit.new_string.is_empty() {
                    // A deletion edit has no new content to parse, but it can still remove assertions.
                    if include_body_tests && !edit.old_string.is_empty() {
                        findings.extend(rules::diff_findings(&edit.old_string, "", file_path));
                    }
                    continue;
                }
                findings.extend(detector.detect_edit(
                    &edit.old_string,
                    &edit.new_string,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ));
            }
            findings
        }
        _ => {
            let content = get_content_to_check(hook_input);
            if content.is_empty() {
                return skip("No content to check");
            }
            detector.detect(
                content,
                file_path,
                include_commented_out_tests,
                include_body_tests,
            )
        }
    };

    if marker_added {
        findings.push(Finding::new(
            "allow-added",
            Confidence::Medium,
            "A test-checker opt-out marker was added in this change; suppressing findings you just introduced is suspicious.",
            1,
            file_path,
            "",
        ));
    }
    gate_and_format(&findings, custom_prompt, min_confidence)
}

/// Apply the filters and the confidence gate, then render: the shared tail of every check path.
fn gate_and_format(
    findings: &[Finding],
    custom_prompt: &str,
    min_confidence: Confidence,
) -> HookCheckResult {
    let filtered: Vec<Finding> = apply_filters(findings)
        .into_iter()
        .filter(|finding| confidence_rank(finding.confidence) >= confidence_rank(min_confidence))
        .collect();
    if filtered.is_empty() {
        return success();
    }
    HookCheckResult::block(format_hook_message(&filtered, custom_prompt))
}

/// Score the added lines of a harness/config file (no test-file gate, no tree-sitter needed).
fn harness_result(
    hook_input: &HookInput,
    file_path: &str,
    custom_prompt: &str,
    min_confidence: Confidence,
) -> HookCheckResult {
    let mut findings = Vec::new();
    for (line_number, text) in raw_added_lines(hook_input) {
        // A gate marker inside a comment (e.g. a workflow's `# … || true` note) is not a real
        // weakening; comment lines are invisible to the line rules by design.
        if rules::is_comment_line(&text) {
            continue;
        }
        let line = AddedLine {
            line_number,
            text: &text,
        };
        if let Some(finding) = rules::classify_harness_line(file_path, &line) {
            findings.push(finding);
        }
    }
    gate_and_format(&findings, custom_prompt, min_confidence)
}

/// Added lines (1-based line number, raw text) for the tool input.
fn raw_added_lines(hook_input: &HookInput) -> Vec<(usize, String)> {
    match hook_input.tool_name.as_str() {
        "Edit" => {
            let added = engine::added_lines(
                &hook_input.tool_input.old_string,
                &hook_input.tool_input.new_string,
            );
            hook_input
                .tool_input
                .new_string
                .lines()
                .enumerate()
                .filter(|(index, _)| added.contains(index))
                .map(|(index, line)| (index + 1, line.to_string()))
                .collect()
        }
        "MultiEdit" => {
            let mut lines = Vec::new();
            for edit in &hook_input.tool_input.edits {
                let added = engine::added_lines(&edit.old_string, &edit.new_string);
                for (index, line) in edit.new_string.lines().enumerate() {
                    if added.contains(&index) {
                        lines.push((index + 1, line.to_string()));
                    }
                }
            }
            lines
        }
        _ => get_content_to_check(hook_input)
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line.to_string()))
            .collect(),
    }
}

/// Minimum confidence from `--min-confidence` (default `medium`).
fn parse_min_confidence(value: Option<&str>) -> Confidence {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("low") => Confidence::Low,
        Some("high") => Confidence::High,
        _ => Confidence::Medium,
    }
}

const fn confidence_rank(confidence: Confidence) -> u8 {
    match confidence {
        Confidence::Low => 0,
        Confidence::Medium => 1,
        Confidence::High => 2,
    }
}

#[must_use]
pub fn check_hook_input_json(input: &str, custom_prompt: &str) -> String {
    engine::result_json(&check_hook_input(input, custom_prompt))
}

#[must_use]
pub fn detect_findings_json(
    content: &str,
    file_path: &str,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> String {
    if language_for_path(file_path).is_none() || !is_test_path(file_path) {
        return "[]".to_string();
    }
    let detector = TestDetector::new();
    let findings = detector.detect(
        content,
        file_path,
        include_commented_out_tests,
        include_body_tests,
    );
    match serde_json::to_string(&findings) {
        Ok(json) => json,
        Err(error) => engine::fallback_json(&error.to_string()),
    }
}

fn get_content_to_check(input: &HookInput) -> &str {
    if input.tool_input.content.is_empty() {
        input.tool_input.new_string.as_str()
    } else {
        input.tool_input.content.as_str()
    }
}

/// Whether the suppression marker sits on a line the change added (agent self-suppression).
fn marker_added_in_change(hook_input: &HookInput) -> bool {
    match hook_input.tool_name.as_str() {
        "Edit" => added_line_has_marker(
            &hook_input.tool_input.old_string,
            &hook_input.tool_input.new_string,
        ),
        "MultiEdit" => hook_input
            .tool_input
            .edits
            .iter()
            .any(|edit| added_line_has_marker(&edit.old_string, &edit.new_string)),
        _ => SuppressionFilter.is_file_suppressed(get_content_to_check(hook_input)),
    }
}

/// Whether the suppression marker is present anywhere in the analyzed text.
fn file_suppressed(hook_input: &HookInput) -> bool {
    let suppression = SuppressionFilter;
    match hook_input.tool_name.as_str() {
        "Edit" => suppression.is_file_suppressed(&hook_input.tool_input.new_string),
        "MultiEdit" => hook_input
            .tool_input
            .edits
            .iter()
            .any(|edit| suppression.is_file_suppressed(&edit.new_string)),
        _ => suppression.is_file_suppressed(get_content_to_check(hook_input)),
    }
}

/// Whether the suppression marker sits on a line the edit added (agent self-suppression).
fn added_line_has_marker(old_content: &str, new_content: &str) -> bool {
    let added = engine::added_lines(old_content, new_content);
    new_content
        .lines()
        .enumerate()
        .any(|(index, line)| added.contains(&index) && is_line_suppressed(line))
}

fn skip(reason: &str) -> HookCheckResult {
    HookCheckResult::pass(format!("[test-checker] Skipping: {reason}\n"))
}

fn success() -> HookCheckResult {
    HookCheckResult::pass("[test-checker] Success: No problematic tests found\n")
}
