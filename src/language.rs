/// Canonical grammar name (for `tree-sitter-language-pack`) plus the consumer
/// plugin's language key. TSX shares the TypeScript rule table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageSpec {
    pub name: &'static str,
    pub plugin: &'static str,
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
    let (name, plugin) = match ext.as_str() {
        "py" | "pyi" => ("python", "python"),
        "js" | "jsx" | "mjs" | "cjs" => ("javascript", "js"),
        "ts" | "mts" | "cts" => ("typescript", "ts"),
        "tsx" => ("tsx", "ts"),
        "go" => ("go", "go"),
        "rs" => ("rust", "rust"),
        _ => return None,
    };
    Some(LanguageSpec { name, plugin })
}

#[cfg(test)]
mod tests {
    use super::{language_for_extension, language_for_path};

    #[test]
    fn language_for_path_maps_supported_extensions() {
        // given / when / then
        assert_eq!(
            language_for_path("src/foo.test.ts").map(|s| (s.name, s.plugin)),
            Some(("typescript", "ts"))
        );
        assert_eq!(
            language_for_path("pkg/a_test.go").map(|s| (s.name, s.plugin)),
            Some(("go", "go"))
        );
        assert_eq!(
            language_for_path("tests/test_a.py").map(|s| (s.name, s.plugin)),
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
            assert_eq!(spec.map(|s| s.plugin), Some(plugin), "alias {ext}");
        }
    }
}
