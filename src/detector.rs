//! The test-weakening detector: tree-sitter parsing plus the 18 rule ids.
//!
//! Only added lines are candidates, comments are stripped, the `is_test_file`
//! gate applies, and every failure path degrades to no findings (fail-open).

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Parser};

use crate::filters;
use crate::language::language_for_path;
use crate::models::{Confidence, Finding};

/// The derived diff the rules operate on.
#[derive(Clone, Debug)]
pub struct Change {
    pub file_path: String,
    pub language: &'static str,
    pub new_text: String,
    pub old_text: String,
    pub is_new: bool,
    pub is_test_file: bool,
}

/// A whole test body. `start_line` is the declaration line for Python and the
/// opening brace for the brace languages.
#[derive(Clone, Debug)]
pub struct TestBlock {
    pub start_line: usize,
    pub end_line: usize,
    pub lines: Vec<String>,
}

/// Everything a rule needs for one changed file.
struct Derived<'a> {
    change: &'a Change,
    added: Vec<String>,
    removed: Vec<String>,
    blocks: Vec<TestBlock>,
}

impl Derived<'_> {
    const fn language(&self) -> &'static str {
        self.change.language
    }
}

#[derive(Clone, Copy)]
struct Rule {
    id: &'static str,
    confidence: Confidence,
    run: fn(&Derived<'_>) -> Vec<Finding>,
}

const RULES: &[Rule] = &[
    Rule {
        id: "protected-paths",
        confidence: Confidence::High,
        run: run_protected_paths,
    },
    Rule {
        id: "skip-focus-added",
        confidence: Confidence::High,
        run: run_skip_focus_added,
    },
    Rule {
        id: "tautological-assertion",
        confidence: Confidence::High,
        run: run_tautological,
    },
    Rule {
        id: "empty-test",
        confidence: Confidence::Medium,
        run: run_empty_test,
    },
    Rule {
        id: "unknown-test",
        confidence: Confidence::Medium,
        run: run_unknown_test,
    },
    Rule {
        id: "gutted-test",
        confidence: Confidence::High,
        run: run_gutted_test,
    },
    Rule {
        id: "matcher-loosened",
        confidence: Confidence::High,
        run: run_matcher_loosened,
    },
    Rule {
        id: "swallowed-error",
        confidence: Confidence::Medium,
        run: run_swallowed_error,
    },
    Rule {
        id: "duplicate-test",
        confidence: Confidence::Medium,
        run: run_duplicate_test,
    },
    Rule {
        id: "forced-success",
        confidence: Confidence::High,
        run: run_forced_success,
    },
    Rule {
        id: "mock-of-sut",
        confidence: Confidence::Medium,
        run: run_mock_of_sut,
    },
    Rule {
        id: "over-mocking",
        confidence: Confidence::Low,
        run: run_over_mocking,
    },
    Rule {
        id: "assertion-roulette",
        confidence: Confidence::Low,
        run: run_assertion_roulette,
    },
    Rule {
        id: "redundant-assertion",
        confidence: Confidence::Low,
        run: run_redundant_assertion,
    },
    Rule {
        id: "weakened-config",
        confidence: Confidence::Medium,
        run: run_weakened_config,
    },
    Rule {
        id: "tests-not-run",
        confidence: Confidence::Medium,
        run: run_tests_not_run,
    },
    Rule {
        id: "expected-from-sut",
        confidence: Confidence::Medium,
        run: run_expected_from_sut,
    },
    Rule {
        id: "negative-control-unrelated",
        confidence: Confidence::Medium,
        run: run_negative_control_unrelated,
    },
];

/// Every emitted rule id, in catalog order.
#[must_use]
pub fn rule_ids() -> Vec<&'static str> {
    RULES.iter().map(|rule| rule.id).collect()
}

#[derive(Debug, Default)]
pub struct TestDetector;

impl TestDetector {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Runs the catalog over one change set.
    #[must_use]
    pub fn detect(&self, change: &Change) -> Vec<Finding> {
        if !change.is_test_file || filters::is_file_disabled(&change.new_text) {
            return Vec::new();
        }
        let Some(spec) = language_for_path(&change.file_path) else {
            return Vec::new();
        };
        let Ok(grammar) = tree_sitter_language_pack::get_language(spec.name) else {
            return Vec::new();
        };
        let mut parser = Parser::new();
        if parser.set_language(&grammar).is_err() {
            return Vec::new();
        }
        let Some(tree) = parser.parse(change.new_text.as_bytes(), None) else {
            return Vec::new();
        };
        let (added, removed) = diff_lines(&change.old_text, &change.new_text);
        let blocks = find_test_blocks(&tree, change.new_text.as_bytes(), change.language);
        let derived = Derived {
            change,
            added,
            removed,
            blocks,
        };
        let mut findings: Vec<Finding> = Vec::new();
        for rule in RULES {
            let mut rule_findings = (rule.run)(&derived);
            for finding in &mut rule_findings {
                finding.confidence = rule.confidence;
            }
            findings.extend(rule_findings);
        }
        dedupe_sorted(findings)
    }
}

// --- Rules -----------------------------------------------------------------

fn run_protected_paths(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file || d.change.is_new {
        return Vec::new();
    }
    vec![Finding::new(
        &d.change.file_path,
        0,
        "protected-paths",
        "Edit of an existing test file.",
        Confidence::High,
    )]
}

fn run_skip_focus_added(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
    added_line_findings(
        d,
        FindingSpec {
            rule: "skip-focus-added",
            confidence: Confidence::High,
            message: "A test was disabled or focused (.skip/.only/.todo, xit/xdescribe, skip marker, #[ignore], t.Skip).",
            mode: LineMode::Code,
        },
        |code| filters::skip_focus_hit(code, d.language()),
        filters::is_conditional_skip,
    )
}

fn run_tautological(d: &Derived<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in &d.added {
        let code = filters::code_text(line, d.language());
        if code.trim().is_empty() || !filters::tautological_hit(&code) {
            continue;
        }
        push_if_allowed(
            &mut out,
            d,
            line,
            "tautological-assertion",
            "Assertion is tautological (always true).",
            Confidence::High,
        );
    }
    out
}

fn run_empty_test(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
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

fn run_unknown_test(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
    let mut out = Vec::new();
    for block in &d.blocks {
        if !block_is_added(d, block) {
            continue;
        }
        let body = block.lines.join("\n");
        if filters::count_real_lines(&body, d.language()) == 0 || filters::has_assertion(&body) {
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

fn run_gutted_test(d: &Derived<'_>) -> Vec<Finding> {
    let removed = unique_assertions(&d.removed, d.language());
    if removed.is_empty() || !unique_assertions(&d.added, d.language()).is_empty() {
        return Vec::new();
    }
    let declaration = declaration_pattern(d.language());
    if declaration {
        let before = d
            .removed
            .iter()
            .filter(|line| filters::is_test_declaration(line, d.language()))
            .count();
        let after = d
            .added
            .iter()
            .filter(|line| filters::is_test_declaration(line, d.language()))
            .count();
        if before != after {
            return Vec::new();
        }
    }
    let sample = d
        .removed
        .iter()
        .find(|line| filters::is_assertion_line(&filters::code_text(line, d.language())));
    let Some(sample) = sample else {
        return Vec::new();
    };
    vec![Finding::new(
        &d.change.file_path,
        locate_line(&d.change.old_text, sample),
        "gutted-test",
        "All assertions were removed and nothing replaced them.",
        Confidence::High,
    )]
}

fn run_matcher_loosened(d: &Derived<'_>) -> Vec<Finding> {
    if d.added.is_empty() || d.removed.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for message in filters::matcher_loosening_messages() {
        let removed_hit = d.removed.iter().any(|line| {
            let code = filters::code_text(line, d.language());
            filters::matches_loosening_before(&code, message)
                && !filters::matches_loosening_after(&code, message)
        });
        if !removed_hit {
            continue;
        }
        let added_hit = d.added.iter().find(|line| {
            filters::matches_loosening_after(&filters::code_text(line, d.language()), message)
        });
        let Some(added_hit) = added_hit else {
            continue;
        };
        push_if_allowed(
            &mut out,
            d,
            added_hit,
            "matcher-loosened",
            message,
            Confidence::High,
        );
    }
    out
}

fn run_swallowed_error(d: &Derived<'_>) -> Vec<Finding> {
    let joined = d
        .added
        .iter()
        .map(|line| filters::code_text(line, d.language()))
        .collect::<Vec<_>>()
        .join("\n");
    if !swallowed_error_hit(&joined, d.language()) {
        return Vec::new();
    }
    let sample = d
        .added
        .iter()
        .find(|line| {
            swallowed_error_line_hit(&filters::code_text(line, d.language()), d.language())
        })
        .or_else(|| d.added.first());
    let Some(sample) = sample else {
        return Vec::new();
    };
    let mut out = Vec::new();
    push_if_allowed(
        &mut out,
        d,
        sample,
        "swallowed-error",
        "Errors are swallowed (empty catch / except pass / Err(_) => {}).",
        Confidence::Medium,
    );
    out
}

fn run_duplicate_test(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
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

fn run_forced_success(d: &Derived<'_>) -> Vec<Finding> {
    added_line_findings(
        d,
        FindingSpec {
            rule: "forced-success",
            confidence: Confidence::High,
            message: "Test command forced to succeed regardless of outcome.",
            mode: LineMode::Code,
        },
        filters::forced_success_hit,
        |_| false,
    )
}

fn run_mock_of_sut(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
    match d.language() {
        "js" | "ts" => mock_of_sut_javascript(d),
        "python" => mock_of_sut_python(d),
        _ => Vec::new(),
    }
}

fn mock_of_sut_javascript(d: &Derived<'_>) -> Vec<Finding> {
    let stem = test_stem(&d.change.file_path);
    if stem.is_empty() {
        return Vec::new();
    }
    let imported = imported_names_from_stem(&d.change.new_text, &stem, d.language());
    let mut out = Vec::new();
    for line in &d.added {
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
        push_if_allowed(
            &mut out,
            d,
            line,
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
    for line in &d.added {
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
        push_if_allowed(
            &mut out,
            d,
            line,
            "mock-of-sut",
            MOCK_OF_SUT_MESSAGE,
            Confidence::Medium,
        );
    }
    out
}

const MOCK_OF_SUT_MESSAGE: &str =
    "The module under test is mocked; asserting on the mock cannot validate real behavior.";

fn run_over_mocking(d: &Derived<'_>) -> Vec<Finding> {
    let added = d
        .added
        .iter()
        .filter(|line| filters::mock_identifier_hit(&filters::code_text(line, d.language())))
        .count();
    let removed = d
        .removed
        .iter()
        .filter(|line| filters::mock_identifier_hit(&filters::code_text(line, d.language())))
        .count();
    if added <= removed {
        return Vec::new();
    }
    let sample = d
        .added
        .iter()
        .find(|line| filters::mock_identifier_hit(&filters::code_text(line, d.language())));
    let Some(sample) = sample else {
        return Vec::new();
    };
    vec![Finding::new(
        &d.change.file_path,
        locate_line(&d.change.new_text, sample),
        "over-mocking",
        format!("New mock identifiers added (net +{}).", added - removed),
        Confidence::Low,
    )]
}

fn run_assertion_roulette(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
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

fn run_redundant_assertion(d: &Derived<'_>) -> Vec<Finding> {
    if !d.change.is_test_file {
        return Vec::new();
    }
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

fn run_weakened_config(d: &Derived<'_>) -> Vec<Finding> {
    let mut out = added_line_findings(
        d,
        FindingSpec {
            rule: "weakened-config",
            confidence: Confidence::Medium,
            message: "Configuration weakens test enforcement.",
            mode: LineMode::Raw,
        },
        filters::weakened_config_hit,
        |_| false,
    );
    out.extend(weakened_threshold(d));
    out
}

fn weakened_threshold(d: &Derived<'_>) -> Vec<Finding> {
    let removed: Vec<f64> = d
        .removed
        .iter()
        .filter_map(|line| filters::parse_threshold(line))
        .collect();
    let added: Vec<f64> = d
        .added
        .iter()
        .filter_map(|line| filters::parse_threshold(line))
        .collect();
    if removed.is_empty() || added.is_empty() {
        return Vec::new();
    }
    let previous = removed.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let next = added.iter().copied().fold(f64::INFINITY, f64::min);
    if next >= previous {
        return Vec::new();
    }
    let sample = d
        .added
        .iter()
        .find(|line| filters::parse_threshold(line).is_some());
    let Some(sample) = sample else {
        return Vec::new();
    };
    let line_number = locate_line(&d.change.new_text, sample);
    if filters::within_bypass(&d.change.new_text, line_number) {
        return Vec::new();
    }
    vec![Finding::new(
        &d.change.file_path,
        line_number,
        "weakened-config",
        format!("Coverage/test threshold lowered from {previous} to {next}."),
        Confidence::Medium,
    )]
}

fn run_tests_not_run(d: &Derived<'_>) -> Vec<Finding> {
    added_line_findings(
        d,
        FindingSpec {
            rule: "tests-not-run",
            confidence: Confidence::Medium,
            message: "A test command excludes or skips tests.",
            mode: LineMode::Code,
        },
        filters::tests_not_run_hit,
        |_| false,
    )
}

fn run_expected_from_sut(d: &Derived<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in &d.added {
        let code = filters::code_text(line, d.language());
        if code.trim().is_empty() || !filters::expected_from_sut_hit(&code) {
            continue;
        }
        push_if_allowed(
            &mut out,
            d,
            line,
            "expected-from-sut",
            "Expected value is produced by the code under test; the assertion may be self-referential.",
            Confidence::Medium,
        );
    }
    out
}

fn run_negative_control_unrelated(d: &Derived<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in &d.added {
        let code = filters::strip_comments(line, d.language());
        let code = code.trim();
        if code.is_empty() || !filters::negative_control_hit(code, d.language()) {
            continue;
        }
        push_if_allowed(
            &mut out,
            d,
            line,
            "negative-control-unrelated",
            "Negative control is too broad and can pass for an unrelated reason.",
            Confidence::Medium,
        );
    }
    out
}

// --- Shared rule plumbing --------------------------------------------------

#[derive(Clone, Copy)]
enum LineMode {
    Code,
    Raw,
}

#[derive(Clone, Copy)]
struct FindingSpec {
    rule: &'static str,
    confidence: Confidence,
    message: &'static str,
    mode: LineMode,
}

fn transform(line: &str, language: &str, mode: LineMode) -> String {
    match mode {
        LineMode::Code => filters::code_text(line, language),
        LineMode::Raw => line.to_owned(),
    }
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
    for line in &d.added {
        let code = transform(line, d.language(), spec.mode);
        if code.trim().is_empty() || skip(line) || !predicate(&code) {
            continue;
        }
        let line_number = locate_line(&d.change.new_text, line);
        if filters::within_bypass(&d.change.new_text, line_number) {
            continue;
        }
        out.push(Finding::new(
            &d.change.file_path,
            line_number,
            spec.rule,
            spec.message,
            spec.confidence,
        ));
    }
    out
}

fn push_if_allowed(
    out: &mut Vec<Finding>,
    d: &Derived<'_>,
    line: &str,
    rule: &'static str,
    message: &str,
    confidence: Confidence,
) {
    let line_number = locate_line(&d.change.new_text, line);
    if filters::within_bypass(&d.change.new_text, line_number) {
        return;
    }
    out.push(Finding::new(
        &d.change.file_path,
        line_number,
        rule,
        message,
        confidence,
    ));
}

fn declaration_pattern(language: &str) -> bool {
    matches!(language, "js" | "ts" | "python" | "go" | "rust")
}

fn unique_assertions(lines: &[String], language: &str) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for line in lines {
        let code = filters::code_text(line, language);
        let code = code.trim();
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

fn swallowed_error_line_hit(code: &str, language: &str) -> bool {
    swallowed_error_hit(code, language)
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
        if let Some((_, tail)) = rest.split_once(':') {
            if tail.trim_start().starts_with("pass") {
                return true;
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

// --- Test block extraction -------------------------------------------------

fn find_test_blocks(tree: &tree_sitter::Tree, source: &[u8], plugin: &str) -> Vec<TestBlock> {
    let mut blocks = Vec::new();
    collect_blocks(tree.root_node(), source, plugin, &mut blocks);
    blocks
}

fn collect_blocks(node: Node<'_>, source: &[u8], plugin: &str, out: &mut Vec<TestBlock>) {
    match plugin {
        "python" => collect_python(node, source, out),
        "go" => collect_go(node, source, out),
        "rust" => collect_rust(node, source, out),
        "js" | "ts" => collect_js(node, source, plugin, out),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_blocks(child, source, plugin, out);
    }
}

fn collect_python(node: Node<'_>, source: &[u8], out: &mut Vec<TestBlock>) {
    if node.kind() != "function_definition" || !node_name(node, source).starts_with("test_") {
        return;
    }
    if let Some(body) = node.child_by_field_name("body") {
        out.push(make_block(node, body, source, false));
    }
}

fn collect_go(node: Node<'_>, source: &[u8], out: &mut Vec<TestBlock>) {
    if node.kind() != "function_declaration" || !node_name(node, source).starts_with("Test") {
        return;
    }
    if let Some(body) = node.child_by_field_name("body") {
        out.push(make_block(body, body, source, true));
    }
}

fn collect_rust(node: Node<'_>, source: &[u8], out: &mut Vec<TestBlock>) {
    if node.kind() != "function_item" || !rust_has_attribute(node, source, "test") {
        return;
    }
    if let Some(body) = node.child_by_field_name("body") {
        out.push(make_block(body, body, source, true));
    }
}

fn collect_js(node: Node<'_>, source: &[u8], plugin: &str, out: &mut Vec<TestBlock>) {
    if node.kind() != "call_expression" {
        return;
    }
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    let function_text = format!("{}(", node_text(function, source));
    if !filters::is_test_declaration(&function_text, plugin) {
        return;
    }
    let Some(callback) = js_callback(node) else {
        return;
    };
    let Some(body) = callback.child_by_field_name("body") else {
        return;
    };
    if body.kind() == "statement_block" {
        out.push(make_block(body, body, source, true));
    }
}

fn make_block(start: Node<'_>, body: Node<'_>, source: &[u8], brace: bool) -> TestBlock {
    let raw = node_text(body, source);
    let content = if brace {
        let inner = raw.strip_prefix('{').unwrap_or(raw);
        inner.strip_suffix('}').unwrap_or(inner)
    } else {
        raw
    };
    TestBlock {
        start_line: start.start_position().row + 1,
        end_line: body.end_position().row + 1,
        lines: content.split('\n').map(str::to_owned).collect(),
    }
}

fn js_callback(call: Node<'_>) -> Option<Node<'_>> {
    let arguments = call.child_by_field_name("arguments")?;
    let count = arguments.named_child_count();
    for index in (0..count).rev() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let Some(argument) = arguments.named_child(index) else {
            continue;
        };
        if matches!(
            argument.kind(),
            "arrow_function" | "function_expression" | "function"
        ) {
            return Some(argument);
        }
    }
    None
}

fn rust_has_attribute(node: Node<'_>, source: &[u8], name: &str) -> bool {
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        if current.kind() != "attribute_item" {
            return false;
        }
        if rust_attribute_name(node_text(current, source)) == name {
            return true;
        }
        sibling = current.prev_named_sibling();
    }
    false
}

fn rust_attribute_name(attribute: &str) -> String {
    let mut value = attribute.trim();
    value = value.strip_prefix('#').unwrap_or(value);
    value = value.strip_prefix('!').unwrap_or(value);
    value = value
        .trim()
        .strip_prefix('[')
        .unwrap_or_else(|| value.trim());
    value = value.strip_suffix(']').unwrap_or(value);
    let value = value.trim();
    let end = value.find(['(', ' ', '=']).unwrap_or(value.len());
    value[..end].trim().to_lowercase()
}

fn node_name(node: Node<'_>, source: &[u8]) -> String {
    node.child_by_field_name("name")
        .map(|name| node_text(name, source).to_owned())
        .unwrap_or_default()
}

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn block_is_added(d: &Derived<'_>, block: &TestBlock) -> bool {
    if d.change.is_new {
        return true;
    }
    let mut added: HashMap<String, i64> = HashMap::new();
    for line in &d.added {
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
        let line = raw.trim();
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

fn diff_lines(old_text: &str, new_text: &str) -> (Vec<String>, Vec<String>) {
    let old_lines = split_lines(old_text);
    let new_lines = split_lines(new_text);
    let old_count = count_occurrences(&old_lines);
    let new_count = count_occurrences(&new_lines);

    let mut added = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for line in &new_lines {
        if !seen.insert(line) {
            continue;
        }
        let delta =
            new_count.get(line).copied().unwrap_or(0) - old_count.get(line).copied().unwrap_or(0);
        for _ in 0..delta.max(0) {
            added.push((*line).to_owned());
        }
    }
    let mut removed = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for line in &old_lines {
        if !seen.insert(line) {
            continue;
        }
        let delta =
            old_count.get(line).copied().unwrap_or(0) - new_count.get(line).copied().unwrap_or(0);
        for _ in 0..delta.max(0) {
            removed.push((*line).to_owned());
        }
    }
    (added, removed)
}

fn count_occurrences<'a>(lines: &[&'a str]) -> HashMap<&'a str, i64> {
    let mut counts: HashMap<&str, i64> = HashMap::new();
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

fn dedupe_sorted(mut findings: Vec<Finding>) -> Vec<Finding> {
    findings.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.rule.cmp(right.rule))
            .then_with(|| left.message.cmp(&right.message))
    });
    let mut seen: HashSet<(usize, &'static str, String)> = HashSet::new();
    findings.retain(|finding| seen.insert((finding.line, finding.rule, finding.message.clone())));
    findings
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
    let rest = code
        .split_once("patch(")
        .map(|(_, tail)| tail)
        .or_else(|| code.strip_prefix("patch("))?;
    read_quoted(rest)
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

#[cfg(test)]
mod tests {
    use super::{Rule, rule_ids};

    #[test]
    fn rule_ids_match_the_plugin_contract() {
        let ids = rule_ids();
        assert_eq!(ids.len(), 18);
        assert_eq!(ids[0], "protected-paths");
        assert_eq!(ids[10], "mock-of-sut");
        assert_eq!(ids[17], "negative-control-unrelated");
        let _: &[Rule] = super::RULES;
    }
}
