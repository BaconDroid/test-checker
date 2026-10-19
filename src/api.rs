use serde::{Deserialize, Serialize};

use crate::detector::{DetectError, DetectResult, TestDetector};
use crate::engine::{self, null_default};
use crate::filters::{SuppressionFilter, apply_filters, is_line_suppressed};
use crate::language::{is_harness_path, is_test_path, language_for_path};
use crate::models::{Confidence, Edit, Finding};
use crate::output::format_hook_message;
use crate::rules::{self, AddedLine};

const EXIT_PASS: i32 = 0;
const EXIT_INTERNAL_FAILURE: i32 = 1;
const EXIT_BLOCK: i32 = 2;

/// Content above this size is skipped rather than parsed, so a huge write cannot hang the hook.
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;

/// The hook tool that produced a change. Claude Code sends `Write`/`Edit`/`MultiEdit`; the
/// `OpenCode` plugin normalizes to the same names but also forwards `apply_patch`, so matching is
/// case-insensitive and `apply_patch` behaves like an edit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolKind {
    Write,
    Edit,
    MultiEdit,
}

fn classify_tool(name: &str) -> Option<ToolKind> {
    if name.eq_ignore_ascii_case("write") {
        Some(ToolKind::Write)
    } else if name.eq_ignore_ascii_case("edit") || name.eq_ignore_ascii_case("apply_patch") {
        Some(ToolKind::Edit)
    } else if name.eq_ignore_ascii_case("multiedit") {
        Some(ToolKind::MultiEdit)
    } else {
        None
    }
}

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

/// The explicit outcome of one hook check, before it becomes a [`HookCheckResult`].
///
/// `Clean` and `Skipped` exit 0, `Findings` exits 2, and `Failed` exits 1: a check that could not
/// run is never reported as a pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HookOutcome {
    /// The check ran and found nothing.
    Clean,
    /// The check ran and found weakening; the payload is the rendered block message.
    Findings(String),
    /// The check did not apply to this write (unsupported language, non-edit tool, bad input).
    Skipped(String),
    /// The check could not run at all; `reason` explains what broke.
    Failed(String),
}

impl HookOutcome {
    /// The process exit code this outcome maps to.
    #[must_use]
    pub(crate) const fn exit_code(&self) -> i32 {
        match self {
            Self::Clean | Self::Skipped(_) => EXIT_PASS,
            Self::Findings(_) => EXIT_BLOCK,
            Self::Failed(_) => EXIT_INTERNAL_FAILURE,
        }
    }

    /// The hook's stderr message, including its trailing newline.
    #[must_use]
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Clean => "[test-checker] Success: No problematic tests found\n".to_owned(),
            Self::Findings(message) => message.clone(),
            Self::Skipped(reason) => format!("[test-checker] Skipping: {reason}\n"),
            Self::Failed(reason) => {
                format!("[test-checker] did not run — nothing checked this write.\n{reason}\n")
            }
        }
    }

    /// The wire result `src/cli.rs` reads.
    #[must_use]
    pub(crate) fn into_result(self) -> HookCheckResult {
        HookCheckResult {
            exit_code: self.exit_code(),
            message: self.message(),
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
    #[serde(default, deserialize_with = "null_default")]
    cwd: String,
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
        return skip("No input provided").into_result();
    }

    serde_json::from_str::<HookInput>(input).map_or_else(
        |_| skip("Invalid input format").into_result(),
        |hook_input| {
            evaluate(
                &hook_input,
                custom_prompt,
                Confidence::Medium,
                include_commented_out_tests,
                include_body_tests,
            )
            .into_result()
        },
    )
}

#[must_use]
pub fn check_hook_input_from_reader<R: std::io::Read>(
    reader: R,
    options: &crate::engine::Options,
) -> HookCheckResult {
    serde_json::from_reader::<R, HookInput>(reader).map_or_else(
        |_| skip("Invalid input format").into_result(),
        |hook_input| {
            evaluate(
                &hook_input,
                &options.prompt,
                parse_min_confidence(options.min_confidence.as_deref()),
                true,
                true,
            )
            .into_result()
        },
    )
}

fn evaluate(
    hook_input: &HookInput,
    custom_prompt: &str,
    min_confidence: Confidence,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> HookOutcome {
    let Some(tool) = classify_tool(&hook_input.tool_name) else {
        return skip("Unsupported tool");
    };
    let file_path = hook_input.tool_input.file_path.as_str();
    if file_path.is_empty() {
        return skip("No file path provided");
    }
    let is_test = hook_input
        .is_test_file
        .unwrap_or_else(|| is_test_path(file_path));
    if !is_test {
        if is_harness_path(file_path) {
            return harness_result(tool, hook_input, file_path, custom_prompt, min_confidence);
        }
        if language_for_path(file_path).is_none() {
            return skip("Non-code file");
        }
        return skip("Non-test file");
    }
    if language_for_path(file_path).is_none() {
        return skip("Non-code file");
    }
    if exceeds_content_limit(tool, hook_input) {
        return skip("File too large");
    }

    let marker_added = marker_added_in_change(tool, hook_input);
    if !marker_added && file_suppressed(tool, hook_input) {
        return skip("Suppressed by test-checker: allow");
    }

    let detector = TestDetector::new();
    let mut baseline_note: Option<&'static str> = None;
    let detection: DetectResult = match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => {
            let old_string = hook_input.tool_input.old_string.as_str();
            let new_string = hook_input.tool_input.new_string.as_str();
            if new_string.is_empty() {
                if old_string.is_empty() {
                    return skip("No content to check");
                }
                // A deletion edit has no new content to parse, but it can still remove assertions.
                Ok(if include_body_tests {
                    rules::diff_findings(old_string, "", file_path)
                } else {
                    Vec::new()
                })
            } else {
                detector.detect_edit_checked(
                    old_string,
                    new_string,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                )
            }
        }
        ToolKind::Edit | ToolKind::MultiEdit => {
            if hook_input.tool_input.edits.is_empty() {
                return skip("No content to check");
            }
            edits_findings(
                &detector,
                &hook_input.tool_input.edits,
                file_path,
                include_commented_out_tests,
                include_body_tests,
            )
        }
        ToolKind::Write => {
            let content = get_content_to_check(hook_input);
            if content.is_empty() {
                return skip("No content to check");
            }
            match head_content(&hook_input.cwd, file_path) {
                Baseline::Head(head) => detector.detect_edit_checked(
                    &head,
                    content,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ),
                Baseline::Skipped(reason) => {
                    baseline_note = Some(reason);
                    detector.detect_checked(
                        content,
                        file_path,
                        include_commented_out_tests,
                        include_body_tests,
                    )
                }
                Baseline::NotRequested => detector.detect_checked(
                    content,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ),
            }
        }
    };
    let mut findings = match detection {
        Ok(findings) => findings,
        Err(DetectError::BudgetExceeded) => return skip("Parse budget exceeded"),
        Err(DetectError::Internal(reason)) => return HookOutcome::Failed(reason),
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

    let mut result = gate_and_format(&findings, custom_prompt, min_confidence);
    if let Some(reason) = baseline_note {
        if result.exit_code == EXIT_PASS {
            return skip(reason);
        }
        result.message = format!("[test-checker] Skipping: {reason}\n{}", result.message);
    }
    result
}

/// The result of looking up a `Write`'s committed baseline.
enum Baseline {
    Head(String),
    Skipped(&'static str),
    NotRequested,
}

/// The committed content of `file_path` (`git show HEAD:<path>`), bounded by a 150 ms timeout.
///
/// A `Write` carries no old text; HEAD gives the diff rules something to compare. A payload with no
/// `cwd` did not ask for a baseline (behaviour unchanged). When a `cwd` is present but git is
/// unavailable, the path is unsafe, the file is untracked, or the lookup times out, the caller skips
/// the diff rules and shows a visible `Skipping:` line instead of failing silently.
fn head_content(cwd: &str, file_path: &str) -> Baseline {
    if cwd.is_empty() {
        return Baseline::NotRequested;
    }
    let path = std::path::Path::new(file_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Baseline::Skipped("refusing an unsafe path for the git baseline");
    }
    let Ok(mut child) = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .arg("show")
        .arg(format!("HEAD:{file_path}"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return Baseline::Skipped("git is unavailable");
    };
    let Some(mut stdout) = child.stdout.take() else {
        return Baseline::Skipped("git produced no stdout");
    };
    let reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buffer);
        buffer
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Baseline::Skipped("git timed out");
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(_) => {
                let _ = reader.join();
                return Baseline::Skipped("git failed");
            }
        }
    }
    let Ok(status) = child.wait() else {
        return Baseline::Skipped("git failed");
    };
    let Ok(buffer) = reader.join() else {
        return Baseline::Skipped("git reader failed");
    };
    if !status.success() {
        return Baseline::Skipped("file is not committed in HEAD");
    }
    String::from_utf8(buffer).map_or(
        Baseline::Skipped("HEAD content is not UTF-8"),
        Baseline::Head,
    )
}

/// Whether any content this call would inspect is over [`MAX_CONTENT_BYTES`].
fn exceeds_content_limit(tool: ToolKind, hook_input: &HookInput) -> bool {
    match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => {
            hook_input.tool_input.new_string.len() > MAX_CONTENT_BYTES
        }
        ToolKind::Edit | ToolKind::MultiEdit => hook_input
            .tool_input
            .edits
            .iter()
            .any(|edit| edit.new_string.len() > MAX_CONTENT_BYTES),
        ToolKind::Write => get_content_to_check(hook_input).len() > MAX_CONTENT_BYTES,
    }
}

/// Findings for a list of discrete edits: a `MultiEdit`, or an `Edit`/`apply_patch` that attached an
/// `edits[]` array (the `OpenCode` plugin's shape). A deletion edit has no content to parse, but it can
/// still remove assertions.
fn edits_findings(
    detector: &TestDetector,
    edits: &[Edit],
    file_path: &str,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> DetectResult {
    let mut findings = Vec::new();
    for edit in edits {
        if edit.new_string.is_empty() {
            if include_body_tests && !edit.old_string.is_empty() {
                findings.extend(rules::diff_findings(&edit.old_string, "", file_path));
            }
            continue;
        }
        findings.extend(detector.detect_edit_checked(
            &edit.old_string,
            &edit.new_string,
            file_path,
            include_commented_out_tests,
            include_body_tests,
        )?);
    }
    Ok(findings)
}

/// Apply the filters and the confidence gate, then render: the shared tail of every check path.
fn gate_and_format(
    findings: &[Finding],
    custom_prompt: &str,
    min_confidence: Confidence,
) -> HookOutcome {
    let filtered: Vec<Finding> = apply_filters(findings)
        .into_iter()
        .filter(|finding| confidence_rank(finding.confidence) >= confidence_rank(min_confidence))
        .collect();
    if filtered.is_empty() {
        return HookOutcome::Clean;
    }
    HookOutcome::Findings(format_hook_message(&filtered, custom_prompt))
}

/// Score the added lines of a harness/config file (no test-file gate, no tree-sitter needed).
fn harness_result(
    tool: ToolKind,
    hook_input: &HookInput,
    file_path: &str,
    custom_prompt: &str,
    min_confidence: Confidence,
) -> HookOutcome {
    let added = raw_added_lines(tool, hook_input);
    let mut findings = Vec::new();
    // Any added non-blank line of a harness/config file is itself worth reviewing.
    if let Some((line_number, text)) = added.iter().find(|(_, text)| !text.trim().is_empty()) {
        findings.push(rules::harness_edited_finding(
            file_path,
            &AddedLine {
                line_number: *line_number,
                text: text.as_str(),
            },
        ));
    }
    for (line_number, text) in &added {
        // A gate marker inside a comment (e.g. a workflow's `# … || true` note) is not a real
        // weakening; comment lines are invisible to the line rules by design.
        if rules::is_comment_line(text) {
            continue;
        }
        let line = AddedLine {
            line_number: *line_number,
            text: text.as_str(),
        };
        if let Some(finding) = rules::classify_harness_line(file_path, &line) {
            findings.push(finding);
        }
    }
    gate_and_format(&findings, custom_prompt, min_confidence)
}

/// Added lines (1-based line number, raw text) for the tool input.
fn raw_added_lines(tool: ToolKind, hook_input: &HookInput) -> Vec<(usize, String)> {
    match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => {
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
        ToolKind::Edit | ToolKind::MultiEdit => {
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
        ToolKind::Write => get_content_to_check(hook_input)
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
    let findings = match detector.detect_checked(
        content,
        file_path,
        include_commented_out_tests,
        include_body_tests,
    ) {
        Ok(findings) => findings,
        Err(error) => {
            return engine::result_json(&HookOutcome::Failed(error.to_string()).into_result());
        }
    };
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
fn marker_added_in_change(tool: ToolKind, hook_input: &HookInput) -> bool {
    match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => added_line_has_marker(
            &hook_input.tool_input.old_string,
            &hook_input.tool_input.new_string,
        ),
        ToolKind::Edit | ToolKind::MultiEdit => hook_input
            .tool_input
            .edits
            .iter()
            .any(|edit| added_line_has_marker(&edit.old_string, &edit.new_string)),
        ToolKind::Write => SuppressionFilter.is_file_suppressed(get_content_to_check(hook_input)),
    }
}

/// Whether the suppression marker is present anywhere in the analyzed text.
fn file_suppressed(tool: ToolKind, hook_input: &HookInput) -> bool {
    let suppression = SuppressionFilter;
    match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => {
            suppression.is_file_suppressed(&hook_input.tool_input.new_string)
        }
        ToolKind::Edit | ToolKind::MultiEdit => hook_input
            .tool_input
            .edits
            .iter()
            .any(|edit| suppression.is_file_suppressed(&edit.new_string)),
        ToolKind::Write => suppression.is_file_suppressed(get_content_to_check(hook_input)),
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

fn skip(reason: &str) -> HookOutcome {
    HookOutcome::Skipped(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::check_hook_input;
    use crate::detector::{FORCE_BUDGET_EXCEEDED, FORCE_INTERNAL_FAILURE};

    /// The seams are process-wide, so the tests that flip them must not run concurrently: one test
    /// enabling a seam would otherwise force the other test's check down that same path.
    static SEAM_LOCK: Mutex<()> = Mutex::new(());

    fn seam_guard() -> MutexGuard<'static, ()> {
        SEAM_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Clears the seam on drop so a failed assertion cannot leak the forced failure into another
    /// unit test in the same process.
    struct ResetFailure;

    impl Drop for ResetFailure {
        fn drop(&mut self) {
            FORCE_INTERNAL_FAILURE.store(false, Ordering::SeqCst);
        }
    }

    /// Clears the parse-budget seam on drop, for the same reason.
    struct ResetBudget;

    impl Drop for ResetBudget {
        fn drop(&mut self) {
            FORCE_BUDGET_EXCEEDED.store(false, Ordering::SeqCst);
        }
    }

    #[test]
    fn check_hook_input_given_internal_failure_exits_one_with_the_did_not_run_message() {
        // given: the runtime seam makes the detector report an internal failure
        let _seam = seam_guard();
        let _reset = ResetFailure;
        FORCE_INTERNAL_FAILURE.store(true, Ordering::SeqCst);
        let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    assert add(1, 2) == 3\n"}}"#;

        // when
        let result = check_hook_input(input, "");

        // then
        assert_eq!(result.exit_code, 1);
        assert_eq!(
            result.message,
            "[test-checker] did not run — nothing checked this write.\nforced internal failure\n"
        );
    }

    #[test]
    fn check_hook_input_given_budget_exceeded_skips_without_the_did_not_run_message() {
        // given: the parse seam makes the detector report a budget abort on a valid test payload
        let _seam = seam_guard();
        let _reset = ResetBudget;
        FORCE_BUDGET_EXCEEDED.store(true, Ordering::SeqCst);
        let input = r#"{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    assert add(1, 2) == 3\n"}}"#;

        // when
        let result = check_hook_input(input, "");

        // then: a budget abort is a visible skip, not a failure
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.message,
            "[test-checker] Skipping: Parse budget exceeded\n"
        );
        assert!(!result.message.contains("did not run"));
    }
}
