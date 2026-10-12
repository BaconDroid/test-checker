#[must_use]
pub fn language_for_path(file_path: &str) -> Option<&'static str> {
    tree_sitter_language_pack::detect_language(file_path)
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
        _ => None,
    }
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

/// Test-path globs, the extension-independent half of the test gate.
///
/// The payload's explicit `is_test_file` wins when present; this is the fallback.
#[must_use]
pub fn is_test_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/").to_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or("");
    let in_test_dir = normalized
        .split('/')
        .any(|segment| matches!(segment, "tests" | "test" | "__tests__" | "spec" | "specs"));

    in_test_dir
        || file.starts_with("test_")
        || file.starts_with("test-")
        || file.contains("_test.")
        || file.contains(".test.")
        || file.contains(".spec.")
        || file.contains("_spec.")
}

/// Harness/config paths that carry a test gate but are not test files themselves.
#[must_use]
pub fn is_harness_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/").to_lowercase();
    let file = normalized.rsplit('/').next().unwrap_or("");
    normalized.contains(".github/workflows/")
        || normalized.contains(".gitlab-ci")
        || matches!(
            file,
            "pytest.ini"
                | "tox.ini"
                | "setup.cfg"
                | "pyproject.toml"
                | "package.json"
                | "makefile"
                | "codecov.yml"
        )
        || file.starts_with("jest.config")
        || file.starts_with("vitest.config")
        || file.starts_with("karma.conf")
        || file.starts_with(".nycrc")
}

#[cfg(test)]
mod tests {
    use super::{
        is_harness_path, is_test_callee, is_test_function_name, is_test_path, language_for_path,
        test_query,
    };

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
    fn language_for_path_given_known_extension_returns_name() {
        // then
        assert_eq!(language_for_path("main.py"), Some("python"));
    }

    #[test]
    fn language_for_path_given_unknown_extension_returns_none() {
        // then
        assert_eq!(language_for_path("main.xyz"), None);
    }

    #[test]
    fn language_for_path_given_known_extensionless_path_still_resolves() {
        // then
        assert_eq!(language_for_path("Dockerfile"), Some("dockerfile"));
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
    }

    #[test]
    fn is_test_path_given_production_path_returns_false() {
        // then
        assert!(!is_test_path("src/app.py"));
        assert!(!is_test_path("src/testing_utils.rs"));
    }

    #[test]
    fn test_query_given_framework_language_returns_query() {
        // then
        assert!(test_query("python").is_some());
        assert!(test_query("rust").is_some());
    }

    #[test]
    fn test_query_given_no_framework_language_returns_none() {
        // then
        assert!(test_query("toml").is_none());
        assert!(test_query("yaml").is_none());
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
}
