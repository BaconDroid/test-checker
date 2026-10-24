use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::engine;
use crate::models::{Confidence, Finding};

const SUPPRESSION_MARKER: &str = "test-checker: allow";
/// The committed, human-owned file that owns file-wide suppression.
pub const CONFIG_FILE_NAME: &str = ".test-checker.json";
const MIN_REASON_CHARS: usize = 12;

/// Rule ids accepted in a `test-checker: allow <rule> -- <reason>` marker.
///
/// Keep in sync with the catalog in `rules.rs`, `detector.rs`, and `output.rs`.
pub const KNOWN_RULES: &[&str] = &[
    "agent-memo-test",
    "allow-added",
    "allow-invalid",
    "assertion-removed",
    "broad-exception",
    "commented-out-test",
    "conditional-assertion",
    "config-edited",
    "constant-assertion",
    "duplicate-test",
    "expected-copied-from-actual",
    "focus-added",
    "gate-weakened",
    "gutted-test",
    "harness-edited",
    "incomplete-assertion",
    "mock-of-sut",
    "near-duplicate-test",
    "no-assertion",
    "over-mocking",
    "retry-added",
    "skip-added",
    "snapshot-regenerated",
    "source-grep-test",
    "swallowed-error",
    "tautological-assertion",
    "test-removed",
    "test-volume",
    "tolerance-broadened",
    "unawaited-assertion",
    "weakened-assertion",
];

/// Whether `rule` is a known rule id (case-insensitive).
#[must_use]
fn is_known_rule(rule: &str) -> bool {
    KNOWN_RULES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(rule))
}

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

/// A parsed `test-checker: allow <rule> -- <reason>` marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Marker<'a> {
    rule: &'a str,
    reason: &'a str,
}

/// Why a marker is invalid. An invalid marker suppresses nothing and is itself reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MarkerError<'a> {
    /// The bare, formerly file-wide form.
    FileWide,
    /// No `<rule> -- <reason>` tail.
    MissingSeparator,
    /// The rule id is not in [`KNOWN_RULES`].
    UnknownRule(&'a str),
    /// The reason after `--` is shorter than `MIN_REASON_CHARS`.
    ShortReason,
}

impl MarkerError<'_> {
    fn describe(&self) -> String {
        match self {
            Self::FileWide => format!(
                "the bare `test-checker: allow` marker no longer suppresses anything; use `{CONFIG_FILE_NAME}` for file-wide suppression, or `test-checker: allow <rule> -- <reason>` on the line"
            ),
            Self::MissingSeparator => {
                "expected `test-checker: allow <rule> -- <reason>`".to_owned()
            }
            Self::UnknownRule(rule) => format!("unknown rule `{rule}`"),
            Self::ShortReason => {
                format!("the reason after `--` must be at least {MIN_REASON_CHARS} characters")
            }
        }
    }
}

/// Parse the first marker in `text`, if any.
///
/// The marker is valid only when its rule is known and its reason has at least
/// `MIN_REASON_CHARS` characters.
#[must_use]
fn parse_marker(text: &str) -> Option<Result<Marker<'_>, MarkerError<'_>>> {
    let start = text.to_ascii_lowercase().find(SUPPRESSION_MARKER)?;
    let rest = text[start + SUPPRESSION_MARKER.len()..].trim();
    Some(parse_marker_tail(rest))
}

fn parse_marker_tail(rest: &str) -> Result<Marker<'_>, MarkerError<'_>> {
    if rest.is_empty() {
        return Err(MarkerError::FileWide);
    }
    let Some((rule, reason)) = rest.split_once("--") else {
        return Err(MarkerError::MissingSeparator);
    };
    let rule = rule.trim();
    let reason = reason.trim();
    if rule.is_empty() {
        return Err(MarkerError::MissingSeparator);
    }
    if !is_known_rule(rule) {
        return Err(MarkerError::UnknownRule(rule));
    }
    if reason.chars().count() < MIN_REASON_CHARS {
        return Err(MarkerError::ShortReason);
    }
    Ok(Marker { rule, reason })
}

/// The marker start offset on `text`, only when it lies inside one of `ranges` (absolute bytes).
fn marker_start_in_comments(
    text: &str,
    line_start: usize,
    ranges: &[(usize, usize)],
) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find(SUPPRESSION_MARKER) {
        let index = search_from + offset;
        let absolute = line_start + index;
        if ranges
            .iter()
            .any(|&(start, end)| start <= absolute && absolute < end)
        {
            return Some(index);
        }
        search_from = index + 1;
    }
    None
}

fn parse_marker_in_line<'a>(
    text: &'a str,
    line_start: usize,
    ranges: &[(usize, usize)],
) -> Option<Result<Marker<'a>, MarkerError<'a>>> {
    let start = marker_start_in_comments(text, line_start, ranges)?;
    let rest = text[start + SUPPRESSION_MARKER.len()..].trim();
    Some(parse_marker_tail(rest))
}

impl SuppressionFilter {
    #[must_use]
    pub fn should_skip(&self, finding: &Finding) -> bool {
        matches!(
            parse_marker(&finding.text),
            Some(Ok(marker)) if marker.rule.eq_ignore_ascii_case(&finding.rule)
        )
    }

    /// Whether any line suppresses the whole file.
    ///
    /// No in-file marker does: file-wide suppression is owned by `.test-checker.json`, read by the
    /// hook's config layer. This stays for the shared filter surface.
    #[must_use]
    pub const fn is_file_suppressed(&self, _content: &str) -> bool {
        false
    }
}

/// Whether the line carries any marker, valid or invalid.
#[must_use]
pub fn is_line_suppressed(text: &str) -> bool {
    parse_marker(text).is_some()
}

#[derive(Clone, Debug)]
struct InvalidMarker {
    line_number: usize,
    text: String,
    description: String,
}

/// Valid markers indexed by line for one analyzed fragment, plus the invalid markers it carries.
#[derive(Clone, Debug, Default)]
pub(crate) struct SuppressionIndex {
    rules_by_line: HashMap<usize, Vec<String>>,
    invalid: Vec<InvalidMarker>,
}

impl SuppressionIndex {
    /// Build the index from `content`. Markers on a line whose 0-based index is in `added` are
    /// inert: a marker introduced by the same change may not suppress that change.
    #[must_use]
    pub(crate) fn from_content(
        content: &str,
        added: &HashSet<usize>,
        comment_ranges: &[(usize, usize)],
    ) -> Self {
        let mut index = Self::default();
        let mut line_start = 0;
        for (offset, line) in content.split('\n').enumerate() {
            let marker = parse_marker_in_line(line, line_start, comment_ranges);
            line_start += line.len() + 1;
            let Some(marker) = marker else {
                continue;
            };
            match marker {
                Ok(marker) => {
                    if !added.contains(&offset) {
                        index
                            .rules_by_line
                            .entry(offset + 1)
                            .or_default()
                            .push(marker.rule.to_ascii_lowercase());
                    }
                }
                Err(error) => index.invalid.push(InvalidMarker {
                    line_number: offset + 1,
                    text: line.to_owned(),
                    description: error.describe(),
                }),
            }
        }
        index
    }

    /// Whether a valid marker on the finding's line or the line above lists its rule.
    #[must_use]
    pub(crate) fn is_suppressed(&self, finding: &Finding) -> bool {
        let line = finding.line_number;
        [line, line.saturating_sub(1)].into_iter().any(|number| {
            self.rules_by_line.get(&number).is_some_and(|rules| {
                rules
                    .iter()
                    .any(|rule| rule.eq_ignore_ascii_case(&finding.rule))
            })
        })
    }

    /// The `allow-invalid` findings for every invalid marker in the fragment.
    #[must_use]
    pub(crate) fn invalid_findings(&self, file_path: &str) -> Vec<Finding> {
        self.invalid
            .iter()
            .map(|marker| {
                Finding::new(
                    "allow-invalid",
                    Confidence::High,
                    format!(
                        "Invalid test-checker marker: {}; it suppresses nothing.",
                        marker.description
                    ),
                    marker.line_number,
                    file_path,
                    marker.text.as_str(),
                )
            })
            .collect()
    }
}

/// A validated `.test-checker.json` file-wide allow entry.
#[derive(Clone, Debug)]
struct ConfigAllow {
    glob: String,
    rules: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    allow: Vec<ConfigEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigEntry {
    #[serde(default)]
    glob: String,
    #[serde(default)]
    rules: Vec<String>,
    #[serde(default)]
    reason: String,
}

/// The committed file-wide suppression config, read from `<git root>/.test-checker.json`.
#[derive(Clone, Debug, Default)]
pub(crate) struct SuppressionConfig {
    entries: Vec<ConfigAllow>,
    cwd: Option<String>,
}

impl SuppressionConfig {
    /// Read the config from the git root that contains the payload `cwd`, falling back to the
    /// process cwd.
    ///
    /// The hook walks up from the payload directory to the first directory containing `.git` and
    /// reads `.test-checker.json` only there. A config anywhere else (a subdirectory, a directory
    /// above the repository, or a directory without a `.git` ancestor) is ignored. A missing,
    /// unreadable, or malformed file yields no suppression; an entry with an empty glob, no rules,
    /// or a missing/empty reason is dropped.
    #[must_use]
    pub(crate) fn load(cwd: Option<&str>) -> Self {
        let cwd = cwd
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.to_string_lossy().into_owned())
            });
        let Some(base) = cwd.as_deref() else {
            return Self::default();
        };
        let Some(root) = git_root(std::path::Path::new(base)) else {
            return Self::default();
        };
        let root = root.to_string_lossy().into_owned();
        let path = std::path::Path::new(&root).join(CONFIG_FILE_NAME);
        // Stat before reading: only an actual file at the repository root is opened.
        if !path.is_file() {
            return Self {
                entries: Vec::new(),
                cwd: Some(root),
            };
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self {
                entries: Vec::new(),
                cwd: Some(root),
            };
        };
        let Ok(config) = serde_json::from_str::<ConfigFile>(&raw) else {
            return Self {
                entries: Vec::new(),
                cwd: Some(root),
            };
        };
        let entries = config.allow.iter().filter_map(valid_config_entry).collect();
        Self {
            entries,
            cwd: Some(root),
        }
    }

    /// Whether a matching committed entry suppresses `rule` for `file_path`.
    #[must_use]
    pub(crate) fn suppresses(&self, file_path: &str, rule: &str) -> bool {
        let relative = relative_to_cwd(file_path, self.cwd.as_deref());
        self.entries.iter().any(|entry| {
            entry
                .rules
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(rule))
                && glob_matches(&entry.glob, &relative)
        })
    }
}

/// The first ancestor of `start` (inclusive) that contains a `.git` entry.
///
/// This only stats candidate paths; it never reads a config from a non-root directory.
fn git_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn valid_config_entry(entry: &ConfigEntry) -> Option<ConfigAllow> {
    let glob = entry.glob.trim();
    let reason = entry.reason.trim();
    let rules: Vec<String> = entry
        .rules
        .iter()
        .map(|rule| rule.trim().to_owned())
        .filter(|rule| !rule.is_empty())
        .collect();
    if glob.is_empty() || rules.is_empty() || reason.is_empty() {
        return None;
    }
    Some(ConfigAllow {
        glob: glob.to_owned(),
        rules,
    })
}

/// Whether `file_path` names the committed suppression config.
#[must_use]
pub(crate) fn is_config_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    normalized.rsplit('/').next() == Some(CONFIG_FILE_NAME)
}

fn relative_to_cwd(file_path: &str, cwd: Option<&str>) -> String {
    let file = normalize_path(file_path);
    let Some(base) = cwd.map(normalize_path) else {
        return file;
    };
    let base = base.trim_end_matches('/');
    if file == base {
        return String::new();
    }
    file.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('/'))
        .map_or_else(|| file.clone(), ToOwned::to_owned)
}

fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./")
        .map_or_else(|| path.clone(), ToOwned::to_owned)
}

/// Match a `/`-separated path glob. `**` matches zero or more path segments; `*` and `?` match
/// within one segment.
fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = normalize_path(pattern);
    let pattern: Vec<&str> = pattern
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let path: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    match_segments(&pattern, &path)
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|index| match_segments(rest, &path[index..])),
        Some((segment, rest)) => path.split_first().is_some_and(|(path_segment, path_rest)| {
            segment_matches(segment, path_segment) && match_segments(rest, path_rest)
        }),
    }
}

fn segment_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut matched = vec![vec![false; text.len() + 1]; pattern.len() + 1];
    matched[0][0] = true;
    for row in 0..pattern.len() {
        if pattern[row] == '*' {
            matched[row + 1][0] = matched[row][0];
        }
        for column in 0..text.len() {
            matched[row + 1][column + 1] = match pattern[row] {
                '*' => matched[row][column + 1] || matched[row + 1][column],
                '?' => matched[row][column],
                character => matched[row][column] && character == text[column],
            };
        }
    }
    matched[pattern.len()][text.len()]
}

/// Whether any marker inside a comment node sits on an added line.
///
/// `added` is `None` for a whole-file write, where every line counts as added.
#[must_use]
pub(crate) fn has_marker_in_comments(
    content: &str,
    added: Option<&HashSet<usize>>,
    comment_ranges: &[(usize, usize)],
) -> bool {
    let mut line_start = 0;
    for (offset, line) in content.split('\n').enumerate() {
        let on_added_line = added.is_none_or(|added| added.contains(&offset));
        let found =
            on_added_line && parse_marker_in_line(line, line_start, comment_ranges).is_some();
        line_start += line.len() + 1;
        if found {
            return true;
        }
    }
    false
}

#[must_use]
pub fn apply_filters(findings: &[Finding]) -> Vec<Finding> {
    let suppression = SuppressionFilter;
    apply_filters_without_suppression(findings)
        .into_iter()
        .filter(|finding| !suppression.should_skip(finding))
        .collect()
}

/// The allow-list filters without the suppression marker.
///
/// The hook applies suppression with a [`SuppressionIndex`], which also knows whether a marker
/// was introduced by the change. Calling that context-aware filter and then [`apply_filters`]
/// again would re-enable an added marker, so the hook uses this half only.
#[must_use]
pub(crate) fn apply_filters_without_suppression(findings: &[Finding]) -> Vec<Finding> {
    let bdd = BddFilter;
    let directive = DirectiveFilter;
    let shebang = ShebangFilter;
    findings
        .iter()
        .filter(|finding| {
            !bdd.should_skip(finding)
                && !directive.should_skip(finding)
                && !shebang.should_skip(finding)
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

#[cfg(test)]
mod tests {
    use super::{MarkerError, SuppressionIndex, parse_marker};
    use crate::models::{Confidence, Finding};
    use std::collections::HashSet;

    fn line_finding(rule: &str, line_number: usize) -> Finding {
        Finding::new(
            rule,
            Confidence::High,
            "m",
            line_number,
            "tests/test_app.py",
            "",
        )
    }

    #[test]
    fn parse_marker_reports_why_a_marker_is_invalid() {
        // then: the bare form is its own error, so the notice can name the replacement config
        assert!(matches!(
            parse_marker("# test-checker: allow"),
            Some(Err(MarkerError::FileWide))
        ));
        assert!(matches!(
            parse_marker("# test-checker: allow not-a-rule -- tracked upstream #12"),
            Some(Err(MarkerError::UnknownRule("not-a-rule")))
        ));
        assert!(matches!(
            parse_marker("# test-checker: allow skip-added -- short"),
            Some(Err(MarkerError::ShortReason))
        ));
        assert!(matches!(
            parse_marker("# test-checker: allow skip-added tracked upstream #12"),
            Some(Err(MarkerError::MissingSeparator))
        ));
        assert!(matches!(
            parse_marker("# test-checker: allow skip-added -- tracked upstream #12"),
            Some(Ok(marker)) if marker.rule == "skip-added"
        ));
    }

    #[test]
    fn suppression_index_suppresses_the_marker_line_and_the_line_below() {
        // given
        let content = "def test_a():\n    # test-checker: allow skip-added -- tracked upstream #12\n    @pytest.mark.skip\n    assert x == 1\n";
        let added = HashSet::new();
        let ranges = [(0, content.len())];

        // when
        let index = SuppressionIndex::from_content(content, &added, &ranges);

        // then: line 3 (below the marker) is suppressed, line 4 is not
        assert!(index.is_suppressed(&line_finding("skip-added", 3)));
        assert!(!index.is_suppressed(&line_finding("skip-added", 4)));
        assert!(!index.is_suppressed(&line_finding("focus-added", 3)));
    }

    #[test]
    fn suppression_index_ignores_a_marker_outside_comment_ranges() {
        // given: the marker text sits on the line, but no comment node covers it (a string literal)
        let content = "fn payload() { let input = \"# test-checker: allow skip-added -- tracked upstream #12\"; }\n";
        let added = HashSet::new();

        // when
        let index = SuppressionIndex::from_content(content, &added, &[(0, 0)]);

        // then: it neither suppresses nor reports itself
        assert!(!index.is_suppressed(&line_finding("skip-added", 1)));
        assert_eq!(index.invalid_findings("tests/api.rs").len(), 0);
    }

    #[test]
    fn suppression_index_ignores_a_marker_on_an_added_line() {
        // given: the marker was introduced by this change (0-based line 1 is added)
        let content = "def test_a():\n    # test-checker: allow skip-added -- tracked upstream #12\n    @pytest.mark.skip\n";
        let added: HashSet<usize> = std::iter::once(1).collect();
        let ranges = [(0, content.len())];

        // when
        let index = SuppressionIndex::from_content(content, &added, &ranges);

        // then
        assert!(!index.is_suppressed(&line_finding("skip-added", 3)));
    }
}
