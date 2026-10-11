use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use tree_sitter::{ParseOptions, ParseState, Parser, Query, QueryCursor, StreamingIterator};

use crate::language::{
    comment_query, is_test_callee, is_test_function_name, language_for_path, test_query,
};
use crate::models::{Confidence, Finding};
use crate::rules::{self, AddedLine};

const PARSE_TIME_BUDGET: Duration = Duration::from_millis(50);

struct Runtime {
    name: String,
    language: tree_sitter::Language,
    comment_query: Query,
    test_query: Option<Query>,
    injection_query: Option<Query>,
}

static RUNTIMES: LazyLock<Mutex<HashMap<String, Arc<Runtime>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
    static PARSERS: RefCell<HashMap<String, Parser>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, Debug)]
struct Body {
    start_byte: usize,
    end_byte: usize,
    start_row: usize,
}

#[derive(Debug, Default)]
pub struct TestDetector;

impl TestDetector {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Analyze a whole file's content (`Write`): every line is treated as newly added.
    #[must_use]
    pub fn detect(
        &self,
        content: &str,
        file_path: &str,
        include_commented_out_tests: bool,
        include_body_tests: bool,
    ) -> Vec<Finding> {
        let Some(name) = language_for_path(file_path) else {
            return Vec::new();
        };
        let Some(runtime) = runtime_by_name(name) else {
            return Vec::new();
        };
        let source = content.as_bytes();
        let Some(tree) = parse_with_runtime(&runtime, source) else {
            return Vec::new();
        };

        let mut findings = Vec::new();
        if include_commented_out_tests {
            let mut comments = Vec::new();
            collect_comments(&runtime, &tree, content, 0, &mut comments);
            findings.extend(commented_out_findings(
                file_path,
                &comments,
                &HashSet::new(),
            ));
        }
        if include_body_tests {
            if let Some(query) = &runtime.test_query {
                let bodies = capture_bodies(&runtime, query, &tree, source);
                findings.extend(analyze_new_bodies(file_path, content, &bodies));
            }
        }
        findings
    }

    /// Analyze an edit: report only weakening newly introduced by `new_string`.
    #[must_use]
    pub fn detect_edit(
        &self,
        old_string: &str,
        new_string: &str,
        file_path: &str,
        include_commented_out_tests: bool,
        include_body_tests: bool,
    ) -> Vec<Finding> {
        let Some(name) = language_for_path(file_path) else {
            return Vec::new();
        };
        let Some(runtime) = runtime_by_name(name) else {
            return Vec::new();
        };
        let source = new_string.as_bytes();
        let Some(tree) = parse_with_runtime(&runtime, source) else {
            return Vec::new();
        };

        let mut findings = Vec::new();
        if include_commented_out_tests {
            let mut old_comments = Vec::new();
            if let Some(old_tree) = parse_with_runtime(&runtime, old_string.as_bytes()) {
                collect_comments(&runtime, &old_tree, old_string, 0, &mut old_comments);
            }
            let old_texts: HashSet<String> = old_comments
                .iter()
                .map(|(_, text)| normalized_comment(text))
                .collect();
            let mut new_comments = Vec::new();
            collect_comments(&runtime, &tree, new_string, 0, &mut new_comments);
            findings.extend(commented_out_findings(file_path, &new_comments, &old_texts));
        }
        if include_body_tests {
            findings.extend(rules::diff_findings(old_string, new_string, file_path));
            if let Some(query) = &runtime.test_query {
                let bodies = capture_bodies(&runtime, query, &tree, source);
                let added = added_lines(old_string, new_string);
                findings.extend(analyze_edited_bodies(
                    file_path, new_string, &bodies, &added,
                ));
            }
        }
        findings
    }
}

fn runtime_by_name(name: &str) -> Option<Arc<Runtime>> {
    let Ok(mut runtimes) = RUNTIMES.lock() else {
        return None;
    };
    if let Some(runtime) = runtimes.get(name) {
        return Some(Arc::clone(runtime));
    }

    let language = tree_sitter_language_pack::get_language(name).ok()?;
    let comment = Query::new(&language, comment_query(name)).ok()?;
    let body = test_query(name).and_then(|source| Query::new(&language, source).ok());
    let injection = tree_sitter_language_pack::get_injections_query(name)
        .and_then(|source| Query::new(&language, source).ok());
    let runtime = Arc::new(Runtime {
        name: name.to_owned(),
        language,
        comment_query: comment,
        test_query: body,
        injection_query: injection,
    });
    runtimes.insert(name.to_owned(), Arc::clone(&runtime));
    Some(runtime)
}

/// Whether the language has a compiling test-body query (i.e. can yield body-based findings).
#[must_use]
pub fn supports_test_bodies(name: &str) -> bool {
    runtime_by_name(name).is_some_and(|runtime| runtime.test_query.is_some())
}

fn parse_with_runtime(runtime: &Runtime, source: &[u8]) -> Option<tree_sitter::Tree> {
    PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        if let Some(parser) = parsers.get_mut(&runtime.name) {
            parse_with_budget(parser, source)
        } else {
            let mut parser = Parser::new();
            if parser.set_language(&runtime.language).is_err() {
                return None;
            }
            let tree = parse_with_budget(&mut parser, source);
            parsers.insert(runtime.name.clone(), parser);
            tree
        }
    })
}

fn parse_with_budget(parser: &mut Parser, source: &[u8]) -> Option<tree_sitter::Tree> {
    let start = Instant::now();
    let mut progress = |_state: &ParseState| {
        if start.elapsed() > PARSE_TIME_BUDGET {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = ParseOptions::new().progress_callback(&mut progress);
    let len = source.len();
    let mut read = |offset: usize, _: tree_sitter::Point| {
        if offset < len {
            &source[offset..]
        } else {
            Default::default()
        }
    };
    parser.parse_with_options(&mut read, None, Some(options))
}

fn collect_comments(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    content: &str,
    row_offset: usize,
    out: &mut Vec<(usize, String)>,
) {
    let source = content.as_bytes();
    let root = tree.root_node();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(0..source.len());
    let mut captures = cursor.captures(&runtime.comment_query, root, source);
    captures.advance();
    while let Some((query_match, capture_index)) = captures.get() {
        if let Some(capture) = query_match.captures.get(*capture_index) {
            if let Ok(text) = capture.node.utf8_text(source) {
                out.push((
                    capture.node.start_position().row + 1 + row_offset,
                    text.to_string(),
                ));
            }
        }
        captures.advance();
    }
    collect_injected_comments(runtime, tree, content, row_offset, out);
}

fn collect_injected_comments(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    content: &str,
    row_offset: usize,
    out: &mut Vec<(usize, String)>,
) {
    let Some(query) = &runtime.injection_query else {
        return;
    };
    let source = content.as_bytes();
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(0..source.len());
    let mut matches = cursor.matches(query, tree.root_node(), source);
    matches.advance();
    while let Some(query_match) = matches.get() {
        let mut injected_content: Option<tree_sitter::Node> = None;
        let mut injected_language: Option<&str> = None;
        for setting in query.property_settings(query_match.pattern_index) {
            if setting.key.as_ref() == "injection.language" {
                injected_language = setting.value.as_deref();
            }
        }
        for capture in query_match.captures {
            match capture_names.get(capture.index as usize).copied() {
                Some("injection.content") => injected_content = Some(capture.node),
                Some("injection.language") => {
                    injected_language = capture.node.utf8_text(source).ok();
                }
                _ => {}
            }
        }
        if let (Some(node), Some(injected)) = (injected_content, injected_language) {
            if let (Ok(text), Some(inner)) =
                (node.utf8_text(source), runtime_by_name(injected.trim()))
            {
                if let Some(inner_tree) = parse_with_runtime(&inner, text.as_bytes()) {
                    collect_comments(
                        &inner,
                        &inner_tree,
                        text,
                        node.start_position().row + row_offset,
                        out,
                    );
                }
            }
        }
        matches.advance();
    }
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
                && !old_texts.contains(&normalized_comment(text))
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

fn normalized_comment(text: &str) -> String {
    text.trim().to_lowercase()
}

fn capture_bodies(
    runtime: &Runtime,
    query: &Query,
    tree: &tree_sitter::Tree,
    source: &[u8],
) -> Vec<Body> {
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(0..source.len());
    let mut matches = cursor.matches(query, tree.root_node(), source);
    matches.advance();
    let mut bodies = Vec::new();
    while let Some(query_match) = matches.get() {
        let mut test_node: Option<tree_sitter::Node> = None;
        let mut identifier: Option<String> = None;
        for capture in query_match.captures {
            match capture_names.get(capture.index as usize).copied() {
                Some("test") => test_node = Some(capture.node),
                Some("test.name" | "test.callee") => {
                    identifier = capture.node.utf8_text(source).ok().map(str::to_owned);
                }
                _ => {}
            }
        }
        if let Some(node) = test_node {
            if is_body_candidate(&runtime.name, identifier.as_deref(), node, source) {
                let (start_byte, start_row) = expanded_start(&runtime.name, node);
                bodies.push(Body {
                    start_byte,
                    end_byte: node.end_byte(),
                    start_row,
                });
            }
        }
        matches.advance();
    }
    bodies
}

/// Extend a test body upward to include the attribute/marker lines that gate it (Rust
/// `#[test]`/`#[ignore]`, Elixir `@tag :skip`).
fn expanded_start(language: &str, node: tree_sitter::Node) -> (usize, usize) {
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

fn is_body_candidate(
    language: &str,
    identifier: Option<&str>,
    node: tree_sitter::Node,
    source: &[u8],
) -> bool {
    match language {
        "python" | "go" => identifier.is_some_and(is_test_function_name),
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

fn analyze_new_bodies(file_path: &str, content: &str, bodies: &[Body]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for body in bodies {
        let lines = body_added_lines(content, body, None);
        findings.extend(classify_lines(file_path, &lines));
        findings.extend(rules::classify_test_body(
            file_path,
            body.start_row + 1,
            &lines,
        ));
        let signature = body_signature(&lines);
        if !signature.is_empty() {
            if let Some(first_line) = seen.get(&signature) {
                findings.push(Finding::new(
                    "duplicate-test",
                    Confidence::Medium,
                    format!(
                        "This test duplicates another test already defined at line {first_line}."
                    ),
                    body.start_row + 1,
                    file_path,
                    "",
                ));
            } else {
                seen.insert(signature, body.start_row + 1);
            }
        }
    }
    findings
}

fn analyze_edited_bodies(
    file_path: &str,
    content: &str,
    bodies: &[Body],
    added: &HashSet<usize>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for body in bodies {
        let lines = body_added_lines(content, body, Some(added));
        if lines.is_empty() {
            continue;
        }
        findings.extend(classify_lines(file_path, &lines));
        let signature = body_signature(&lines);
        if !signature.is_empty() {
            if let Some(first_line) = seen.get(&signature) {
                findings.push(Finding::new(
                    "duplicate-test",
                    Confidence::Medium,
                    format!(
                        "This test duplicates another test already defined at line {first_line}."
                    ),
                    body.start_row + 1,
                    file_path,
                    "",
                ));
            } else {
                seen.insert(signature, body.start_row + 1);
            }
        }
    }
    findings
}

fn body_added_lines<'a>(
    content: &'a str,
    body: &Body,
    added: Option<&HashSet<usize>>,
) -> Vec<AddedLine<'a>> {
    let Some(text) = content.get(body.start_byte..body.end_byte) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .filter(|(index, _)| added.is_none_or(|set| set.contains(&(body.start_row + index))))
        .map(|(index, line)| AddedLine {
            line_number: body.start_row + 1 + index,
            text: strip_line_comment(line),
        })
        .collect()
}

fn classify_lines(file_path: &str, lines: &[AddedLine<'_>]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for index in 0..lines.len() {
        findings.extend(rules::classify_added_line(file_path, lines, index));
    }
    findings
}

fn body_signature(lines: &[AddedLine<'_>]) -> String {
    lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|line| !line.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("\n")
}

fn added_lines(old_content: &str, new_content: &str) -> HashSet<usize> {
    let old: HashSet<String> = old_content
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    new_content
        .lines()
        .enumerate()
        .filter(|(_, line)| !old.contains(line.trim()))
        .map(|(index, _)| index)
        .collect()
}

fn strip_line_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut previous_is_space = true;
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
        match character {
            '"' | '\'' => {
                in_string = true;
                quote = character;
                previous_is_space = false;
            }
            '#' if previous_is_space => {
                let rest = &line[index + 1..];
                if !rest.starts_with('[') && !rest.starts_with('!') {
                    return &line[..index];
                }
                previous_is_space = false;
            }
            '/' if line[index..].starts_with("//") => return &line[..index],
            _ => previous_is_space = character.is_whitespace(),
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::strip_line_comment;

    #[test]
    fn strip_line_comment_given_trailing_comment_removes_it() {
        // then
        assert_eq!(
            strip_line_comment("assert x == y  # not a comment marker"),
            "assert x == y  "
        );
        assert_eq!(
            strip_line_comment("expect(a).toBe(b); // note"),
            "expect(a).toBe(b); "
        );
    }

    #[test]
    fn strip_line_comment_given_hash_inside_string_keeps_it() {
        // then
        assert_eq!(
            strip_line_comment("assert label == \"#1\""),
            "assert label == \"#1\""
        );
    }
}
