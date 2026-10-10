//! Test-path classification, mirroring the plugin's `DEFAULT_TEST_PATTERNS`.

/// The plugin's `DEFAULT_TEST_PATTERNS`, in declaration order.
const DEFAULT_TEST_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*_test.*",
    "**/test_*.py",
    "**/tests/**",
    "**/__tests__/**",
    "**/*.spec.*",
];

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
    use super::{glob_match, is_test_path};

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
