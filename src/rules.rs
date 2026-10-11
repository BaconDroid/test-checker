//! The 11 rule classifiers over one derived change set.
//!
//! `Derived` captures the diff (added/removed [`DiffLine`]s, each with its line,
//! raw text and comment-/string-stripped code, plus the parsed test blocks)
//! once; each unit struct below classifies it. `evaluate` is the single place a
//! finding is constructed and stamped with its rule's catalog confidence.

use std::collections::{HashMap, HashSet};

use crate::detector::{Change, TestBlock};
use crate::filters;
use crate::models::{Confidence, Finding};

/// One diff occurrence: its 1-based line in the source stream, the raw text, and
/// the comment-/string-stripped code, all computed once so they cannot drift.
struct DiffLine {
    line: usize,
    text: String,
    code: String,
}

/// A rule hit before its rule id and confidence are stamped from [`RULES`].
pub struct Hit {
    pub line: usize,
    pub message: String,
}

/// Everything the classifiers need for one changed file, captured once.
///
/// `added`/`removed` carry each diff occurrence's real line (in `new_text` for
/// added, `old_text` for removed) together with its precomputed code text.
pub struct Derived<'a> {
    change: &'a Change,
    added: Vec<DiffLine>,
    removed: Vec<DiffLine>,
    added_by_code: HashMap<String, i64>,
    blocks: Vec<TestBlock>,
}

impl<'a> Derived<'a> {
    /// Builds the order-insensitive diff, precomputing each line's code and the
    /// `strip_comments`-normalized added-line census once.
    #[must_use]
    pub fn new(change: &'a Change, blocks: Vec<TestBlock>) -> Self {
        let (added, removed) = diff_lines(&change.old_text, &change.new_text, change.language);
        let mut added_by_code: HashMap<String, i64> = HashMap::new();
        for diff_line in &added {
            *added_by_code
                .entry(
                    filters::strip_comments(&diff_line.text, change.language)
                        .trim()
                        .to_owned(),
                )
                .or_insert(0) += 1;
        }
        Self {
            change,
            added,
            removed,
            added_by_code,
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
    hits: fn(&Derived<'_>) -> Vec<Hit>,
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

/// Runs the 11 classifiers and constructs each finding from its rule id and
/// catalog confidence — the single finding-construction site.
#[must_use]
pub fn evaluate(d: &Derived<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for rule in RULES {
        for hit in (rule.hits)(d) {
            findings.push(make_finding(&d.change.file_path, rule, hit));
        }
    }
    findings
}

/// The single finding-construction helper: pairs a rule hit with its catalog id
/// and confidence.
fn make_finding(file: &str, rule: &Rule, hit: Hit) -> Finding {
    Finding::new(file, hit.line, rule.id, hit.message, rule.confidence)
}

// --- Classifiers -----------------------------------------------------------

/// `skip-focus-added` (high).
#[derive(Clone, Copy, Debug, Default)]
pub struct SkipFocusAddedFilter;

impl SkipFocusAddedFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
        added_line_findings(
            d,
            "A test was disabled or focused (.skip/.only/.todo, xit/xdescribe, skip marker, #[ignore], t.Skip).",
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
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
        let mut out = Vec::new();
        for diff_line in &d.added {
            if diff_line.code.trim().is_empty() || !filters::tautological_hit(&diff_line.code) {
                continue;
            }
            push_hit(
                &mut out,
                diff_line.line,
                "Assertion is tautological (always true).",
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
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
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
            push_hit(
                &mut out,
                block.start_line,
                "Test body has no executable statement.",
            );
        }
        out
    }
}

/// `unknown-test` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct UnknownTestFilter;

impl UnknownTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
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
            push_hit(
                &mut out,
                block.start_line,
                "Test has no assertion, expect, fail, or expected exception.",
            );
        }
        out
    }
}

/// `gutted-test` (high).
#[derive(Clone, Copy, Debug, Default)]
pub struct GuttedTestFilter;

impl GuttedTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
        let removed = unique_assertions(&d.removed);
        if removed.is_empty() || !unique_assertions(&d.added).is_empty() {
            return Vec::new();
        }
        if matches!(d.language(), "js" | "ts" | "python" | "go" | "rust") {
            let before = d
                .removed
                .iter()
                .filter(|line| filters::is_test_declaration(&line.text, d.language()))
                .count();
            let after = d
                .added
                .iter()
                .filter(|line| filters::is_test_declaration(&line.text, d.language()))
                .count();
            if before != after {
                return Vec::new();
            }
        }
        let sample = d
            .removed
            .iter()
            .find(|line| filters::is_assertion_line(&line.code))
            .map(|line| line.line);
        let Some(line) = sample else {
            return Vec::new();
        };
        let mut out = Vec::new();
        push_hit(
            &mut out,
            line,
            "All assertions were removed and nothing replaced them.",
        );
        out
    }
}

/// `swallowed-error` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct SwallowedErrorFilter;

impl SwallowedErrorFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
        let joined = d
            .added
            .iter()
            .map(|line| line.code.as_str())
            .collect::<Vec<&str>>()
            .join("\n");
        if !filters::swallowed_error_hit(&joined, d.language()) {
            return Vec::new();
        }
        let sample = d
            .added
            .iter()
            .find(|line| filters::swallowed_error_hit(&line.code, d.language()))
            .map(|line| line.line)
            .or_else(|| d.added.first().map(|line| line.line));
        let Some(line) = sample else {
            return Vec::new();
        };
        let mut out = Vec::new();
        push_hit(
            &mut out,
            line,
            "Errors are swallowed (empty catch / except pass / Err(_) => {}).",
        );
        out
    }
}

/// `duplicate-test` (medium).
#[derive(Clone, Copy, Debug, Default)]
pub struct DuplicateTestFilter;

impl DuplicateTestFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
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
                push_hit(
                    &mut out,
                    block.start_line,
                    format!("Duplicate test body (identical to the test at line {first})."),
                );
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
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
        match d.language() {
            "js" | "ts" => mock_of_sut_javascript(d),
            "python" => mock_of_sut_python(d),
            _ => Vec::new(),
        }
    }
}

const MOCK_OF_SUT_MESSAGE: &str =
    "The module under test is mocked; asserting on the mock cannot validate real behavior.";

fn mock_of_sut_javascript(d: &Derived<'_>) -> Vec<Hit> {
    let stem = filters::test_stem(&d.change.file_path);
    if stem.is_empty() {
        return Vec::new();
    }
    let imported = filters::imported_names_from_stem(&d.change.new_text, &stem, d.language());
    let mut out = Vec::new();
    for diff_line in &d.added {
        let code = filters::strip_comments(&diff_line.text, d.language());
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let hit = filters::extract_js_mock_target(code)
            .is_some_and(|target| filters::mocked_module_stem(&target) == stem)
            || filters::extract_js_spy_target(code).is_some_and(|name| imported.contains(&name));
        if !hit {
            continue;
        }
        push_hit(&mut out, diff_line.line, MOCK_OF_SUT_MESSAGE);
    }
    out
}

fn mock_of_sut_python(d: &Derived<'_>) -> Vec<Hit> {
    let stem = filters::test_stem(&d.change.file_path);
    if stem.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for diff_line in &d.added {
        let code = filters::strip_comments(&diff_line.text, d.language());
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let Some(target) = filters::extract_python_patch_target(code) else {
            continue;
        };
        if filters::patched_module_root(&target) != stem {
            continue;
        }
        push_hit(&mut out, diff_line.line, MOCK_OF_SUT_MESSAGE);
    }
    out
}

/// `over-mocking` (low).
#[derive(Clone, Copy, Debug, Default)]
pub struct OverMockingFilter;

impl OverMockingFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
        let added = d
            .added
            .iter()
            .filter(|line| filters::mock_identifier_hit(&line.code))
            .count();
        let removed = d
            .removed
            .iter()
            .filter(|line| filters::mock_identifier_hit(&line.code))
            .count();
        if added <= removed {
            return Vec::new();
        }
        let sample = d
            .added
            .iter()
            .find(|line| filters::mock_identifier_hit(&line.code))
            .map(|line| line.line);
        let Some(line) = sample else {
            return Vec::new();
        };
        let mut out = Vec::new();
        push_hit(
            &mut out,
            line,
            format!("New mock identifiers added (net +{}).", added - removed),
        );
        out
    }
}

/// `assertion-roulette` (low).
#[derive(Clone, Copy, Debug, Default)]
pub struct AssertionRouletteFilter;

impl AssertionRouletteFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
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
            push_hit(
                &mut out,
                block.start_line,
                format!("{assertions} assertions with no message; failures are indistinguishable."),
            );
        }
        out
    }
}

/// `redundant-assertion` (low).
#[derive(Clone, Copy, Debug, Default)]
pub struct RedundantAssertionFilter;

impl RedundantAssertionFilter {
    #[must_use]
    pub fn hits(&self, d: &Derived<'_>) -> Vec<Hit> {
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
                    push_hit(
                        &mut out,
                        locate_within_block(&d.change.new_text, block, raw),
                        "The same assertion is repeated in one test.",
                    );
                    break;
                }
            }
        }
        out
    }
}

// --- Shared rule plumbing --------------------------------------------------

fn added_line_findings<F, S>(
    d: &Derived<'_>,
    message: &'static str,
    predicate: F,
    skip: S,
) -> Vec<Hit>
where
    F: Fn(&str) -> bool,
    S: Fn(&str) -> bool,
{
    let mut out = Vec::new();
    for diff_line in &d.added {
        if diff_line.code.trim().is_empty() || skip(&diff_line.text) || !predicate(&diff_line.code)
        {
            continue;
        }
        push_hit(&mut out, diff_line.line, message);
    }
    out
}

fn push_hit(out: &mut Vec<Hit>, line: usize, message: impl Into<String>) {
    out.push(Hit {
        line,
        message: message.into(),
    });
}

fn unique_assertions(lines: &[DiffLine]) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for diff_line in lines {
        let code = diff_line.code.trim();
        if code.is_empty() || !filters::is_assertion_line(code) {
            continue;
        }
        set.insert(filters::normalize_assertion(code));
    }
    set.into_iter().collect()
}

fn block_is_added(d: &Derived<'_>, block: &TestBlock) -> bool {
    if d.change.is_new {
        return true;
    }
    let mut needed: HashMap<String, i64> = HashMap::new();
    let mut code_lines = 0_usize;
    for raw in &block.lines {
        let stripped = filters::strip_comments(raw, d.language());
        let line = stripped.trim();
        if line.is_empty() || filters::is_comment_line(raw, d.language()) {
            continue;
        }
        code_lines += 1;
        *needed.entry(line.to_owned()).or_insert(0) += 1;
    }
    code_lines > 0
        && needed
            .iter()
            .all(|(line, count)| d.added_by_code.get(line).is_some_and(|have| have >= count))
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
/// `added` carries the occurrence's line in `new_text` and `removed` its line in
/// `old_text`, each paired with the precomputed code text. For a text with `old`
/// and `new` occurrences, the first `min(old, new)` occurrences are treated as
/// matched; any surplus is the additive/removal set. Tracking the line per
/// occurrence keeps two identical added lines as two distinct findings instead
/// of collapsing them onto the first match. Texts stay grouped by first
/// appearance, as before, so the only behavioral change is the line/code pairing.
fn diff_lines(old_text: &str, new_text: &str, language: &str) -> (Vec<DiffLine>, Vec<DiffLine>) {
    let old_lines = split_lines(old_text);
    let new_lines = split_lines(new_text);
    let old_count = count_occurrences(&old_lines);
    let new_count = count_occurrences(&new_lines);
    (
        surplus_lines(&new_lines, &old_count, &new_count, language),
        surplus_lines(&old_lines, &new_count, &old_count, language),
    )
}

/// Occurrences of `lines` beyond the `min(mine, other)` match count, grouped by
/// first appearance and carrying each occurrence's 1-based line and code text.
fn surplus_lines(
    lines: &[&str],
    other: &HashMap<&str, usize>,
    mine: &HashMap<&str, usize>,
    language: &str,
) -> Vec<DiffLine> {
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
                out.push(DiffLine {
                    line: index + 1,
                    text: key.to_owned(),
                    code: filters::code_text(key, language),
                });
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

/// Locates `needle` within `block`'s own line range in `text` (1-based),
/// falling back to the block's start line. Restricting the search to the block
/// keeps each test's duplicate assertion at its own line instead of collapsing
/// onto the first match in the file. Exact matches win over trimmed ones.
fn locate_within_block(text: &str, block: &TestBlock, needle: &str) -> usize {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = block.start_line.max(1);
    let end = block.end_line.min(lines.len());
    let trimmed = needle.trim();
    for exact in [true, false] {
        for line_number in start..=end {
            let Some(line) = lines.get(line_number - 1) else {
                continue;
            };
            let hit = if exact {
                *line == needle
            } else {
                line.trim() == trimmed
            };
            if hit {
                return line_number;
            }
        }
    }
    block.start_line
}

#[cfg(test)]
mod tests {
    use super::{Confidence, RULES};

    #[test]
    fn rules_table_pins_each_id_and_confidence_in_order() {
        // given: the frozen (id, confidence) wire contract
        let expected = [
            ("skip-focus-added", Confidence::High),
            ("tautological-assertion", Confidence::High),
            ("empty-test", Confidence::Medium),
            ("unknown-test", Confidence::Medium),
            ("gutted-test", Confidence::High),
            ("swallowed-error", Confidence::Medium),
            ("duplicate-test", Confidence::Medium),
            ("mock-of-sut", Confidence::Medium),
            ("over-mocking", Confidence::Low),
            ("assertion-roulette", Confidence::Low),
            ("redundant-assertion", Confidence::Low),
        ];

        // when
        let actual: Vec<(&str, Confidence)> = RULES
            .iter()
            .map(|rule| (rule.id, rule.confidence))
            .collect();

        // then
        assert_eq!(actual, expected);
    }
}
