use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
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

    #[must_use]
    pub fn identity(&self) -> String {
        format!("{}\u{1f}{}", self.rule, self.normalized_text())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Edit {
    #[serde(default, alias = "oldString", deserialize_with = "null_default")]
    pub old_string: String,
    #[serde(default, alias = "newString", deserialize_with = "null_default")]
    pub new_string: String,
}

pub(crate) fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{Confidence, Finding};

    #[test]
    fn finding_round_trips_through_json() -> serde_json::Result<()> {
        // given
        let finding = Finding::new(
            "skip-added",
            Confidence::High,
            "A test skip marker was added.",
            7,
            "tests/test_app.py",
            "    @pytest.mark.skip",
        );

        // when
        let encoded = serde_json::to_string(&finding)?;
        let decoded: Finding = serde_json::from_str(&encoded)?;

        // then
        assert_eq!(decoded, finding);
        assert!(encoded.contains("\"lineNumber\":7"));
        assert!(encoded.contains("\"confidence\":\"high\""));
        Ok(())
    }

    #[test]
    fn finding_identity_given_same_rule_and_text_matches() {
        // given
        let a = Finding::new("skip-added", Confidence::High, "m", 1, "a.py", "  IT.SKIP ");
        let b = Finding::new("skip-added", Confidence::High, "m", 9, "b.py", "it.skip");

        // when / then
        assert_eq!(a.identity(), b.identity());
    }
}
