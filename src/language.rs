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

#[cfg(test)]
mod tests {
    use super::{language_for_extension, language_for_path, plugin_language};

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
}
