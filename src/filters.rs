use crate::engine;
use crate::models::Finding;

const BDD_KEYWORDS: &[&str] = &[
    "given",
    "when",
    "then",
    "arrange",
    "act",
    "assert",
    "when & then",
    "when&then",
];

const DIRECTIVE_PREFIXES: &[&str] = &[
    "type:",
    "noqa",
    "pyright:",
    "ruff:",
    "mypy:",
    "pylint:",
    "flake8:",
    "pyre:",
    "pytype:",
    "eslint-disable",
    "eslint-ignore",
    "prettier-ignore",
    "ts-ignore",
    "ts-expect-error",
    "clippy:",
    "allow",
    "deny",
    "warn",
    "forbid",
];

const SUPPRESSION_MARKER: &str = "test-checker: allow";

#[derive(Clone, Copy, Debug, Default)]
pub struct BddFilter;

impl BddFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        let normalized =
            engine::strip_one_prefix(finding.text.trim(), &["#", "//", "--", "/*", "*"]);
        BDD_KEYWORDS
            .iter()
            .any(|keyword| normalized.eq_ignore_ascii_case(keyword))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DirectiveFilter;

impl DirectiveFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        let mut normalized =
            engine::strip_one_prefix(finding.text.trim(), &["#", "//", "/*", "--", "*"]);
        if let Some(rest) = normalized.strip_prefix('@') {
            normalized = rest.trim();
        }
        DIRECTIVE_PREFIXES
            .iter()
            .any(|directive| engine::starts_with_ignore_ascii_case(normalized, directive))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShebangFilter;

impl ShebangFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        finding.text.trim().starts_with("#!")
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SuppressionFilter;

impl SuppressionFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        is_line_suppressed(&finding.text)
    }

    #[must_use]
    pub fn is_file_suppressed(&self, content: &str) -> bool {
        content.lines().any(is_line_suppressed)
    }
}

#[must_use]
pub fn is_line_suppressed(text: &str) -> bool {
    let lowered = text.to_lowercase();
    lowered.contains(SUPPRESSION_MARKER)
}

#[must_use]
pub fn apply_filters(findings: &[Finding]) -> Vec<Finding> {
    let bdd = BddFilter;
    let directive = DirectiveFilter;
    let shebang = ShebangFilter;
    let suppression = SuppressionFilter;
    findings
        .iter()
        .filter(|finding| {
            !bdd.should_skip(finding)
                && !directive.should_skip(finding)
                && !shebang.should_skip(finding)
                && !suppression.should_skip(finding)
        })
        .cloned()
        .collect()
}

/// The agent-memo phrase catalog, reused from `comment-checker` and applied to test names and
/// assertion messages instead of comment text.
#[must_use]
pub fn is_agent_memo_phrase(text: &str) -> bool {
    let text = text.trim();
    let starts_like_memo = match engine::first_ascii_lowercase_byte(text) {
        Some(b'a') => engine::starts_with_ignore_ascii_case(text, "after this"),
        Some(b'b') => engine::starts_with_ignore_ascii_case(text, "before this"),
        Some(b'c') => {
            engine::starts_with_word_pair(text, "changed", &["from", "to"])
                || engine::starts_with_word_pair(text, "change", &["from", "to"])
                || engine::starts_with_word_pair(text, "converted", &["from", "to"])
                || engine::starts_with_word_pair(text, "convert", &["from", "to"])
        }
        Some(b'h') => engine::starts_with_ignore_ascii_case(text, "here we"),
        Some(b'i') => engine::starts_with_word_pair(text, "implementation", &["of", "note"]),
        Some(b'm') => {
            engine::starts_with_word_pair(text, "modified", &["from", "to"])
                || engine::starts_with_word_pair(text, "modify", &["from", "to"])
                || engine::starts_with_word_pair(text, "moved", &["from", "to"])
                || engine::starts_with_word_pair(text, "move", &["from", "to"])
                || engine::starts_with_word_pair(text, "migrated", &["from", "to"])
                || engine::starts_with_word_pair(text, "migrate", &["from", "to"])
        }
        Some(b'n') => {
            engine::starts_with_word_pair(text, "now", &["we", "this", "it"])
                || engine::starts_with_ignore_ascii_case(text, "note:")
        }
        Some(b'p') => engine::starts_with_ignore_ascii_case(text, "previously"),
        Some(b'r') => {
            engine::starts_with_word_pair(text, "renamed", &["from", "to"])
                || engine::starts_with_word_pair(text, "rename", &["from", "to"])
        }
        Some(b's') => {
            engine::starts_with_word_pair(text, "switched", &["from", "to"])
                || engine::starts_with_word_pair(text, "switch", &["from", "to"])
        }
        Some(b't') => engine::starts_with_word_pair(
            text,
            "this",
            &[
                "implements",
                "implement",
                "adds",
                "add",
                "removes",
                "remove",
                "changes",
                "change",
                "fixes",
                "fix",
            ],
        ),
        Some(b'u') => {
            engine::starts_with_word_pair(text, "updated", &["from", "to"])
                || engine::starts_with_word_pair(text, "update", &["from", "to"])
        }
        Some(b'w') => engine::starts_with_word_pair(text, "was", &["changed"]),
        _ => false,
    };
    starts_like_memo || engine::has_ascii_arrow(text)
}
