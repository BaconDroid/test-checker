use serde::{Deserialize, Serialize};

/// Confidence attached to a finding, mirroring the plugin's severity mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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

/// A single test-weakening signal, rendered into a `<finding>` element.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub rule: &'static str,
    pub message: String,
    pub confidence: Confidence,
}

impl Finding {
    #[must_use]
    pub fn new(
        file: impl Into<String>,
        line: usize,
        rule: &'static str,
        message: impl Into<String>,
        confidence: Confidence,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            rule,
            message: message.into(),
            confidence,
        }
    }
}

/// One entry of a `MultiEdit` payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Edit {
    #[serde(default)]
    pub old_string: String,
    #[serde(default)]
    pub new_string: String,
}

/// File-oriented fields of a `Write`/`Edit`/`MultiEdit` call.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ToolInput {
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub new_string: String,
    #[serde(default)]
    pub old_string: String,
    #[serde(default)]
    pub edits: Vec<Edit>,
}

/// Full hook payload. Unknown fields are ignored on purpose.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Payload {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: ToolInput,
    /// Optional caller-supplied test-file classification; `None` falls back to
    /// the built-in test-path matcher.
    #[serde(default)]
    pub is_test_file: Option<bool>,
}
