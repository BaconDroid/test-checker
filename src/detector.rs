use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use tree_sitter::{Query, QueryCursor, StreamingIterator};

use crate::engine;
use crate::language::{
    is_test_callee, is_test_function_name, language_for_path, string_node_kinds,
    string_query_source, test_query,
};
use crate::models::{Confidence, Finding};
use crate::rules::{self, AddedLine, DiffContext};

struct Runtime {
    name: String,
    language: tree_sitter::Language,
    comment_query: Query,
    test_query: Option<Query>,
    injection_query: Option<Query>,
    /// Matches every string-literal node in the grammar (see `language::string_node_kinds`).
    string_query: Option<Query>,
}

static RUNTIMES: LazyLock<Mutex<HashMap<String, Arc<Runtime>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Work counters for [`masked_view`], compiled only into the crate's unit tests so a release build
/// carries no counter, atomic, or branch.
#[cfg(test)]
#[derive(Debug, Default)]
struct MaskedViewCounters {
    constructions: std::sync::atomic::AtomicUsize,
    nodes_visited: std::sync::atomic::AtomicUsize,
    bytes_copied: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
static MASKED_VIEW_COUNTERS: MaskedViewCounters = MaskedViewCounters {
    constructions: std::sync::atomic::AtomicUsize::new(0),
    nodes_visited: std::sync::atomic::AtomicUsize::new(0),
    bytes_copied: std::sync::atomic::AtomicUsize::new(0),
};

#[cfg(test)]
impl MaskedViewCounters {
    fn reset(&self) {
        use std::sync::atomic::Ordering;
        self.constructions.store(0, Ordering::Relaxed);
        self.nodes_visited.store(0, Ordering::Relaxed);
        self.bytes_copied.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> WorkCounts {
        use std::sync::atomic::Ordering;
        WorkCounts {
            constructions: self.constructions.load(Ordering::Relaxed),
            nodes_visited: self.nodes_visited.load(Ordering::Relaxed),
            bytes_copied: self.bytes_copied.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkCounts {
    constructions: usize,
    nodes_visited: usize,
    bytes_copied: usize,
}

// Unit-test seam for the internal-failure path. Compiled only into the crate's unit tests, so no
// binary, environment variable, or command-line flag can reach it. Thread-local so a test that sets
// it cannot force another test's parse down the same path.
#[cfg(test)]
thread_local! {
    pub(crate) static FORCE_INTERNAL_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// Unit-test seam for the parse-budget path. Like `FORCE_INTERNAL_FAILURE`, it exists only in the
// crate's unit tests and is unreachable from any binary, environment variable, or flag.
#[cfg(test)]
thread_local! {
    pub(crate) static FORCE_BUDGET_EXCEEDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Why a detection run produced no findings to judge.
///
/// A `BudgetExceeded` is a deliberate fail-open skip; an `Internal` failure means the check never
/// happened and must not be reported as success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DetectError {
    /// The per-parse time budget aborted parsing.
    BudgetExceeded,
    /// The detector could not run: a missing runtime, an uncompilable query, or a failed parse.
    Internal(String),
}

/// The detector's findings, or the reason the check could not run.
pub(crate) type DetectResult = Result<Vec<Finding>, DetectError>;

impl std::fmt::Display for DetectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BudgetExceeded => formatter.write_str("parse budget exceeded"),
            Self::Internal(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for DetectError {}

#[derive(Clone, Debug)]
struct Body {
    start_byte: usize,
    end_byte: usize,
    start_row: usize,
    name: Option<String>,
    /// The fragment contains this complete test node (no parse error inside it); whole-test rules
    /// such as `no-assertion` and `echo-test` must not run on an incomplete fragment.
    reliable: bool,
}

#[derive(Debug, Default)]
pub struct TestDetector;

impl TestDetector {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Analyze a whole file's content (`Write`): every line is treated as newly added.
    ///
    /// This compatibility wrapper returns an empty list when the check cannot run. The hook path
    /// (`src/api.rs`) calls [`Self::detect_checked`] so a failure is never read as a clean pass.
    #[must_use]
    pub fn detect(
        &self,
        content: &str,
        file_path: &str,
        include_commented_out_tests: bool,
        include_body_tests: bool,
    ) -> Vec<Finding> {
        self.detect_checked(
            content,
            file_path,
            include_commented_out_tests,
            include_body_tests,
        )
        .unwrap_or_default()
    }

    /// Analyze a whole file, reporting a failed check instead of degrading it to no findings.
    ///
    /// The detector is stateless, so `self` is only here to mirror the public method.
    #[allow(clippy::unused_self)]
    pub(crate) fn detect_checked(
        &self,
        content: &str,
        file_path: &str,
        include_commented_out_tests: bool,
        include_body_tests: bool,
    ) -> DetectResult {
        if !(include_commented_out_tests || include_body_tests) {
            return Ok(Vec::new());
        }
        let Some(name) = language_for_path(file_path) else {
            return Ok(Vec::new());
        };
        let runtime = runtime_by_name(name)?;
        let source = content.as_bytes();
        let tree = parse_with_runtime(&runtime, source, file_path)?;

        let mut findings = Vec::new();
        if include_commented_out_tests {
            let mut comments = Vec::new();
            collect_comments(&runtime, &tree, content, 0, file_path, &mut comments)?;
            findings.extend(commented_out_findings(
                file_path,
                &comments,
                &HashSet::new(),
            ));
        }
        if include_body_tests {
            if let Some(query) = &runtime.test_query {
                let bodies = capture_bodies(&runtime, query, &tree, source);
                let masked = masked_view(&runtime, &tree, content);
                findings.extend(analyze_new_bodies(
                    &runtime.name,
                    file_path,
                    content,
                    masked.as_deref().unwrap_or(content),
                    &bodies,
                ));
            }
        }
        Ok(dedupe(findings))
    }

    /// Analyze an edit: report only weakening newly introduced by `new_string`.
    ///
    /// This compatibility wrapper returns an empty list when the check cannot run. The hook path
    /// (`src/api.rs`) calls [`Self::detect_edit_checked`] so a failure is never read as a clean pass.
    #[must_use]
    pub fn detect_edit(
        &self,
        old_string: &str,
        new_string: &str,
        file_path: &str,
        include_commented_out_tests: bool,
        include_body_tests: bool,
    ) -> Vec<Finding> {
        self.detect_edit_checked(
            old_string,
            new_string,
            file_path,
            include_commented_out_tests,
            include_body_tests,
        )
        .unwrap_or_default()
    }

    /// Analyze an edit, reporting a failed check instead of degrading it to no findings.
    ///
    /// The detector is stateless, so `self` is only here to mirror the public method.
    #[allow(clippy::unused_self)]
    pub(crate) fn detect_edit_checked(
        &self,
        old_string: &str,
        new_string: &str,
        file_path: &str,
        include_commented_out_tests: bool,
        include_body_tests: bool,
    ) -> DetectResult {
        if !(include_commented_out_tests || include_body_tests) {
            return Ok(Vec::new());
        }
        let Some(name) = language_for_path(file_path) else {
            return Ok(Vec::new());
        };
        let runtime = runtime_by_name(name)?;
        let source = new_string.as_bytes();
        let tree = parse_with_runtime(&runtime, source, file_path)?;
        // Parse the old fragment once: it feeds both comment capture and old-body capture.
        let old_tree = if old_string.is_empty() {
            None
        } else {
            Some(parse_with_runtime(
                &runtime,
                old_string.as_bytes(),
                file_path,
            )?)
        };

        let mut findings = Vec::new();
        if include_commented_out_tests {
            let mut old_comments = Vec::new();
            if let Some(old_tree) = &old_tree {
                collect_comments(
                    &runtime,
                    old_tree,
                    old_string,
                    0,
                    file_path,
                    &mut old_comments,
                )?;
            }
            let old_texts: HashSet<String> = old_comments
                .iter()
                .map(|(_, text)| text.trim().to_lowercase())
                .collect();
            let mut new_comments = Vec::new();
            collect_comments(&runtime, &tree, new_string, 0, file_path, &mut new_comments)?;
            findings.extend(commented_out_findings(file_path, &new_comments, &old_texts));
        }
        if include_body_tests {
            let masked_new = masked_view(&runtime, &tree, new_string);
            let masked_old = old_tree
                .as_ref()
                .and_then(|old_tree| masked_view(&runtime, old_tree, old_string));
            let new_view = masked_new.as_deref().unwrap_or(new_string);
            let old_view = masked_old.as_deref().unwrap_or(old_string);
            findings.extend(rules::diff_findings_on_views(
                old_view, new_view, old_string, new_string, file_path,
            ));
            if let Some(query) = &runtime.test_query {
                let bodies = capture_bodies(&runtime, query, &tree, source);
                let added = engine::added_lines(old_string, new_string);
                findings.extend(analyze_edited_bodies(
                    &runtime.name,
                    file_path,
                    &bodies,
                    &added,
                    Views {
                        original: new_string,
                        old_original: old_string,
                        old: old_view,
                        masked: new_view,
                    },
                ));
                if let Some(old_tree) = &old_tree {
                    let old_bodies =
                        capture_bodies(&runtime, query, old_tree, old_string.as_bytes());
                    // `test-removed` needs both fragments to contain their complete tests: a
                    // parse error means the fragment is partial, so absence proves nothing.
                    if !tree.root_node().has_error() && !old_tree.root_node().has_error() {
                        findings.extend(removed_test_findings(
                            &runtime.name,
                            file_path,
                            old_string,
                            &old_bodies,
                            new_string,
                            &bodies,
                        ));
                    }
                }
            }
        }
        Ok(dedupe(findings))
    }
}

/// A test present before the edit but absent after it (tolerating rename by body signature).
fn removed_test_findings(
    language: &str,
    file_path: &str,
    old_content: &str,
    old_bodies: &[Body],
    new_content: &str,
    new_bodies: &[Body],
) -> Vec<Finding> {
    let new_names: HashSet<&str> = new_bodies
        .iter()
        .filter_map(|body| body.name.as_deref())
        .collect();
    let new_signatures: HashSet<String> = new_bodies
        .iter()
        .map(|body| body_signature(&body_added_lines(new_content, body, language)))
        .collect();
    let mut findings = Vec::new();
    for old_body in old_bodies {
        let signature = body_signature(&body_added_lines(old_content, old_body, language));
        let name_matches = old_body
            .name
            .as_deref()
            .is_some_and(|name| new_names.contains(name));
        let signature_matches = !signature.is_empty() && new_signatures.contains(&signature);
        if !name_matches && !signature_matches {
            findings.push(Finding::new(
                "test-removed",
                Confidence::High,
                "A test present before this edit is gone; deleting a test is a strong tampering signal. STOP and ask the human to add a .test-checker.json entry with a reason if the removal is intended.",
                1,
                file_path,
                old_body.name.as_deref().unwrap_or(""),
            ));
        }
    }
    findings
}

fn runtime_by_name(name: &str) -> Result<Arc<Runtime>, DetectError> {
    #[cfg(test)]
    if FORCE_INTERNAL_FAILURE.with(std::cell::Cell::get) {
        return Err(DetectError::Internal("forced internal failure".to_owned()));
    }

    let Ok(mut runtimes) = RUNTIMES.lock() else {
        return Err(DetectError::Internal(
            "language runtime unavailable: runtime cache poisoned".to_owned(),
        ));
    };
    if let Some(runtime) = runtimes.get(name) {
        return Ok(Arc::clone(runtime));
    }

    let unavailable = || DetectError::Internal(format!("language runtime unavailable: {name}"));
    let uncompilable = || DetectError::Internal(format!("query compilation failed: {name}"));

    let language = tree_sitter_language_pack::get_language(name).map_err(|_| unavailable())?;
    let comment = Query::new(&language, engine::comment_query(name)).map_err(|_| uncompilable())?;
    let body = match test_query(name) {
        Some(source) => Some(Query::new(&language, source).map_err(|_| uncompilable())?),
        None => None,
    };
    let injection = match tree_sitter_language_pack::get_injections_query(name) {
        Some(source) => Some(Query::new(&language, source).map_err(|_| uncompilable())?),
        None => None,
    };
    let string_query = string_query(&language, name);
    let runtime = Arc::new(Runtime {
        name: name.to_owned(),
        language,
        comment_query: comment,
        test_query: body,
        injection_query: injection,
        string_query,
    });
    runtimes.insert(name.to_owned(), Arc::clone(&runtime));
    Ok(runtime)
}

/// Parse `source`, classifying a failure by how long the attempt took.
///
/// `engine::parse_cached` returns `None` both when the per-parse time budget aborts and when the
/// parser cannot be configured, and it exposes no flag distinguishing the two. A precise abort flag
/// would change `engine.rs` (shared byte-for-byte with `comment-checker`).
///
/// Ambiguity resolves toward skipping: only a `None` that arrives **before half** the budget is
/// provably a fast failure, because a budget abort needs the progress callback to fire and the
/// engine only stops once it observes `elapsed > PARSE_TIME_BUDGET`. A `None` at or after half the
/// budget could be either a late hard failure or an abort observed on an earlier tick, so it is
/// reported as `BudgetExceeded` (visible exit 0) instead of `Internal` (exit 1).
fn parse_with_runtime(
    runtime: &Runtime,
    source: &[u8],
    file_path: &str,
) -> Result<tree_sitter::Tree, DetectError> {
    #[cfg(test)]
    if FORCE_BUDGET_EXCEEDED.with(std::cell::Cell::get) {
        return Err(DetectError::BudgetExceeded);
    }
    let started = Instant::now();
    match engine::parse_cached(&runtime.name, &runtime.language, source) {
        Some(tree) => Ok(tree),
        None if started.elapsed() >= engine::PARSE_TIME_BUDGET / 2 => {
            Err(DetectError::BudgetExceeded)
        }
        None => Err(DetectError::Internal(format!("parse failed: {file_path}"))),
    }
}

/// Build the string-literal query for a grammar from the one kind table.
fn string_query(language: &tree_sitter::Language, name: &str) -> Option<Query> {
    let kinds = string_node_kinds(name);
    if kinds.is_empty() {
        return None;
    }
    Query::new(language, &string_query_source(kinds)).ok()
}

/// A length-preserving copy of `content` where every string literal's content is blanked.
///
/// Newlines and every string delimiter character are preserved, so line numbers and byte offsets do
/// not move; only the bytes between the delimiters become spaces. `None` when the grammar has no
/// string kinds or the content is not valid UTF-8 (it always is: ASCII spaces replace whole bytes).
fn masked_view(runtime: &Runtime, tree: &tree_sitter::Tree, content: &str) -> Option<String> {
    let query = runtime.string_query.as_ref()?;
    let source = content.as_bytes();
    #[cfg(test)]
    {
        use std::sync::atomic::Ordering;
        MASKED_VIEW_COUNTERS
            .constructions
            .fetch_add(1, Ordering::Relaxed);
        MASKED_VIEW_COUNTERS
            .bytes_copied
            .fetch_add(source.len(), Ordering::Relaxed);
    }
    let mut bytes = source.to_vec();
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, tree.root_node(), source);
    captures.advance();
    while let Some((query_match, capture_index)) = captures.get() {
        if let Some(capture) = query_match.captures.get(*capture_index) {
            #[cfg(test)]
            {
                use std::sync::atomic::Ordering;
                MASKED_VIEW_COUNTERS
                    .nodes_visited
                    .fetch_add(1, Ordering::Relaxed);
            }
            if let Some(range) = bytes.get_mut(capture.node.byte_range()) {
                for byte in range.iter_mut() {
                    if !matches!(*byte, b'\n' | b'\r' | b'"' | b'\'' | b'`') {
                        *byte = b' ';
                    }
                }
            }
        }
        captures.advance();
    }
    String::from_utf8(bytes).ok()
}

/// Byte ranges of every comment node in `content`, for callers that must distinguish a marker
/// in a comment from the same text inside a string literal.
#[must_use]
pub(crate) fn comment_ranges(content: &str, file_path: &str) -> Vec<(usize, usize)> {
    let Some(name) = language_for_path(file_path) else {
        return Vec::new();
    };
    let Ok(runtime) = runtime_by_name(name) else {
        return Vec::new();
    };
    let Ok(tree) = parse_with_runtime(&runtime, content.as_bytes(), file_path) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    collect_comment_ranges(&runtime, &tree, content.as_bytes(), &mut ranges);
    ranges
}

fn collect_comment_ranges(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    source: &[u8],
    out: &mut Vec<(usize, usize)>,
) {
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(0..source.len());
    let mut captures = cursor.captures(&runtime.comment_query, tree.root_node(), source);
    captures.advance();
    while let Some((query_match, capture_index)) = captures.get() {
        if let Some(capture) = query_match.captures.get(*capture_index) {
            out.push((capture.node.start_byte(), capture.node.end_byte()));
        }
        captures.advance();
    }
}

fn collect_comments(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    content: &str,
    row_offset: usize,
    file_path: &str,
    out: &mut Vec<(usize, String)>,
) -> Result<(), DetectError> {
    for (row, _kind, text) in
        engine::capture_texts(&runtime.comment_query, tree, content.as_bytes())
    {
        out.push((row + 1 + row_offset, text));
    }
    collect_injected_comments(runtime, tree, content, row_offset, file_path, out)
}

fn collect_injected_comments(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    content: &str,
    row_offset: usize,
    file_path: &str,
    out: &mut Vec<(usize, String)>,
) -> Result<(), DetectError> {
    let Some(query) = &runtime.injection_query else {
        return Ok(());
    };
    for injection in engine::injection_regions(query, tree, content.as_bytes()) {
        let Some(inner) = injected_runtime(&injection.language)? else {
            continue;
        };
        let inner_tree = parse_with_runtime(&inner, injection.content.as_bytes(), file_path)?;
        collect_comments(
            &inner,
            &inner_tree,
            &injection.content,
            injection.row + row_offset,
            file_path,
            out,
        )?;
    }
    Ok(())
}

/// The runtime for an inner language an injection query names, or `None` when the pack has no
/// grammar for it.
///
/// Host grammars inject regions as `comment`/`jsdoc`, which the path resolver never selects:
/// skipping those is not a failure. A supported grammar's own runtime or parse failure is.
fn injected_runtime(name: &str) -> Result<Option<Arc<Runtime>>, DetectError> {
    if tree_sitter_language_pack::get_language(name).is_err() {
        return Ok(None);
    }
    runtime_by_name(name).map(Some)
}

fn commented_out_findings(
    file_path: &str,
    comments: &[(usize, String)],
    old_texts: &HashSet<String>,
) -> Vec<Finding> {
    comments
        .iter()
        .filter(|(_, text)| {
            rules::looks_like_commented_out_test(text)
                && !old_texts.contains(&text.trim().to_lowercase())
        })
        .map(|(line, text)| {
            Finding::new(
                "commented-out-test",
                Confidence::High,
                "Disabled test/assertion code was commented out instead of deleted or fixed.",
                *line,
                file_path,
                text,
            )
        })
        .collect()
}

/// Nested test bodies (a `describe` containing an `it`) can classify the same line twice.
fn dedupe(findings: Vec<Finding>) -> Vec<Finding> {
    engine::dedupe_by(findings, |finding| {
        (
            finding.rule.clone(),
            finding.line_number,
            finding.normalized_text(),
        )
    })
}

fn capture_bodies(
    runtime: &Runtime,
    query: &Query,
    tree: &tree_sitter::Tree,
    source: &[u8],
) -> Vec<Body> {
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source);
    matches.advance();
    let mut bodies = Vec::new();
    while let Some(query_match) = matches.get() {
        let mut test_node: Option<tree_sitter::Node> = None;
        let mut name: Option<String> = None;
        let mut callee: Option<String> = None;
        for capture in query_match.captures {
            match capture_names.get(capture.index as usize).copied() {
                Some("test") => test_node = Some(capture.node),
                Some("test.name") => name = capture.node.utf8_text(source).ok().map(str::to_owned),
                Some("test.callee") => {
                    callee = capture.node.utf8_text(source).ok().map(str::to_owned);
                }
                _ => {}
            }
        }
        if let Some(node) = test_node {
            // The test-body identity is the macro/label callee when present (bats `@test`, Spock
            // `def`), otherwise the declared function name.
            let identifier = callee.as_deref().or(name.as_deref());
            if is_body_candidate(&runtime.name, identifier, node, source) {
                let (start_byte, start_row) = expanded_start(&runtime.name, node, source);
                bodies.push(Body {
                    start_byte,
                    end_byte: expanded_end(&runtime.name, node, source),
                    start_row,
                    name: name.or(callee),
                    reliable: !node.has_error(),
                });
            }
        }
        matches.advance();
    }
    bodies
}

/// Extend a test body upward to include the attribute/marker lines that gate it (Rust
/// `#[test]`/`#[ignore]`, Elixir `@tag :skip`, Groovy `@Ignore`).
fn expanded_start(language: &str, node: tree_sitter::Node, source: &[u8]) -> (usize, usize) {
    if language == "python" {
        // `@pytest.mark.skip` style decorators live in a `decorated_definition` parent,
        // outside the `function_definition` span captured by the test query.
        if let Some(parent) = node.parent() {
            if parent.kind() == "decorated_definition" {
                return (parent.start_byte(), parent.start_position().row);
            }
        }
    }
    if language == "groovy" {
        // A Spock `@Ignore`/`@PendingFeature` line is a sibling `command` holding a `decorate`;
        // the captured body is the method `block`, so anchor the walk at its `command` parent.
        let anchor = node
            .parent()
            .filter(|parent| parent.kind() == "command")
            .unwrap_or(node);
        let mut start_byte = anchor.start_byte();
        let mut start_row = anchor.start_position().row;
        let mut sibling = anchor.prev_sibling();
        while let Some(current) = sibling {
            let is_decorator = current.kind() == "command"
                && current
                    .utf8_text(source)
                    .is_ok_and(|text| text.trim_start().starts_with('@'));
            if !is_decorator {
                break;
            }
            start_byte = current.start_byte();
            start_row = current.start_position().row;
            sibling = current.prev_sibling();
        }
        return (start_byte, start_row);
    }
    let attribute_kinds: &[&str] = match language {
        "rust" => &["attribute_item"],
        "elixir" => &["unary_operator"],
        _ => &[],
    };
    let mut start_byte = node.start_byte();
    let mut start_row = node.start_position().row;
    let mut sibling = node.prev_sibling();
    while let Some(current) = sibling {
        if !attribute_kinds.contains(&current.kind()) {
            break;
        }
        start_byte = current.start_byte();
        start_row = current.start_position().row;
        sibling = current.prev_sibling();
    }
    (start_byte, start_row)
}

/// Extend a test body downward where the grammar splits the body from its declaration: a bats
/// `@test "…" { … }` parses as sibling commands closed by a standalone `}` command.
fn expanded_end(language: &str, node: tree_sitter::Node, source: &[u8]) -> usize {
    if language == "bash" {
        let mut sibling = node.next_sibling();
        while let Some(current) = sibling {
            if current
                .utf8_text(source)
                .is_ok_and(|text| text.trim() == "}")
            {
                return current.end_byte();
            }
            sibling = current.next_sibling();
        }
    }
    node.end_byte()
}

fn is_body_candidate(
    language: &str,
    identifier: Option<&str>,
    node: tree_sitter::Node,
    source: &[u8],
) -> bool {
    match language {
        "python" | "go" | "ocaml" | "elm" => identifier.is_some_and(is_test_function_name),
        "swift" => {
            identifier.is_some_and(|name| name.starts_with("test") || name.starts_with("Test"))
        }
        "rust" => has_rust_test_attribute(node, source),
        "java" => node_text_contains(
            node,
            source,
            &[
                "@Test",
                "@ParameterizedTest",
                "@RepeatedTest",
                "@TestFactory",
            ],
        ),
        "csharp" => node_text_contains(
            node,
            source,
            &["[Test]", "[Fact]", "[Theory]", "[TestMethod]"],
        ),
        "kotlin" => node_text_contains(node, source, &["@Test"]),
        "php" => identifier.is_some_and(|name| {
            name.starts_with("test") || name.starts_with("Test") || is_test_callee(name)
        }),
        "ruby" => identifier.is_some_and(|name| name.starts_with("test_") || is_test_callee(name)),
        "javascript" | "typescript" | "tsx" | "elixir" | "lua" => {
            identifier.is_some_and(is_test_callee)
        }
        "scala" => identifier.is_some_and(|name| name.starts_with("test") || is_test_callee(name)),
        "c" | "cpp" => identifier.is_some_and(|name| {
            matches!(
                name,
                "TEST" | "TEST_F" | "TEST_P" | "TEST_CASE" | "TEST_CASE_METHOD" | "SCENARIO"
            )
        }),
        "groovy" => identifier == Some("def"),
        "bash" => identifier == Some("@test"),
        _ => false,
    }
}

fn node_text_contains(node: tree_sitter::Node, source: &[u8], markers: &[&str]) -> bool {
    node.utf8_text(source)
        .is_ok_and(|text| markers.iter().any(|marker| text.contains(marker)))
}

fn has_rust_test_attribute(node: tree_sitter::Node, source: &[u8]) -> bool {
    let mut sibling = node.prev_sibling();
    while let Some(current) = sibling {
        if current.kind() != "attribute_item" {
            break;
        }
        if let Ok(text) = current.utf8_text(source) {
            if text.contains("#[test]") || text.contains("#[tokio::test]") {
                return true;
            }
        }
        sibling = current.prev_sibling();
    }
    false
}

fn analyze_new_bodies(
    language: &str,
    file_path: &str,
    content: &str,
    view: &str,
    bodies: &[Body],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen = DuplicateSignatures::default();
    for body in bodies {
        let lines = body_added_lines(view, body, language);
        let original_lines = body_added_lines(content, body, language);
        findings.extend(classify_lines(file_path, &lines, &original_lines));
        findings.extend(rules::classify_test_body_with_original(
            file_path,
            body.start_row + 1,
            &lines,
            &original_lines,
            body.reliable,
        ));
        // `duplicate-test` compares the whole body, so adding the same line to two different
        // tests is not mistaken for one test duplicating another. Every captured body is recorded;
        // a `Write` adds every line, so every body is touched and emits its finding.
        let exact_lines = body_raw_lines(content, body);
        if body.reliable
            && let Some(finding) = push_duplicate(
                &mut seen,
                file_path,
                body.start_row + 1,
                &exact_lines,
                &original_lines,
            )
        {
            findings.push(finding);
        }
    }
    let added_test_lines: Vec<usize> = bodies.iter().map(|body| body.start_row + 1).collect();
    if let Some(finding) = rules::test_volume_finding(file_path, &added_test_lines) {
        findings.push(finding);
    }
    findings
}

/// The original content and its masked view, plus the masked view of the pre-edit content.
#[derive(Clone, Copy)]
struct Views<'a> {
    original: &'a str,
    old_original: &'a str,
    old: &'a str,
    masked: &'a str,
}

fn analyze_edited_bodies(
    language: &str,
    file_path: &str,
    bodies: &[Body],
    added: &HashSet<usize>,
    views: Views<'_>,
) -> Vec<Finding> {
    let Views {
        original: content,
        old_original: old_content,
        old: old_view,
        masked: view,
    } = views;
    let mut findings = Vec::new();
    let mut seen = DuplicateSignatures::default();
    let (removed, removed_original) = removed_lines(old_view, view, old_content);
    let diff = DiffContext {
        removed: &removed,
        removed_original: &removed_original,
    };
    for body in bodies {
        let full = body_added_lines(view, body, language);
        let original_lines = body_added_lines(content, body, language);
        let touched: Vec<AddedLine<'_>> = full
            .iter()
            .copied()
            .filter(|line| added.contains(&(line.line_number - 1)))
            .collect();
        let touched_original: Vec<AddedLine<'_>> = original_lines
            .iter()
            .copied()
            .filter(|line| added.contains(&(line.line_number - 1)))
            .collect();
        let is_touched = !touched.is_empty();
        if is_touched {
            findings.extend(classify_diff_lines(
                file_path,
                &touched,
                &touched_original,
                &diff,
            ));
            if let Some(finding) =
                rules::classify_echo_body(file_path, &original_lines, body.reliable, Some(added))
            {
                findings.push(finding);
            }
        }
        // `duplicate-test` compares the whole body, so adding the same line to two different
        // tests is not mistaken for one test duplicating another. Every captured body is
        // recorded, so an unchanged test still serves as the baseline a touched near-duplicate is
        // compared against; only a touched body emits the finding it earns.
        let exact_lines = body_raw_lines(content, body);
        if body.reliable
            && let Some(finding) = push_duplicate(
                &mut seen,
                file_path,
                body.start_row + 1,
                &exact_lines,
                &original_lines,
            )
            && is_touched
        {
            findings.push(finding);
        }
    }
    let added_test_lines: Vec<usize> = bodies
        .iter()
        .filter(|body| added.contains(&body.start_row))
        .map(|body| body.start_row + 1)
        .collect();
    if let Some(finding) = rules::test_volume_finding(file_path, &added_test_lines) {
        findings.push(finding);
    }
    findings
}

/// The removed (LCS-unmatched old) lines of an edit, trimmed for the diff-aware rules.
///
/// `engine::added_lines(from, to)` returns the unmatched line indices of its **second** argument,
/// so the arguments are deliberately reversed here: `added_lines(new, old)` yields the *old* lines
/// the edit removed, indexed against `old_view`. Each removed line is returned twice: the masked
/// view (structure) and the original (literal values), at the same line index.
fn removed_lines<'a>(
    old_view: &'a str,
    new_view: &str,
    old_content: &'a str,
) -> (Vec<&'a str>, Vec<&'a str>) {
    let removed = engine::added_lines(new_view, old_view);
    let mut views = Vec::new();
    let mut originals = Vec::new();
    for (index, (view, original)) in old_view.lines().zip(old_content.lines()).enumerate() {
        if removed.contains(&index) && !rules::is_comment_line(view) {
            views.push(view.trim());
            originals.push(original.trim());
        }
    }
    (views, originals)
}

/// The first line of each earlier body, keyed by its signature: `exact` preserves comments (the
/// `duplicate-test` basis) and `comments` is comment-stripped (the `near-duplicate-test` basis).
#[derive(Debug, Default)]
struct DuplicateSignatures {
    exact: HashMap<String, usize>,
    comments: HashMap<String, usize>,
}

/// `duplicate-test` keys on the exact, comment-preserving body signature, so two tests that differ
/// only by a comment are not exact duplicates; those fall through to `near-duplicate-test`, keyed on
/// the comment-stripped signature. An exact match wins and suppresses the near check.
///
/// The body's signatures are always recorded in `seen`, and the finding it earns is returned for
/// the caller to emit only when the change touched the body: an unchanged test still serves as the
/// baseline a later near-duplicate is compared against without being reported itself.
fn push_duplicate(
    seen: &mut DuplicateSignatures,
    file_path: &str,
    start_line: usize,
    exact_lines: &[AddedLine<'_>],
    comment_lines: &[AddedLine<'_>],
) -> Option<Finding> {
    let signature = body_signature(exact_lines);
    if signature.is_empty() {
        return None;
    }
    if let Some(first_line) = seen.exact.get(&signature) {
        return Some(Finding::new(
            "duplicate-test",
            Confidence::Medium,
            format!("This test duplicates another test already defined at line {first_line}."),
            start_line,
            file_path,
            "",
        ));
    }
    seen.exact.insert(signature, start_line);

    let comment_signature = body_signature(comment_lines);
    if comment_signature.is_empty() {
        return None;
    }
    if let Some(first_line) = seen.comments.get(&comment_signature) {
        return Some(Finding::new(
            "near-duplicate-test",
            Confidence::Low,
            format!("This test is identical to the test at line {first_line} except for comments."),
            start_line,
            file_path,
            "",
        ));
    }
    seen.comments.insert(comment_signature, start_line);
    None
}

/// The raw body lines with their comments intact (`body_signature` still trims whitespace). This is
/// the exact `duplicate-test` basis: a body that differs from another only by a comment line is not
/// an exact duplicate, so it is reported as `near-duplicate-test` instead.
fn body_raw_lines<'a>(content: &'a str, body: &Body) -> Vec<AddedLine<'a>> {
    let Some(text) = content.get(body.start_byte..body.end_byte) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .map(|(index, line)| AddedLine {
            line_number: body.start_row + 1 + index,
            text: line,
        })
        .collect()
}

fn body_added_lines<'a>(content: &'a str, body: &Body, language: &str) -> Vec<AddedLine<'a>> {
    let Some(text) = content.get(body.start_byte..body.end_byte) else {
        return Vec::new();
    };
    let mut in_block_comment = false;
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            let code = strip_block_comment(line, &mut in_block_comment).unwrap_or("");
            AddedLine {
                line_number: body.start_row + 1 + index,
                text: strip_line_comment(code, language),
            }
        })
        .collect()
}

/// The code portion of a body line, tracking `/* … */` state across lines: a line wholly inside a
/// block comment yields `None`, while code after a `*/` on the closing line is kept.
fn strip_block_comment<'a>(line: &'a str, in_block_comment: &mut bool) -> Option<&'a str> {
    if *in_block_comment {
        let (_, rest) = line.split_once("*/")?;
        *in_block_comment = false;
        Some(rest)
    } else if let Some(rest) = line.trim_start().strip_prefix("/*") {
        if let Some((_, after)) = rest.split_once("*/") {
            Some(after)
        } else {
            *in_block_comment = true;
            None
        }
    } else if rules::is_comment_line(line) {
        None
    } else {
        Some(line)
    }
}

fn classify_lines(
    file_path: &str,
    lines: &[AddedLine<'_>],
    original_lines: &[AddedLine<'_>],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for index in 0..lines.len() {
        let original = original_lines.get(index).map_or("", |line| line.text);
        findings.extend(rules::classify_added_line_with_original(
            file_path, lines, index, original,
        ));
    }
    findings
}

fn classify_diff_lines(
    file_path: &str,
    lines: &[AddedLine<'_>],
    original_lines: &[AddedLine<'_>],
    diff: &DiffContext<'_>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for index in 0..lines.len() {
        let original = original_lines.get(index).map_or("", |line| line.text);
        findings.extend(rules::classify_diff_line_with_original(
            file_path, lines, index, original, diff,
        ));
    }
    findings
}

fn body_signature(lines: &[AddedLine<'_>]) -> String {
    // Skip the declaration line so two tests that differ only by name are still duplicates; case is
    // preserved so tests that differ only by a literal's case (a real distinction) are not.
    lines
        .iter()
        .skip(1)
        .map(|line| line.text.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// New (added) line indices in `new_content`, computed with an LCS over trimmed lines so a line
/// whose text happens to appear elsewhere in the old content is not dropped.
fn line_comment_token(language: &str) -> Option<&'static str> {
    match language {
        "python" | "ruby" | "bash" | "dockerfile" | "elixir" | "hcl" | "toml" | "yaml" | "cue" => {
            Some("#")
        }
        "lua" | "sql" | "elm" => Some("--"),
        "javascript" | "typescript" | "tsx" | "java" | "go" | "rust" | "c" | "cpp" | "csharp"
        | "kotlin" | "scala" | "swift" | "groovy" | "php" | "proto" | "css" | "svelte" => {
            Some("//")
        }
        _ => None,
    }
}

fn strip_line_comment<'a>(line: &'a str, language: &str) -> &'a str {
    let Some(token) = line_comment_token(language) else {
        return line;
    };
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = false;
            }
            continue;
        }
        if character == '"' || character == '\'' || character == '`' {
            in_string = true;
            quote = character;
            continue;
        }
        if line[index..].starts_with(token) {
            return &line[..index];
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{MASKED_VIEW_COUNTERS, TestDetector, WorkCounts, strip_line_comment};

    const TYPICAL_BYTES: usize = 1_500;
    const LARGE_BYTES: usize = 200_000;

    /// A zero-cost filler so each literal is long enough that a 200 KB payload has a shallow
    /// parse tree; tree-sitter's debug-build parse stays inside the shared 50 ms budget while the
    /// literal count still grows with the payload.
    fn filler(index: usize, length: usize) -> String {
        let mut text = format!("assert {index} ");
        text.push_str(&"x".repeat(length.saturating_sub(text.len())));
        text
    }

    /// A TypeScript test carrying a plain string, a template literal, and a raw template string
    /// per iteration, grown until `target` bytes.
    fn typescript_payload(target: usize) -> String {
        let mut payload = String::with_capacity(target + 256);
        payload.push_str("it('bulk', () => {\n");
        let literal_len = if target >= LARGE_BYTES { 1_000 } else { 96 };
        let mut index = 0;
        while payload.len() < target {
            let text = filler(index, literal_len);
            write!(
                payload,
                "  const a{index} = \"{text}\";\n  const b{index} = `{text}`;\n  const c{index} = String.raw`{text}`;\n"
            )
            .expect("writing to a String cannot fail");
            index += 1;
        }
        payload.push_str("  expect(add(1, 2)).toBe(3);\n});\n");
        payload
    }

    /// A Rust test carrying a plain string, a raw string, and a byte string per iteration, grown
    /// until `target` bytes.
    fn rust_payload(target: usize) -> String {
        let mut payload = String::with_capacity(target + 256);
        payload.push_str("#[test]\nfn bulk() {\n");
        let literal_len = if target >= LARGE_BYTES { 1_000 } else { 96 };
        let mut index = 0;
        while payload.len() < target {
            let text = filler(index, literal_len);
            write!(
                payload,
                "    let a{index} = \"{text}\";\n    let b{index} = r#\"{text}\"#;\n    let c{index} = b\"{text}\";\n"
            )
            .expect("writing to a String cannot fail");
            index += 1;
        }
        payload.push_str("    assert_eq!(add(1, 2), 3);\n}\n");
        payload
    }

    /// Run one `detect` call with the counters reset, returning the work it performed.
    fn measure(path: &str, payload: &str) -> WorkCounts {
        MASKED_VIEW_COUNTERS.reset();
        let _ = TestDetector::new().detect(payload, path, true, true);
        MASKED_VIEW_COUNTERS.snapshot()
    }

    /// The masked view's work bound.
    ///
    /// One `detect` call builds the view exactly once (never per rule), copies the input at most
    /// twice (the implementation is one length-preserving pass, so the real count is exactly one
    /// input), and visits at most one string-literal node per input byte. The 200 KB/typical node
    /// ratio is capped at 300x: a ~1.8 KB typical payload grows ~110x in bytes to 200 KB, so 300x
    /// keeps ~2.7x headroom for a denser string layout while rejecting superlinear rescans (a
    /// per-rule rebuild would also break the construction and byte bounds).
    #[test]
    fn masked_view_work_is_bounded_and_linear() {
        const MAX_NODE_RATIO: usize = 300;

        let ts_typical = typescript_payload(TYPICAL_BYTES);
        let ts_large = typescript_payload(LARGE_BYTES);
        let rust_typical = rust_payload(TYPICAL_BYTES);
        let rust_large = rust_payload(LARGE_BYTES);

        let ts_typical_counts = measure("tests/app.test.ts", &ts_typical);
        let ts_large_counts = measure("tests/app.test.ts", &ts_large);
        let rust_typical_counts = measure("tests/app_test.rs", &rust_typical);
        let rust_large_counts = measure("tests/app_test.rs", &rust_large);

        eprintln!(
            "masked view work: TS typical {} bytes {ts_typical_counts:?}; TS 200KB {} bytes {ts_large_counts:?}; Rust typical {} bytes {rust_typical_counts:?}; Rust 200KB {} bytes {rust_large_counts:?}",
            ts_typical.len(),
            ts_large.len(),
            rust_typical.len(),
            rust_large.len(),
        );

        for (label, counts, input) in [
            ("TS typical", ts_typical_counts, ts_typical.len()),
            ("TS 200KB", ts_large_counts, ts_large.len()),
            ("Rust typical", rust_typical_counts, rust_typical.len()),
            ("Rust 200KB", rust_large_counts, rust_large.len()),
        ] {
            assert_eq!(
                counts.constructions, 1,
                "{label}: the masked view must be built exactly once per detect call"
            );
            assert!(
                counts.nodes_visited > 0,
                "{label}: the payload must contain string literals"
            );
            assert!(
                counts.bytes_copied <= 2 * input,
                "{label}: bytes copied {} exceeds 2x the {input}-byte input",
                counts.bytes_copied
            );
            assert!(
                counts.nodes_visited <= input,
                "{label}: visited {} string nodes for {input} input bytes",
                counts.nodes_visited
            );
        }

        assert!(
            ts_large_counts.nodes_visited <= MAX_NODE_RATIO * ts_typical_counts.nodes_visited,
            "TS node growth is superlinear: {ts_typical_counts:?} -> {ts_large_counts:?}"
        );
        assert!(
            rust_large_counts.nodes_visited <= MAX_NODE_RATIO * rust_typical_counts.nodes_visited,
            "Rust node growth is superlinear: {rust_typical_counts:?} -> {rust_large_counts:?}"
        );
    }

    #[test]
    fn strip_line_comment_given_trailing_comment_removes_it() {
        // then
        assert_eq!(
            strip_line_comment("assert x == y  # note", "python"),
            "assert x == y  "
        );
        assert_eq!(
            strip_line_comment("expect(a).toBe(b); // note", "javascript"),
            "expect(a).toBe(b); "
        );
        assert_eq!(strip_line_comment("x = 1 -- lua comment", "lua"), "x = 1 ");
    }

    #[test]
    fn strip_line_comment_given_hash_inside_string_keeps_it() {
        // then
        assert_eq!(
            strip_line_comment("assert label == \"#1\"", "python"),
            "assert label == \"#1\""
        );
    }

    #[test]
    fn strip_line_comment_given_javascript_private_field_keeps_it() {
        // then
        assert_eq!(
            strip_line_comment("#count = 0;", "javascript"),
            "#count = 0;"
        );
    }
}
