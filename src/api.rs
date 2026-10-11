use serde::{Deserialize, Serialize};

use crate::detector::{Change, TestDetector};
use crate::language::{language_for_path, plugin_language};
use crate::models::Payload;
use crate::output::format_hook_message;
use crate::testpath::is_test_path;

pub const EXIT_PASS: i32 = 0;
pub const EXIT_BLOCK: i32 = 2;
pub const SUCCESS_MESSAGE: &str = "[test-checker] Success: No problematic tests found\n";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookCheckResult {
    pub exit_code: i32,
    pub message: String,
}

impl HookCheckResult {
    #[must_use]
    pub fn pass(message: impl Into<String>) -> Self {
        Self {
            exit_code: EXIT_PASS,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn block(message: impl Into<String>) -> Self {
        Self {
            exit_code: EXIT_BLOCK,
            message: message.into(),
        }
    }
}

/// Core entry point: parse a hook payload and return the hook verdict.
#[must_use]
pub fn check_hook_input(input: &str, custom_prompt: &str) -> HookCheckResult {
    if input.is_empty() {
        return skip("No input provided");
    }
    let Ok(payload) = serde_json::from_str::<Payload>(input) else {
        return skip("Invalid input format");
    };
    let file_path = payload.tool_input.file_path.trim();
    if file_path.is_empty() {
        return skip("No file path provided");
    }
    let Some(spec) = language_for_path(file_path) else {
        return skip("Unsupported file extension");
    };
    let Some((old_text, new_text, is_new)) = build_change(&payload) else {
        return skip("No content to check");
    };
    if new_text.trim().is_empty() {
        return skip("No content to check");
    }
    let is_test_file = payload
        .is_test_file
        .unwrap_or_else(|| is_test_path(file_path));
    if !is_test_file {
        return skip("Non-test file");
    }

    let change = Change {
        file_path: file_path.to_owned(),
        language: plugin_language(spec.name),
        new_text,
        old_text,
        is_new,
        is_test_file,
    };
    let findings = TestDetector::new().detect(&change);
    let findings = crate::filters::apply_filters(&findings, &change.new_text);
    if findings.is_empty() {
        return HookCheckResult::pass(SUCCESS_MESSAGE);
    }
    HookCheckResult::block(format_hook_message(&findings, custom_prompt))
}

#[must_use]
pub fn check_hook_input_json(input: &str, custom_prompt: &str) -> String {
    serde_json::to_string(&check_hook_input(input, custom_prompt))
        .unwrap_or_else(|_| fallback_json())
}

/// Runs the detector over a whole file's content, returning findings as JSON.
///
/// Mirrors comment-checker's `detect_comments_json`. Unsupported extensions
/// yield `[]`; serialization never panics.
#[must_use]
pub fn detect_findings_json(content: &str, file_path: &str, is_test_file: bool) -> String {
    let Some(spec) = language_for_path(file_path) else {
        return String::from("[]");
    };
    let change = Change {
        file_path: file_path.to_owned(),
        language: plugin_language(spec.name),
        new_text: content.to_owned(),
        old_text: String::new(),
        is_new: true,
        is_test_file,
    };
    let findings = TestDetector::new().detect(&change);
    let findings = crate::filters::apply_filters(&findings, &change.new_text);
    serde_json::to_string(&findings).unwrap_or_else(|_| String::from("[]"))
}

fn build_change(payload: &Payload) -> Option<(String, String, bool)> {
    let input = &payload.tool_input;
    if !input.edits.is_empty() {
        let mut old_text = String::new();
        let mut new_text = String::new();
        for edit in &input.edits {
            old_text.push_str(&edit.old_string);
            old_text.push('\n');
            new_text.push_str(&edit.new_string);
            new_text.push('\n');
        }
        return Some((old_text, new_text, false));
    }
    match payload.tool_name.trim().to_lowercase().as_str() {
        "write" | "create" => return Some((String::new(), input.content.clone(), true)),
        "edit" | "patch" => {
            return Some((input.old_string.clone(), input.new_string.clone(), false));
        }
        _ => {}
    }
    if !input.new_string.is_empty() {
        Some((input.old_string.clone(), input.new_string.clone(), false))
    } else if !input.content.is_empty() {
        Some((String::new(), input.content.clone(), true))
    } else {
        None
    }
}

fn skip(reason: &str) -> HookCheckResult {
    HookCheckResult::pass(format!("[test-checker] Skipping: {reason}\n"))
}

fn fallback_json() -> String {
    String::from("{\"exitCode\":0,\"message\":\"[test-checker] Skipping: Invalid arguments\"}")
}

#[cfg(test)]
mod tests {
    use crate::models::Payload;

    #[test]
    fn payload_given_write_input_deserializes_tool_fields() {
        // given
        let json = r#"{
            "tool_name": "Write",
            "tool_input": {
                "file_path": "src/app.py",
                "content": "print(1)",
                "old_string": "old",
                "new_string": "new",
                "edits": [{ "old_string": "x", "new_string": "y" }]
            }
        }"#;

        // when
        let payload: Payload = serde_json::from_str(json).unwrap();

        // then
        assert_eq!(payload.tool_name, "Write");
        assert_eq!(payload.tool_input.file_path, "src/app.py");
        assert_eq!(payload.tool_input.content, "print(1)");
        assert_eq!(payload.tool_input.old_string, "old");
        assert_eq!(payload.tool_input.new_string, "new");
        assert_eq!(payload.tool_input.edits.len(), 1);
        assert_eq!(payload.tool_input.edits[0].old_string, "x");
        assert_eq!(payload.tool_input.edits[0].new_string, "y");
    }

    #[test]
    fn payload_given_absent_is_test_file_is_none() {
        // given
        let json = r#"{"tool_name":"Write","tool_input":{"file_path":"a.py"}}"#;

        // when
        let payload: Payload = serde_json::from_str(json).unwrap();

        // then
        assert_eq!(payload.is_test_file, None);
    }

    #[test]
    fn payload_given_explicit_is_test_file_is_some() {
        // given
        let truthy =
            r#"{"tool_name":"Write","tool_input":{"file_path":"a.py"},"is_test_file":true}"#;
        let falsy =
            r#"{"tool_name":"Write","tool_input":{"file_path":"a.py"},"is_test_file":false}"#;

        // when / then
        assert_eq!(
            serde_json::from_str::<Payload>(truthy)
                .unwrap()
                .is_test_file,
            Some(true)
        );
        assert_eq!(
            serde_json::from_str::<Payload>(falsy).unwrap().is_test_file,
            Some(false)
        );
    }

    #[test]
    fn payload_given_unknown_fields_ignores_them() {
        // given
        let json =
            r#"{"tool_name":"Edit","unexpected":1,"tool_input":{"file_path":"a.py","extra":true}}"#;

        // when
        let payload: Payload = serde_json::from_str(json).unwrap();

        // then
        assert_eq!(payload.tool_name, "Edit");
        assert_eq!(payload.tool_input.file_path, "a.py");
    }
}
