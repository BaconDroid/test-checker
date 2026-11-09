use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detector::{DetectError, DetectResult, TestDetector, TestSpan, comment_ranges};
use crate::engine::{self, null_default};
use crate::filters::{
    SuppressionConfig, SuppressionIndex, apply_filters_without_suppression, has_marker_in_comments,
    is_config_path,
};
use crate::language::{is_harness_path, is_test_path, language_for_path};
use crate::models::{Confidence, Edit, Finding};
use crate::output::format_hook_message;
use crate::rules::{self, AddedLine};

const EXIT_PASS: i32 = 0;
const EXIT_INTERNAL_FAILURE: i32 = 1;
const EXIT_BLOCK: i32 = 2;

/// Content above this size is skipped rather than parsed, so a huge write cannot hang the hook.
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;

/// File-level superfluity rules a cleanup pass may remove once a human approves them.
///
/// Keep in sync with `CANDIDATE_RULES` in `scripts/cleanup-candidates`; the diff-based integrity
/// rules are never cleanup candidates.
const REMOVABLE_RULES: &[&str] = &[
    "duplicate-test",
    "near-duplicate-test",
    "tautological-assertion",
    "no-assertion",
    "source-grep-test",
    "gutted-test",
    "echo-test",
    "mirror-implementation-test",
];

fn is_removable_rule(rule: &str) -> bool {
    REMOVABLE_RULES.contains(&rule)
}

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
    /// The check ran and found nothing, but the cleanup pass has context to report.
    CleanWith(String),
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
            Self::Clean | Self::CleanWith(_) | Self::Skipped(_) => EXIT_PASS,
            Self::Findings(_) => EXIT_BLOCK,
            Self::Failed(_) => EXIT_INTERNAL_FAILURE,
        }
    }

    /// The hook's stderr message, including its trailing newline.
    #[must_use]
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Clean => "[test-checker] Success: No problematic tests found\n".to_owned(),
            Self::CleanWith(message) | Self::Findings(message) => message.clone(),
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
                false,
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
                options.strip,
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
    strip: bool,
) -> HookOutcome {
    let Some(tool) = classify_tool(&hook_input.tool_name) else {
        return skip("Unsupported tool");
    };
    let file_path = hook_input.tool_input.file_path.as_str();
    if file_path.is_empty() {
        return skip("No file path provided");
    }
    if is_config_path(file_path) {
        let findings = [Finding::new(
            "config-edited",
            Confidence::High,
            "The test-checker suppression config was edited; keep every suppression narrow and justified.",
            1,
            file_path,
            "",
        )];
        return gate_and_format(&findings, custom_prompt, min_confidence);
    }
    // A harness path is recognised by its path and is checked without language resolution, so it
    // wins over the test gate. Otherwise a harness file that also matches a test path (for example
    // `tests/pytest.ini` or `tests/conftest.py`) would be routed to the language gate, where its
    // non-code extension is not a language this binary parses.
    if is_harness_path(file_path) {
        return harness_result(tool, hook_input, file_path, custom_prompt, min_confidence);
    }
    let is_test = hook_input
        .is_test_file
        .unwrap_or_else(|| is_test_path(file_path));
    if !is_test {
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

    let cwd = Some(hook_input.cwd.as_str()).filter(|value| !value.is_empty());
    let suppression_config = SuppressionConfig::load(cwd);
    let detector = TestDetector::new();
    if strip {
        return evaluate_strip(
            tool,
            hook_input,
            &detector,
            file_path,
            cwd,
            &suppression_config,
            custom_prompt,
            min_confidence,
            include_commented_out_tests,
            include_body_tests,
        );
    }
    let marker_added = marker_added_in_change(tool, hook_input, file_path);
    let ChangeFindings {
        mut findings,
        invalid_markers,
        baseline_note,
    } = match analyze_change(
        tool,
        hook_input,
        &detector,
        file_path,
        include_commented_out_tests,
        include_body_tests,
    ) {
        Ok(change) => change,
        Err(skip_result) => return skip_result,
    };

    findings.retain(|finding| !suppression_config.suppresses(file_path, &finding.rule));
    findings.extend(invalid_markers);
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

    apply_baseline_note(
        gate_and_format(&findings, custom_prompt, min_confidence),
        baseline_note,
    )
}

/// Resolve the cleanup target against the payload `cwd`, falling back to the process cwd.
fn resolve_target_path(file_path: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(file_path);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    let base = cwd
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    Some(base.join(path))
}

fn read_target(path: &Path) -> Option<String> {
    if path.is_file() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

/// Run the cleanup pass: analyze the file on disk as a whole, then remove only the candidates a
/// committed `.test-checker.json` `remove` entry approves.
#[allow(clippy::too_many_arguments)]
fn evaluate_strip(
    tool: ToolKind,
    hook_input: &HookInput,
    detector: &TestDetector,
    file_path: &str,
    cwd: Option<&str>,
    suppression_config: &SuppressionConfig,
    custom_prompt: &str,
    min_confidence: Confidence,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> HookOutcome {
    let resolved = resolve_target_path(file_path, cwd);
    let disk_content = resolved.as_deref().and_then(read_target);
    if let Some(content) = disk_content.as_deref() {
        if content.len() > MAX_CONTENT_BYTES {
            return skip("File too large");
        }
        let match_path = resolved.as_deref().map_or_else(
            || file_path.to_owned(),
            |path| path.to_string_lossy().into_owned(),
        );
        let findings = match analyze_whole_file(
            content,
            file_path,
            &match_path,
            detector,
            suppression_config,
            include_commented_out_tests,
            include_body_tests,
        ) {
            Ok(findings) => findings,
            Err(outcome) => return outcome,
        };
        return strip_report(
            &findings,
            Some(content),
            resolved.as_deref(),
            file_path,
            &match_path,
            detector,
            suppression_config,
            custom_prompt,
            min_confidence,
        );
    }

    if tool == ToolKind::Write {
        let content = get_content_to_check(hook_input);
        if content.len() > MAX_CONTENT_BYTES {
            return skip("File too large");
        }
        let findings = match analyze_whole_file(
            content,
            file_path,
            file_path,
            detector,
            suppression_config,
            include_commented_out_tests,
            include_body_tests,
        ) {
            Ok(findings) => findings,
            Err(outcome) => return outcome,
        };
        return strip_report(
            &findings,
            Some(content),
            None,
            file_path,
            file_path,
            detector,
            suppression_config,
            custom_prompt,
            min_confidence,
        );
    }

    // A fragment payload names a file that is not on disk: analyze the fragment, never rewrite.
    let marker_added = marker_added_in_change(tool, hook_input, file_path);
    let ChangeFindings {
        mut findings,
        invalid_markers,
        ..
    } = match analyze_change(
        tool,
        hook_input,
        detector,
        file_path,
        include_commented_out_tests,
        include_body_tests,
    ) {
        Ok(change) => change,
        Err(skip_result) => return skip_result,
    };
    findings.retain(|finding| !suppression_config.suppresses(file_path, &finding.rule));
    findings.extend(invalid_markers);
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
    strip_report(
        &findings,
        None,
        None,
        file_path,
        file_path,
        detector,
        suppression_config,
        custom_prompt,
        min_confidence,
    )
}

/// Whole-file analysis for the cleanup pass: every line counts as added and markers are inert.
#[allow(clippy::too_many_arguments)]
fn analyze_whole_file(
    content: &str,
    file_path: &str,
    match_path: &str,
    detector: &TestDetector,
    suppression_config: &SuppressionConfig,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> Result<Vec<Finding>, HookOutcome> {
    let mut findings = detector
        .detect_checked(
            content,
            file_path,
            include_commented_out_tests,
            include_body_tests,
        )
        .map_err(detect_failure)?;
    let added: HashSet<usize> = (0..content.lines().count()).collect();
    let index =
        SuppressionIndex::from_content(content, &added, &comment_ranges(content, file_path));
    findings.retain(|finding| !index.is_suppressed(finding));
    let invalid_markers = index.invalid_findings(file_path);
    findings.retain(|finding| !suppression_config.suppresses(match_path, &finding.rule));
    findings.extend(invalid_markers);
    if has_marker_in_comments(content, None, &comment_ranges(content, file_path)) {
        findings.push(Finding::new(
            "allow-added",
            Confidence::Medium,
            "A test-checker opt-out marker was added in this change; suppressing findings you just introduced is suspicious.",
            1,
            file_path,
            "",
        ));
    }
    Ok(findings)
}

/// Build the normal report, apply approved removals, and append the cleanup summary.
#[allow(clippy::too_many_arguments)]
fn strip_report(
    findings: &[Finding],
    content: Option<&str>,
    target: Option<&Path>,
    file_path: &str,
    match_path: &str,
    detector: &TestDetector,
    suppression_config: &SuppressionConfig,
    custom_prompt: &str,
    min_confidence: Confidence,
) -> HookOutcome {
    let candidates: Vec<&Finding> = findings
        .iter()
        .filter(|finding| is_removable_rule(&finding.rule))
        .collect();
    let mut spans = Vec::new();
    let mut span_error = None;
    if let Some(content) = content
        && !candidates.is_empty()
    {
        match detector.test_spans(content, file_path) {
            Ok(found) => spans = found,
            Err(error) => span_error = Some(error.to_string()),
        }
    }

    let mut removable_spans = Vec::new();
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    let mut seen_spans = HashSet::new();
    for candidate in &candidates {
        let approved = suppression_config.removable(match_path, &candidate.rule);
        let span = enclosing_span(&spans, candidate.line_number);
        let name = span
            .and_then(|span| span.name.clone())
            .unwrap_or_else(|| candidate.text.trim().to_owned());
        if approved && target.is_some() {
            let Some(span) = span else {
                let reason = span_error.as_deref().unwrap_or("no enclosing test block");
                return HookOutcome::Failed(format!(
                    "cleanup pass could not locate the {} finding at line {} ({reason}); nothing was changed",
                    candidate.rule, candidate.line_number
                ));
            };
            removable_spans.push((span.start_byte, span.end_byte));
            if seen_spans.insert((span.start_byte, span.end_byte)) {
                removed.push((display_name(&name), candidate.rule.clone()));
            }
        } else {
            kept.push((display_name(&name), candidate.rule.clone()));
        }
    }

    let approved_without_file = target.is_none()
        && candidates
            .iter()
            .any(|candidate| suppression_config.removable(match_path, &candidate.rule));
    if let (Some(target), Some(content)) = (target, content)
        && !removable_spans.is_empty()
    {
        let merged = merge_spans(removable_spans);
        let Some(updated) = remove_spans(content, &merged) else {
            return HookOutcome::Failed(
                "cleanup pass produced an invalid byte range; nothing was changed".to_owned(),
            );
        };
        if updated != content
            && let Err(error) = std::fs::write(target, updated.as_bytes())
        {
            return HookOutcome::Failed(format!(
                "cleanup pass could not write {}: {error}",
                target.display()
            ));
        }
    }

    let note =
        approved_without_file.then_some("the target file is not on disk; nothing was rewritten");
    let summary = strip_summary(&removed, &kept, note);
    append_strip_summary(
        gate_and_format(findings, custom_prompt, min_confidence),
        &summary,
    )
}

/// The innermost captured test block containing `line` (1-based), if it is unambiguous.
fn enclosing_span(spans: &[TestSpan], line: usize) -> Option<&TestSpan> {
    let mut best: Option<&TestSpan> = None;
    for span in spans {
        if line < span.start_row + 1 || line > span.end_row + 1 {
            continue;
        }
        let length = span.end_byte.saturating_sub(span.start_byte);
        match best {
            None => best = Some(span),
            Some(current) => {
                let current_length = current.end_byte.saturating_sub(current.start_byte);
                if length < current_length {
                    best = Some(span);
                } else if length == current_length
                    && (span.start_byte != current.start_byte || span.end_byte != current.end_byte)
                {
                    return None;
                }
            }
        }
    }
    best
}

/// Sort and union overlapping spans; each removal is still one test block.
fn merge_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        if let Some((_, current_end)) = merged.last_mut()
            && start < *current_end
        {
            *current_end = (*current_end).max(end);
            continue;
        }
        merged.push((start, end));
    }
    merged
}

/// Remove every byte range in `spans`, leaving the rest of the file byte-identical.
fn remove_spans(content: &str, spans: &[(usize, usize)]) -> Option<String> {
    let mut output = String::with_capacity(content.len());
    let mut cursor = 0;
    for &(start, end) in spans {
        if start < cursor || end < start || end > content.len() {
            return None;
        }
        output.push_str(content.get(cursor..start)?);
        cursor = end;
    }
    output.push_str(content.get(cursor..)?);
    Some(output)
}

fn display_name(name: &str) -> String {
    if name.trim().is_empty() {
        "(unnamed)".to_owned()
    } else {
        name.trim().to_owned()
    }
}

fn strip_summary(
    removed: &[(String, String)],
    kept: &[(String, String)],
    note: Option<&str>,
) -> String {
    let mut summary = format!(
        "[test-checker] cleanup pass: {} removed, {} kept awaiting approval.\n",
        removed.len(),
        kept.len()
    );
    for (name, rule) in removed {
        summary.push_str("  removed: ");
        summary.push_str(name);
        summary.push_str(" [");
        summary.push_str(rule);
        summary.push_str("]\n");
    }
    for (name, rule) in kept {
        summary.push_str("  kept awaiting approval: ");
        summary.push_str(name);
        summary.push_str(" [");
        summary.push_str(rule);
        summary.push_str("]\n");
    }
    if let Some(note) = note {
        summary.push_str("  note: ");
        summary.push_str(note);
        summary.push('\n');
    }
    summary.push_str(
        "  approve a removal with a \"remove\" entry ({\"glob\",\"rules\",\"reason\"}) in .test-checker.json\n",
    );
    summary
}

fn append_strip_summary(outcome: HookOutcome, summary: &str) -> HookOutcome {
    match outcome {
        HookOutcome::Clean => HookOutcome::CleanWith(format!(
            "[test-checker] Success: No problematic tests found\n{summary}"
        )),
        HookOutcome::Findings(mut message) => {
            if !message.ends_with('\n') {
                message.push('\n');
            }
            message.push_str(summary);
            HookOutcome::Findings(message)
        }
        other => other,
    }
}

/// Map a detector failure onto a hook outcome: a parse-budget abort is a visible skip, and any
/// other detector failure is an internal failure (exit 1), never a clean pass.
fn detect_failure(error: DetectError) -> HookOutcome {
    match error {
        DetectError::BudgetExceeded => skip("Parse budget exceeded"),
        DetectError::Internal(reason) => HookOutcome::Failed(reason),
    }
}

/// Fold the Write-baseline skip note into the outcome: a clean result becomes a visible skip, and a
/// findings result gets the note prepended.
fn apply_baseline_note(outcome: HookOutcome, baseline_note: Option<&'static str>) -> HookOutcome {
    let Some(reason) = baseline_note else {
        return outcome;
    };
    match outcome {
        HookOutcome::Clean => skip(reason),
        HookOutcome::Findings(message) => {
            HookOutcome::Findings(format!("[test-checker] Skipping: {reason}\n{message}"))
        }
        other => other,
    }
}

/// The baseline a `Write` should compare against: HEAD only for test files in supported languages.
/// The baseline is opt-in (`TEST_CHECKER_WRITE_BASELINE`) and only runs for a test file in a
/// supported language inside a git work tree (checked without spawning).
fn write_baseline(hook_input: &HookInput, file_path: &str) -> Baseline {
    if !baseline_enabled()
        || !is_test_path(file_path)
        || language_for_path(file_path).is_none()
        || hook_input.cwd.is_empty()
        || !has_git_ancestor(&hook_input.cwd)
    {
        return Baseline::NotRequested;
    }
    head_content(&hook_input.cwd, file_path)
}

fn baseline_enabled() -> bool {
    std::env::var("TEST_CHECKER_WRITE_BASELINE").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Whether `cwd` is inside a git work tree. Walks up looking for `.git`; no process spawn.
fn has_git_ancestor(cwd: &str) -> bool {
    let mut dir = std::path::Path::new(cwd);
    loop {
        if dir.join(".git").exists() {
            return true;
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return false,
        }
    }
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

/// The per-fragment results of one check: findings, invalid-marker findings, and a Write-baseline
/// skip note that the caller folds into the final outcome.
struct ChangeFindings {
    findings: Vec<Finding>,
    invalid_markers: Vec<Finding>,
    baseline_note: Option<&'static str>,
}

/// Detect findings for the tool input and apply the fragment-scoped suppression index.
fn analyze_change(
    tool: ToolKind,
    hook_input: &HookInput,
    detector: &TestDetector,
    file_path: &str,
    include_commented_out_tests: bool,
    include_body_tests: bool,
) -> Result<ChangeFindings, HookOutcome> {
    let mut findings = Vec::new();
    let mut invalid_markers = Vec::new();
    let mut baseline_note = None;
    match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => {
            let old_string = hook_input.tool_input.old_string.as_str();
            let new_string = hook_input.tool_input.new_string.as_str();
            if new_string.is_empty() {
                if old_string.is_empty() {
                    return Err(skip("No content to check"));
                }
                // A deletion edit has no new content to parse, but it can still remove assertions.
                if include_body_tests {
                    findings = rules::diff_findings(old_string, "", file_path);
                }
            } else {
                findings = detect_findings(detector.detect_edit_checked(
                    old_string,
                    new_string,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ))?;
                let index = SuppressionIndex::from_content(
                    new_string,
                    &engine::added_lines(old_string, new_string),
                    &comment_ranges(new_string, file_path),
                );
                findings.retain(|finding| !index.is_suppressed(finding));
                invalid_markers = index.invalid_findings(file_path);
            }
        }
        ToolKind::Edit | ToolKind::MultiEdit => {
            if hook_input.tool_input.edits.is_empty() {
                return Err(skip("No content to check"));
            }
            for edit in &hook_input.tool_input.edits {
                if edit.new_string.is_empty() {
                    if include_body_tests && !edit.old_string.is_empty() {
                        findings.extend(rules::diff_findings(&edit.old_string, "", file_path));
                    }
                    continue;
                }
                let mut detected = detect_findings(detector.detect_edit_checked(
                    &edit.old_string,
                    &edit.new_string,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ))?;
                let index = SuppressionIndex::from_content(
                    &edit.new_string,
                    &engine::added_lines(&edit.old_string, &edit.new_string),
                    &comment_ranges(&edit.new_string, file_path),
                );
                detected.retain(|finding| !index.is_suppressed(finding));
                findings.extend(detected);
                invalid_markers.extend(index.invalid_findings(file_path));
            }
        }
        ToolKind::Write => {
            let content = get_content_to_check(hook_input);
            if content.is_empty() {
                return Err(skip("No content to check"));
            }
            // Only a test file in a supported language has diff rules to compare with HEAD.
            findings = match write_baseline(hook_input, file_path) {
                Baseline::Head(head) => detect_findings(detector.detect_edit_checked(
                    &head,
                    content,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ))?,
                Baseline::Skipped(reason) => {
                    baseline_note = Some(reason);
                    detect_findings(detector.detect_checked(
                        content,
                        file_path,
                        include_commented_out_tests,
                        include_body_tests,
                    ))?
                }
                Baseline::NotRequested => detect_findings(detector.detect_checked(
                    content,
                    file_path,
                    include_commented_out_tests,
                    include_body_tests,
                ))?,
            };
            let added: HashSet<usize> = (0..content.lines().count()).collect();
            let index = SuppressionIndex::from_content(
                content,
                &added,
                &comment_ranges(content, file_path),
            );
            findings.retain(|finding| !index.is_suppressed(finding));
            invalid_markers = index.invalid_findings(file_path);
        }
    }
    Ok(ChangeFindings {
        findings,
        invalid_markers,
        baseline_note,
    })
}

/// Map a detector failure onto a hook outcome: a parse-budget abort is a visible skip, and any
/// other detector failure is an internal failure (exit 1), never a clean pass.
fn detect_findings(detection: DetectResult) -> Result<Vec<Finding>, HookOutcome> {
    detection.map_err(|error| match error {
        DetectError::BudgetExceeded => skip("Parse budget exceeded"),
        DetectError::Internal(reason) => HookOutcome::Failed(reason),
    })
}

/// Apply the filters and the confidence gate, then render: the shared tail of every check path.
fn gate_and_format(
    findings: &[Finding],
    custom_prompt: &str,
    min_confidence: Confidence,
) -> HookOutcome {
    let filtered: Vec<Finding> = apply_filters_without_suppression(findings)
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

/// Whether a suppression marker in a comment sits on a line the change added (agent
/// self-suppression). Markers inside string literals are invisible.
fn marker_added_in_change(tool: ToolKind, hook_input: &HookInput, file_path: &str) -> bool {
    match tool {
        ToolKind::Edit if hook_input.tool_input.edits.is_empty() => added_line_has_marker(
            file_path,
            &hook_input.tool_input.old_string,
            &hook_input.tool_input.new_string,
        ),
        ToolKind::Edit | ToolKind::MultiEdit => hook_input
            .tool_input
            .edits
            .iter()
            .any(|edit| added_line_has_marker(file_path, &edit.old_string, &edit.new_string)),
        ToolKind::Write => {
            let content = get_content_to_check(hook_input);
            has_marker_in_comments(content, None, &comment_ranges(content, file_path))
        }
    }
}

fn added_line_has_marker(file_path: &str, old_content: &str, new_content: &str) -> bool {
    let added = engine::added_lines(old_content, new_content);
    has_marker_in_comments(
        new_content,
        Some(&added),
        &comment_ranges(new_content, file_path),
    )
}

fn skip(reason: &str) -> HookOutcome {
    HookOutcome::Skipped(reason.to_owned())
}

#[cfg(test)]
mod tests {
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
            FORCE_INTERNAL_FAILURE.with(|flag| flag.set(false));
        }
    }

    /// Clears the parse-budget seam on drop, for the same reason.
    struct ResetBudget;

    impl Drop for ResetBudget {
        fn drop(&mut self) {
            FORCE_BUDGET_EXCEEDED.with(|flag| flag.set(false));
        }
    }

    #[test]
    fn check_hook_input_given_internal_failure_exits_one_with_the_did_not_run_message() {
        // given: the runtime seam makes the detector report an internal failure
        let _seam = seam_guard();
        let _reset = ResetFailure;
        FORCE_INTERNAL_FAILURE.with(|flag| flag.set(true));
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
        FORCE_BUDGET_EXCEEDED.with(|flag| flag.set(true));
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
