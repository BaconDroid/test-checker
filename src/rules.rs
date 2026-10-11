//! The 11 rule classifiers over one derived change set.
//!
//! `Derived` captures the diff (added/removed lines, their comment- and
//! string-stripped code text, and the parsed test blocks) once; each unit
//! struct below classifies it. `evaluate` runs the catalog in order and stamps
//! every finding with the rule's confidence.

use std::collections::{HashMap, HashSet};

use crate::detector::{Change, TestBlock};
use crate::filters;
use crate::models::{Confidence, Finding};

/// One diff occurrence: its 1-based line in the source stream plus the text.
type LineOccurrence = (usize, String);

/// Added and removed occurrences, in source order.
type LineDiff = (Vec<LineOccurrence>, Vec<LineOccurrence>);

/// Everything the classifiers need for one changed file, captured once.
///
/// `added` and `removed` pair each diff occurrence with the 1-based line it
/// occupies in `new_text` (added) or `old_text` (removed), so a rule can report
/// the occurrence's real line without re-searching for the first matching text.
pub struct Derived<'a> {
    change: &'a Change,
    added: Vec<LineOccurrence>,
    removed: Vec<LineOccurrence>,
    added_code: Vec<String>,
    removed_code: Vec<String>,
    blocks: Vec<TestBlock>,
}

impl<'a> Derived<'a> {
    /// Builds the order-insensitive diff and precomputes the code text once.
    #[must_use]
    pub fn new(change: &'a Change, blocks: Vec<TestBlock>) -> Self {
        let (added, removed) = diff_lines(&change.old_text, &change.new_text);
        let added_code = added
            .iter()
            .map(|(_, line)| filters::code_text(line, change.language))
            .collect();
        let removed_code = removed
            .iter()
            .map(|(_, line)| filters::code_text(line, change.language))
            .collect();
        Self {
            change,
            added,
            removed,
            added_code,
            removed_code,
            blocks,
        }
    }

    const fn language(&self) -> &'static str {
        self.change.language
    }
}

#[derive(Clone, Copy)]
struct Rule {
    id: &'static str,
    confidence: Confidence,
    hits: fn(&Derived<'_>) -> Vec<Finding>,
}

const RULES: &[Rule] = &[
    Rule {
        id: "skip-focus-added",
        confidence: Confidence::High,
        hits: |d| SkipFocusAddedFilter.hits(d),
    },
    Rule {
        id: "tautological-assertion",
        confidence: Confidence::High,
        hits: |d| TautologicalAssertionFilter.hits(d),
    },
    Rule {
        id: "empty-test",
        confidence: Confidence::Medium,
        hits: |d| EmptyTestFilter.hits(d),
    },
    Rule {
        id: "unknown-test",
        confidence: Confidence::Medium,
        hits: |d| UnknownTestFilter.hits(d),
    },
    Rule {
        id: "gutted-test",
        confidence: Confidence::High,
        hits: |d| GuttedTestFilter.hits(d),
    },
    Rule {
        id: "swallowed-error",
        confidence: Confidence::Medium,
        hits: |d| SwallowedErrorFilter.hits(d),
    },
    Rule {
        id: "duplicate-test",
        confidence: Confidence::Medium,
        hits: |d| DuplicateTestFilter.hits(d),
    },
    Rule {
        id: "mock-of-sut",
        confidence: Confidence::Medium,
        hits: |d| MockOfSutFilter.hits(d),
    },
    Rule {
        id: "over-mocking",
        confidence: Confidence::Low,
        hits: |d| OverMockingFilter.hits(d),
    },
    Rule {
        id: "assertion-roulette",
        confidence: Confidence::Low,
        hits: |d| AssertionRouletteFilter.hits(d),
    },
    Rule {
        id: "redundant-assertion",
        confidence: Confidence::Low,
        hits: |d| RedundantAssertionFilter.hits(d),
    },
];

/// Every emitted rule id, in catalog order.
#[must_use]
pub fn rule_ids() -> Vec<&'static str> {
    RULES.iter().map(|rule| rule.id).collect()
}

/// Runs the 11 classifiers and stamps each finding with its rule confidence.
#[must_use]
pub fn evaluate(d: &Derived<'_>) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    for rule in RULES {
        let mut rule_findings = (rule.hits)(d);
        for finding in &mut rule_findings {
            finding.confidence = rule.confidence;
        }
        findings.extend(rule_findings);
    }
    findings
}

// --- Classifiers -----------------------------------------------------------

/// `skip-focus-added` (high).
#[derive(Clone, Copy, Debug, Default)]
pub struct SkipFocusAddedFilter;

impl SkipFocusAddedFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        added_line_findings(
            d,
            FindingSpec {
                rule: "skip-focus-added",
                confidence: Confidence::High,
                message: "A test was disabled or focused (.skip/.only/.todo, xit/xdescribe, skip marker, #[ignore], t.Skip).",
            },
            |code| filters::skip_focus_hit(code, d.language()),
            filters::is_conditional_skip,
        )
    }
}

/// `tautological-assertion` (high).
#[derive(Clone, Copy, Debug, Default)]
pub struct TautologicalAssertionFilter;

impl TautologicalAssertionFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for ((line_number, _), code) in d.added.iter().zip(&d.added_code) {
            if code.trim().is_empty() || !filters::tautological_hit(code) {
                continue;
            }
            push_finding(
                &mut out,
                d,
                *line_number,
                "tautological-assertion",
                "Assertion is tautological (always true).",
                Confidence::High,
            );
        }
        out
    }
}

/// `empty-test` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyTestFilter;

impl EmptyTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for block in &d.blocks {
            if !block_is_added(d, block) {
                continue;
            }
            let body = block.lines.join("\n");
            let executable = filters::count_real_lines(&body, d.language());
            if executable > 0 && !filters::is_trivial_body(&body) {
                continue;
            }
            out.push(Finding::new(
                &d.change.file_path,
                block.start_line,
                "empty-test",
                "Test body has no executable statement.",
                Confidence::Medium,
            ));
        }
        out
    }
}

/// `unknown-test` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct UnknownTestFilter;

impl UnknownTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for block in &d.blocks {
            if !block_is_added(d, block) {
                continue;
            }
            let body = block.lines.join("\n");
            if filters::count_real_lines(&body, d.language()) == 0
                || filters::is_trivial_body(&body)
                || filters::has_assertion(&body)
            {
                continue;
            }
            out.push(Finding::new(
                &d.change.file_path,
                block.start_line,
                "unknown-test",
                "Test has no assertion, expect, fail, or expected exception.",
                Confidence::Medium,
            ));
        }
        out
    }
}

/// `gutted-test` (high).
#[derive(Clone, Copy, Debug, Default)]
pub struct GuttedTestFilter;

impl GuttedTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let removed = unique_assertions(&d.removed_code);
        if removed.is_empty() || !unique_assertions(&d.added_code).is_empty() {
            return Vec::new();
        }
        if matches!(d.language(), "js" | "ts" | "python" | "go" | "rust") {
            let before = d
                .removed
                .iter()
                .filter(|(_, line)| filters::is_test_declaration(line, d.language()))
                .count();
            let after = d
                .added
                .iter()
                .filter(|(_, line)| filters::is_test_declaration(line, d.language()))
                .count();
            if before != after {
                return Vec::new();
            }
        }
        let sample = d
            .removed
            .iter()
            .zip(&d.removed_code)
            .find(|(_, code)| filters::is_assertion_line(code))
            .map(|((line_number, _), _)| *line_number);
        let Some(line_number) = sample else {
            return Vec::new();
        };
        vec![Finding::new(
            &d.change.file_path,
            line_number,
            "gutted-test",
            "All assertions were removed and nothing replaced them.",
            Confidence::High,
        )]
    }
}

/// `swallowed-error` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct SwallowedErrorFilter;

impl SwallowedErrorFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let joined = d.added_code.join("\n");
        if !swallowed_error_hit(&joined, d.language()) {
            return Vec::new();
        }
        let sample = d
            .added
            .iter()
            .zip(&d.added_code)
            .find(|(_, code)| swallowed_error_hit(code, d.language()))
            .map(|((line_number, _), _)| *line_number)
            .or_else(|| d.added.first().map(|(line_number, _)| *line_number));
        let Some(line_number) = sample else {
            return Vec::new();
        };
        let mut out = Vec::new();
        push_finding(
            &mut out,
            d,
            line_number,
            "swallowed-error",
            "Errors are swallowed (empty catch / except pass / Err(_) => {}).",
            Confidence::Medium,
        );
        out
    }
}

/// `duplicate-test` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct DuplicateTestFilter;

impl DuplicateTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::new();
        for block in &d.blocks {
            let normalized =
                filters::normalize_body(block.lines.iter().map(String::as_str), d.language());
            if normalized.is_empty() {
                continue;
            }
            if let Some(first) = seen.get(&normalized) {
                if !block_is_added(d, block) && !d.change.is_new {
                    continue;
                }
                out.push(Finding::new(
                    &d.change.file_path,
                    block.start_line,
                    "duplicate-test",
                    format!("Duplicate test body (identical to the test at line {first})."),
                    Confidence::Medium,
                ));
            } else {
                seen.insert(normalized, block.start_line);
            }
        }
        out
    }
}

/// `mock-of-sut` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct MockOfSutFilter;

impl MockOfSutFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        match d.language() {
            "js" | "ts" => mock_of_sut_javascript(d),
            "python" => mock_of_sut_python(d),
            _ => Vec::new(),
        }
    }
}

const MOCK_OF_SUT_MESSAGE: &str =
    "The module under test is mocked; asserting on the mock cannot validate real behavior.";

fn mock_of_sut_javascript(d: &Derived<'_>) -> Vec<Finding> {
    let stem = test_stem(&d.change.file_path);
    if stem.is_empty() {
        return Vec::new();
    }
    let imported = imported_names_from_stem(&d.change.new_text, &stem, d.language());
    let mut out = Vec::new();
    for (line_number, line) in &d.added {
        let code = filters::strip_comments(line, d.language());
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let hit = extract_js_mock_target(code)
            .is_some_and(|target| mocked_module_stem(&target) == stem)
            || extract_js_spy_target(code).is_some_and(|name| imported.contains(&name));
        if !hit {
            continue;
        }
        push_finding(
            &mut out,
            d,
            *line_number,
            "mock-of-sut",
            MOCK_OF_SUT_MESSAGE,
            Confidence::Medium,
        );
    }
    out
}

fn mock_of_sut_python(d: &Derived<'_>) -> Vec<Finding> {
    let stem = test_stem(&d.change.file_path);
    if stem.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (line_number, line) in &d.added {
        let code = filters::strip_comments(line, d.language());
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let Some(target) = extract_python_patch_target(code) else {
            continue;
        };
        if patched_module_root(&target) != stem {
            continue;
        }
        push_finding(
            &mut out,
            d,
            *line_number,
            "mock-of-sut",
            MOCK_OF_SUT_MESSAGE,
            Confidence::Medium,
        );
    }
    out
}

/// `over-mocking` (low).
#[derive(Clone, Copy, Debug, Default)]
pub struct OverMockingFilter;

impl OverMockingFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let added = d
            .added_code
            .iter()
            .filter(|code| filters::mock_identifier_hit(code))
            .count();
        let removed = d
            .removed_code
            .iter()
            .filter(|code| filters::mock_identifier_hit(code))
            .count();
        if added <= removed {
            return Vec::new();
        }
        let sample = d
            .added
            .iter()
            .zip(&d.added_code)
            .find(|(_, code)| filters::mock_identifier_hit(code))
            .map(|((line_number, _), _)| *line_number);
        let Some(line_number) = sample else {
            return Vec::new();
        };
        vec![Finding::new(
            &d.change.file_path,
            line_number,
            "over-mocking",
            format!("New mock identifiers added (net +{}).", added - removed),
            Confidence::Low,
        )]
    }
}

/// `assertion-roulette` (low).
#[derive(Clone, Copy, Debug, Default)]
pub struct AssertionRouletteFilter;

impl AssertionRouletteFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for block in &d.blocks {
            if !block_is_added(d, block) {
                continue;
            }
            let assertions = block
                .lines
                .iter()
                .filter(|line| filters::is_assertion_line(&filters::code_text(line, d.language())))
                .count();
            if assertions <= 1 {
                continue;
            }
            let body = block.lines.join("\n");
            if filters::has_assertion_message(&body) {
                continue;
            }
            out.push(Finding::new(
                &d.change.file_path,
                block.start_line,
                "assertion-roulette",
                format!("{assertions} assertions with no message; failures are indistinguishable."),
                Confidence::Low,
            ));
        }
        out
    }
}

/// `redundant-assertion` (low).
#[derive(Clone, Copy, Debug, Default)]
pub struct RedundantAssertionFilter;

impl RedundantAssertionFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Finding> {
        let mut out = Vec::new();
        for block in &d.blocks {
            if !block_is_added(d, block) {
                continue;
            }
            let mut seen: HashSet<String> = HashSet::new();
            for raw in &block.lines {
                let code = filters::code_text(raw, d.language());
                let code = code.trim();
                if code.is_empty() || !filters::is_assertion_line(code) {
                    continue;
                }
                if !seen.insert(code.to_owned()) {
                    out.push(Finding::new(
                        &d.change.file_path,
                        locate_line(&d.change.new_text, raw),
                        "redundant-assertion",
                        "The same assertion is repeated in one test.",
                        Confidence::Low,
                    ));
                    break;
                }
            }
        }
        out
    }
}

// --- Shared rule plumbing --------------------------------------------------

#[derive(Clone, Copy)]
struct FindingSpec {
    rule: &'static str,
    confidence: Confidence,
    message: &'static str,
}

fn added_line_findings<F, S>(
    d: &Derived<'_>,
    spec: FindingSpec,
    predicate: F,
    skip: S,
) -> Vec<Finding>
where
    F: Fn(&str) -> bool,
    S: Fn(&str) -> bool,
{
    let mut out = Vec::new();
    for ((line_number, raw), code) in d.added.iter().zip(&d.added_code) {
        if code.trim().is_empty() || skip(raw) || !predicate(code) {
            continue;
        }
        push_finding(
            &mut out,
            d,
            *line_number,
            spec.rule,
            spec.message,
            spec.confidence,
        );
    }
    out
}

fn push_finding(
    out: &mut Vec<Finding>,
    d: &Derived<'_>,
    line_number: usize,
    rule: &'static str,
    message: &str,
    confidence: Confidence,
) {
    out.push(Finding::new(
        &d.change.file_path,
        line_number,
        rule,
        message,
        confidence,
    ));
}

fn unique_assertions(code_lines: &[String]) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for raw in code_lines {
        let code = raw.trim();
        if code.is_empty() || !filters::is_assertion_line(code) {
            continue;
        }
        set.insert(filters::normalize_assertion(code));
    }
    set.into_iter().collect()
}

fn swallowed_error_hit(joined: &str, language: &str) -> bool {
    match language {
        "js" | "ts" => has_empty_catch(joined),
        "python" => has_except_pass(joined),
        "rust" => has_empty_err_arm(joined),
        _ => false,
    }
}

fn has_empty_catch(text: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = text[start..].find("catch") {
        let index = start + offset;
        let mut rest = text[index + "catch".len()..].trim_start();
        if let Some(after) = rest.strip_prefix('(') {
            if let Some(close) = after.find(')') {
                rest = after[close + 1..].trim_start();
            }
        }
        if let Some(body) = rest.strip_prefix('{') {
            if body
                .split('}')
                .next()
                .is_some_and(|inner| inner.trim().is_empty())
            {
                return true;
            }
        }
        start = index + "catch".len();
    }
    false
}

fn has_except_pass(text: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = text[start..].find("except") {
        let index = start + offset;
        let rest = &text[index + "except".len()..];
        if let Some((clause, tail)) = rest.split_once(':') {
            let clause = clause.trim();
            if matches!(clause, "" | "Exception" | "BaseException") {
                if let Some(after) = tail.trim_start().strip_prefix("pass") {
                    if after.is_empty() || !filters::is_word_byte(after.as_bytes()[0]) {
                        return true;
                    }
                }
            }
        }
        start = index + "except".len();
    }
    false
}

fn has_empty_err_arm(text: &str) -> bool {
    let squashed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    squashed.contains("Err(_)=>{}") || squashed.contains("Err(_)=>()")
}

fn block_is_added(d: &Derived<'_>, block: &TestBlock) -> bool {
    if d.change.is_new {
        return true;
    }
    let mut added: HashMap<String, i64> = HashMap::new();
    for (_, line) in &d.added {
        *added
            .entry(
                filters::strip_comments(line, d.language())
                    .trim()
                    .to_owned(),
            )
            .or_insert(0) += 1;
    }
    let mut code_lines = 0_usize;
    for raw in &block.lines {
        let stripped = filters::strip_comments(raw, d.language());
        let line = stripped.trim();
        if line.is_empty() || filters::is_comment_line(raw, d.language()) {
            continue;
        }
        code_lines += 1;
        let entry = added.get_mut(line);
        match entry {
            Some(count) if *count > 0 => *count -= 1,
            _ => return false,
        }
    }
    code_lines > 0
}

// --- Diff and line helpers -------------------------------------------------

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').collect()
}

/// Order-insensitive multiset line diff that preserves each occurrence's real
/// line number.
///
/// Returns `added` as `(line in new_text, text)` and `removed` as
/// `(line in old_text, text)`. For a text with `old` and `new` occurrences, the
/// first `min(old, new)` occurrences are treated as matched; any surplus is the
/// additive/removal set. Tracking the line per occurrence keeps two identical
/// added lines as two distinct findings instead of collapsing them onto the
/// first match. Texts stay grouped by first appearance, as before, so the only
/// behavioral change is the per-occurrence line.
fn diff_lines(old_text: &str, new_text: &str) -> LineDiff {
    let old_lines = split_lines(old_text);
    let new_lines = split_lines(new_text);
    let old_count = count_occurrences(&old_lines);
    let new_count = count_occurrences(&new_lines);
    (
        surplus_lines(&new_lines, &old_count, &new_count),
        surplus_lines(&old_lines, &new_count, &old_count),
    )
}

/// Occurrences of `lines` beyond the `min(mine, other)` match count, grouped by
/// first appearance and carrying each occurrence's 1-based line.
fn surplus_lines(
    lines: &[&str],
    other: &HashMap<&str, usize>,
    mine: &HashMap<&str, usize>,
) -> Vec<LineOccurrence> {
    let mut out = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for line in lines {
        let key = *line;
        if !seen.insert(key) {
            continue;
        }
        let matched = other
            .get(key)
            .copied()
            .unwrap_or(0)
            .min(mine.get(key).copied().unwrap_or(0));
        let mut occurrence = 0_usize;
        for (index, candidate) in lines.iter().enumerate() {
            if *candidate != key {
                continue;
            }
            occurrence += 1;
            if occurrence > matched {
                out.push((index + 1, key.to_owned()));
            }
        }
    }
    out
}

fn count_occurrences<'a>(lines: &[&'a str]) -> HashMap<&'a str, usize> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for line in lines {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

fn locate_line(new_text: &str, needle: &str) -> usize {
    let lines: Vec<&str> = new_text.split('\n').collect();
    if let Some(index) = lines.iter().position(|line| *line == needle) {
        return index + 1;
    }
    let trimmed = needle.trim();
    lines
        .iter()
        .position(|line| line.trim() == trimmed)
        .map_or(0, |index| index + 1)
}

// --- Module-under-test helpers --------------------------------------------

fn test_stem(file_path: &str) -> String {
    let base = file_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_path)
        .to_owned();
    let Some(dot) = base.rfind('.') else {
        return base;
    };
    let mut stem = base[..dot].to_owned();
    if let Some(inner) = stem.strip_suffix(".test") {
        stem = inner.to_owned();
    } else if let Some(inner) = stem.strip_suffix(".spec") {
        stem = inner.to_owned();
    } else if let Some(inner) = stem.strip_prefix("test_") {
        stem = inner.to_owned();
    } else if let Some(inner) = stem.strip_suffix("_test") {
        stem = inner.to_owned();
    }
    stem
}

fn mocked_module_stem(spec: &str) -> String {
    let mut spec = spec.trim().to_owned();
    if let Some(index) = spec.rfind(['/', '\\']) {
        spec = spec[index + 1..].to_owned();
    }
    if let Some(index) = spec.rfind('.') {
        if index > 0 {
            spec = spec[..index].to_owned();
        }
    }
    spec
}

fn patched_module_root(spec: &str) -> String {
    let spec = spec.trim();
    spec.find(['.', '/'])
        .map_or_else(|| spec.to_owned(), |index| spec[..index].to_owned())
}

fn extract_js_mock_target(code: &str) -> Option<String> {
    for prefix in ["vi.mock(", "jest.mock("] {
        if let Some(rest) = code.split_once(prefix).map(|(_, tail)| tail) {
            if let Some(target) = read_quoted(rest) {
                return Some(target);
            }
        }
    }
    None
}

fn extract_js_spy_target(code: &str) -> Option<String> {
    for prefix in ["vi.spyOn(", "jest.spyOn("] {
        if let Some(rest) = code.split_once(prefix).map(|(_, tail)| tail) {
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn extract_python_patch_target(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    let mut start = 0;
    while let Some(offset) = code[start..].find("patch(") {
        let index = start + offset;
        // Require a non-word byte before `patch(` so `mocker.patch(`,
        // `patch(` and `@patch(` match, while `dispatch(` does not.
        if index == 0 || !filters::is_word_byte(bytes[index - 1]) {
            return read_quoted(&code[index + "patch(".len()..]);
        }
        start = index + "patch(".len();
    }
    None
}

fn read_quoted(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let quote = trimmed.chars().next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }
    let rest = &trimmed[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

fn imported_names_from_stem(text: &str, stem: &str, language: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    for raw in text.split('\n') {
        let line = filters::strip_comments(raw, language);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(clause) = line.strip_prefix("import ") {
            if let Some((binding, spec)) = clause.split_once(" from ") {
                if let Some(module) = read_quoted(spec) {
                    if mocked_module_stem(&module) == stem {
                        collect_js_bindings(binding, &mut names);
                    }
                }
            }
            continue;
        }
        if let Some(eq) = line.find('=') {
            let binding = line[..eq]
                .trim()
                .trim_start_matches("const ")
                .trim_start_matches("let ")
                .trim_start_matches("var ")
                .trim();
            if let Some(rest) = line[eq + 1..].trim().strip_prefix("require(") {
                if let Some(module) = read_quoted(rest) {
                    if mocked_module_stem(&module) == stem {
                        collect_js_bindings(binding, &mut names);
                    }
                }
            }
        }
    }
    names
}

fn collect_js_bindings(binding: &str, names: &mut HashSet<String>) {
    let mut binding = binding.trim().to_owned();
    if let Some(open) = binding.find('{') {
        if let Some(close) = binding[open..].find('}') {
            collect_named_bindings(&binding[open + 1..open + close], names);
            let mut combined = binding[..open].to_owned();
            combined.push_str(&binding[open + close + 1..]);
            binding = combined;
        }
    }
    let binding = binding.trim().trim_end_matches(',').trim();
    if binding.is_empty() {
        return;
    }
    if let Some(rest) = binding.strip_prefix('*') {
        if let Some((_, alias)) = rest.trim().rsplit_once(' ') {
            names.insert(alias.trim().to_owned());
        }
        return;
    }
    if let Some(first) = binding.split_whitespace().next() {
        names.insert(first.to_owned());
    }
}

fn collect_named_bindings(inner: &str, names: &mut HashSet<String>) {
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let local = part
            .split_once(" as ")
            .map_or(part, |(_, alias)| alias.trim());
        names.insert(local.to_owned());
    }
}
