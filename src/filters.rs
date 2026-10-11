//! Inline test-guard suppression markers plus the plain-text helpers.
//!
//! No regex engine is involved: every predicate is a small, auditable string
//! scan, mirroring the reference implementation's filters.

use crate::models::Finding;

/// `test-guard: allow` window, in lines, on each side of a finding.
const BYPASS_WINDOW: usize = 2;

#[must_use]
pub fn is_file_disabled(text: &str) -> bool {
    contains_ignore_ascii_case(text, "test-guard-disable-file")
}

/// Reports whether a `test-guard: allow` marker sits within +/-2 lines of the
/// given 1-based line. Line `0` never matches.
#[must_use]
pub fn within_bypass(text: &str, line: usize) -> bool {
    if line == 0 {
        return false;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let from = line.saturating_sub(1 + BYPASS_WINDOW);
    let to = (line + BYPASS_WINDOW).min(lines.len());
    (from..to).any(|index| {
        lines
            .get(index)
            .is_some_and(|candidate| has_allow_marker(candidate))
    })
}

/// Drops every finding whose line sits within a `test-guard: allow` window in
/// `source`, preserving the input order of the survivors.
///
/// `gutted-test` is exempt: its line is a position in the old text (a removed
/// assertion has no line in `new_text`), so bypass-checking it against `source`
/// would suppress it using an unrelated new-text line.
#[must_use]
pub fn apply_filters(findings: &[Finding], source: &str) -> Vec<Finding> {
    findings
        .iter()
        .filter(|finding| finding.rule == "gutted-test" || !within_bypass(source, finding.line))
        .cloned()
        .collect()
}

/// Reports whether a line contains the `test-guard:\s*allow\b` marker: the
/// `test-guard:` key is matched case-insensitively, ASCII whitespace is
/// skipped, then `allow` must be followed by a non-word byte (or end of line).
fn has_allow_marker(line: &str) -> bool {
    const KEY: &str = "test-guard:";
    let lower = line.to_ascii_lowercase();
    let mut start = 0;
    while let Some(offset) = lower[start..].find(KEY) {
        let index = start + offset;
        let rest = lower[index + KEY.len()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
        if let Some(after) = rest.strip_prefix("allow") {
            if after.is_empty() || !is_word_byte(after.as_bytes()[0]) {
                return true;
            }
        }
        start = index + KEY.len();
    }
    false
}

/// Strips block comments, Python triple-quoted strings, shebangs and trailing
/// line comments. Multi-line aware only for block/triple constructs.
#[must_use]
pub fn strip_comments(text: &str, language: &str) -> String {
    let markers: &[&str] = if language == "python" {
        &["#"]
    } else {
        &["//"]
    };
    let without_blocks = strip_block_comments(text);
    let without_triples = if language == "python" {
        strip_python_triples(&without_blocks)
    } else {
        without_blocks
    };
    let mut lines: Vec<&str> = Vec::new();
    for line in without_triples.split('\n') {
        if line.trim_start_matches([' ', '\t']).starts_with("#!") {
            lines.push("");
            continue;
        }
        let mut result = line;
        for marker in markers {
            if let Some(index) = find_comment_index(result, marker) {
                result = &result[..index];
                break;
            }
        }
        lines.push(result);
    }
    lines.join("\n")
}

/// Comment-stripped then string-literal-stripped text, as the plugin's
/// `codeText` helper.
#[must_use]
pub fn code_text(line: &str, language: &str) -> String {
    strip_string_literals(&strip_comments(line, language))
}

#[must_use]
pub fn strip_string_literals(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut quote: Option<u8> = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'"' | b'\'' | b'`') {
            quote = Some(byte);
            index += 1;
            continue;
        }
        push_char(text, index, &mut output);
        index += char_len(text, index);
    }
    output
}

#[must_use]
pub fn is_comment_line(line: &str, language: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("*/")
    {
        return true;
    }
    if language == "python"
        && (trimmed.starts_with('#') || trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''"))
    {
        return true;
    }
    false
}

#[must_use]
pub fn count_real_lines(text: &str, language: &str) -> usize {
    text.split('\n')
        .filter(|line| !strip_comments(line, language).trim().is_empty())
        .count()
}

/// Normalizes an assertion line the way the reference dedup does: remove all
/// whitespace, drop a `self.` receiver, fold `assertEqual` to `assertEquals`
/// and `.toBe` to `.toEqual`.
#[must_use]
pub fn normalize_assertion(code: &str) -> String {
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let without_self = squashed.replace("self.assert", "assert");
    let folded = replace_ignore_ascii_case(&without_self, "assertEqual", "assertEquals");
    replace_word_suffix(&folded, ".toBe", ".toEqual")
}

/// Collapses a test body to a comparable string (comment-only lines dropped).
#[must_use]
pub fn normalize_body<'a>(lines: impl IntoIterator<Item = &'a str>, language: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for line in lines {
        let stripped = strip_comments(line, language).trim().to_owned();
        if !stripped.is_empty() {
            parts.push(stripped);
        }
    }
    parts.join("\n")
}

// --- Rule predicates -------------------------------------------------------

#[must_use]
pub fn skip_focus_hit(code: &str, plugin: &str) -> bool {
    match plugin {
        "js" | "ts" => {
            has_call(code, ".skip")
                || has_call(code, ".only")
                || has_call(code, ".todo")
                || has_word_call(code, "xit")
                || has_word_call(code, "xdescribe")
                || has_word_call(code, "xtest")
        }
        "python" => {
            let trimmed = code.trim_start();
            trimmed.starts_with("@pytest.mark.skip")
                || trimmed.contains("@pytest.mark.xfail")
                || trimmed.contains("@unittest.skip")
                || has_call(code, ".skipIf")
        }
        "go" => {
            has_call(code, "t.Skip") || has_call(code, "t.Skipf") || has_call(code, "t.SkipNow")
        }
        "rust" => code.contains("#[ignore]"),
        _ => false,
    }
}

#[must_use]
pub fn is_conditional_skip(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("skipif")
        || lower.contains("skipunless")
        || lower.contains("skip_if")
        || lower
            .find("if")
            .zip(lower.find("skip"))
            .is_some_and(|(if_index, skip_index)| if_index < skip_index)
}

#[must_use]
pub fn tautological_hit(code: &str) -> bool {
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if squashed.contains("assertTrue(True)")
        || squashed.contains("expect(true).toBe(true)")
        || squashed.contains("assert.ok(true)")
    {
        return true;
    }
    if let Some(rest) = after_word(code, "assert") {
        let trimmed = rest.trim_start();
        if let Some(after_true) = trimmed.strip_prefix("True") {
            if after_true.is_empty() || !is_word_byte(after_true.as_bytes()[0]) {
                return true;
            }
        }
    }
    if let Some((left, right)) = parse_assert_equals(code) {
        if left == right {
            return true;
        }
    }
    if let Some((left, right)) = parse_assert_eqeq(code) {
        if left == right {
            return true;
        }
    }
    false
}

#[must_use]
pub fn has_assertion(text: &str) -> bool {
    contains_word(text, "assert")
        || has_assert_call(text)
        || has_call(text, "expect")
        || has_word_call(text, "fail")
        || has_word_call(text, "verify")
        || contains_word(text, "should")
        || contains_word(text, "raises")
        || contains_word(text, "throws")
        || contains_word(text, "assertRaises")
        || text.contains("t.Error")
        || text.contains("t.Fatal")
        || has_call(text, "panic!")
        || text.contains("require.")
}

#[must_use]
pub fn is_assertion_line(code: &str) -> bool {
    has_assert_call(code)
        || after_word(code, "assert").is_some_and(|rest| rest.starts_with([' ', '\t']))
        || has_call(code, "expect")
        || contains_word(code, "assertEqual")
        || contains_word(code, "assertRaises")
        || contains_word(code, "verify")
        || contains_word(code, "assertThat")
        || code.contains("self.assert")
}

#[must_use]
pub fn has_assertion_message(body: &str) -> bool {
    call_has_message(body, "assert")
        || call_has_message(body, "expect")
        || python_assert_message(body)
}

/// Reports whether a Python `assert <expr>, "<msg>"` statement carries a
/// message: a top-level `assert` whose expression is followed, outside any
/// nested brackets, by a comma and then a string literal.
fn python_assert_message(body: &str) -> bool {
    for line in body.split('\n') {
        let trimmed = line.trim_start();
        if let Some(rest) = after_word(trimmed, "assert") {
            if top_level_comma_then_string(rest) {
                return true;
            }
        }
    }
    false
}

fn top_level_comma_then_string(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                let tail = text[index + 1..].trim_start();
                if tail.starts_with(['"', '\'']) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[must_use]
pub fn is_trivial_body(body: &str) -> bool {
    let trimmed = body.trim();
    trimmed.is_empty() || trimmed == "pass" || trimmed == "..."
}

#[must_use]
pub fn mock_identifier_hit(code: &str) -> bool {
    has_mock_prefix(code) || contains_word(code, "monkeypatch") || has_word_call(code, "patch")
}

#[must_use]
pub fn is_test_declaration(code: &str, plugin: &str) -> bool {
    match plugin {
        "js" | "ts" => js_test_declaration(code),
        "python" => {
            let trimmed = code.trim_start();
            trimmed.starts_with("def test_")
                || trimmed
                    .strip_prefix("async ")
                    .is_some_and(|rest| rest.starts_with("def test_"))
        }
        "go" => code.trim_start().starts_with("func Test"),
        "rust" => code.contains("#[test]"),
        _ => false,
    }
}

// --- Primitives ------------------------------------------------------------

fn push_char(text: &str, index: usize, output: &mut String) {
    if let Some(ch) = text[index..].chars().next() {
        output.push(ch);
    }
}

fn char_len(text: &str, index: usize) -> usize {
    text[index..].chars().next().map_or(1, char::len_utf8)
}

pub(crate) const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn contains_word(haystack: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(word) {
        let index = start + offset;
        let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
        let after = index + word.len();
        let after_ok = after >= bytes.len() || !is_word_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = index + word.len();
    }
    false
}

fn after_word<'a>(haystack: &'a str, word: &str) -> Option<&'a str> {
    if word.is_empty() {
        return None;
    }
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(word) {
        let index = start + offset;
        let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
        if before_ok {
            return Some(&haystack[index + word.len()..]);
        }
        start = index + word.len();
    }
    None
}

/// `name` followed (after optional spaces) by `(`.
fn has_call(code: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(offset) = code[start..].find(name) {
        let index = start + offset;
        let rest = code[index + name.len()..].trim_start();
        if rest.starts_with('(') {
            return true;
        }
        start = index + name.len();
    }
    false
}

/// `word` at a word boundary followed (after optional spaces) by `(`.
fn has_word_call(code: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let bytes = code.as_bytes();
    let mut start = 0;
    while let Some(offset) = code[start..].find(word) {
        let index = start + offset;
        let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
        let rest = code[index + word.len()..].trim_start();
        if before_ok && rest.starts_with('(') {
            return true;
        }
        start = index + word.len();
    }
    false
}

fn has_assert_call(code: &str) -> bool {
    let bytes = code.as_bytes();
    let mut start = 0;
    while let Some(offset) = code[start..].find("assert") {
        let index = start + offset;
        // Walk back to the start of the identifier so the `debug_assert`
        // family is recognized, then accept only the Rust assert macro
        // families (plain `assert` / `debug_assert`), not arbitrary
        // identifiers that merely end in `assert`.
        let mut ident_start = index;
        while ident_start > 0 && is_word_byte(bytes[ident_start - 1]) {
            ident_start -= 1;
        }
        let ident = &code[ident_start..];
        let accepted = ident.starts_with("assert") || ident.starts_with("debug_assert");
        if accepted {
            let mut end = index + "assert".len();
            while end < bytes.len() && is_word_byte(bytes[end]) {
                end += 1;
            }
            let mut rest = code[end..].trim_start();
            if let Some(after) = rest.strip_prefix('!') {
                rest = after.trim_start();
            }
            if rest.starts_with('(') {
                return true;
            }
        }
        start = index + "assert".len();
    }
    false
}

fn identifier(value: &str) -> Option<&str> {
    let value = value.trim();
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Some(value)
    } else {
        None
    }
}

fn parse_assert_equals(code: &str) -> Option<(String, String)> {
    let index = code.find("assertEquals")?;
    let rest = code[index + "assertEquals".len()..].trim_start();
    let inner = rest.strip_prefix('(')?;
    let close = inner.find(')')?;
    let (left, right) = inner[..close].split_once(',')?;
    let left = identifier(left)?;
    let right = identifier(right)?;
    Some((left.to_owned(), right.to_owned()))
}

fn parse_assert_eqeq(code: &str) -> Option<(String, String)> {
    let rest = after_word(code, "assert")?;
    let (left, right) = rest.split_once("==")?;
    let left = identifier(left)?;
    let right = identifier(right)?;
    Some((left.to_owned(), right.to_owned()))
}

fn has_mock_prefix(code: &str) -> bool {
    let bytes = code.as_bytes();
    let lower = code.to_ascii_lowercase();
    for needle in ["dummy", "stub", "mock", "spy", "fake"] {
        let mut start = 0;
        while let Some(offset) = lower[start..].find(needle) {
            let index = start + offset;
            if index == 0 || !is_word_byte(bytes[index - 1]) {
                return true;
            }
            start = index + needle.len();
        }
    }
    false
}

fn call_has_message(body: &str, keyword: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = body[start..].find(keyword) {
        let index = start + offset;
        let rest = body[index + keyword.len()..].trim_start();
        if let Some(inner) = rest.strip_prefix('(') {
            let mut depth = 1;
            let mut end = 0;
            for (position, ch) in inner.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = position;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let arguments = &inner[..end];
            if arguments
                .split_once(',')
                .is_some_and(|(_, tail)| tail.trim_start().starts_with(['"', '\'', '`']))
            {
                return true;
            }
        }
        start = index + keyword.len();
    }
    false
}

/// JS/TS test functions: `it(...)` and `test(...)`, including modifiers such as
/// `it.only(...)`, `test.skip(...)` or `it.each([...])(...)`. Containers
/// (`describe`/`test.describe`), lifecycle hooks (`test.beforeEach`,
/// `test.afterEach`, `test.beforeAll`, `test.afterAll`) and `test.step(...)` are
/// not test bodies and must not be captured as blocks.
fn js_test_declaration(code: &str) -> bool {
    let bytes = code.as_bytes();
    for keyword in ["it", "test"] {
        let mut cursor = 0;
        while let Some(offset) = code[cursor..].find(keyword) {
            let index = cursor + offset;
            let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
            let not_method = index == 0 || bytes[index - 1] != b'.';
            if before_ok && not_method {
                let mut end = index + keyword.len();
                if code[end..].starts_with('.') {
                    end += 1;
                    let member_start = end;
                    while end < bytes.len() && is_word_byte(bytes[end]) {
                        end += 1;
                    }
                    if is_non_test_js_member(&code[member_start..end]) {
                        cursor = index + keyword.len();
                        continue;
                    }
                }
                if code[end..].trim_start().starts_with('(') {
                    return true;
                }
            }
            cursor = index + keyword.len();
        }
    }
    false
}

/// Member names on `test`/`it` that are containers, lifecycle hooks or
/// interaction steps rather than test bodies.
fn is_non_test_js_member(member: &str) -> bool {
    matches!(
        member,
        "describe" | "beforeEach" | "afterEach" | "beforeAll" | "afterAll" | "step"
    )
}

fn replace_ignore_ascii_case(haystack: &str, needle: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(haystack.len());
    let bytes = haystack.as_bytes();
    let needle = needle.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].len() >= needle.len()
            && bytes[index..index + needle.len()].eq_ignore_ascii_case(needle)
        {
            output.push_str(replacement);
            index += needle.len();
        } else {
            push_char(haystack, index, &mut output);
            index += char_len(haystack, index);
        }
    }
    output
}

/// Replaces `needle` only when it is not followed by a word byte.
fn replace_word_suffix(haystack: &str, needle: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(haystack.len());
    let bytes = haystack.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if haystack[index..].starts_with(needle) {
            let after = index + needle.len();
            if after >= bytes.len() || !is_word_byte(bytes[after]) {
                output.push_str(replacement);
                index = after;
                continue;
            }
        }
        push_char(haystack, index, &mut output);
        index += char_len(haystack, index);
    }
    output
}

fn strip_block_comments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            if let Some(offset) = text[index + 2..].find("*/") {
                index = index + 2 + offset + 2;
                continue;
            }
            break;
        }
        push_char(text, index, &mut output);
        index += char_len(text, index);
    }
    output
}

fn strip_python_triples(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if let Some(delimiter) = ["\"\"\"", "'''"]
            .into_iter()
            .find(|candidate| rest.starts_with(candidate))
        {
            if let Some(offset) = text[index + 3..].find(delimiter) {
                index = index + 3 + offset + 3;
                continue;
            }
            break;
        }
        push_char(text, index, &mut output);
        index += char_len(text, index);
    }
    output
}

fn find_comment_index(line: &str, marker: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let marker_bytes = marker.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'"' | b'\'' | b'`') {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(marker_bytes) {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{code_text, count_real_lines, normalize_assertion, strip_comments, within_bypass};

    #[test]
    fn strip_comments_removes_line_and_block_comments() {
        assert_eq!(strip_comments("x = 1 # note", "python"), "x = 1 ");
        assert_eq!(
            strip_comments("const a = 1; // note", "js"),
            "const a = 1; "
        );
        assert_eq!(strip_comments("/* block */ y", "js"), " y");
        assert_eq!(strip_comments("#!/usr/bin/env python", "python"), "");
    }

    #[test]
    fn code_text_blanks_string_literals() {
        assert_eq!(
            code_text("assert x == \"literal\"", "python"),
            "assert x == "
        );
    }

    #[test]
    fn count_real_lines_ignores_blank_and_comment_lines() {
        assert_eq!(count_real_lines("// c\n\nx\ny", "js"), 2);
    }

    #[test]
    fn within_bypass_matches_nearby_marker() {
        let text = "line1\ntest-guard: allow\nline3";
        assert!(within_bypass(text, 2));
        assert!(!within_bypass(text, 0));
    }

    #[test]
    fn normalize_assertion_folds_equivalents() {
        assert_eq!(
            normalize_assertion("self.assertEqual(a, a)"),
            "assertEquals(a,a)"
        );
        assert_eq!(
            normalize_assertion("expect(x).toBe(y)"),
            "expect(x).toEqual(y)"
        );
    }
}
