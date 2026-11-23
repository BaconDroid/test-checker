use serde::{Deserialize, Serialize};

use crate::engine::null_default;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// How much `--strip` may remove on its own, shared with comment-checker.
///
/// `conservative` removes only proven structural problems, `balanced` adds the audited
/// heuristics, and `aggressive` removes everything the detector flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StripPolicy {
    Conservative,
    Balanced,
    Aggressive,
}

impl StripPolicy {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "conservative" => Some(Self::Conservative),
            "balanced" => Some(Self::Balanced),
            "aggressive" => Some(Self::Aggressive),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Aggressive => "aggressive",
        }
    }

    #[must_use]
    pub(crate) const fn allows(self, confidence: Confidence) -> bool {
        match self {
            Self::Aggressive => true,
            Self::Balanced => matches!(confidence, Confidence::High | Confidence::Medium),
            Self::Conservative => matches!(confidence, Confidence::High),
        }
    }
}

/// Environment variable that sets the minimum confidence when no valid `--min-confidence` is given.
pub(crate) const MIN_CONFIDENCE_ENV: &str = "TEST_CHECKER_MIN_CONFIDENCE";

impl Confidence {
    /// The confidence named by `value` (`low`, `medium` or `high`, case-insensitive).
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// The minimum confidence from [`MIN_CONFIDENCE_ENV`]; an absent, empty or unknown value is
    /// `medium`.
    pub(crate) fn min_from_env() -> Self {
        Self::min_from_env_value(std::env::var(MIN_CONFIDENCE_ENV).ok().as_deref())
    }

    fn min_from_env_value(value: Option<&str>) -> Self {
        value
            .and_then(|value| Self::parse(value.trim()))
            .unwrap_or(Self::Medium)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub file_path: String,
    pub line_number: usize,
    pub rule: String,
    pub confidence: Confidence,
    pub message: String,
    #[serde(skip)]
    pub text: String,
}

impl Finding {
    #[must_use]
    pub fn new(
        rule: impl Into<String>,
        confidence: Confidence,
        message: impl Into<String>,
        line_number: usize,
        file_path: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            file_path: file_path.into(),
            line_number,
            rule: rule.into(),
            confidence,
            message: message.into(),
            text: text.into(),
        }
    }

    #[must_use]
    pub fn normalized_text(&self) -> String {
        self.text.trim().to_lowercase()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Edit {
    #[serde(default, alias = "oldString", deserialize_with = "null_default")]
    pub old_string: String,
    #[serde(default, alias = "newString", deserialize_with = "null_default")]
    pub new_string: String,
}

#[cfg(test)]
mod tests {
    use super::{Confidence, Finding};

    #[test]
    fn min_from_env_value_maps_each_input_to_a_confidence() {
        // given: absent, empty, blank, unknown and the three named values
        let cases = [
            (None, Confidence::Medium),
            (Some(""), Confidence::Medium),
            (Some("  "), Confidence::Medium),
            (Some("critical"), Confidence::Medium),
            (Some("low"), Confidence::Low),
            (Some("medium"), Confidence::Medium),
            (Some("high"), Confidence::High),
            (Some(" HIGH "), Confidence::High),
        ];

        for (value, expected) in cases {
            // when / then
            assert_eq!(Confidence::min_from_env_value(value), expected, "{value:?}");
        }
    }

    #[test]
    fn parse_given_unknown_or_empty_value_returns_none() {
        // given / when / then: the CLI gate relies on `None` to defer to the environment
        assert_eq!(Confidence::parse(""), None);
        assert_eq!(Confidence::parse("bogus"), None);
        assert_eq!(Confidence::parse("LOW"), Some(Confidence::Low));
    }

    #[test]
    fn finding_round_trips_through_json() -> serde_json::Result<()> {
        // given
        let finding = Finding::new(
            "skip-added",
            Confidence::High,
            "A test skip marker was added.",
            7,
            "tests/test_app.py",
            "",
        );

        // when
        let encoded = serde_json::to_string(&finding)?;
        let decoded: Finding = serde_json::from_str(&encoded)?;

        // then
        assert_eq!(decoded, finding);
        assert!(encoded.contains("\"lineNumber\":7"));
        assert!(encoded.contains("\"confidence\":\"high\""));
        assert!(!encoded.contains("\"text\""), "empty text is omitted");
        Ok(())
    }
}
