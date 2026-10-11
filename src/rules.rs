use crate::filters::is_agent_memo_phrase;
use crate::models::{Confidence, Finding};

/// A single added line with its real (1-based) line number in the file.
#[derive(Clone, Copy, Debug)]
pub struct AddedLine<'a> {
    pub line_number: usize,
    pub text: &'a str,
}

const SKIP_MARKERS: &[&str] = &[
    ".skip(",
    ".skip)",
    ".skip;",
    ".skip ",
    "test.skip",
    "it.skip",
    "describe.skip",
    "xit(",
    "xtest(",
    "xdescribe(",
    "@pytest.mark.skip",
    "@pytest.mark.skipif",
    "@pytest.mark.xfail",
    "pytest.skip(",
    "unittest.skip",
    "self.skipTest(",
    "t.Skip(",
    "t.Skipf(",
    "t.SkipNow(",
    "testing.Short()",
    "skip \"",
    "skip(",
    "pending()",
    "markTestSkipped(",
    "XCTSkip(",
    "@tag :skip",
    "cancel(",
    "#[ignore]",
    "#[ignore =",
    "[Ignore]",
    "@Disabled",
    "@Ignore",
];

const FOCUS_MARKERS: &[&str] = &[
    "test.only",
    "it.only",
    "describe.only",
    "context.only",
    "specify.only",
    "fit(",
    "fdescribe(",
    "iit(",
];

const WEAKENED_MARKERS: &[&str] = &[
    ".toBeTruthy()",
    ".toBeFalsy()",
    ".toBeDefined()",
    ".toBeUndefined()",
    ".not.toBeNull()",
    "assertIsNotNone(",
    "assertIsInstance(",
    ".anything()",
];

const SNAPSHOT_MARKERS: &[&str] = &[
    "toMatchSnapshot",
    "toMatchInlineSnapshot",
    "toMatchFileSnapshot",
    "assertSnapshot",
    "assert_snapshot",
    "assert_debug_snapshot",
    "--update-snapshots",
    "--updateSnapshot",
    "verifyAll(",
    "approvals.verify",
];

const TOLERANCE_MARKERS: &[&str] = &[
    "assertAlmostEqual",
    "assertAlmostEquals",
    "toBeCloseTo(",
    "assert_allclose",
    "pytest.approx",
    "delta=",
    "time.sleep(",
    "Thread.Sleep",
    "cy.wait(",
    "await sleep(",
];

const RETRY_MARKERS: &[&str] = &[
    "@retry",
    "@flaky",
    "@pytest.mark.flaky",
    "rerun",
    "retries=",
    "retry(",
    "flaky(",
    "@RepeatedTest",
    "@Retry",
    "--reruns",
    "retryTimes",
];

const MOCK_MARKERS: &[&str] = &[
    "mock.patch(",
    "@patch(",
    "jest.mock(",
    "vi.mock(",
    "sinon.stub(",
    "mock.patch.object(",
    "jest.spyOn(",
    "MagicMock(",
    "Mock(",
];

const ASSERTION_MARKERS: &[&str] = &[
    "assert",
    "expect(",
    "expect (",
    ".should",
    "assertThat",
    "assertEquals",
    "assertEqual",
    "assertTrue",
    "assertFalse",
    "assert_eq!",
    "assert_ne!",
    "assert!(",
    "require.Equal",
    "assert.Equal",
    "Assert.Equal(",
];

const fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn has_marker(line: &str, markers: &[&str]) -> bool {
    let haystack = line.as_bytes();
    markers.iter().any(|marker| {
        let mut search_from = 0;
        while let Some(offset) = line[search_from..].find(marker) {
            let index = search_from + offset;
            let starts_with_identifier = marker.as_bytes()[0].is_ascii_alphanumeric();
            let boundary_ok =
                !starts_with_identifier || index == 0 || !is_identifier_byte(haystack[index - 1]);
            if boundary_ok {
                return true;
            }
            search_from = index + 1;
        }
        false
    })
}

fn has_word(text: &str, word: &str) -> bool {
    text.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|token| token == word)
}

fn has_ternary(text: &str) -> bool {
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut seen_question = false;
    for character in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' | '\'' => {
                in_string = true;
                quote = character;
            }
            '?' => seen_question = true,
            ':' if seen_question => return true,
            _ => {}
        }
    }
    false
}

fn finish(
    rule: &str,
    confidence: Confidence,
    message: &str,
    line: &AddedLine<'_>,
    file_path: &str,
) -> Finding {
    Finding::new(
        rule,
        confidence,
        message,
        line.line_number,
        file_path,
        line.text,
    )
}

/// Classify one added line against the line-oriented rules.
#[must_use]
pub fn classify_added_line(file_path: &str, added: &[AddedLine<'_>], index: usize) -> Vec<Finding> {
    let Some(line) = added.get(index).copied() else {
        return Vec::new();
    };
    let text = line.text.trim();
    let mut findings = Vec::new();

    if has_marker(text, SKIP_MARKERS) {
        findings.push(finish(
            "skip-added",
            Confidence::High,
            "A test/assertion was disabled with a skip/ignore marker.",
            &line,
            file_path,
        ));
    }
    if has_marker(text, FOCUS_MARKERS) {
        findings.push(finish(
            "focus-added",
            Confidence::High,
            "A focused test restricts the suite to a subset of tests.",
            &line,
            file_path,
        ));
    }
    if let Some(finding) = classify_assertion(file_path, &line, text) {
        findings.push(finding);
    }
    if has_marker(text, WEAKENED_MARKERS) {
        findings.push(finish(
            "weakened-assertion",
            Confidence::Medium,
            "The assertion was loosened to a weaker matcher.",
            &line,
            file_path,
        ));
    }
    if has_marker(text, SNAPSHOT_MARKERS) {
        findings.push(finish(
            "snapshot-regenerated",
            Confidence::Medium,
            "A snapshot/approval assertion may have been regenerated without review.",
            &line,
            file_path,
        ));
    }
    if has_marker(text, TOLERANCE_MARKERS) {
        findings.push(finish(
            "tolerance-broadened",
            Confidence::Medium,
            "A tolerance/timeout/sleep may have been widened to hide flakiness.",
            &line,
            file_path,
        ));
    }
    if has_marker(text, RETRY_MARKERS) {
        findings.push(finish(
            "retry-added",
            Confidence::Medium,
            "A retry/flaky marker was added to make an unstable test pass.",
            &line,
            file_path,
        ));
    }
    if let Some(finding) = classify_swallowed_error(file_path, added, index, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_conditional_assertion(file_path, added, index, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_mock_of_sut(file_path, &line, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_agent_memo(file_path, &line, text) {
        findings.push(finding);
    }
    findings
}

fn classify_assertion(file_path: &str, line: &AddedLine<'_>, text: &str) -> Option<Finding> {
    if is_constant_assertion(text) {
        return Some(finish(
            "constant-assertion",
            Confidence::High,
            "The assertion compares a literal with a literal and cannot fail.",
            line,
            file_path,
        ));
    }
    if let Some((left, right)) = assertion_operands(text) {
        if left == right && !is_literal(&left) {
            return Some(finish(
                "tautological-assertion",
                Confidence::High,
                "The assertion compares an expression with itself and cannot fail.",
                line,
                file_path,
            ));
        }
    }
    None
}

fn classify_swallowed_error(
    file_path: &str,
    added: &[AddedLine<'_>],
    index: usize,
    text: &str,
) -> Option<Finding> {
    let next = added.get(index + 1).map(|line| line.text.trim());
    let window = added
        .get(index..=index + 2)
        .map(|slice| {
            slice
                .iter()
                .map(|line| line.text.trim())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let swallows = if text.contains("except") {
        let tail = text.split("except").nth(1).unwrap_or("");
        has_word(tail, "pass") || next == Some("pass")
    } else if text.contains("catch") {
        let brace_empty = text.contains("{}") || text.contains("{ }");
        let brace_closed_next = next == Some("}") || window.get(1).is_some_and(|line| *line == "}");
        brace_empty || brace_closed_next
    } else {
        false
    };
    swallows.then(|| {
        finish(
            "swallowed-error",
            Confidence::High,
            "The failure is caught and swallowed instead of failing the test.",
            &added[index],
            file_path,
        )
    })
}

fn classify_conditional_assertion(
    file_path: &str,
    added: &[AddedLine<'_>],
    index: usize,
    text: &str,
) -> Option<Finding> {
    let next = added.get(index + 1).map(|line| line.text.trim());
    let assertion_here = has_marker(text, ASSERTION_MARKERS);
    let conditional_here = text.starts_with("if ")
        || text.starts_with("if(")
        || text.starts_with("if (")
        || has_ternary(text);
    let triggers = conditional_here
        && (assertion_here || next.is_some_and(|line| has_marker(line, ASSERTION_MARKERS)));
    triggers.then(|| {
        finish(
            "conditional-assertion",
            Confidence::High,
            "The assertion is guarded by a runtime condition and may never run.",
            &added[index],
            file_path,
        )
    })
}

fn classify_mock_of_sut(file_path: &str, line: &AddedLine<'_>, text: &str) -> Option<Finding> {
    let stem = subject_stem(file_path);
    if stem.is_empty() || !has_marker(text, MOCK_MARKERS) {
        return None;
    }
    text.contains(&stem).then(|| {
        finish(
            "mock-of-sut",
            Confidence::Medium,
            "The system under test itself is mocked or stubbed.",
            line,
            file_path,
        )
    })
}

fn classify_agent_memo(file_path: &str, line: &AddedLine<'_>, text: &str) -> Option<Finding> {
    let mut candidates = quoted_strings(text);
    if let Some(name) = test_name_literal(text) {
        candidates.push(name);
    }
    if candidates.is_empty() {
        return None;
    }
    candidates
        .iter()
        .any(|candidate| is_agent_memo_phrase(candidate))
        .then(|| {
            finish(
                "agent-memo-test",
                Confidence::Medium,
                "A test name or assertion message describes what changed instead of the behavior.",
                line,
                file_path,
            )
        })
}

/// Classify a captured test body for body-level rules that need no diff.
#[must_use]
pub fn classify_test_body(
    file_path: &str,
    body_start: usize,
    body: &[AddedLine<'_>],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let meaningful: Vec<&str> = body
        .iter()
        .map(|line| line.text.trim())
        .filter(|line| !line.is_empty())
        .collect();
    if meaningful.is_empty() {
        let line = AddedLine {
            line_number: body_start,
            text: "",
        };
        findings.push(finish(
            "gutted-test",
            Confidence::High,
            "The test body is empty.",
            &line,
            file_path,
        ));
        return findings;
    }

    let mock_count = meaningful
        .iter()
        .filter(|line| has_marker(line, MOCK_MARKERS))
        .count();
    let assertion_count = meaningful
        .iter()
        .filter(|line| has_marker(line, ASSERTION_MARKERS))
        .count();
    if mock_count >= 2 && mock_count > assertion_count {
        findings.push(Finding::new(
            "over-mocking",
            Confidence::Medium,
            "Mock setup outnumbers the assertions; the test may be asserting on a mock.",
            body_start,
            file_path,
            meaningful.first().copied().unwrap_or(""),
        ));
    }
    findings
}

/// Diff-only findings: newly introduced weakening between an edit's old and new content.
#[must_use]
pub fn diff_findings(old_content: &str, new_content: &str, file_path: &str) -> Vec<Finding> {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();
    let mut findings = Vec::new();

    let old_assertions = old_lines
        .iter()
        .filter(|line| has_marker(line.trim(), ASSERTION_MARKERS))
        .count();
    let new_assertions = new_lines
        .iter()
        .filter(|line| has_marker(line.trim(), ASSERTION_MARKERS))
        .count();
    if new_assertions < old_assertions {
        let removed = old_assertions - new_assertions;
        findings.push(Finding::new(
            "assertion-removed",
            Confidence::High,
            format!("{removed} existing assertion(s) were removed without a replacement."),
            1,
            file_path,
            "",
        ));
        if new_assertions == 0 {
            findings.push(Finding::new(
                "gutted-test",
                Confidence::High,
                "The test was stripped of all assertions (or emptied) by this edit.",
                1,
                file_path,
                "",
            ));
        }
    }

    for (index, new_line) in new_lines.iter().enumerate() {
        let trimmed = new_line.trim();
        let Some((left, right)) = assertion_operands(trimmed) else {
            continue;
        };
        if left != right || is_literal(&left) {
            continue;
        }
        let was_literal = old_lines.iter().any(|old_line| {
            assertion_operands(old_line.trim())
                .is_some_and(|(old_left, old_right)| old_left == left && is_literal(&old_right))
        });
        if was_literal {
            findings.push(Finding::new(
                "expected-copied-from-actual",
                Confidence::High,
                "The expected value was replaced by a copy of the actual value.",
                index + 1,
                file_path,
                *new_line,
            ));
        }
    }

    findings
}

fn subject_stem(file_path: &str) -> String {
    let file = file_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let mut stem = file.split('.').next().unwrap_or_default().to_string();
    stem = stem
        .strip_prefix("test_")
        .or_else(|| stem.strip_prefix("test-"))
        .unwrap_or(&stem)
        .to_string();
    for suffix in ["_test", "-test", "_spec", "-spec"] {
        if let Some(stripped) = stem.strip_suffix(suffix) {
            stem = stripped.to_string();
        }
    }
    stem
}

fn quoted_strings(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character != '"' && character != '\'' {
            continue;
        }
        let quote = character;
        let content_start = start + quote.len_utf8();
        let mut end = None;
        while let Some(&(index, next)) = chars.peek() {
            if next == quote {
                end = Some(index);
                chars.next();
                break;
            }
            chars.next();
        }
        if let Some(end) = end {
            if end > content_start {
                strings.push(text[content_start..end].to_string());
            }
        }
    }
    strings
}

fn test_name_literal(text: &str) -> Option<String> {
    let marker = ["it(", "test(", "describe(", "specify(", "context("]
        .into_iter()
        .find(|marker| text.contains(*marker))?;
    let after = &text[text.find(marker)?..];
    quoted_strings(after).into_iter().next()
}

fn is_constant_assertion(text: &str) -> bool {
    let compact: String = text.split_whitespace().collect();
    if compact.contains("assertTrue(true)")
        || compact.contains("assertFalse(false)")
        || compact.contains("assert(true)")
    {
        return true;
    }
    assertion_operands(text).is_some_and(|(left, right)| is_literal(&left) && is_literal(&right))
}

fn is_literal(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
        return true;
    }
    if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
        || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
    {
        return true;
    }
    matches!(
        value,
        "true" | "false" | "True" | "False" | "None" | "null" | "nil" | "undefined"
    )
}

/// Extract the two compared operands of an equality-style assertion, if any.
#[must_use]
pub fn assertion_operands(line: &str) -> Option<(String, String)> {
    let stripped = line.trim();
    if !has_marker(stripped, ASSERTION_MARKERS) {
        return None;
    }
    if let Some(inner) = inner_args(stripped, "assert_eq!(") {
        return split_two(inner);
    }
    if let Some(inner) = inner_args(stripped, "assert!(") {
        if inner.contains("==") {
            return split_two(&inner.replace("==", ","));
        }
        return None;
    }
    if let Some(inner) = inner_args(stripped, "assertEqual(") {
        return split_two(inner);
    }
    if let Some(inner) = inner_args(stripped, "assertEquals(") {
        return split_two(inner);
    }
    if let Some(inner) = inner_args(stripped, "assert.Equal(") {
        return split_skip_first(inner);
    }
    if let Some(inner) = inner_args(stripped, "require.Equal(") {
        return split_skip_first(inner);
    }
    if let Some((expected, actual)) = expect_to_operands(stripped) {
        return Some((expected, actual));
    }
    if let Some((_left, right)) = stripped.split_once("assert ") {
        let right = right.trim();
        if let Some((left, right)) = right.split_once(" == ") {
            let (left, right) = (left.trim(), right.trim());
            if !left.is_empty() && !right.is_empty() && !left.contains('=') {
                return Some((left.to_string(), right.to_string()));
            }
        }
    }
    None
}

fn expect_to_operands(line: &str) -> Option<(String, String)> {
    let inner = inner_args(line, "expect(")?;
    let matcher = [".toBe(", ".toEqual(", ".toStrictEqual("]
        .iter()
        .find_map(|matcher| inner_args(line, matcher))?;
    Some((inner.trim().to_string(), matcher.trim().to_string()))
}

fn inner_args<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let mut depth = 1_u32;
    for (index, character) in rest.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[..index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_two(inner: &str) -> Option<(String, String)> {
    match split_top_level(inner).as_slice() {
        [left, right] => Some((left.trim().to_string(), right.trim().to_string())),
        _ => None,
    }
}

fn split_skip_first(inner: &str) -> Option<(String, String)> {
    let parts = split_top_level(inner);
    match parts.as_slice() {
        [_skip, left, right] => Some((left.trim().to_string(), right.trim().to_string())),
        _ => None,
    }
}

fn split_top_level(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut quote = '\0';
    let mut current = String::new();
    let mut escaped = false;
    for character in inner.chars() {
        if in_string {
            current.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' | '\'' => {
                in_string = true;
                quote = character;
                current.push(character);
            }
            '(' | '[' | '{' => {
                depth += 1;
                current.push(character);
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() || !parts.is_empty() {
        parts.push(current);
    }
    parts
}

/// Whether a comment body looks like commented-out test/assertion code.
#[must_use]
pub fn looks_like_commented_out_test(comment_body: &str) -> bool {
    let stripped = strip_comment_markers(comment_body);
    let text = stripped.trim();
    if text.is_empty() {
        return false;
    }
    let starts_like_test = [
        "def test",
        "func Test",
        "func Benchmark",
        "#[test]",
        "@Test",
        "@Fact",
        "it(",
        "test(",
        "describe(",
        "xit(",
        "xtest(",
        "xdescribe(",
        "specify(",
        "context(",
    ]
    .iter()
    .any(|marker| text.starts_with(marker));
    if starts_like_test {
        return true;
    }
    let includes_assertion_call = [
        "assertEqual(",
        "assertEquals(",
        "assertTrue(",
        "assertFalse(",
        "assert_eq!(",
        "assert!(",
        "expect(",
        ".toBe(",
        ".toEqual(",
        ".toStrictEqual(",
        "pytest.raises(",
        "self.assert",
    ]
    .iter()
    .any(|marker| text.contains(marker));
    if includes_assertion_call {
        return true;
    }
    text.contains("assert ") && (text.contains("==") || text.contains("!=") || text.contains('('))
}

fn strip_comment_markers(text: &str) -> &str {
    let mut stripped = text.trim();
    for prefix in ["#", "//", "--", "*", "/*", "--[[", "<!--"] {
        if let Some(rest) = stripped.strip_prefix(prefix) {
            stripped = rest;
            break;
        }
    }
    stripped.trim()
}

#[cfg(test)]
mod tests {
    use super::{
        AddedLine, assertion_operands, classify_added_line, diff_findings,
        looks_like_commented_out_test,
    };

    fn lines<'a>(texts: &'a [&'a str]) -> Vec<AddedLine<'a>> {
        texts
            .iter()
            .enumerate()
            .map(|(index, text)| AddedLine {
                line_number: index + 1,
                text,
            })
            .collect()
    }

    #[test]
    fn classify_added_line_given_skip_marker_reports_skip_added() {
        // given
        let added = lines(&["    @pytest.mark.skip"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "skip-added");
    }

    #[test]
    fn classify_added_line_given_focus_marker_reports_focus_added() {
        // given
        let added = lines(&["it.only('adds', () => {})"]);

        // when
        let findings = classify_added_line("app.test.js", &added, 0);

        // then
        assert!(findings.iter().any(|finding| finding.rule == "focus-added"));
    }

    #[test]
    fn classify_added_line_given_tautology_reports_tautological_assertion() {
        // given
        let added = lines(&["    assert value == value"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "tautological-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_literal_pair_reports_constant_assertion() {
        // given
        let added = lines(&["    assertEquals(1, 1)"]);

        // when
        let findings = classify_added_line("src/test/java/AppTest.java", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "constant-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_swallowed_except_reports_swallowed_error() {
        // given
        let added = lines(&["    except Exception:", "        pass"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "swallowed-error")
        );
    }

    #[test]
    fn classify_added_line_given_conditional_assert_reports_conditional_assertion() {
        // given
        let added = lines(&["    if flag:", "        assert value == expected"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "conditional-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_clean_line_reports_nothing() {
        // given
        let added = lines(&["    result = add(1, 2)"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn assertion_operands_given_go_equality_skips_the_testing_argument() {
        // when
        let operands = assertion_operands("assert.Equal(t, expected, actual)");

        // then
        assert_eq!(
            operands,
            Some(("expected".to_string(), "actual".to_string()))
        );
    }

    #[test]
    fn diff_findings_given_removed_assertion_reports_assertion_removed() {
        // given
        let old = "def test_a():\n    assert add(1, 1) == 2\n    assert add(2, 2) == 4\n";
        let new = "def test_a():\n    assert add(1, 1) == 2\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed")
        );
    }

    #[test]
    fn diff_findings_given_expected_copied_reports_expected_copied_from_actual() {
        // given
        let old = "def test_a():\n    assert compute() == 5\n";
        let new = "def test_a():\n    assert compute() == compute()\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "expected-copied-from-actual")
        );
    }

    #[test]
    fn looks_like_commented_out_test_given_test_code_returns_true() {
        // then
        assert!(looks_like_commented_out_test("# assert x == y"));
        assert!(looks_like_commented_out_test("// expect(x).toBe(y)"));
        assert!(looks_like_commented_out_test("# def test_foo():"));
        assert!(looks_like_commented_out_test("// it('adds', () => {})"));
        assert!(!looks_like_commented_out_test(
            "# explain the algorithm below"
        ));
    }

    #[test]
    fn classify_added_line_given_identifier_suffix_not_a_focus_marker() {
        // given
        let added = lines(&["    benefit(value)"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(!findings.iter().any(|finding| finding.rule == "focus-added"));
    }

    #[test]
    fn classify_added_line_given_conditional_text_in_string_is_not_conditional() {
        // given
        let added = lines(&["    assertEquals(3, parse(\"Item? extra\"))"]);

        // when
        let findings = classify_added_line("src/test/java/AppTest.java", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "conditional-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_non_swallowing_handler_reports_nothing() {
        // given
        let added = lines(&["    except Exception:", "        handle_error()"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "swallowed-error")
        );
    }

    #[test]
    fn diff_findings_given_partial_assertion_removal_does_not_gut() {
        // given
        let old = "def test_a():\n    assert a() == 1\n    assert b() == 2\n    assert c() == 3\n";
        let new = "def test_a():\n    assert a() == 1\n    assert b() == 2\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed")
        );
        assert!(!findings.iter().any(|finding| finding.rule == "gutted-test"));
    }

    #[test]
    fn classify_added_line_given_behavioral_test_name_is_not_agent_memo() {
        // given
        let added = lines(&["it('adds numbers', () => {})"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "agent-memo-test")
        );
    }
}
