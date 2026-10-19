use crate::engine;
use crate::models::Finding;

const SUPPRESSION_MARKER: &str = "test-checker: allow";

/// Comment prefixes stripped before BDD/directive matching. The five are pairwise non-prefixing,
/// so a single shared order serves both filters.
const COMMENT_PREFIXES: &[&str] = &["#", "//", "/*", "--", "*"];

#[derive(Clone, Copy, Debug, Default)]
pub struct BddFilter;

impl BddFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        let normalized = engine::strip_one_prefix(finding.text.trim(), COMMENT_PREFIXES);
        engine::BDD_KEYWORDS
            .iter()
            .any(|keyword| normalized.eq_ignore_ascii_case(keyword))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DirectiveFilter;

impl DirectiveFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        let mut normalized = engine::strip_one_prefix(finding.text.trim(), COMMENT_PREFIXES);
        if let Some(rest) = normalized.strip_prefix('@') {
            normalized = rest.trim();
        }
        engine::DIRECTIVE_PREFIXES
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

/// How a `test-checker: allow` marker scopes suppression.
enum MarkerScope<'a> {
    /// Bare marker: file-wide, reserved for human-owned configuration.
    FileWide,
    /// `test-checker: allow <rule> -- <reason>`: only that rule, on the marker's line.
    Rule(&'a str),
}

fn marker_scope(text: &str) -> Option<MarkerScope<'_>> {
    let index = text.to_ascii_lowercase().find(SUPPRESSION_MARKER)?;
    let rest = text[index + SUPPRESSION_MARKER.len()..].trim();
    match rest.split_once("--") {
        Some((rule, _reason)) if !rule.trim().is_empty() => Some(MarkerScope::Rule(rule.trim())),
        _ => Some(MarkerScope::FileWide),
    }
}

impl SuppressionFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        match marker_scope(&finding.text) {
            Some(MarkerScope::FileWide) => true,
            Some(MarkerScope::Rule(rule)) => rule.eq_ignore_ascii_case(&finding.rule),
            None => false,
        }
    }

    #[must_use]
    pub fn is_file_suppressed(&self, content: &str) -> bool {
        content
            .lines()
            .any(|line| matches!(marker_scope(line), Some(MarkerScope::FileWide)))
    }
}

/// Whether the human-owned configuration enables the file-wide marker.
#[must_use]
pub(crate) fn file_wide_allowed() -> bool {
    std::env::var("TEST_CHECKER_ALLOW_FILE_WIDE").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Whether the line carries any suppression marker (bare or scoped).
#[must_use]
pub fn is_line_suppressed(text: &str) -> bool {
    engine::is_line_suppressed(text, SUPPRESSION_MARKER)
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
