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
    blocks: Vec<TestBlock>,
}

impl<'a> Derived<'a> {
    /// Builds the order-insensitive diff, precomputing each line's code once.
    #[must_use]
    pub fn new(change: &'a Change, blocks: Vec<TestBlock>) -> Self {
        let (added, removed) = diff_lines(&change.old_text, &change.new_text, change.language);
        Self {
            change,
            added,
            removed,
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
        if !swallowed_error_hit(&joined, d.language()) {
            return Vec::new();
        }
        let sample = d
            .added
            .iter()
            .find(|line| swallowed_error_hit(&line.code, d.language()))
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
    let stem = test_stem(&d.change.file_path);
    if stem.is_empty() {
        return Vec::new();
    }
    let imported = imported_names_from_stem(&d.change.new_text, &stem, d.language());
    let mut out = Vec::new();
    for diff_line in &d.added {
        let code = filters::strip_comments(&diff_line.text, d.language());
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
        push_hit(&mut out, diff_line.line, MOCK_OF_SUT_MESSAGE);
    }
    out
}

fn mock_of_sut_python(d: &Derived<'_>) -> Vec<Hit> {
    let stem = test_stem(&d.change.file_path);
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
        let Some(target) = extract_python_patch_target(code) else {
            continue;
        };
        if patched_module_root(&target) != stem {
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
    for diff_line in &d.added {
        *added
            .entry(
                filters::strip_comments(&diff_line.text, d.language())
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
