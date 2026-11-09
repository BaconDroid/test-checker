#[must_use]
pub fn language_for_path(file_path: &str) -> Option<&'static str> {
    // The language pack maps bash to `.sh`/`.bash`; bats test files use `.bats` for the same
    // grammar, so resolve them explicitly.
    if file_path.to_ascii_lowercase().ends_with(".bats") {
        return Some("bash");
    }
    tree_sitter_language_pack::detect_language(file_path)
}

/// The language for a path only when this build ships its grammar.
///
/// [`language_for_path`] follows the pack's extension map, which also names languages this binary
/// does not compile (markdown, json, make, ini, ...). Those files are not analyzable code here, so
/// a caller that would otherwise run the parser must treat them as non-code instead of failing.
pub(crate) fn analyzable_language_for_path(file_path: &str) -> Option<&'static str> {
    language_for_path(file_path).filter(|name| tree_sitter_language_pack::has_language(name))
}

/// Test-body query for a language with a test framework.
///
/// Returns `None` for languages with no test framework/test-body query: those parse cleanly but
/// never yield a body-based finding (comments-only findings such as `commented-out-test` still
/// apply across every language).
pub(crate) fn test_query(name: &str) -> Option<&'static str> {
    match name {
        "python" => Some("(function_definition name: (identifier) @test.name) @test"),
        "javascript" | "typescript" | "tsx" => Some(
            "(call_expression function: (identifier) @test.callee) @test\n(call_expression function: (member_expression object: (identifier) @test.callee)) @test",
        ),
        "go" => Some("(function_declaration name: (identifier) @test.name) @test"),
        "rust" => Some("(function_item name: (identifier) @test.name) @test"),
        "ruby" => Some(
            "(method name: (identifier) @test.name) @test\n(call method: (identifier) @test.callee) @test",
        ),
        "java" | "csharp" => Some("(method_declaration name: (identifier) @test.name) @test"),
        "php" => Some(
            "(method_declaration name: (name) @test.name) @test\n(function_call_expression function: (name) @test.callee) @test",
        ),
        "kotlin" => Some("(function_declaration (simple_identifier) @test.name) @test"),
        "swift" => Some("(function_declaration name: (simple_identifier) @test.name) @test"),
        "scala" => Some(
            "(function_definition name: (identifier) @test.name) @test\n(call_expression function: (identifier) @test.callee) @test\n(call_expression function: (call_expression function: (identifier) @test.callee)) @test",
        ),
        "elixir" => Some("(call target: (identifier) @test.callee) @test"),
        "lua" => Some("(function_call name: (identifier) @test.callee) @test"),
        "c" => Some(
            "(_ (expression_statement (call_expression function: (identifier) @test.callee)) . (compound_statement) @test)",
        ),
        "cpp" => Some(
            "(function_definition declarator: (function_declarator declarator: (identifier) @test.name)) @test\n(_ (expression_statement (call_expression function: (identifier) @test.callee)) . (compound_statement) @test)",
        ),
        "groovy" => {
            Some("(command (unit (identifier) @test.callee) (block (string) @test.name) @test)")
        }
        "ocaml" => Some("(value_definition (let_binding pattern: (value_name) @test.name)) @test"),
        "elm" => Some(
            "(value_declaration (function_declaration_left (lower_case_identifier) @test.name)) @test",
        ),
        "bash" => Some("(command (command_name (word) @test.callee) (string) @test.name) @test"),
        _ => None,
    }
}

/// Per-language node kinds whose text is a string literal.
///
/// One table, consumed by [`crate::detector`] to build the masked view: the content of every node
/// of these kinds is blanked to spaces (newlines and quotes preserved), so a marker that appears
/// only inside a string cannot drive a line rule. A language with no entry parses as before and its
/// line rules see the unmasked text.
pub(crate) fn string_node_kinds(name: &str) -> &'static [&'static str] {
    match name {
        "python" | "lua" | "groovy" | "r" | "toml" | "cue" | "proto" => &["string"],
        "javascript" | "typescript" | "tsx" => &["string", "template_string"],
        "go" => &["interpreted_string_literal", "raw_string_literal"],
        "rust" | "cpp" => &["string_literal", "raw_string_literal", "char_literal"],
        "java" | "kotlin" => &["string_literal", "character_literal"],
        "ruby" => &["string", "heredoc_body"],
        "csharp" => &[
            "string_literal",
            "verbatim_string_literal",
            "character_literal",
        ],
        "php" => &["string", "encapsed_string", "heredoc", "nowdoc"],
        "swift" => &["line_string_literal", "multi_line_string_literal"],
        "scala" => &["string", "interpolated_string", "character_literal"],
        "elixir" => &["string", "charlist", "sigil"],
        "c" => &["string_literal", "char_literal"],
        "ocaml" => &["string", "quoted_string", "character"],
        "elm" => &["string_constant_expr", "char_constant_expr"],
        "bash" => &["string", "raw_string", "ansi_c_string", "heredoc_body"],
        "haskell" => &["string", "char"],
        "yaml" => &[
            "string_scalar",
            "single_quote_scalar",
            "double_quote_scalar",
            "block_scalar",
            "plain_scalar",
        ],
        "hcl" => &["string_lit", "heredoc_template"],
        "css" => &["string_value"],
        "html" => &["quoted_attribute_value"],
        "dart" => &["string_literal"],
        "zig" => &["STRINGLITERALSINGLE"],
        _ => &[],
    }
}

/// The tree-sitter query built from a language's string kinds.
pub(crate) fn string_query_source(kinds: &[&str]) -> String {
    let mut source = String::new();
    for kind in kinds {
        source.push('(');
        source.push_str(kind);
        source.push_str(") @string\n");
    }
    source
}

/// Whether the name is a test-function identifier for the languages that gate on the name.
pub(crate) fn is_test_function_name(name: &str) -> bool {
    name.starts_with("test_") || name.starts_with("Test") || name.starts_with("spec_")
}

/// Whether the callee name denotes a test/describe/spec block in the call-style frameworks.
pub(crate) fn is_test_callee(callee: &str) -> bool {
    matches!(
        callee,
        "it" | "test"
            | "describe"
            | "suite"
            | "specify"
            | "context"
            | "scenario"
            | "fit"
            | "xit"
            | "xtest"
            | "fdescribe"
            | "xdescribe"
            | "should"
            | "property"
    )
}

// The path predicates below are one table each. `eval/path-predicates.json` is generated from these
// constants (see `path_predicates_contract_matches_language_tables`) and the Python tooling reads
// that committed file instead of keeping its own regex copies.

pub(crate) const TEST_PATH_SEGMENTS: &[&str] = &["tests", "test", "__tests__", "spec", "specs"];

pub(crate) const TEST_FILENAME_PREFIXES: &[&str] = &["test_", "test-"];

pub(crate) const TEST_FILENAME_INFIXES: &[&str] = &["_test.", ".test.", ".spec.", "_spec."];

/// Extension-scoped filename rules for languages whose test convention is a class-name prefix or
/// suffix (`TestParser.java`, `ParserTest.kt`) rather than a path segment or a `test_` prefix.
pub(crate) struct ExtensionTestNameRule {
    pub(crate) extensions: &'static [&'static str],
    pub(crate) filename_prefixes: &'static [&'static str],
    pub(crate) filename_suffixes: &'static [&'static str],
}

pub(crate) const EXTENSION_TEST_NAME_RULES: &[ExtensionTestNameRule] = &[ExtensionTestNameRule {
    extensions: &["java", "kt"],
    filename_prefixes: &["test"],
    filename_suffixes: &["test"],
}];

pub(crate) const HARNESS_PATH_SUBSTRINGS: &[&str] = &[".github/workflows/", ".gitlab-ci"];

pub(crate) const HARNESS_BASENAMES: &[&str] = &[
    "pytest.ini",
    "tox.ini",
    "conftest.py",
    "setup.cfg",
    "pyproject.toml",
    "package.json",
    "makefile",
    "codecov.yml",
];

pub(crate) const HARNESS_FILENAME_PREFIXES: &[&str] =
    &["jest.config", "vitest.config", "karma.conf", ".nycrc"];

pub(crate) const SENSITIVE_HARNESS_PATH_SUBSTRINGS: &[&str] = &[".github/workflows/", ".gitlab-ci"];

pub(crate) const SENSITIVE_HARNESS_BASENAMES: &[&str] = &["pytest.ini", "tox.ini", "conftest.py"];

pub(crate) const SENSITIVE_HARNESS_FILENAME_PREFIXES: &[&str] = &["jest.config", "vitest.config"];

/// Test-path globs, the extension-independent half of the test gate.
///
/// The payload's explicit `is_test_file` wins when present; this is the fallback.
#[must_use]
pub fn is_test_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/").to_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or("");
    let (stem, extension) = file.rsplit_once('.').unwrap_or((file, ""));
    normalized
        .split('/')
        .any(|segment| TEST_PATH_SEGMENTS.contains(&segment))
        || TEST_FILENAME_PREFIXES
            .iter()
            .any(|prefix| file.starts_with(prefix))
        || TEST_FILENAME_INFIXES
            .iter()
            .any(|infix| file.contains(infix))
        || EXTENSION_TEST_NAME_RULES.iter().any(|rule| {
            rule.extensions.contains(&extension)
                && (rule
                    .filename_prefixes
                    .iter()
                    .any(|prefix| file.starts_with(prefix))
                    || rule
                        .filename_suffixes
                        .iter()
                        .any(|suffix| stem.ends_with(suffix)))
        })
}

/// Harness/config paths that carry a test gate but are not test files themselves.
#[must_use]
pub fn is_harness_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/").to_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or("");
    HARNESS_PATH_SUBSTRINGS
        .iter()
        .any(|substring| normalized.contains(substring))
        || HARNESS_BASENAMES.contains(&file)
        || HARNESS_FILENAME_PREFIXES
            .iter()
            .any(|prefix| file.starts_with(prefix))
}

/// Harness paths whose edit directly decides what CI runs.
///
/// The sensitive subset of [`is_harness_path`]: CI workflows and the runner's own pytest/tox/
/// conftest or jest/vitest configuration. Per policy D29 a finding on one of these clears the
/// Medium bar, while every other harness file (`package.json`, `codecov.yml`, `makefile`, …) stays
/// Low telemetry. [`is_harness_path`] remains the outer gate; this narrows within it.
#[must_use]
pub(crate) fn is_sensitive_harness_path(file_path: &str) -> bool {
    if !is_harness_path(file_path) {
        return false;
    }
    let normalized = file_path.replace('\\', "/").to_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or("");
    SENSITIVE_HARNESS_PATH_SUBSTRINGS
        .iter()
        .any(|substring| normalized.contains(substring))
        || SENSITIVE_HARNESS_BASENAMES.contains(&file)
        || SENSITIVE_HARNESS_FILENAME_PREFIXES
            .iter()
            .any(|prefix| file.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::{
        EXTENSION_TEST_NAME_RULES, HARNESS_BASENAMES, HARNESS_FILENAME_PREFIXES,
        HARNESS_PATH_SUBSTRINGS, SENSITIVE_HARNESS_BASENAMES, SENSITIVE_HARNESS_FILENAME_PREFIXES,
        SENSITIVE_HARNESS_PATH_SUBSTRINGS, TEST_FILENAME_INFIXES, TEST_FILENAME_PREFIXES,
        TEST_PATH_SEGMENTS, is_harness_path, is_sensitive_harness_path, is_test_callee,
        is_test_function_name, is_test_path,
    };

    const PATH_PREDICATES_JSON: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/eval/path-predicates.json");

    fn path_predicates_document() -> serde_json::Value {
        let extension_test_name_rules = EXTENSION_TEST_NAME_RULES
            .iter()
            .map(|rule| {
                serde_json::json!({
                    "extensions": rule.extensions,
                    "filename_prefixes": rule.filename_prefixes,
                    "filename_suffixes": rule.filename_suffixes,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "generated_from": "src/language.rs",
            "test_path_segments": TEST_PATH_SEGMENTS,
            "test_filename_prefixes": TEST_FILENAME_PREFIXES,
            "test_filename_infixes": TEST_FILENAME_INFIXES,
            "extension_test_name_rules": extension_test_name_rules,
            "harness_path_substrings": HARNESS_PATH_SUBSTRINGS,
            "harness_basenames": HARNESS_BASENAMES,
            "harness_filename_prefixes": HARNESS_FILENAME_PREFIXES,
            "sensitive_harness_path_substrings": SENSITIVE_HARNESS_PATH_SUBSTRINGS,
            "sensitive_harness_basenames": SENSITIVE_HARNESS_BASENAMES,
            "sensitive_harness_filename_prefixes": SENSITIVE_HARNESS_FILENAME_PREFIXES,
        })
    }

    #[test]
    fn path_predicates_contract_matches_language_tables() -> Result<(), Box<dyn std::error::Error>>
    {
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&path_predicates_document())?
        );
        let path = std::path::Path::new(PATH_PREDICATES_JSON);
        if std::env::var_os("UPDATE_PATH_PREDICATES").is_some() {
            std::fs::write(path, &rendered)?;
        }
        let committed = std::fs::read_to_string(path)?;
        assert_eq!(
            committed, rendered,
            "eval/path-predicates.json is stale; run scripts/generate-path-predicates"
        );
        Ok(())
    }

    #[test]
    fn is_harness_path_given_config_or_ci_returns_true() {
        // then
        assert!(is_harness_path("pytest.ini"));
        assert!(is_harness_path(".github/workflows/ci.yml"));
        assert!(is_harness_path("package.json"));
        assert!(is_harness_path("jest.config.js"));
        assert!(!is_harness_path("src/app.py"));
    }

    #[test]
    fn is_sensitive_harness_path_given_ci_or_runner_config_returns_true() {
        // then: the paths that directly decide what CI runs are sensitive
        assert!(is_sensitive_harness_path("pytest.ini"));
        assert!(is_sensitive_harness_path("tox.ini"));
        assert!(is_sensitive_harness_path("conftest.py"));
        assert!(is_sensitive_harness_path(".github/workflows/ci.yml"));
        assert!(is_sensitive_harness_path(".gitlab-ci.yml"));
        assert!(is_sensitive_harness_path("jest.config.js"));
        assert!(is_sensitive_harness_path("vitest.config.ts"));
    }

    #[test]
    fn is_sensitive_harness_path_given_other_harness_file_returns_false() {
        // then: a harness file that does not gate the run stays low-confidence telemetry
        assert!(!is_sensitive_harness_path("package.json"));
        assert!(!is_sensitive_harness_path("codecov.yml"));
        assert!(!is_sensitive_harness_path("pyproject.toml"));
        assert!(!is_sensitive_harness_path("src/app.py"));
    }

    #[test]
    fn is_test_path_given_test_directory_returns_true() {
        // then
        assert!(is_test_path("src/tests/module.rs"));
        assert!(is_test_path("app/__tests__/widget.js"));
    }

    #[test]
    fn is_test_path_given_test_file_name_returns_true() {
        // then
        assert!(is_test_path("pkg/test_parser.py"));
        assert!(is_test_path("pkg/parser_test.go"));
        assert!(is_test_path("src/widget.test.ts"));
        assert!(is_test_path("src/widget.spec.tsx"));
        assert!(is_test_path("src/ParserTest.java"));
        assert!(is_test_path("src/TestHelpers.kt"));
    }

    #[test]
    fn is_test_path_given_production_path_returns_false() {
        // then
        assert!(!is_test_path("src/app.py"));
        assert!(!is_test_path("src/testing_utils.rs"));
        assert!(!is_test_path("src/App.java"));
        assert!(!is_test_path("src/ContestHelper.java"));
    }

    #[test]
    fn is_test_function_name_given_prefixed_name_returns_true() {
        // then
        assert!(is_test_function_name("test_adds"));
        assert!(is_test_function_name("TestParser"));
        assert!(!is_test_function_name("helper"));
    }

    #[test]
    fn is_test_callee_given_framework_call_returns_true() {
        // then
        assert!(is_test_callee("it"));
        assert!(is_test_callee("describe"));
        assert!(!is_test_callee("expect_throws"));
        assert!(!is_test_callee("console"));
    }

    /// Every table entry must name a node kind its grammar actually has, or the masking query
    /// silently fails to build and the string content leaks back into the line rules.
    #[test]
    fn string_node_kinds_build_a_query_for_every_language() {
        use super::string_node_kinds;
        use tree_sitter::Query;

        let names = [
            "bash",
            "c",
            "cpp",
            "csharp",
            "css",
            "cue",
            "dart",
            "dockerfile",
            "elixir",
            "elm",
            "go",
            "groovy",
            "haskell",
            "hcl",
            "html",
            "java",
            "javascript",
            "kotlin",
            "lua",
            "ocaml",
            "php",
            "proto",
            "python",
            "r",
            "ruby",
            "rust",
            "scala",
            "sql",
            "svelte",
            "swift",
            "toml",
            "tsx",
            "typescript",
            "yaml",
            "zig",
        ];
        for name in names {
            let kinds = string_node_kinds(name);
            if kinds.is_empty() {
                continue;
            }
            let language = tree_sitter_language_pack::get_language(name)
                .unwrap_or_else(|error| panic!("{name}: grammar unavailable: {error}"));
            let source = super::string_query_source(kinds);
            Query::new(&language, &source)
                .unwrap_or_else(|error| panic!("{name}: invalid string kinds {kinds:?}: {error}"));
        }
    }
}
