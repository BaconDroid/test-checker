//! Inline test-guard suppression markers plus the plain-text helpers.
//!
//! No regex engine is involved: every predicate is a small, auditable string
//! scan, mirroring the reference implementation's filters.

/// `test-guard: allow` window, in lines, on each side of a finding.
const BYPASS_WINDOW: usize = 2;

/// The plugin's `DEFAULT_TEST_PATTERNS`, in declaration order.
const DEFAULT_TEST_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*_test.*",
    "**/test_*.py",
    "**/tests/**",
    "**/__tests__/**",
    "**/*.spec.*",
];

#[must_use]
pub fn is_test_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    DEFAULT_TEST_PATTERNS
        .iter()
        .any(|pattern| glob_match(pattern, &normalized))
}

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
            .is_some_and(|candidate| contains_ignore_ascii_case(candidate, "test-guard: allow"))
    })
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

/// Matches the "after" form of a loosening, used to locate the added line.
#[must_use]
pub fn matches_loosening_after(code: &str, message: &str) -> bool {
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    match message {
        "assertEqual loosened to assertTrue" => has_call(&squashed, "assertTrue"),
        "toBe loosened to toBeTruthy" => has_call(&squashed, ".toBeTruthy"),
        "toBe loosened to toBeDefined" => has_call(&squashed, ".toBeDefined"),
        "assertRaises narrowed to Exception" => {
            squashed.contains("assertRaises(Exception)")
                || squashed.contains("assertRaises(BaseException)")
        }
        "toThrow inverted to not.toThrow" => squashed.contains(".not.toThrow"),
        _ => false,
    }
}

#[must_use]
pub fn matches_loosening_before(code: &str, message: &str) -> bool {
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    match message {
        "assertEqual loosened to assertTrue" => has_call(&squashed, "assertEqual"),
        "toBe loosened to toBeTruthy" | "toBe loosened to toBeDefined" => {
            has_call(&squashed, ".toBe")
        }
        "assertRaises narrowed to Exception" => has_call(&squashed, "assertRaises"),
        "toThrow inverted to not.toThrow" => has_call(&squashed, ".toThrow"),
        _ => false,
    }
}

#[must_use]
pub const fn matcher_loosening_messages() -> &'static [&'static str] {
    &[
        "assertEqual loosened to assertTrue",
        "toBe loosened to toBeTruthy",
        "toBe loosened to toBeDefined",
        "assertRaises narrowed to Exception",
        "toThrow inverted to not.toThrow",
    ]
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
        || (lower.contains("if") && lower.contains("skip"))
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
    call_has_message(body, "assert") || call_has_message(body, "expect")
}

#[must_use]
pub fn is_trivial_body(body: &str) -> bool {
    let trimmed = body.trim();
    trimmed.is_empty() || trimmed == "pass" || trimmed == "..."
}

#[must_use]
pub fn forced_success_hit(code: &str) -> bool {
    if let Some(index) = code.rfind("||") {
        let rest = code[index + 2..].trim();
        if rest == "true" || rest == ":" {
            return true;
        }
    }
    if let Some(rest) = after_word(code, "exit") {
        let trimmed = rest.trim_start();
        if let Some(after_zero) = trimmed.strip_prefix('0') {
            if after_zero.is_empty() || !is_word_byte(after_zero.as_bytes()[0]) {
                return true;
            }
        }
    }
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    squashed.contains("sys.exit(0")
        || squashed.contains("process.exit(0")
        || squashed.contains("continue-on-error:true")
        || code.contains("--passWithNoTests")
}

#[must_use]
pub fn weakened_config_hit(code: &str) -> bool {
    code.contains("@ts-nocheck")
        || code.contains("--no-verify")
        || has_ruff_noqa(code)
        || threshold_is_zero(code)
}

#[must_use]
pub fn parse_threshold(line: &str) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    let key = ["fail_under", "cov-fail-under"]
        .iter()
        .find(|candidate| lower.contains(**candidate))?;
    let start = lower.find(key)? + key.len();
    let rest = line[start..].trim_start();
    let rest = rest.strip_prefix(['=', ':'])?.trim_start();
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<f64>().ok()
}

#[must_use]
pub fn tests_not_run_hit(code: &str) -> bool {
    has_delimited(code, "--ignore", &['=', ' ', '\t'])
        || contains_word(code, "--exclude")
        || contains_word(code, "--deselect")
        || has_k_not(code)
        || code.contains("--testPathIgnorePatterns")
        || code.contains("--passWithNoTests")
}

#[must_use]
pub fn negative_control_hit(code: &str, plugin: &str) -> bool {
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    match plugin {
        "python" => {
            squashed.contains("pytest.raises(Exception)")
                || squashed.contains("pytest.raises(BaseException)")
                || squashed.contains("assertRaises(Exception)")
                || squashed.contains("assertRaises(BaseException)")
                || squashed.contains("assertRaisesRegex(Exception")
                || squashed.contains("assertRaisesRegex(BaseException")
        }
        "js" | "ts" => {
            squashed.contains(".toThrow()")
                || squashed.contains(".toThrow(Error)")
                || squashed.contains(".not.toThrow()")
        }
        _ => false,
    }
}

#[must_use]
pub fn expected_from_sut_hit(code: &str) -> bool {
    let gated = contains_word(code, "assert")
        || has_call(code, "expect")
        || code.contains("require.")
        || contains_word(code, "should");
    if !gated {
        return false;
    }
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for name in call_identifiers(code) {
        if is_assertion_framework_call(name) {
            continue;
        }
        if let Some(entry) = counts.iter_mut().find(|(existing, _)| *existing == name) {
            entry.1 += 1;
        } else {
            counts.push((name, 1));
        }
    }
    counts.iter().any(|(_, count)| *count >= 2)
}

#[must_use]
pub fn mock_identifier_hit(code: &str) -> bool {
    has_mock_prefix(code) || contains_word(code, "monkeypatch") || has_call(code, "patch")
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

const fn is_word_byte(byte: u8) -> bool {
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
        let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
        if !before_ok {
            start = index + "assert".len();
            continue;
        }
        let mut end = index + "assert".len();
        while end < bytes.len() && is_word_byte(bytes[end]) {
            end += 1;
        }
        let rest = code[end..].trim_start();
        if rest.starts_with('(') {
            return true;
        }
        start = end;
    }
    false
}

/// Recognizes `--flag` followed by one of the given delimiters.
fn has_delimited(code: &str, flag: &str, delimiters: &[char]) -> bool {
    let mut start = 0;
    while let Some(offset) = code[start..].find(flag) {
        let index = start + offset;
        let rest = &code[index + flag.len()..];
        if rest.starts_with(delimiters) {
            return true;
        }
        start = index + flag.len();
    }
    false
}

fn has_k_not(code: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = code[start..].find("-k") {
        let index = start + offset;
        let rest = code[index + 2..].trim_start();
        let rest = rest.trim_start_matches(['"', '\'']);
        if let Some(after) = rest.strip_prefix("not") {
            if after.is_empty() || !is_word_byte(after.as_bytes()[0]) {
                return true;
            }
        }
        start = index + 2;
    }
    false
}

fn has_ruff_noqa(code: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = code[start..].find('#') {
        let index = start + offset;
        let rest = code[index + 1..].trim_start();
        if let Some(after) = rest.strip_prefix("ruff:") {
            if after.trim_start().starts_with("noqa") {
                return true;
            }
        }
        start = index + 1;
    }
    false
}

fn threshold_is_zero(code: &str) -> bool {
    parse_threshold(code).is_some_and(|value| value == 0.0)
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

fn call_identifiers(code: &str) -> Vec<&str> {
    let bytes = code.as_bytes();
    let mut names = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            while index < bytes.len() && is_word_byte(bytes[index]) {
                index += 1;
            }
            let name = &code[start..index];
            if code[index..].trim_start().starts_with('(') {
                names.push(name);
            }
        } else {
            index += 1;
        }
    }
    names
}

fn is_assertion_framework_call(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "expect"
            | "tobe"
            | "toequal"
            | "tobetruthy"
            | "tobedefined"
            | "tothrow"
            | "tocontain"
            | "tomatch"
            | "tomatchobject"
            | "should"
            | "require"
            | "equal"
            | "errorf"
            | "fatalf"
    ) || lower.starts_with("assert")
        || lower.starts_with("expect")
        || lower.starts_with("require")
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

fn js_test_declaration(code: &str) -> bool {
    let bytes = code.as_bytes();
    for keyword in ["describe", "it", "test"] {
        let mut cursor = 0;
        while let Some(offset) = code[cursor..].find(keyword) {
            let index = cursor + offset;
            let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
            let not_method = index == 0 || bytes[index - 1] != b'.';
            if before_ok && not_method {
                let mut end = index + keyword.len();
                if code[end..].starts_with('.') {
                    end += 1;
                    while end < bytes.len() && is_word_byte(bytes[end]) {
                        end += 1;
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

/// Zero-dependency glob matcher supporting `**`, `*` and `?`.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), path.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], path: &[u8]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    match pattern[0] {
        b'*' => {
            if pattern.len() >= 2 && pattern[1] == b'*' {
                let mut rest = &pattern[2..];
                if rest.first() == Some(&b'/') {
                    rest = &rest[1..];
                    if glob_match_bytes(rest, path) {
                        return true;
                    }
                }
                return (0..=path.len()).any(|index| glob_match_bytes(rest, &path[index..]));
            }
            let rest = &pattern[1..];
            let limit = path
                .iter()
                .position(|byte| *byte == b'/')
                .unwrap_or(path.len());
            glob_match_bytes(rest, path)
                || (0..limit).any(|index| glob_match_bytes(rest, &path[index + 1..]))
        }
        b'?' => !path.is_empty() && path[0] != b'/' && glob_match_bytes(&pattern[1..], &path[1..]),
        byte => !path.is_empty() && path[0] == byte && glob_match_bytes(&pattern[1..], &path[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        code_text, count_real_lines, glob_match, is_test_path, normalize_assertion, strip_comments,
        within_bypass,
    };

    #[test]
    fn glob_supports_double_star_and_extension_wildcards() {
        assert!(glob_match("**/*.test.*", "src/a.test.ts"));
        assert!(glob_match("**/*_test.*", "pkg/a_test.go"));
        assert!(glob_match("**/test_*.py", "tests/test_a.py"));
        assert!(glob_match("**/tests/**", "tests/deep/a.py"));
        assert!(!glob_match("**/*.test.*", "src/a.ts"));
    }

    #[test]
    fn is_test_path_recognizes_default_patterns() {
        assert!(is_test_path("src/a.test.ts"));
        assert!(is_test_path("src/a.spec.ts"));
        assert!(!is_test_path("src/a.ts"));
    }

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
