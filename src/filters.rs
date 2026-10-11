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
        let normalized = strip_one_prefix(finding.text.trim(), &["#", "//", "--", "/*", "*"]);
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
        let mut normalized = strip_one_prefix(finding.text.trim(), &["#", "//", "/*", "--", "*"]);
        if let Some(rest) = normalized.strip_prefix('@') {
            normalized = rest.trim();
        }
        DIRECTIVE_PREFIXES
            .iter()
            .any(|directive| starts_with_ignore_ascii_case(normalized, directive))
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
    let suppression = SuppressionFilter;
    findings
        .iter()
        .filter(|finding| {
            !bdd.should_skip(finding)
                && !directive.should_skip(finding)
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
    let starts_like_memo = match first_ascii_lowercase_byte(text) {
        Some(b'a') => starts_with_any(text, &["added", "add", "after this"]),
        Some(b'b') => starts_with_ignore_ascii_case(text, "before this"),
        Some(b'c') => {
            starts_with_word_pair(text, "changed", &["from", "to"])
                || starts_with_word_pair(text, "change", &["from", "to"])
                || starts_with_word_pair(text, "converted", &["from", "to"])
                || starts_with_word_pair(text, "convert", &["from", "to"])
        }
        Some(b'd') => starts_with_any(text, &["deleted", "delete"]),
        Some(b'h') => starts_with_ignore_ascii_case(text, "here we"),
        Some(b'i') => {
            starts_with_any(text, &["implemented", "implement"])
                || starts_with_word_pair(text, "implementation", &["of", "note"])
        }
        Some(b'm') => {
            starts_with_word_pair(text, "modified", &["from", "to"])
                || starts_with_word_pair(text, "modify", &["from", "to"])
                || starts_with_word_pair(text, "moved", &["from", "to"])
                || starts_with_word_pair(text, "move", &["from", "to"])
                || starts_with_word_pair(text, "migrated", &["from", "to"])
                || starts_with_word_pair(text, "migrate", &["from", "to"])
        }
        Some(b'n') => {
            starts_with_word_pair(text, "now", &["we", "this", "it"])
                || starts_with_ignore_ascii_case(text, "note:")
        }
        Some(b'p') => starts_with_ignore_ascii_case(text, "previously"),
        Some(b'r') => {
            starts_with_any(
                text,
                &["refactor", "replaced", "replace", "removed", "remove"],
            ) || starts_with_word_pair(text, "renamed", &["from", "to"])
                || starts_with_word_pair(text, "rename", &["from", "to"])
        }
        Some(b's') => {
            starts_with_word_pair(text, "switched", &["from", "to"])
                || starts_with_word_pair(text, "switch", &["from", "to"])
        }
        Some(b't') => starts_with_word_pair(
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
            starts_with_word_pair(text, "updated", &["from", "to"])
                || starts_with_word_pair(text, "update", &["from", "to"])
        }
        Some(b'w') => starts_with_word_pair(text, "was", &["changed"]),
        _ => false,
    };
    starts_like_memo || has_ascii_arrow(text)
}

fn strip_one_prefix<'a>(text: &'a str, prefixes: &[&str]) -> &'a str {
    let mut normalized = text.trim();
    for prefix in prefixes {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            normalized = rest.trim();
            break;
        }
    }
    normalized
}

fn first_ascii_lowercase_byte(text: &str) -> Option<u8> {
    text.as_bytes().first().map(u8::to_ascii_lowercase)
}

fn starts_with_any(text: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| starts_with_ignore_ascii_case(text, prefix))
}

fn starts_with_word_pair(text: &str, first: &str, seconds: &[&str]) -> bool {
    let Some(rest) = strip_prefix_ignore_ascii_case(text, first) else {
        return false;
    };
    let rest = rest.trim_start();
    seconds
        .iter()
        .any(|second| starts_with_ignore_ascii_case(rest, second))
}

fn has_ascii_arrow(text: &str) -> bool {
    let Some((left, right)) = text.split_once("->") else {
        return false;
    };
    let left = left.trim();
    let right = right.trim();
    !left.is_empty()
        && !right.is_empty()
        && left.bytes().all(|byte| byte.is_ascii_alphabetic())
        && right
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
}

fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with_ignore_ascii_case(text, prefix).then(|| &text[prefix.len()..])
}

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        BddFilter, DirectiveFilter, SuppressionFilter, apply_filters, is_agent_memo_phrase,
    };
    use crate::models::{Confidence, Finding};

    fn finding(text: &str) -> Finding {
        Finding::new(
            "commented-out-test",
            Confidence::High,
            "m",
            1,
            "tests/test_app.py",
            text,
        )
    }

    #[test]
    fn bdd_filter_given_bdd_keyword_returns_true() {
        // given
        let filter = BddFilter;

        // when / then
        assert!(filter.should_skip(&finding("# given")));
        assert!(filter.should_skip(&finding("// when & then")));
        assert!(!filter.should_skip(&finding("# regular")));
    }

    #[test]
    fn directive_filter_given_directive_returns_true() {
        // given
        let filter = DirectiveFilter;

        // when / then
        assert!(filter.should_skip(&finding("# noqa: E501")));
        assert!(filter.should_skip(&finding("// @ts-ignore")));
        assert!(!filter.should_skip(&finding("# regular")));
    }

    #[test]
    fn suppression_filter_given_marker_returns_true() {
        // given
        let filter = SuppressionFilter;

        // when / then
        assert!(filter.should_skip(&finding("# test-checker: allow")));
        assert!(filter.is_file_suppressed("x = 1  # test-checker: allow"));
        assert!(!filter.is_file_suppressed("x = 1"));
    }

    #[test]
    fn apply_filters_given_allowed_and_suppressed_returns_empty() {
        // given
        let findings = vec![
            finding("# given"),
            finding("// @ts-ignore"),
            finding("# test-checker: allow"),
        ];

        // when
        let result = apply_filters(&findings);

        // then
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn agent_memo_phrase_given_memo_text_returns_true() {
        // then
        assert!(is_agent_memo_phrase("Changed from old to new"));
        assert!(is_agent_memo_phrase("now this uses the new parser"));
        assert!(is_agent_memo_phrase("oldValue -> newValue"));
        assert!(!is_agent_memo_phrase("returns the sum of two numbers"));
    }
}
