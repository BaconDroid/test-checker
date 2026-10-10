//! AST capture for the test-weakening detector.
//!
//! This module owns tree-sitter parsing and test-body extraction only; the
//! 11 rule classifiers live in [`crate::rules`]. Only added lines are
//! candidates, comments are stripped, the `is_test_file` gate applies, and
//! every failure path degrades to no findings (fail-open).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::filters;
use crate::language::{LanguageSpec, language_for_path, plugin_language};
use crate::models::Finding;
use crate::rules::{self, Derived};

/// A parsed grammar plus its test-body query, built once per language.
struct Runtime {
    language: tree_sitter::Language,
    test_query: Query,
}

static RUNTIMES: LazyLock<Mutex<HashMap<&'static str, Arc<Runtime>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
    static PARSERS: RefCell<HashMap<&'static str, Parser>> = RefCell::new(HashMap::new());
}

/// One changed file, as the classifiers see it.
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

#[derive(Debug, Default)]
pub struct TestDetector;

impl TestDetector {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Gates, parses, captures test blocks, then runs the rule catalog.
    #[must_use]
    pub fn detect(&self, change: &Change) -> Vec<Finding> {
        if !change.is_test_file || filters::is_file_disabled(&change.new_text) {
            return Vec::new();
        }
        let Some(spec) = language_for_path(&change.file_path) else {
            return Vec::new();
        };
        let Some(runtime) = runtime_for(spec) else {
            return Vec::new();
        };
        let source = change.new_text.as_bytes();
        let Some(tree) = parse_with_runtime(&runtime, spec.name, source) else {
            return Vec::new();
        };
        let blocks = find_test_blocks(&runtime, &tree, source, spec);
        let derived = Derived::new(change, blocks);
        dedupe_sorted(rules::evaluate(&derived))
    }
}

// --- Runtime cache ---------------------------------------------------------

fn runtime_for(spec: LanguageSpec) -> Option<Arc<Runtime>> {
    let Ok(mut runtimes) = RUNTIMES.lock() else {
        return None;
    };
    if let Some(runtime) = runtimes.get(spec.name) {
        return Some(Arc::clone(runtime));
    }

    let language = tree_sitter_language_pack::get_language(spec.name).ok()?;
    let test_query = Query::new(&language, spec.test_query).ok()?;
    let runtime = Arc::new(Runtime {
        language,
        test_query,
    });
    runtimes.insert(spec.name, Arc::clone(&runtime));
    Some(runtime)
}

fn parse_with_runtime(
    runtime: &Runtime,
    language_name: &'static str,
    source: &[u8],
) -> Option<tree_sitter::Tree> {
    PARSERS.with(|parsers| {
        let mut parsers = parsers.borrow_mut();
        if let Some(parser) = parsers.get_mut(language_name) {
            parser.parse(source, None)
        } else {
            let mut parser = Parser::new();
            if parser.set_language(&runtime.language).is_err() {
                return None;
            }
            let tree = parser.parse(source, None);
            parsers.insert(language_name, parser);
            tree
        }
    })
}

// --- Test block extraction -------------------------------------------------

fn find_test_blocks(
    runtime: &Runtime,
    tree: &tree_sitter::Tree,
    source: &[u8],
    spec: LanguageSpec,
) -> Vec<TestBlock> {
    let names = runtime.test_query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut blocks = Vec::new();
    let plugin = plugin_language(spec.name);
    let mut matches = cursor.matches(&runtime.test_query, tree.root_node(), source);
    while let Some(query_match) = matches.next() {
        let mut name_node: Option<Node<'_>> = None;
        let mut body_node: Option<Node<'_>> = None;
        let mut call_node: Option<Node<'_>> = None;
        for capture in query_match.captures {
            match names.get(capture.index as usize).copied() {
                Some("test.name") => name_node = Some(capture.node),
                Some("test.body") => body_node = Some(capture.node),
                Some("test.call") => call_node = Some(capture.node),
                _ => {}
            }
        }
        match plugin {
            "python" => {
                if let (Some(name), Some(body)) = (name_node, body_node) {
                    if node_text(name, source).starts_with("test_") {
                        blocks.push(make_block(name, body, source, false));
                    }
                }
            }
            "go" => {
                if let (Some(name), Some(body)) = (name_node, body_node) {
                    if node_text(name, source).starts_with("Test") {
                        blocks.push(make_block(body, body, source, true));
                    }
                }
            }
            "rust" => {
                if let (Some(name), Some(body)) = (name_node, body_node) {
                    if let Some(function) = name.parent() {
                        if rust_has_attribute(function, source, "test") {
                            blocks.push(make_block(body, body, source, true));
                        }
                    }
                }
            }
            "js" | "ts" => {
                if let Some(call) = call_node {
                    collect_js(call, source, plugin, &mut blocks);
                }
            }
            _ => {}
        }
    }
    blocks
}

fn collect_js(call: Node<'_>, source: &[u8], plugin: &str, out: &mut Vec<TestBlock>) {
    let Some(function) = call.child_by_field_name("function") else {
        return;
    };
    let function_text = format!("{}(", node_text(function, source));
    if !filters::is_test_declaration(&function_text, plugin) {
        return;
    }
    let Some(callback) = js_callback(call) else {
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

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

// --- Ordering --------------------------------------------------------------

fn dedupe_sorted(mut findings: Vec<Finding>) -> Vec<Finding> {
    findings.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.rule.cmp(right.rule))
            .then_with(|| left.message.cmp(&right.message))
    });
    findings.dedup_by(|left, right| {
        left.line == right.line && left.rule == right.rule && left.message == right.message
    });
    findings
}
