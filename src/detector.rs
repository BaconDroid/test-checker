use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use tree_sitter::{Query, QueryCursor, StreamingIterator};

use crate::engine;
use crate::language::{is_test_callee, is_test_function_name, language_for_path, test_query};
use crate::models::{Confidence, Finding};
use crate::rules::{self, AddedLine, DiffContext};

struct Runtime {
    name: String,
    language: tree_sitter::Language,
    comment_query: Query,
    test_query: Option<Query>,
    injection_query: Option<Query>,
}

static RUNTIMES: LazyLock<Mutex<HashMap<String, Arc<Runtime>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
struct Body {
    start_byte: usize,
    end_byte: usize,
    start_row: usize,
    name: Option<String>,
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
                findings.extend(analyze_new_bodies(
                    &runtime.name,
                    file_path,
                    content,
                    &bodies,
                ));
            }
        }
        dedupe(findings)
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
        // Parse the old fragment once: it feeds both comment capture and old-body capture.
        let old_tree = if old_string.is_empty() {
            None
        } else {
            parse_with_runtime(&runtime, old_string.as_bytes())
        };

        let mut findings = Vec::new();
        if include_commented_out_tests {
            let mut old_comments = Vec::new();
            if let Some(old_tree) = &old_tree {
                collect_comments(&runtime, old_tree, old_string, 0, &mut old_comments);
            }
            let old_texts: HashSet<String> = old_comments
                .iter()
                .map(|(_, text)| text.trim().to_lowercase())
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
                    &runtime.name,
                    file_path,
                    old_string,
                    new_string,
                    &bodies,
                    &added,
                ));
                if let Some(old_tree) = &old_tree {
                    let old_bodies =
                        capture_bodies(&runtime, query, old_tree, old_string.as_bytes());
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
        dedupe(findings)
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
                "A test present before this edit is gone; deleting a test is a strong tampering signal.",
                1,
                file_path,
                old_body.name.as_deref().unwrap_or(""),
            ));
        }
    }
    findings
}

fn runtime_by_name(name: &str) -> Option<Arc<Runtime>> {
    let Ok(mut runtimes) = RUNTIMES.lock() else {
        return None;
    };
    if let Some(runtime) = runtimes.get(name) {
        return Some(Arc::clone(runtime));
    }

    let language = tree_sitter_language_pack::get_language(name).ok()?;
    let comment = Query::new(&language, engine::comment_query(name)).ok()?;
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

fn parse_with_runtime(runtime: &Runtime, source: &[u8]) -> Option<tree_sitter::Tree> {
    engine::parse_cached(&runtime.name, &runtime.language, source)
}

fn collect_comments(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    content: &str,
    row_offset: usize,
    out: &mut Vec<(usize, String)>,
) {
    for (row, _kind, text) in
        engine::capture_texts(&runtime.comment_query, tree, content.as_bytes())
    {
        out.push((row + 1 + row_offset, text));
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
    for injection in engine::injection_regions(query, tree, content.as_bytes()) {
        let Some(inner) = runtime_by_name(&injection.language) else {
            continue;
        };
        let Some(inner_tree) =
            engine::parse_cached(&inner.name, &inner.language, injection.content.as_bytes())
        else {
            continue;
        };
        collect_comments(
            &inner,
            &inner_tree,
            &injection.content,
            injection.row + row_offset,
            out,
        );
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
    cursor.set_byte_range(0..source.len());
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
    bodies: &[Body],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for body in bodies {
        let lines = body_added_lines(content, body, language);
        findings.extend(classify_lines(file_path, &lines));
        findings.extend(rules::classify_test_body(
            file_path,
            body.start_row + 1,
            &lines,
        ));
        push_duplicate(
            &mut findings,
            &mut seen,
            file_path,
            body.start_row + 1,
            &lines,
        );
    }
    findings
}

fn analyze_edited_bodies(
    language: &str,
    file_path: &str,
    old_content: &str,
    content: &str,
    bodies: &[Body],
    added: &HashSet<usize>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let removed = removed_lines(old_content, content);
    let diff = DiffContext { removed: &removed };
    for body in bodies {
        let full = body_added_lines(content, body, language);
        let touched: Vec<AddedLine<'_>> = full
            .iter()
            .copied()
            .filter(|line| added.contains(&(line.line_number - 1)))
            .collect();
        if touched.is_empty() {
            continue;
        }
        findings.extend(classify_diff_lines(file_path, &touched, &diff));
        // `duplicate-test` compares the whole body, so adding the same line to two different
        // tests is not mistaken for one test duplicating another.
        push_duplicate(
            &mut findings,
            &mut seen,
            file_path,
            body.start_row + 1,
            &full,
        );
    }
    findings
}

/// The removed (LCS-unmatched old) lines of an edit, trimmed for the diff-aware rules.
///
/// `engine::added_lines(from, to)` returns the unmatched line indices of its **second** argument,
/// so the arguments are deliberately reversed here: `added_lines(new, old)` yields the *old* lines
/// the edit removed, indexed against `old_content`.
fn removed_lines<'a>(old_content: &'a str, new_content: &str) -> Vec<&'a str> {
    let removed = engine::added_lines(new_content, old_content);
    old_content
        .lines()
        .enumerate()
        .filter(|(index, _)| removed.contains(index))
        .map(|(_, line)| line.trim())
        .collect()
}

fn push_duplicate(
    findings: &mut Vec<Finding>,
    seen: &mut HashMap<String, usize>,
    file_path: &str,
    start_line: usize,
    lines: &[AddedLine<'_>],
) {
    let signature = body_signature(lines);
    if signature.is_empty() {
        return;
    }
    if let Some(first_line) = seen.get(&signature) {
        findings.push(Finding::new(
            "duplicate-test",
            Confidence::Medium,
            format!("This test duplicates another test already defined at line {first_line}."),
            start_line,
            file_path,
            "",
        ));
    } else {
        seen.insert(signature, start_line);
    }
}

fn body_added_lines<'a>(content: &'a str, body: &Body, language: &str) -> Vec<AddedLine<'a>> {
    let Some(text) = content.get(body.start_byte..body.end_byte) else {
        return Vec::new();
    };
    text.lines()
        .enumerate()
        .map(|(index, line)| AddedLine {
            line_number: body.start_row + 1 + index,
            text: strip_line_comment(line, language),
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

fn classify_diff_lines(
    file_path: &str,
    lines: &[AddedLine<'_>],
    diff: &DiffContext<'_>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for index in 0..lines.len() {
        findings.extend(rules::classify_diff_line(file_path, lines, index, diff));
    }
    findings
}

fn body_signature(lines: &[AddedLine<'_>]) -> String {
    // Skip the declaration line so two tests that differ only by name are still duplicates.
    lines
        .iter()
        .skip(1)
        .map(|line| line.text.trim())
        .filter(|line| !line.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("\n")
}

/// New (added) line indices in `new_content`, computed with an LCS over trimmed lines so a line
/// whose text happens to appear elsewhere in the old content is not dropped.
fn added_lines(old_content: &str, new_content: &str) -> HashSet<usize> {
    engine::added_lines(old_content, new_content)
}

fn line_comment_token(language: &str) -> Option<&'static str> {
    match language {
        "python" | "ruby" | "bash" | "dockerfile" | "elixir" | "hcl" | "toml" | "yaml" | "cue"
        | "perl" => Some("#"),
        "lua" | "sql" | "elm" | "haskell" => Some("--"),
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
    use super::strip_line_comment;

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
