/// Canonical grammar name plus the tree-sitter query that captures whole test
/// bodies for the grammar.
///
/// TSX shares the TypeScript rule table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageSpec {
    pub name: &'static str,
    pub test_query: &'static str,
}

#[must_use]
pub fn language_for_path(file_path: &str) -> Option<LanguageSpec> {
    let name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    let (_, ext) = name.rsplit_once('.')?;
    language_for_extension(ext)
}

#[must_use]
pub fn language_for_extension(extension: &str) -> Option<LanguageSpec> {
    let ext = extension.trim_start_matches('.').to_ascii_lowercase();
    let (name, test_query) = match ext.as_str() {
        "py" | "pyi" => (
            "python",
            "(function_definition name: (identifier) @test.name body: (block) @test.body)",
        ),
        "js" | "jsx" | "mjs" | "cjs" => ("javascript", "(call_expression) @test.call"),
        "ts" | "mts" | "cts" => ("typescript", "(call_expression) @test.call"),
        "tsx" => ("tsx", "(call_expression) @test.call"),
        "go" => (
            "go",
            "(function_declaration name: (identifier) @test.name body: (block) @test.body)",
        ),
        "rs" => (
            "rust",
            "(function_item name: (identifier) @test.name body: (block) @test.body)",
        ),
        _ => return None,
    };
    Some(LanguageSpec { name, test_query })
}

/// The filter-object language key used by the plain-text predicates: the
/// grammar name folded to the plugin's convention (`javascript` -> `js`,
/// `typescript`/`tsx` -> `ts`).
#[must_use]
pub fn plugin_language(name: &str) -> &'static str {
    match name {
        "javascript" => "js",
        "typescript" | "tsx" => "ts",
        "python" => "python",
        "go" => "go",
        "rust" => "rust",
        _ => "unknown",
    }
}

// --- Test-path classification ----------------------------------------------

/// The plugin's `DEFAULT_TEST_PATTERNS`, in declaration order.
const DEFAULT_TEST_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*_test.*",
    "**/test_*.py",
    "**/tests/**",
    "**/__tests__/**",
    "**/*.spec.*",
];

/// Reports whether `file_path` matches one of the plugin's default test-path
/// globs, after normalizing Windows separators.
#[must_use]
pub fn is_test_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    DEFAULT_TEST_PATTERNS
        .iter()
        .any(|pattern| glob_match(pattern, &normalized))
}

/// Zero-dependency glob matcher supporting `**`, `*` and `?`.
fn glob_match(pattern: &str, path: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), path.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], path: &[u8]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    match pattern[0] {
        b'*' => {
            if pattern.len() >= 2 && pattern[1] == b'*' {
                let mut rest = &pattern[2..];
                if rest.first() == Some(&b'/') {
                    rest = &rest[1..];
                    if glob_match_bytes(rest, path) {
                        return true;
                    }
                }
                return (0..=path.len()).any(|index| glob_match_bytes(rest, &path[index..]));
            }
            let rest = &pattern[1..];
            let limit = path
                .iter()
                .position(|byte| *byte == b'/')
                .unwrap_or(path.len());
            glob_match_bytes(rest, path)
                || (0..limit).any(|index| glob_match_bytes(rest, &path[index + 1..]))
        }
        b'?' => !path.is_empty() && path[0] != b'/' && glob_match_bytes(&pattern[1..], &path[1..]),
        byte => !path.is_empty() && path[0] == byte && glob_match_bytes(&pattern[1..], &path[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        glob_match, is_test_path, language_for_extension, language_for_path, plugin_language,
    };

    #[test]
    fn language_for_path_maps_supported_extensions() {
        // given / when / then
        assert_eq!(
            language_for_path("src/foo.test.ts").map(|s| (s.name, plugin_language(s.name))),
            Some(("typescript", "ts"))
        );
        assert_eq!(
            language_for_path("pkg/a_test.go").map(|s| (s.name, plugin_language(s.name))),
            Some(("go", "go"))
        );
        assert_eq!(
            language_for_path("tests/test_a.py").map(|s| (s.name, plugin_language(s.name))),
            Some(("python", "python"))
        );
    }

    #[test]
    fn language_for_path_rejects_unknown_extensions() {
        assert!(language_for_path("README.md").is_none());
        assert!(language_for_path("no-extension").is_none());
    }

    #[test]
    fn language_for_extension_accepts_all_documented_aliases() {
        let aliases = [
            ("py", "python"),
            ("pyi", "python"),
            ("js", "js"),
            ("jsx", "js"),
            ("mjs", "js"),
            ("cjs", "js"),
            ("ts", "ts"),
            ("mts", "ts"),
            ("cts", "ts"),
            ("tsx", "ts"),
            ("go", "go"),
            ("rs", "rust"),
        ];
        for (ext, plugin) in aliases {
            let spec = language_for_extension(ext);
            assert_eq!(
                spec.map(|s| plugin_language(s.name)),
                Some(plugin),
                "alias {ext}"
            );
        }
    }

    #[test]
    fn language_for_extension_exposes_a_test_query_per_language() {
        for ext in ["py", "js", "ts", "tsx", "go", "rs"] {
            let spec = language_for_extension(ext);
            assert!(
                spec.is_some_and(|s| !s.test_query.is_empty()),
                "alias {ext}"
            );
        }
    }

    #[test]
    fn glob_supports_double_star_and_extension_wildcards() {
        assert!(glob_match("**/*.test.*", "src/a.test.ts"));
        assert!(glob_match("**/*_test.*", "pkg/a_test.go"));
        assert!(glob_match("**/test_*.py", "tests/test_a.py"));
        assert!(glob_match("**/tests/**", "tests/deep/a.py"));
        assert!(!glob_match("**/*.test.*", "src/a.ts"));
    }

    #[test]
    fn glob_single_star_respects_path_separator() {
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("src/*.rs", "src/deep/main.rs"));
    }

    #[test]
    fn is_test_path_recognizes_default_patterns() {
        assert!(is_test_path("src/a.test.ts"));
        assert!(is_test_path("src/a.spec.ts"));
        assert!(is_test_path("tests/test_a.py"));
        assert!(is_test_path("pkg/foo_test.go"));
    }

    #[test]
    fn is_test_path_rejects_source_layout() {
        assert!(!is_test_path("src/a.ts"));
        assert!(!is_test_path("README.md"));
    }
}
