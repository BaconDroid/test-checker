use std::collections::HashSet;

use crate::filters::is_agent_memo_phrase;
use crate::language::is_sensitive_harness_path;
use crate::models::{Confidence, Finding};

/// A single added line with its real (1-based) line number in the file.
#[derive(Clone, Copy, Debug)]
pub struct AddedLine<'a> {
    pub line_number: usize,
    pub text: &'a str,
}

const SKIP_MARKERS: &[&str] = &[
    ".skip)",
    ".skip;",
    ".skip ",
    "test.skip",
    "it.skip",
    "describe.skip",
    "context.skip",
    "specify.skip",
    "suite.skip",
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
    "pending()",
    "markTestSkipped(",
    "XCTSkip(",
    "@tag :skip",
    "#[ignore]",
    "#[ignore =",
    "[Ignore]",
    "@Disabled",
    "@Ignore",
    "GTEST_SKIP(",
    "TEST_IGNORE",
    "skip <|",
];

/// Skip markers that are only a skip as a bare statement: `<expr>.skip("…")` (an IO skip, say)
/// or `<expr>.cancel("…")` (an axios cancellation) is not a skipped test, but `RSpec`'s
/// `skip("…")` and `ScalaTest`'s `cancel("…")` always are.
const SKIP_STATEMENT_MARKERS: &[&str] = &["cancel(\"", "skip("];

/// The `skip-added` message, shared by the marker table and the statement-position marker.
const SKIP_ADDED_MESSAGE: &str = "A test/assertion was disabled with a skip/ignore marker.";

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

/// JS/Vitest matcher strictness (higher is stricter). A decrease on the same `expect()` subject
/// is a weakening; see `docs/diff-aware-and-query-expansion.md` A3.
const JS_MATCHERS: &[(&str, u8)] = &[
    (".toStrictEqual(", 40),
    (".toEqual(", 30),
    (".toBe(", 20),
    (".toBeTruthy(", 10),
    (".toBeFalsy(", 10),
    (".toBeDefined(", 10),
    (".toBeUndefined(", 10),
    (".toBeNull(", 10),
    (".anything(", 10),
];

/// Python assert-call strictness (higher is stricter).
const PY_ASSERT_CALLS: &[(&str, u8)] = &[
    ("assertEqual(", 30),
    ("assertEquals(", 30),
    ("assertIsNotNone(", 10),
    ("assertTrue(", 10),
    ("assertIsInstance(", 10),
];

/// Snapshot/approval matchers: weaker than an inline equality on the same subject.
const SNAPSHOT_MATCHERS: &[&str] = &[
    ".toMatchSnapshot(",
    ".toMatchInlineSnapshot(",
    ".toMatchFileSnapshot(",
    "assertSnapshot(",
    "assert_snapshot(",
    "assert_debug_snapshot(",
];

/// Sleep/timeout calls whose duration only makes a test slower to fail: a longer duration is looser.
const SLEEP_MARKERS: &[&str] = &["time.sleep(", "cy.wait(", "await sleep(", "Thread.Sleep("];

/// Strength at or below this is the loose (truthiness/existence) end of the table.
const LOOSE_MAX: u8 = 10;
/// Strength at or above this is an exact/deep equality (the strong end).
const EXACT_MIN: u8 = 30;

/// `echo-test` is a heuristic over a whole test body; it stays telemetry until its FP audit clears
/// the Medium bar.
const ECHO_TEST_CONFIDENCE: Confidence = Confidence::Low;
/// `mirror-implementation-test` is line-local but still heuristic: telemetry until audited.
const MIRROR_IMPLEMENTATION_CONFIDENCE: Confidence = Confidence::Low;

const RETRY_MARKERS: &[&str] = &[
    "@retry",
    "@flaky",
    "@pytest.mark.flaky",
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

/// Extra markers that count as "this body asserts", for the `no-assertion` check only.
///
/// Kept separate from `ASSERTION_MARKERS` so that broadening coverage for macro-style frameworks
/// (GoogleTest/Catch2/C, alcotest, elm-test, bats-assert) does not feed `conditional-assertion`.
const ASSERTION_EXISTS_EXTRAS: &[&str] = &[
    "Assert.",
    "XCTAssert",
    "t.Error",
    "t.Fatal",
    "require.",
    "verify(",
    "check(",
    "EXPECT_",
    "ASSERT_",
    "REQUIRE(",
    "CHECK(",
    "TEST_ASSERT",
    "Alcotest.",
    "Expect.",
    "pytest.raises(",
    "assertRaises(",
    ".toThrow",
    "#[should_panic",
];

/// Markers that indicate a body contains nested test definitions (so it is a suite, not a leaf test).
const NESTED_TEST_MARKERS: &[&str] = &[
    "it(",
    "test(",
    "describe(",
    "suite(",
    "context(",
    "fit(",
    "xit(",
];

/// Source-file extensions (without the dot) that mark a literal as naming source code rather than
/// test data; a test that reads one asserts on source text, not behaviour. The list covers each
/// supported grammar's file extension, including the alternate `haskell` spelling alongside `hs`.
const SOURCE_FILE_EXTENSIONS: &[&str] = &[
    "py", "js", "ts", "tsx", "jsx", "rs", "go", "java", "rb", "php", "c", "cpp", "h", "hpp", "cs",
    "kt", "swift", "scala", "lua", "ex", "exs", "hs", "haskell", "dart", "r", "zig", "groovy",
    "ml", "mli", "sh", "bash", "bats", "elm",
];

/// Callee substrings that mark a file-read call; a source-file literal on the same line names
/// the file. The list covers the read-text API of each test language the rule applies to.
const SOURCE_READ_CALLEES: &[&str] = &[
    "read_to_string",
    "read_text",
    "readText",
    "readFileSync",
    "readFile",
    "ReadFile",
    "ReadAllText",
    "readString",
    "read_file",
    "file_get_contents",
    "include_str!",
    "fs::read",
    "File::open",
    "File.read",
    "contentsOfFile",
    "fromFile",
    "io.open",
    "getText",
    "ifstream",
    "File.read!",
    "$(cat ",
];

/// Operations that inspect the *text* of a value (substring, regex, snapshot). A read of a
/// source file is only a `source-grep-test` when one of these consumes the read text.
const SOURCE_TEXT_MARKERS: &[&str] = &[
    " in ",
    " not in ",
    ".includes(",
    ".contains(",
    ".startsWith(",
    ".endsWith(",
    ".startswith(",
    ".endswith(",
    ".match(",
    ".matches(",
    ".toMatch(",
    ".toContain(",
    ".toMatchSnapshot(",
    ".toMatchInlineSnapshot(",
    ".toMatchFileSnapshot(",
    ".include?",
    ".start_with?",
    ".end_with?",
    "include?(",
    "assert_includes",
    "assertIn(",
    "assertContains(",
    "assertStringContainsString(",
    "Assert.Contains(",
    "assertThat(",
    "HasSubstr(",
    "str_contains(",
    "strstr(",
    "preg_match(",
    "strings.Contains(",
    "String.contains?(",
    ":find(",
    "string.find(",
    ".find(",
    ".search(",
    "re.search(",
    "re.match(",
    "re.findall(",
    ".test(",
    "string_match",
    "=~",
    "to include(",
    "[[",
];

/// Config / CI markers that weaken or bypass a test gate.
const GATE_WEAKENING_MARKERS: &[&str] = &[
    "|| true",
    "|| exit 0",
    "continue-on-error: true",
    "continue-on-error:true",
    "allow_failure: true",
    "allow_failure:true",
    "--passWithNoTests",
    "set +e",
];

/// Jest/Vitest matcher markers that make an `expect(...)` a complete assertion.
const INCOMPLETE_ASSERTION_MATCHERS: &[&str] = &[
    ".toBe(",
    ".toBeCloseTo(",
    ".toBeTypeOf(",
    ".toEqual(",
    ".toStrictEqual(",
    ".toMatchSnapshot(",
    ".toMatchInlineSnapshot(",
    ".toMatchFileSnapshot(",
    ".toContain(",
    ".toThrow",
    ".toHaveLength(",
    ".toHaveProperty(",
    ".toHaveBeen",
    ".toBeTruthy(",
    ".toBeFalsy(",
    ".toBeDefined(",
    ".toBeNull(",
    ".toBeUndefined(",
    ".toBeGreaterThan",
    ".toBeLessThan",
    ".toBeInstanceOf",
    ".toMatchObject(",
    ".toMatch(",
    ".toReturn",
    ".toHaveReturned",
    ".resolves",
    ".rejects",
    ".anything(",
    ".not.to",
];

/// A rule that fires when an added line contains any of its markers.
struct MarkerRule {
    id: &'static str,
    confidence: Confidence,
    message: &'static str,
    markers: &'static [&'static str],
}

const MARKER_RULES: &[MarkerRule] = &[
    MarkerRule {
        id: "skip-added",
        confidence: Confidence::High,
        message: SKIP_ADDED_MESSAGE,
        markers: SKIP_MARKERS,
    },
    MarkerRule {
        id: "focus-added",
        confidence: Confidence::High,
        message: "A focused test restricts the suite to a subset of tests.",
        markers: FOCUS_MARKERS,
    },
    MarkerRule {
        id: "retry-added",
        confidence: Confidence::Medium,
        message: "A retry/flaky marker was added to make an unstable test pass.",
        markers: RETRY_MARKERS,
    },
];

/// The removed (LCS-unmatched old) lines of an `Edit`.
///
/// Empty for a `Write`, where the presence rules fall back to Low confidence. `removed` is the
/// masked view (structure only) and `removed_original` carries the literal values at the same line
/// positions.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiffContext<'a> {
    pub removed: &'a [&'a str],
    pub removed_original: &'a [&'a str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MatcherKind {
    Js,
    Python,
}

/// One matcher occurrence: the assertion subject, its strictness, and the matcher family.
#[derive(Debug)]
struct MatcherForm {
    subject: String,
    strength: u8,
    kind: MatcherKind,
}

impl MatcherForm {
    /// A truthiness/definedness/existence assertion, the loose end of the table.
    const fn is_loose(&self) -> bool {
        self.strength <= LOOSE_MAX
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TolerancePolarity {
    /// More decimal places (or a larger `toBeCloseTo` precision) is stricter.
    Precision,
    /// A longer sleep/timeout or a larger `delta` is looser.
    Duration,
}

impl TolerancePolarity {
    fn loosened(self, old: f64, new: f64) -> bool {
        match self {
            Self::Precision => new < old,
            Self::Duration => new > old,
        }
    }
}

/// A numeric tolerance occurrence: the subject, the value, and its polarity.
#[derive(Debug)]
struct ToleranceForm {
    subject: String,
    value: f64,
    polarity: TolerancePolarity,
}

/// The subject text at `span`, read from the original line.
fn subject_at(view: &str, original: &str, span: Span) -> Option<String> {
    let (start, end) = trim_span(view, span);
    let subject = original.get(start..end)?.trim();
    (!subject.is_empty()).then(|| subject.to_string())
}

/// Extract the JS/TS matcher subject and strictness from a line.
fn js_matcher_form_views(view: &str, original: &str) -> Option<MatcherForm> {
    let expect = inner_args_span(view, "expect(")?;
    let subject = subject_at(view, original, expect)?;
    let (_, strength) = JS_MATCHERS
        .iter()
        .find(|(marker, _)| view.contains(*marker))?;
    Some(MatcherForm {
        subject,
        strength: *strength,
        kind: MatcherKind::Js,
    })
}

/// Extract the Python assert-call or `assert` statement subject and strictness.
fn py_matcher_form_views(view: &str, original: &str) -> Option<MatcherForm> {
    for (marker, strength) in PY_ASSERT_CALLS {
        if let Some(inner) = inner_args_span(view, marker)
            && let Some(subject_span) =
                split_top_level_spans(&view[inner.0..inner.1], inner.0).first()
            && let Some(subject) = subject_at(view, original, *subject_span)
        {
            return Some(MatcherForm {
                subject,
                strength: *strength,
                kind: MatcherKind::Python,
            });
        }
    }
    py_assert_statement_form_views(view, original)
}

fn py_assert_statement_form_views(view: &str, original: &str) -> Option<MatcherForm> {
    let rest_start = view.len() - view.strip_prefix("assert ")?.len();
    let rest = &view[rest_start..];
    if let Some((left, _)) = rest.split_once(" == ") {
        let span = trim_span(view, (rest_start, rest_start + left.len()));
        if let Some(subject) = subject_at(view, original, span)
            && !subject.contains('=')
        {
            return Some(MatcherForm {
                subject,
                strength: EXACT_MIN,
                kind: MatcherKind::Python,
            });
        }
    }
    if let Some((left, right)) = rest.split_once(" is not ") {
        let span = trim_span(view, (rest_start, rest_start + left.len()));
        let right_span = trim_span(
            view,
            (
                rest_start + left.len() + " is not ".len(),
                rest_start + left.len() + " is not ".len() + right.len(),
            ),
        );
        let right_value = original.get(right_span.0..right_span.1)?.trim();
        if right_value == "None"
            && let Some(subject) = subject_at(view, original, span)
        {
            return Some(MatcherForm {
                subject,
                strength: LOOSE_MAX,
                kind: MatcherKind::Python,
            });
        }
    }
    None
}

fn matcher_form_views(view: &str, original: &str) -> Option<MatcherForm> {
    py_matcher_form_views(view, original).or_else(|| js_matcher_form_views(view, original))
}

/// Extract the subject a snapshot/approval assertion supersedes.
fn snapshot_subject_views(view: &str, original: &str) -> Option<String> {
    let marker = *SNAPSHOT_MATCHERS
        .iter()
        .find(|marker| view.contains(**marker))?;
    if marker.starts_with(".toMatch") {
        let expect = inner_args_span(view, "expect(")?;
        return subject_at(view, original, expect);
    }
    let inner = inner_args_span(view, marker)?;
    let first = split_top_level_spans(&view[inner.0..inner.1], inner.0)
        .first()
        .copied()?;
    subject_at(view, original, first)
}

/// Extract a numeric tolerance occurrence (places/delta/rtol/precision/approx/sleep duration).
fn tolerance_form_views(view: &str, original: &str) -> Option<ToleranceForm> {
    for marker in [
        "assertAlmostEqual(",
        "assertAlmostEquals(",
        "assert_allclose(",
    ] {
        if let Some(inner) = inner_args_span(view, marker) {
            let subject_span = split_top_level_spans(&view[inner.0..inner.1], inner.0)
                .first()
                .copied()?;
            let subject = subject_at(view, original, subject_span)?;
            if let Some(value) = parse_after_views(view, original, "places=") {
                return Some(ToleranceForm {
                    subject,
                    value,
                    polarity: TolerancePolarity::Precision,
                });
            }
            if let Some(value) = parse_after_views(view, original, "delta=") {
                return Some(ToleranceForm {
                    subject,
                    value,
                    polarity: TolerancePolarity::Duration,
                });
            }
            if let Some(value) = parse_after_views(view, original, "rtol=")
                .or_else(|| parse_after_views(view, original, "atol="))
            {
                return Some(ToleranceForm {
                    subject,
                    value,
                    polarity: TolerancePolarity::Duration,
                });
            }
        }
    }
    if inner_args_span(view, "pytest.approx(").is_some() {
        let subject = py_matcher_form_views(view, original)?.subject;
        let value = parse_after_views(view, original, "rel=")
            .or_else(|| parse_after_views(view, original, "abs="))?;
        return Some(ToleranceForm {
            subject,
            value,
            polarity: TolerancePolarity::Duration,
        });
    }
    if let Some(inner) = inner_args_span(view, "toBeCloseTo(") {
        let expect = inner_args_span(view, "expect(")?;
        let subject = subject_at(view, original, expect)?;
        let spans = split_top_level_spans(&view[inner.0..inner.1], inner.0);
        let value_span = *spans.get(1)?;
        let value = original.get(value_span.0..value_span.1)?;
        let value = parse_first_number(value)?;
        return Some(ToleranceForm {
            subject,
            value,
            polarity: TolerancePolarity::Precision,
        });
    }
    for marker in SLEEP_MARKERS {
        if let Some(inner) = inner_args_span(view, marker) {
            let value = original.get(inner.0..inner.1)?;
            if let Some(value) = parse_first_number(value) {
                return Some(ToleranceForm {
                    subject: marker.trim_end_matches('(').to_string(),
                    value,
                    polarity: TolerancePolarity::Duration,
                });
            }
        }
    }
    None
}

fn parse_after_views(view: &str, original: &str, key: &str) -> Option<f64> {
    let index = view.find(key)? + key.len();
    parse_first_number(&original[index..])
}

fn parse_first_number(text: &str) -> Option<f64> {
    let mut digits = String::new();
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character.is_ascii_digit() {
            digits.push(character);
            started = true;
        } else if character == '.' && started {
            digits.push(character);
        } else if matches!(character, 'e' | 'E') && started {
            digits.push(character);
            if let Some(sign @ ('+' | '-')) = chars.peek().copied() {
                digits.push(sign);
                let _ = chars.next();
            }
        } else if started {
            break;
        }
    }
    digits.trim_end_matches('.').parse().ok()
}

/// The diff-aware presence rules: `weakened-assertion`, `snapshot-regenerated`,
/// `tolerance-broadened`. With no baseline (`Write`) they report at Low confidence; on an `Edit`
/// they fire only when a stricter form for the same subject was removed.
fn classify_weakening(
    file_path: &str,
    line: &AddedLine<'_>,
    view: &str,
    original: &str,
    diff: &DiffContext<'_>,
) -> Vec<Finding> {
    let weakened = matcher_form_views(view, original);
    let snapshot = snapshot_subject_views(view, original);
    let tolerance = tolerance_form_views(view, original);
    if diff.removed.is_empty() {
        return presence_weakening(
            file_path,
            line,
            weakened.as_ref(),
            snapshot.as_deref(),
            tolerance.as_ref(),
        );
    }

    let mut findings = Vec::new();
    if let Some(new_form) = &weakened
        && let Some(old_strength) =
            weaker_match(&new_form.subject, new_form.kind, new_form.strength, diff)
    {
        let confidence = if old_strength >= EXACT_MIN && new_form.strength <= LOOSE_MAX {
            Confidence::High
        } else {
            Confidence::Medium
        };
        findings.push(finish(
            "weakened-assertion",
            confidence,
            "The assertion was loosened to a weaker matcher.",
            line,
            file_path,
        ));
    }
    if let Some(snapshot_subject) = snapshot.as_deref() {
        let superseded = diff
            .removed
            .iter()
            .zip(diff.removed_original)
            .any(|(view, original)| {
                matcher_form_views(view, original)
                    .is_some_and(|old| old.subject == snapshot_subject && old.strength >= EXACT_MIN)
            });
        if superseded {
            findings.push(finish(
                "snapshot-regenerated",
                Confidence::High,
                "A snapshot/approval assertion may have been regenerated without review.",
                line,
                file_path,
            ));
        }
    }
    if let Some(new_tolerance) = &tolerance {
        let widened = diff
            .removed
            .iter()
            .zip(diff.removed_original)
            .any(|(view, original)| {
                tolerance_form_views(view, original).is_some_and(|old| {
                    old.subject == new_tolerance.subject
                        && old.polarity == new_tolerance.polarity
                        && new_tolerance
                            .polarity
                            .loosened(old.value, new_tolerance.value)
                })
            });
        if widened {
            findings.push(finish(
                "tolerance-broadened",
                Confidence::Medium,
                "A tolerance/timeout/sleep may have been widened to hide flakiness.",
                line,
                file_path,
            ));
        }
    }
    findings
}

fn weaker_match(
    subject: &str,
    kind: MatcherKind,
    strength: u8,
    diff: &DiffContext<'_>,
) -> Option<u8> {
    diff.removed
        .iter()
        .zip(diff.removed_original)
        .find_map(|(view, original)| {
            matcher_form_views(view, original).and_then(|old| {
                (old.kind == kind && old.subject == subject && old.strength > strength)
                    .then_some(old.strength)
            })
        })
}

fn presence_weakening(
    file_path: &str,
    line: &AddedLine<'_>,
    weakened: Option<&MatcherForm>,
    snapshot: Option<&str>,
    tolerance: Option<&ToleranceForm>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    if weakened.is_some_and(MatcherForm::is_loose) {
        findings.push(finish(
            "weakened-assertion",
            Confidence::Low,
            "The assertion uses a loose matcher with no baseline to compare against (Write).",
            line,
            file_path,
        ));
    }
    if snapshot.is_some() {
        findings.push(finish(
            "snapshot-regenerated",
            Confidence::Low,
            "A snapshot/approval assertion may have been regenerated without review (Write).",
            line,
            file_path,
        ));
    }
    if tolerance.is_some() {
        findings.push(finish(
            "tolerance-broadened",
            Confidence::Low,
            "A tolerance/timeout/sleep may have been widened to hide flakiness (Write).",
            line,
            file_path,
        ));
    }
    findings
}

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

/// Classify one added line of a harness/config file (outside the test-file gate).
#[must_use]
pub fn classify_harness_line(file_path: &str, line: &AddedLine<'_>) -> Option<Finding> {
    has_marker(line.text, GATE_WEAKENING_MARKERS).then(|| {
        finish(
            "gate-weakened",
            Confidence::Medium,
            "A test gate can be bypassed here (a failure is ignored or tests may be absent).",
            line,
            file_path,
        )
    })
}

/// The `harness-edited` finding for an added non-blank line of a harness/config file.
///
/// Policy D29: a rule meant to change agent behaviour must be Medium or higher. The sensitive
/// harness paths (CI workflows and the pytest/tox/conftest and jest/vitest configs) clear that bar;
/// every other harness file stays Low telemetry.
pub(crate) fn harness_edited_finding(file_path: &str, line: &AddedLine<'_>) -> Finding {
    let confidence = if is_sensitive_harness_path(file_path) {
        Confidence::Medium
    } else {
        Confidence::Low
    };
    finish(
        "harness-edited",
        confidence,
        "A test-harness/config file was edited; confirm the gate was not weakened.",
        line,
        file_path,
    )
}

/// Classify one added line against the line-oriented rules (presence-only, no diff baseline).
#[must_use]
pub fn classify_added_line(file_path: &str, added: &[AddedLine<'_>], index: usize) -> Vec<Finding> {
    classify_line(
        file_path,
        added,
        index,
        &DiffContext::default(),
        added_text(added, index),
    )
}

/// Classify one added line with the removed lines of an edit, so the presence-style rules become
/// diff-aware (fire only when a stricter form for the same subject was replaced).
#[must_use]
pub fn classify_diff_line(
    file_path: &str,
    added: &[AddedLine<'_>],
    index: usize,
    diff: &DiffContext<'_>,
) -> Vec<Finding> {
    classify_line(file_path, added, index, diff, added_text(added, index))
}

/// Classify the line against the masked view while reading `original` for the comment/text rules
/// (`agent-memo-test` names the strings a test carries, which the masked view has blanked).
#[must_use]
pub fn classify_added_line_with_original(
    file_path: &str,
    added: &[AddedLine<'_>],
    index: usize,
    original: &str,
) -> Vec<Finding> {
    classify_line(file_path, added, index, &DiffContext::default(), original)
}

/// [`classify_added_line_with_original`] with an edit's removed lines, for the diff-aware rules.
#[must_use]
pub fn classify_diff_line_with_original(
    file_path: &str,
    added: &[AddedLine<'_>],
    index: usize,
    original: &str,
    diff: &DiffContext<'_>,
) -> Vec<Finding> {
    classify_line(file_path, added, index, diff, original)
}

fn added_text<'a>(added: &'a [AddedLine<'a>], index: usize) -> &'a str {
    added.get(index).map_or("", |line| line.text)
}

fn classify_line(
    file_path: &str,
    added: &[AddedLine<'_>],
    index: usize,
    diff: &DiffContext<'_>,
    original: &str,
) -> Vec<Finding> {
    let Some(line) = added.get(index).copied() else {
        return Vec::new();
    };
    let text = line.text.trim();
    let original = original.trim();
    let mut findings = Vec::new();

    for rule in MARKER_RULES {
        if has_marker(text, rule.markers) {
            findings.push(finish(
                rule.id,
                rule.confidence,
                rule.message,
                &line,
                file_path,
            ));
        }
    }
    if SKIP_STATEMENT_MARKERS
        .iter()
        .any(|marker| text.starts_with(marker))
    {
        findings.push(finish(
            "skip-added",
            Confidence::High,
            SKIP_ADDED_MESSAGE,
            &line,
            file_path,
        ));
    }
    findings.extend(classify_weakening(file_path, &line, text, original, diff));
    if let Some(finding) = classify_assertion(file_path, &line, text, original) {
        findings.push(finding);
    }
    if let Some(finding) = classify_mirror_implementation(file_path, &line, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_swallowed_error(file_path, added, index, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_conditional_assertion(file_path, added, index, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_mock_of_sut(file_path, &line, text, original) {
        findings.push(finding);
    }
    if let Some(finding) = classify_agent_memo(file_path, &line, original) {
        findings.push(finding);
    }
    if let Some(finding) = classify_incomplete_assertion(file_path, &line, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_unawaited_assertion(file_path, &line, text) {
        findings.push(finding);
    }
    if let Some(finding) = classify_broad_exception(file_path, &line, text) {
        findings.push(finding);
    }
    findings
}

fn classify_incomplete_assertion(
    file_path: &str,
    line: &AddedLine<'_>,
    text: &str,
) -> Option<Finding> {
    // The `expect(...)` call must close on this line; a prettier-wrapped
    // `expect(\n  …\n).toBe(3)` is complete, just split across lines.
    inner_args(text, "expect(")?;
    if INCOMPLETE_ASSERTION_MATCHERS
        .iter()
        .any(|matcher| text.contains(matcher))
    {
        return None;
    }
    Some(finish(
        "incomplete-assertion",
        Confidence::High,
        "An expect(...) has no matcher, so the assertion never runs.",
        line,
        file_path,
    ))
}

fn classify_unawaited_assertion(
    file_path: &str,
    line: &AddedLine<'_>,
    text: &str,
) -> Option<Finding> {
    // The promise matcher only counts on an `expect(...)` line; `.resolves.` in a string
    // (a test name, a doc) is not an unawaited assertion.
    let promise_matcher = text.contains(".resolves.") || text.contains(".rejects.");
    if !promise_matcher || !text.contains("expect(") {
        return None;
    }
    let trimmed = text.trim_start();
    if trimmed.starts_with("await ")
        || trimmed.starts_with("return ")
        || trimmed.contains("return await")
    {
        return None;
    }
    Some(finish(
        "unawaited-assertion",
        Confidence::High,
        "A promise assertion is not awaited or returned, so it never blocks.",
        line,
        file_path,
    ))
}

fn classify_broad_exception(file_path: &str, line: &AddedLine<'_>, text: &str) -> Option<Finding> {
    let broad = (text.contains("raises(Exception") && !text.contains("match="))
        || text.contains("raises(BaseException")
        || text.contains("assertRaises(Exception")
        || text.contains("assertThrows(Exception.class");
    let should_panic = text.contains("#[should_panic") && !text.contains("expected");
    (broad || should_panic).then(|| {
        finish(
            "broad-exception",
            Confidence::Medium,
            "The expected exception is too broad; the test can pass for the wrong reason.",
            line,
            file_path,
        )
    })
}

fn classify_assertion(
    file_path: &str,
    line: &AddedLine<'_>,
    text: &str,
    original: &str,
) -> Option<Finding> {
    let operands = assertion_operands_with_original(text, original);
    let literal_pair = operands
        .as_ref()
        .is_some_and(|(left, right)| is_literal(left) && is_literal(right) && left == right);
    if literal_pair || is_constant_bool_assertion(text) {
        return Some(finish(
            "constant-assertion",
            Confidence::High,
            "The assertion compares a literal with a literal and cannot fail.",
            line,
            file_path,
        ));
    }
    if let Some((left, right)) = operands {
        // Values come from the original line, so distinct strings (`"a"`/`"b"`) never collapse
        // into either a literal pair or a tautology.
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

/// `mirror-implementation-test`: the expected value is built from the same arguments with the same
/// shape as the call under test (`assert add(a, b) == a + b`).
fn classify_mirror_implementation(
    file_path: &str,
    line: &AddedLine<'_>,
    text: &str,
) -> Option<Finding> {
    let code = code_only(text);
    let (left, right) = assertion_operands(&code)?;
    if !mirror_pair(&left, &right) && !mirror_pair(&right, &left) {
        return None;
    }
    Some(finish(
        "mirror-implementation-test",
        MIRROR_IMPLEMENTATION_CONFIDENCE,
        "The expected value restates the code under test instead of pinning its behaviour.",
        line,
        file_path,
    ))
}

/// The line with the contents of string literals blanked, so an assertion quoted inside a fixture
/// string is not mistaken for code.
fn code_only(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    for character in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = false;
                out.push(character);
            } else {
                out.push(' ');
            }
            continue;
        }
        if matches!(character, '"' | '\'' | '`') {
            in_string = true;
            quote = character;
        }
        out.push(character);
    }
    out
}

fn mirror_pair(call: &str, expected: &str) -> bool {
    let Some(args) = simple_call_arguments(call) else {
        return false;
    };
    if !args.iter().any(|arg| !is_literal(arg)) {
        return false;
    }
    if !args.iter().any(|arg| contains_token(expected, arg)) {
        return false;
    }
    if contains_call(expected) {
        return false;
    }
    let expected = trim_outer_parens(expected.trim());
    has_binary_operator(expected) && mirror_expression_matches(expected, &args)
}

/// The arguments of a simple call expression (`add(a, b)`, `self.add(a)`), or `None` when the
/// expression is not exactly one call.
fn simple_call_arguments(expr: &str) -> Option<Vec<String>> {
    let expr = trim_outer_parens(expr.trim());
    let open = expr.find('(')?;
    let name = expr[..open].trim();
    if !is_call_name(name) {
        return None;
    }
    let inner = inner_args(expr, "(")?;
    let close = open + 1 + inner.len();
    if expr[close..].trim() != ")" {
        return None;
    }
    let args = split_top_level(inner);
    if args.is_empty() {
        return None;
    }
    Some(
        args.into_iter()
            .map(|arg| arg.trim().to_string())
            .filter(|arg| !arg.is_empty())
            .collect(),
    )
}

fn is_call_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':' | '$')
        })
}

/// A value that is a mirrored operand: an argument of the call, a literal, or a nested expression
/// over those.
fn mirror_expression_matches(expr: &str, args: &[String]) -> bool {
    let expr = trim_outer_parens(expr.trim());
    if args.iter().any(|arg| arg.trim() == expr) || is_literal(expr) {
        return true;
    }
    let Some((left, right)) = split_binary_expression(expr) else {
        return false;
    };
    mirror_expression_matches(left, args) && mirror_expression_matches(right, args)
}

fn has_binary_operator(expr: &str) -> bool {
    split_binary_expression(expr).is_some()
}

/// Split at the first top-level arithmetic operator, ignoring strings and nested delimiters.
fn split_binary_expression(expr: &str) -> Option<(&str, &str)> {
    let bytes = expr.as_bytes();
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut quote = 0_u8;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == quote {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => {
                in_string = true;
                quote = byte;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ if depth == 0 && binary_operator_len(bytes, index).is_some() => {
                let left = expr[..index].trim();
                let right = expr[index + 1..].trim();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, right));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn binary_operator_len(bytes: &[u8], index: usize) -> Option<usize> {
    let byte = bytes[index];
    if !matches!(byte, b'+' | b'-' | b'*' | b'/' | b'%') {
        return None;
    }
    let previous = index.checked_sub(1).and_then(|offset| bytes.get(offset));
    let next = bytes.get(index + 1);
    // Assignment, comparison, arrow, increment and double-slash/power forms are not binary
    // arithmetic operators for this check.
    if matches!(next, Some(b'=' | b'>' | b'+' | b'-' | b'/' | b'*'))
        || matches!(
            previous,
            Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|')
        )
    {
        return None;
    }
    Some(1)
}

fn contains_call(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'(' || index == 0 {
            continue;
        }
        let mut cursor = index;
        while cursor > 0 {
            let previous = bytes[cursor - 1];
            if previous.is_ascii_whitespace() {
                cursor -= 1;
                continue;
            }
            if is_identifier_byte(previous) {
                return true;
            }
            break;
        }
    }
    false
}

fn contains_token(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while from < haystack.len() {
        let Some(offset) = haystack[from..].find(needle) else {
            return false;
        };
        let index = from + offset;
        let end = index + needle.len();
        let before_ok = index == 0 || !is_identifier_byte(bytes[index - 1]);
        let after_ok = end == bytes.len() || !is_identifier_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = index + 1;
    }
    false
}

fn trim_outer_parens(text: &str) -> &str {
    let mut current = text.trim();
    while current.starts_with('(') && current.ends_with(')') {
        let Some(inner) = inner_args(current, "(") else {
            break;
        };
        if inner.len() + 2 != current.len() {
            break;
        }
        current = inner.trim();
    }
    current
}

/// A value written by one statement: `<receiver>.<field> = value`, `<receiver>.setField(value)`,
/// or `<receiver> = Type(value)`.
#[derive(Debug)]
struct EchoWrite {
    receiver: String,
    field: Option<String>,
    value: String,
}

/// `echo-test`: a test writes a value and reads the same value back through a trivial accessor
/// with no statement in between.
fn classify_echo(
    file_path: &str,
    body: &[AddedLine<'_>],
    added: Option<&HashSet<usize>>,
) -> Option<Finding> {
    let meaningful: Vec<&AddedLine<'_>> = body
        .iter()
        .filter(|line| !line.text.trim().is_empty())
        .collect();
    for index in 0..meaningful.len().saturating_sub(1) {
        let Some(write) = parse_echo_write(meaningful[index].text) else {
            continue;
        };
        let assertion = meaningful[index + 1];
        if let Some(lines) = added {
            let write_line = meaningful[index].line_number - 1;
            let assertion_line = assertion.line_number - 1;
            if !lines.contains(&write_line) && !lines.contains(&assertion_line) {
                continue;
            }
        }
        if echo_assertion_operands(&code_only(assertion.text)).is_none() {
            continue;
        }
        let Some((left, right)) = echo_assertion_operands(assertion.text.trim()) else {
            continue;
        };
        let matches = (echo_read_matches(&left, &write)
            && echo_value_matches(&right, &write.value))
            || (echo_read_matches(&right, &write) && echo_value_matches(&left, &write.value));
        if matches {
            return Some(finish(
                "echo-test",
                ECHO_TEST_CONFIDENCE,
                "The test writes a value and reads it back unchanged through a trivial accessor.",
                assertion,
                file_path,
            ));
        }
    }
    None
}

/// The edit-path entry point: run `echo-test` over a touched body, emitting only when the
/// assertion line itself was added by the edit and the fragment contains the complete test.
pub(crate) fn classify_echo_body(
    file_path: &str,
    body: &[AddedLine<'_>],
    reliable: bool,
    added: Option<&HashSet<usize>>,
) -> Option<Finding> {
    if !reliable {
        return None;
    }
    classify_echo(file_path, body, added)
}

fn parse_echo_write(line: &str) -> Option<EchoWrite> {
    let text = line.trim().trim_end_matches(';').trim();
    if text.is_empty() {
        return None;
    }
    if let Some((field, receiver, value)) = parse_setter_call(text) {
        return Some(EchoWrite {
            receiver,
            field: Some(normalize_echo_field(&field)),
            value,
        });
    }

    let (left, right) = split_assignment(text)?;
    let target = left.split_whitespace().last()?.trim();
    if target.is_empty() {
        return None;
    }
    if let Some((receiver, field)) = target.rsplit_once('.') {
        let receiver = receiver.trim();
        let field = field.trim();
        if !is_call_name(receiver) || field.is_empty() {
            return None;
        }
        return Some(EchoWrite {
            receiver: receiver.to_string(),
            field: Some(normalize_echo_field(field)),
            value: right.trim().to_string(),
        });
    }
    Some(EchoWrite {
        receiver: target.to_string(),
        field: None,
        value: single_constructor_value(right)?,
    })
}

/// `obj.set_field(value)` / `obj.setField(value)`.
fn parse_setter_call(text: &str) -> Option<(String, String, String)> {
    let dot = text.find(".set")?;
    let receiver = text[..dot].trim();
    if !is_call_name(receiver) {
        return None;
    }
    let open = dot + text[dot..].find('(')?;
    let method = &text[dot + 1..open];
    if !method.starts_with("set") || method.len() == 3 {
        return None;
    }
    let inner = inner_args(text, "(")?;
    let close = open + 1 + inner.len();
    if text[close..].trim() != ")" {
        return None;
    }
    let args = split_top_level(inner);
    if args.len() != 1 {
        return None;
    }
    let value = args[0].trim().to_string();
    if value.is_empty() {
        return None;
    }
    Some((method[3..].trim().to_string(), receiver.to_string(), value))
}

fn split_assignment(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut quote = 0_u8;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == quote {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => {
                in_string = true;
                quote = byte;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => {
                let previous = index.checked_sub(1).and_then(|offset| bytes.get(offset));
                let next = bytes.get(index + 1);
                if matches!(next, Some(b'=' | b'>'))
                    || matches!(
                        previous,
                        Some(
                            b'=' | b'!'
                                | b'<'
                                | b'>'
                                | b'+'
                                | b'-'
                                | b'*'
                                | b'/'
                                | b'%'
                                | b'&'
                                | b'|'
                        )
                    )
                {
                    index += 1;
                    continue;
                }
                let left = text[..index].trim();
                let right = text[index + 1..].trim();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, right));
                }
                return None;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// The one value carried by `Type(value)`, `new Type(value)`, `Type::new(value)`, or
/// `Type { field: value }`.
fn single_constructor_value(right: &str) -> Option<String> {
    let right = right.trim();
    if let Some(inner) = right.strip_prefix("new ") {
        let inner = inner.trim();
        let open = inner.find('(')?;
        let args = inner_args(inner, "(")?;
        let close = open + 1 + args.len();
        if inner[close..].trim() != ")" {
            return None;
        }
        let args = split_top_level(args);
        return (args.len() == 1).then(|| args[0].trim().to_string());
    }
    if let Some(open) = right.find('(') {
        let name = right[..open].trim().trim_end_matches("::new").trim();
        if !is_call_name(name) {
            return None;
        }
        let inner = inner_args(right, "(")?;
        let close = open + 1 + inner.len();
        if right[close..].trim() != ")" {
            return None;
        }
        let args = split_top_level(inner);
        return (args.len() == 1).then(|| args[0].trim().to_string());
    }
    // Rust struct literal: `Type { field: value }`.
    let open = right.find('{')?;
    let close = right.rfind('}')?;
    if close <= open {
        return None;
    }
    let field = right[open + 1..close].trim();
    let (_, value) = field.split_once(':')?;
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn normalize_echo_field(field: &str) -> String {
    field
        .trim()
        .trim_start_matches('_')
        .replace('_', "")
        .to_ascii_lowercase()
}

fn echo_read_matches(operand: &str, write: &EchoWrite) -> bool {
    let operand = trim_outer_parens(operand.trim());
    let prefix = format!("{}.", write.receiver);
    let Some(rest) = operand.strip_prefix(&prefix) else {
        return false;
    };
    let rest = rest.trim().trim_end_matches("()").trim();
    let field = strip_getter_prefix(rest).unwrap_or(rest);
    if field.is_empty() || !field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    write
        .field
        .as_ref()
        .is_none_or(|expected| normalize_echo_field(field) == *expected)
}

fn strip_getter_prefix(field: &str) -> Option<&str> {
    for prefix in ["get_", "Get_", "get", "Get"] {
        if let Some(rest) = field.strip_prefix(prefix)
            && (prefix.ends_with('_')
                || rest.starts_with(|character: char| character.is_ascii_uppercase()))
        {
            return Some(rest);
        }
    }
    None
}

fn echo_value_matches(operand: &str, value: &str) -> bool {
    let operand = trim_outer_parens(operand.trim());
    let value = trim_outer_parens(value.trim());
    !operand.is_empty() && compact_whitespace(operand) == compact_whitespace(value)
}

fn compact_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The two sides of an equality-style assertion for frameworks not covered by
/// [`assertion_operands`].
fn echo_assertion_operands(text: &str) -> Option<(String, String)> {
    if let Some(operands) = assertion_operands(text) {
        return Some(operands);
    }
    for marker in [
        "XCTAssertEqual(",
        "EXPECT_EQ(",
        "ASSERT_EQ(",
        "assert.are.equal(",
        "assert.equal(",
    ] {
        if let Some(inner) = inner_args(text, marker) {
            let args = split_top_level(inner);
            if let [left, right] = args.as_slice() {
                return Some((left.trim().to_string(), right.trim().to_string()));
            }
        }
    }
    if let Some(rest) = text.strip_prefix("assert_equal ") {
        let args = split_top_level(rest);
        if let [left, right] = args.as_slice() {
            return Some((left.trim().to_string(), right.trim().to_string()));
        }
    }
    if let Some(inner) = inner_args(text, "assert(") {
        if let Some((left, right)) = inner.split_once("==") {
            let (left, right) = (left.trim(), right.trim());
            if !left.is_empty() && !right.is_empty() {
                return Some((left.to_string(), right.to_string()));
            }
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
    let swallows = if text.contains("except") {
        let tail = text.split("except").nth(1).unwrap_or("");
        has_word(tail, "pass") || next == Some("pass")
    } else if has_word(text, "catch") {
        let brace_empty = text.contains("{}") || text.contains("{ }");
        brace_empty || next == Some("}")
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
    let conditional_here = text.starts_with("if ") || text.starts_with("if(");
    if !conditional_here {
        return None;
    }
    let assertions = block_assertions(added, index);
    if assertions.is_empty() {
        return None;
    }
    let condition = conditional_text(text);
    let guard_idents = guard_identifiers(&condition);
    let guard_condition = is_guard_condition(&condition);
    // An error/type guard around assertions that inspect the guarded value is error handling, not
    // an optional assertion. A guard around unrelated assertions is still optional and reports.
    if guard_condition
        && !guard_idents.is_empty()
        && assertions
            .iter()
            .all(|assertion| guard_idents.iter().any(|ident| has_word(assertion, ident)))
    {
        return None;
    }
    Some(finish(
        "conditional-assertion",
        Confidence::High,
        "The assertion is guarded by a runtime condition and may never run.",
        &added[index],
        file_path,
    ))
}

/// The condition text of an `if` line: `if (cond) {`, `if cond {`, `if cond:`.
fn conditional_text(text: &str) -> String {
    let rest = text.strip_prefix("if").unwrap_or(text).trim_start();
    let parenthesized = rest.starts_with('(');
    let rest = rest.strip_prefix('(').unwrap_or(rest);
    let end = rest
        .find('{')
        .into_iter()
        .chain(rest.find(':'))
        .min()
        .unwrap_or(rest.len());
    let condition = rest[..end].trim();
    if parenthesized {
        condition.trim_end_matches(')').trim().to_owned()
    } else {
        condition.to_owned()
    }
}

/// Whether a condition is an error/type/availability guard rather than a business condition.
fn is_guard_condition(condition: &str) -> bool {
    const WORDS: &[&str] = &["err", "error", "exitErr", "ok"];
    const FORMS: &[&str] = &[
        "isinstance(",
        "instanceof ",
        "is None",
        "is not None",
        "let Some(",
        "let Ok(",
        ".is_err()",
        ".is_ok()",
        ".ok()",
        "!= nil",
        "== nil",
        "!= null",
        "== null",
    ];
    WORDS.iter().any(|word| has_word(condition, word))
        || FORMS.iter().any(|form| condition.contains(form))
}

/// Identifiers a guard condition carries, used to tell an error-path assertion from an optional
/// assertion. Calls (`isinstance(`, `get(`) and language keywords are not values.
fn guard_identifiers(condition: &str) -> Vec<String> {
    const KEYWORDS: &[&str] = &[
        "if",
        "let",
        "mut",
        "else",
        "is",
        "not",
        "and",
        "or",
        "in",
        "case",
        "new",
        "instanceof",
        "nil",
        "none",
        "null",
        "undefined",
        "true",
        "false",
        "self",
        "this",
    ];
    let mut identifiers = Vec::new();
    let bytes = condition.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let ident = &condition[start..index];
            let next = condition[index..].trim_start();
            if !next.starts_with('(') && !KEYWORDS.contains(&ident.to_ascii_lowercase().as_str()) {
                identifiers.push(ident.to_owned());
            }
        } else {
            index += 1;
        }
    }
    identifiers
}

/// Assertion texts in the `if` block that starts at `index`.
fn block_assertions<'a>(added: &'a [AddedLine<'a>], index: usize) -> Vec<&'a str> {
    let first = added.get(index).map_or("", |line| line.text);
    let mut assertions = Vec::new();
    if has_marker(first.trim(), ASSERTION_MARKERS) {
        assertions.push(first.trim());
    }
    let colon_style = first.trim_end().ends_with(':');
    let mut depth = brace_delta(first);
    if !colon_style && depth == 0 {
        if assertions.is_empty()
            && let Some(next) = added.get(index + 1)
            && has_marker(next.text.trim(), ASSERTION_MARKERS)
        {
            assertions.push(next.text.trim());
        }
        return assertions;
    }
    let indent = indentation(first);
    for line in added.iter().skip(index + 1) {
        let text = line.text;
        if text.trim().is_empty() {
            continue;
        }
        if depth <= 0 && indentation(text) <= indent && !text.trim_start().starts_with('}') {
            break;
        }
        if has_marker(text.trim(), ASSERTION_MARKERS) {
            assertions.push(text.trim());
        }
        depth += brace_delta(text);
        if depth <= 0 {
            break;
        }
    }
    assertions
}

fn indentation(line: &str) -> usize {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |depth, character| match character {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

fn classify_mock_of_sut(
    file_path: &str,
    line: &AddedLine<'_>,
    text: &str,
    original: &str,
) -> Option<Finding> {
    let stem = subject_stem(file_path);
    if stem.is_empty() || !has_marker(text, MOCK_MARKERS) {
        return None;
    }
    // The mock callee is code (masked view); the module it targets is a string literal
    // (original text), so a `mock.patch(` that only appears inside a string cannot fire.
    has_word(original, &stem).then(|| {
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

/// The literal a test-level message occupies on an assertion line, if it is a bare quoted string.
///
/// Only the message argument counts: the top-level argument list is split (quotes and nesting
/// respected) and the last argument must itself be a quoted literal. A fixture passed to a helper
/// (`assert!(is_agent_memo_phrase("Changed from X to Y"))`) is a nested call, so it is not a message.
#[must_use]
pub(crate) fn assertion_message_literal(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("assert ") {
        let spans = split_top_level_spans(rest, 0);
        if spans.len() >= 2 {
            return quoted_literal(&rest[spans[1].0..spans[1].1]);
        }
    }
    for (marker, min_args) in [
        ("debug_assert!(", 2),
        ("assert_eq!(", 3),
        ("assert_ne!(", 3),
        ("assert!(", 2),
        ("assertEquals(", 3),
        ("assertEqual(", 3),
        ("assert.deepEqual(", 3),
        ("assert.equal(", 3),
        ("assertThat(", 3),
        (".NotContains(", 4),
        (".Contains(", 4),
        (".NotEqual(", 4),
        (".Equal(", 4),
        (".EqualValues(", 4),
    ] {
        if let Some(inner) = inner_args_span(line, marker) {
            let spans = split_top_level_spans(&line[inner.0..inner.1], inner.0);
            if spans.len() >= min_args {
                let last = spans[spans.len() - 1];
                return quoted_literal(&line[last.0..last.1]);
            }
        }
    }
    None
}

fn quoted_literal(text: &str) -> Option<String> {
    let text = text.trim();
    for quote in ['"', '\'', '`'] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            return Some(text[1..text.len() - 1].to_owned());
        }
    }
    None
}

/// Whether `value` (an already-unquoted literal) names a source file by its extension.
fn is_source_file_name(value: &str) -> bool {
    let Some(extension) = std::path::Path::new(value)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
    else {
        return false;
    };
    SOURCE_FILE_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

/// Whether the line reads a source file: a read callee is code (masked `view`) and a literal in
/// `original` names source code. The callee and the literal may be ordered either way, so
/// `Path("…").read_text()`, `File("…").readText()`, and `read_file("…")` all count.
fn reads_source_file_views(view: &str, original: &str) -> bool {
    SOURCE_READ_CALLEES
        .iter()
        .any(|callee| view.contains(callee))
        && quoted_strings(original)
            .iter()
            .any(|literal| is_source_file_name(literal))
}

/// Whether the masked `view` inspects the text of a value rather than the value itself. The
/// markers are code, so a marker that only appears inside a string literal is blanked away.
fn inspects_text(view: &str) -> bool {
    SOURCE_TEXT_MARKERS
        .iter()
        .any(|marker| view.contains(marker))
}

/// The variable that holds the text of a source-file read, if the line is such an assignment.
///
/// The structure comes from the masked `view` (so a string containing `=`, a read callee, or a
/// file name cannot fake an assignment); the file-name literal comes from `original`.
fn source_text_binding(view: &str, original: &str) -> Option<String> {
    if is_comment_line(view) {
        return None;
    }
    let (left, right) = split_assignment_spans(view)?;
    let right_view = view.get(right.0..right.1)?;
    let right_original = original.get(right.0..right.1)?;
    if !reads_source_file_views(right_view, right_original) {
        return None;
    }
    assigned_name(view.get(left.0..left.1)?)
}

/// Split `<left> = <right>` (or `<left> := <right>`), ignoring comparison operators, as byte
/// spans into `text`.
fn split_assignment_spans(text: &str) -> Option<(Span, Span)> {
    let (left_end, right_start) = if let Some(index) = text.find(":=") {
        (index, index + 2)
    } else {
        let bytes = text.as_bytes();
        let mut found = None;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != b'=' {
                continue;
            }
            let previous = index.checked_sub(1).map(|offset| bytes[offset]);
            let next = bytes.get(index + 1).copied();
            if matches!(previous, Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-'))
                || matches!(next, Some(b'=' | b'>'))
            {
                continue;
            }
            found = Some((index, index + 1));
            break;
        }
        found?
    };
    Some(((0, left_end), (right_start, text.len())))
}

/// The name on the left of an assignment: the last identifier before a type annotation, or the
/// first of a Go-style `value, err :=` pair.
fn assigned_name(left: &str) -> Option<String> {
    let without_path_separator = left.replace("::", "");
    let head = without_path_separator
        .split(':')
        .next()
        .unwrap_or(&without_path_separator);
    let tokens: Vec<&str> = head
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .collect();
    let name = if head.contains(',') {
        *tokens.first()?
    } else {
        *tokens.last()?
    };
    (!matches!(
        name,
        "let" | "const" | "var" | "val" | "def" | "local" | "mut"
    ))
    .then(|| name.to_string())
}

/// The assertion line that greps the text of a source file read in this test body.
///
/// `view` is the masked body and `original` the same lines before masking, so the structure
/// (callees, matchers) comes from the view while the read file name and a variable interpolated
/// into a string still come from the original text.
fn source_grep_line<'a, 'b>(
    view: &'a [AddedLine<'b>],
    original: &'a [AddedLine<'b>],
) -> Option<&'a AddedLine<'b>> {
    let bindings: Vec<String> = view
        .iter()
        .zip(original)
        .filter_map(|(line, source)| source_text_binding(line.text.trim(), source.text.trim()))
        .collect();
    view.iter().zip(original).find_map(|(line, source)| {
        let text = line.text.trim();
        let original_text = source.text.trim();
        if is_comment_line(text) || !inspects_text(text) {
            return None;
        }
        (reads_source_file_views(text, original_text)
            || bindings.iter().any(|binding| {
                has_word(text, binding.as_str()) || has_word(original_text, binding.as_str())
            }))
        .then_some(source)
    })
}

/// The number of added tests a single change may carry before `test-volume` reports.
pub(crate) const TEST_VOLUME_LIMIT: usize = 10;

/// The `test-volume` finding when one change adds more than [`TEST_VOLUME_LIMIT`] tests.
pub(crate) fn test_volume_finding(file_path: &str, added_test_lines: &[usize]) -> Option<Finding> {
    if added_test_lines.len() <= TEST_VOLUME_LIMIT {
        return None;
    }
    let first_line = *added_test_lines.iter().min()?;
    Some(Finding::new(
        "test-volume",
        Confidence::Low,
        "More than 10 tests were added in one change; review the volume.",
        first_line,
        file_path,
        "",
    ))
}

/// Classify a captured test body for body-level rules that need no diff.
#[must_use]
pub fn classify_test_body(
    file_path: &str,
    body_start: usize,
    body: &[AddedLine<'_>],
) -> Vec<Finding> {
    classify_test_body_with_original(file_path, body_start, body, body, true)
}

/// Classify a captured test body, suppressing the rules that need the whole test when the parsed
/// fragment is unreliable (it does not contain the complete test).
#[must_use]
#[cfg(test)]
pub(crate) fn classify_test_body_with_reliability(
    file_path: &str,
    body_start: usize,
    body: &[AddedLine<'_>],
    reliable: bool,
) -> Vec<Finding> {
    classify_test_body_with_original(file_path, body_start, body, body, reliable)
}

/// [`classify_test_body`] over the masked view, with `original_body` read only by the rules that
/// depend on literal text (`source-grep-test` names the file a read call opens).
#[must_use]
pub(crate) fn classify_test_body_with_original(
    file_path: &str,
    body_start: usize,
    body: &[AddedLine<'_>],
    original_body: &[AddedLine<'_>],
    reliable: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let meaningful: Vec<&str> = body
        .iter()
        .map(|line| line.text.trim())
        .filter(|line| !line.is_empty())
        .collect();
    let anchor = AddedLine {
        line_number: body_start,
        text: meaningful.first().copied().unwrap_or(""),
    };
    if meaningful.is_empty() {
        if reliable {
            findings.push(finish(
                "gutted-test",
                Confidence::High,
                "The test body is empty.",
                &anchor,
                file_path,
            ));
        }
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
    if reliable && mock_count >= 2 && mock_count > assertion_count {
        findings.push(finish(
            "over-mocking",
            Confidence::Medium,
            "Mock setup outnumbers the assertions; the test may be asserting on a mock.",
            &anchor,
            file_path,
        ));
    }
    let has_assertion = meaningful.iter().any(|line| {
        has_marker(line, ASSERTION_MARKERS) || has_marker(line, ASSERTION_EXISTS_EXTRAS)
    });
    let has_nested_test = meaningful
        .iter()
        .any(|line| has_marker(line, NESTED_TEST_MARKERS));
    if reliable && !has_assertion && !has_nested_test {
        findings.push(finish(
            "no-assertion",
            Confidence::Medium,
            "The test has no assertion and may never fail.",
            &anchor,
            file_path,
        ));
    }
    if reliable && let Some(finding) = classify_echo(file_path, original_body, None) {
        findings.push(finding);
    }
    if has_assertion && let Some(grep_line) = source_grep_line(body, original_body) {
        findings.push(finish(
            "source-grep-test",
            Confidence::Low,
            "This test reads a source file and asserts on its text; test behaviour, not source text.",
            grep_line,
            file_path,
        ));
    }
    findings
}

/// Whether a raw line looks like a comment line (a commented-out assertion is not real code).
pub(crate) fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    // `#[…]` is a Rust attribute (e.g. `#[ignore]`), not a comment.
    if trimmed.starts_with("#[") {
        return false;
    }
    ["#", "//", "--", "/*", "*"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

/// Whether a raw line carries an assertion marker on a code line: a commented-out assertion
/// (`# assert x == y`) must not count towards `assertion-removed`/`gutted-test`.
pub(crate) fn is_assertion_line(line: &str) -> bool {
    has_marker(line.trim(), ASSERTION_MARKERS)
}

fn is_assertion_code_line(line: &str) -> bool {
    !is_comment_line(line) && is_assertion_line(line)
}

/// Diff-only findings: newly introduced weakening between an edit's old and new content.
#[must_use]
pub fn diff_findings(old_content: &str, new_content: &str, file_path: &str) -> Vec<Finding> {
    diff_findings_on_views(
        old_content,
        new_content,
        old_content,
        new_content,
        file_path,
    )
}

/// [`diff_findings`] over the masked views (structure) and the original content (values). Line
/// numbers come from the view, which is length-preserving, so they still name the edited lines.
#[must_use]
pub fn diff_findings_on_views(
    old_view: &str,
    new_view: &str,
    old_original: &str,
    new_original: &str,
    file_path: &str,
) -> Vec<Finding> {
    let old_lines: Vec<&str> = old_view.lines().collect();
    let new_lines: Vec<&str> = new_view.lines().collect();
    let old_original_lines: Vec<&str> = old_original.lines().collect();
    let new_original_lines: Vec<&str> = new_original.lines().collect();
    let mut findings = Vec::new();

    let old_assertions = assertion_keys(&old_lines, &old_original_lines);
    let new_assertions = assertion_keys(&new_lines, &new_original_lines);
    // A removal is a *subject* that no longer has any assertion. Recording the predicate as well
    // keeps two different assertions on one subject from cancelling, and makes an explicit
    // weaker-to-stronger replacement (Contains -> Equal) a predicate change rather than a removal.
    let new_subjects: HashSet<&str> = new_assertions
        .iter()
        .map(|assertion| assertion.subject.as_str())
        .collect();
    let removed = old_assertions
        .iter()
        .filter(|assertion| !new_subjects.contains(assertion.subject.as_str()))
        .count();
    if new_assertions.is_empty() {
        if !old_assertions.is_empty() {
            findings.push(Finding::new(
                "gutted-test",
                Confidence::High,
                "The test was stripped of all assertions (or emptied) by this edit.",
                1,
                file_path,
                "",
            ));
        }
    } else if removed > 0 {
        findings.push(Finding::new(
            "assertion-removed",
            Confidence::High,
            format!(
                "{removed} existing assertion subject(s) were removed without a replacement. STOP and ask the human to add a .test-checker.json entry with a reason if the removal is intended."
            ),
            1,
            file_path,
            "",
        ));
    }

    for (index, new_line) in new_lines.iter().enumerate() {
        let new_original_line = new_original_lines.get(index).copied().unwrap_or("");
        let Some((left, right)) = assertion_operands_with_original(new_line, new_original_line)
        else {
            continue;
        };
        if left != right || is_literal(&left) {
            continue;
        }
        let was_literal =
            old_lines
                .iter()
                .zip(&old_original_lines)
                .any(|(old_line, old_original_line)| {
                    assertion_operands_with_original(old_line, old_original_line).is_some_and(
                        |(old_left, old_right)| old_left == left && is_literal(&old_right),
                    )
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

/// One assertion reduced to the thing under test and the predicate applied to it.
///
/// `subject` is the expression whose behavior the assertion pins (the actual value, or the left
/// operand of a language-level `assert`); `predicate` names the check. Whitespace inside the
/// subject is collapsed so a moved or reindented assertion keeps the same identity. This does not
/// cover renamed variables, expressions re-wrapped across lines, or table-driven loops: those can
/// still read as a removal (the table fixture deliberately does).
#[derive(Clone, Debug, Eq, PartialEq)]
struct Assertion {
    subject: String,
    predicate: String,
}

fn assertion_keys(lines: &[&str], original: &[&str]) -> Vec<Assertion> {
    lines
        .iter()
        .zip(original)
        .filter_map(|(view_line, original_line)| assertion_of_line(view_line, original_line))
        .collect()
}

fn assertion_of_line(view_line: &str, original_line: &str) -> Option<Assertion> {
    if !is_assertion_code_line(view_line) || view_line.len() != original_line.len() {
        return None;
    }
    let (subject, predicate) = assertion_call_subject(view_line, original_line)
        .or_else(|| python_assert_subject(view_line, original_line))?;
    let subject = normalize_assertion_subject(&subject);
    (!subject.is_empty()).then_some(Assertion { subject, predicate })
}

/// A call-shaped assertion: testify/unittest/JUnit/`assert_eq!`/`expect(...).toBe(...)`.
fn assertion_call_subject(view: &str, original: &str) -> Option<(String, String)> {
    // (marker, predicate, which argument is the subject)
    const FIRST: u8 = 0;
    const SECOND: u8 = 1;
    const LAST: u8 = 2;
    for (marker, predicate, which) in [
        (".Contains(", "contains", SECOND),
        (".NotContains(", "not-contains", SECOND),
        (".Equal(", "equal", LAST),
        (".EqualValues(", "equal", LAST),
        (".Exactly(", "equal", LAST),
        ("assertEqual(", "equal", LAST),
        ("assertEquals(", "equal", LAST),
        ("assertNotEqual(", "not-equal", LAST),
        ("assertTrue(", "truthy", FIRST),
        ("assertFalse(", "falsy", FIRST),
        ("debug_assert!(", "truthy", FIRST),
        ("assert!(", "truthy", FIRST),
        ("assert_eq!(", "equal", FIRST),
        ("assert_ne!(", "not-equal", FIRST),
    ] {
        if let Some(inner) = inner_args_span(view, marker) {
            let args = split_top_level_spans(&view[inner.0..inner.1], inner.0);
            let span = match which {
                FIRST => args.first(),
                SECOND => args.get(1),
                _ => args.last(),
            };
            if let Some(span) = span {
                return Some((span_text(original, *span), predicate.to_owned()));
            }
        }
    }
    let (subject_span, _) = expect_to_operand_spans(view)?;
    let predicate = [".toStrictEqual(", ".toEqual(", ".toBe(", ".toBeTruthy("]
        .into_iter()
        .find(|matcher| view.contains(matcher))
        .unwrap_or(".toBe(");
    Some((
        span_text(original, subject_span),
        predicate.trim_matches(['.', '(']).to_owned(),
    ))
}

fn python_assert_subject(view: &str, original: &str) -> Option<(String, String)> {
    let index = view.find("assert ")?;
    let start = index + "assert ".len();
    let rest_view = &view[start..];
    let rest_original = &original[start..];
    for (operator, predicate) in [
        (" in ", "in"),
        (" not in ", "not-in"),
        (" == ", "equal"),
        (" != ", "not-equal"),
        (" is not ", "not-identical"),
        (" is ", "identical"),
    ] {
        if let Some(position) = rest_view.find(operator) {
            return Some((rest_original[..position].to_owned(), predicate.to_owned()));
        }
    }
    let body = rest_original.trim();
    let body = body.strip_prefix("not ").unwrap_or(body);
    (!body.is_empty()).then(|| (body.to_owned(), "truthy".to_owned()))
}

/// Reduce equivalent assertion shapes to one subject identity.
///
/// `assert!(!x.is_empty())` and `assert_eq!(x.len(), 1)` both pin the same collection, so the
/// emptiness/len wrapper and a leading `!` are stripped. Other calls keep their arguments (`add(1,
/// 2)` and `add(2, 3)` stay distinct).
fn normalize_assertion_subject(subject: &str) -> String {
    let collapsed = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    let stripped = collapsed
        .trim()
        .strip_prefix('!')
        .map_or_else(|| collapsed.trim(), str::trim_start);
    let stripped = stripped
        .strip_suffix(".is_empty()")
        .or_else(|| stripped.strip_suffix(".len()"))
        .unwrap_or(stripped);
    stripped.trim().to_owned()
}

fn span_text(original: &str, (start, end): Span) -> String {
    original
        .get(start..end)
        .unwrap_or_default()
        .trim()
        .to_owned()
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
    let index = ["it(", "test(", "describe(", "specify(", "context("]
        .into_iter()
        .find_map(|marker| text.find(marker))?;
    quoted_strings(&text[index..]).into_iter().next()
}

fn is_constant_bool_assertion(text: &str) -> bool {
    let compact: String = text.split_whitespace().collect();
    compact.contains("assertTrue(true)")
        || compact.contains("assertFalse(false)")
        || compact.contains("assert(true)")
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

/// A byte span in a line.
type Span = (usize, usize);

/// Extract the two compared operands of an equality-style assertion, if any.
///
/// The public form reads one text for both structure and values. The detector's form passes the
/// masked view for structure and the original line for values (see
/// [`assertion_operands_with_original`]).
#[must_use]
pub fn assertion_operands(line: &str) -> Option<(String, String)> {
    assertion_operands_with_original(line, line)
}

/// Find the operands in the masked `view`, then read their values from `original` at the same byte
/// offsets. The view is length-preserving, so this keeps structure (an operator inside a string is
/// blanked) while preserving the literal values a distinctness/tautology check needs.
fn assertion_operands_with_original(view: &str, original: &str) -> Option<(String, String)> {
    let stripped = view.trim();
    let base = view.len() - view.trim_start().len();
    let (left, right) = assertion_operand_spans(stripped)?;
    if !maps_to_original(stripped, left) || !maps_to_original(stripped, right) {
        return None;
    }
    let left = original
        .get(base + left.0..base + left.1)?
        .trim()
        .to_string();
    let right = original
        .get(base + right.0..base + right.1)?
        .trim()
        .to_string();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

/// A span that starts on blanked string content cannot be mapped back to a real operand.
fn maps_to_original(view: &str, span: Span) -> bool {
    view.as_bytes()
        .get(span.0)
        .is_some_and(|byte| *byte != b' ')
}

fn assertion_operand_spans(line: &str) -> Option<(Span, Span)> {
    if !has_marker(line, ASSERTION_MARKERS) {
        return None;
    }
    if let Some(inner) = inner_args_span(line, "assert_eq!(") {
        return split_two_spans(line, inner);
    }
    if let Some(inner) = inner_args_span(line, "assert!(") {
        let (start, end) = inner;
        let body = &line[start..end];
        let (left, right) = body.split_once("==")?;
        let left = trim_span(body, (0, left.len()));
        let right_start = left.1 + 2;
        let right = trim_span(body, (right_start, right_start + right.len()));
        return Some((
            (start + left.0, start + left.1),
            (start + right.0, start + right.1),
        ));
    }
    if let Some(inner) = inner_args_span(line, "assertEqual(") {
        return split_two_spans(line, inner);
    }
    if let Some(inner) = inner_args_span(line, "assertEquals(") {
        return split_two_spans(line, inner);
    }
    if let Some(inner) = inner_args_span(line, "assert.Equal(") {
        return split_skip_first_spans(line, inner);
    }
    if let Some(inner) = inner_args_span(line, "require.Equal(") {
        return split_skip_first_spans(line, inner);
    }
    if let Some(operands) = expect_to_operand_spans(line) {
        return Some(operands);
    }
    let marker = "assert ";
    let index = line.find(marker)?;
    let rest_start = index + marker.len();
    let rest = line[rest_start..].trim_start();
    let rest_start = rest_start + (line[rest_start..].len() - rest.len());
    let (left, right) = rest.split_once(" == ")?;
    let left_span = trim_span(rest, (0, left.len()));
    let right_start = left.len() + 4;
    let right_span = trim_span(rest, (right_start, right_start + right.len()));
    let left = &rest[left_span.0..left_span.1];
    let right = &rest[right_span.0..right_span.1];
    if left.is_empty() || right.is_empty() || left.contains('=') {
        return None;
    }
    Some((
        (rest_start + left_span.0, rest_start + left_span.1),
        (rest_start + right_span.0, rest_start + right_span.1),
    ))
}

fn expect_to_operand_spans(line: &str) -> Option<(Span, Span)> {
    let expect = inner_args_span(line, "expect(")?;
    let matcher = [".toBe(", ".toEqual(", ".toStrictEqual("]
        .iter()
        .find_map(|matcher| inner_args_span(line, matcher))?;
    Some((trim_span(line, expect), trim_span(line, matcher)))
}

/// Trim a span of `text` to its non-whitespace edges.
fn trim_span(text: &str, (start, end): Span) -> Span {
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    (start + lead, end - trail)
}

fn inner_args_span(line: &str, marker: &str) -> Option<Span> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let mut depth = 1_u32;
    for (index, character) in rest.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((start, start + index));
                }
            }
            _ => {}
        }
    }
    None
}

fn inner_args<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    inner_args_span(line, marker).map(|(start, end)| &line[start..end])
}

fn split_two_spans(line: &str, inner: Span) -> Option<(Span, Span)> {
    let (start, end) = inner;
    match split_top_level_spans(&line[start..end], start).as_slice() {
        [left, right] => Some((*left, *right)),
        _ => None,
    }
}

fn split_skip_first_spans(line: &str, inner: Span) -> Option<(Span, Span)> {
    let (start, end) = inner;
    match split_top_level_spans(&line[start..end], start).as_slice() {
        [_skip, left, right] => Some((*left, *right)),
        _ => None,
    }
}

/// Split `inner` on top-level commas, returning trimmed spans offset by `base`.
fn split_top_level_spans(inner: &str, base: usize) -> Vec<Span> {
    let mut parts = Vec::new();
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut quote = '\0';
    let mut segment_start = 0;
    let mut escaped = false;
    for (index, character) in inner.char_indices() {
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
            '"' | '\'' | '`' => {
                in_string = true;
                quote = character;
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(offset_span(inner, (segment_start, index), base));
                segment_start = index + 1;
            }
            _ => {}
        }
    }
    if !inner[segment_start..].trim().is_empty() || !parts.is_empty() {
        parts.push(offset_span(inner, (segment_start, inner.len()), base));
    }
    parts
}

fn offset_span(text: &str, span: Span, base: usize) -> Span {
    let (start, end) = trim_span(text, span);
    (base + start, base + end)
}

/// The top-level comma-separated arguments of `inner`, trimmed, for callers that need the text
/// rather than byte offsets.
fn split_top_level(inner: &str) -> Vec<String> {
    split_top_level_spans(inner, 0)
        .into_iter()
        .map(|(start, end)| inner[start..end].to_string())
        .collect()
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
    text.contains("assert ") && (text.contains("==") || text.contains("!="))
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
        AddedLine, Confidence, DiffContext, assertion_operands, classify_added_line,
        classify_diff_line, classify_harness_line, classify_test_body,
        classify_test_body_with_reliability, diff_findings, looks_like_commented_out_test,
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
    fn diff_findings_given_moved_assertion_reports_no_removal() {
        // given: the same two assertions in a different order
        let old = "def test_a():\n    assert add(1, 1) == 2\n    assert add(2, 2) == 4\n";
        let new = "def test_a():\n    assert add(2, 2) == 4\n    assert add(1, 1) == 2\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then: the multiset is unchanged, so nothing was removed
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed")
        );
        assert!(!findings.iter().any(|finding| finding.rule == "gutted-test"));
    }

    #[test]
    fn diff_findings_given_assertion_removed_reports_removal() {
        // given: one assertion genuinely absent after the edit
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
    fn diff_findings_given_assertions_converted_to_a_table_reports_removal() {
        // given: two explicit assertions replaced by one table-driven assertion
        let old = "def test_a():\n    assert add(1, 1) == 2\n    assert add(2, 2) == 4\n";
        let new = "def test_a():\n    for a, b, expected in [(1, 1, 2), (2, 2, 4)]:\n        assert add(a, b) == expected\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then: both original expressions are gone even though one assertion remains
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed")
        );
        assert!(!findings.iter().any(|finding| finding.rule == "gutted-test"));
    }

    #[test]
    fn diff_findings_given_contains_replaced_by_equal_reports_no_removal() {
        // given: two weak Contains checks on the same subject become one strict Equal check
        let old = "func TestReads(t *testing.T) {\n\tassert.Contains(t, result, \"cafe comment\")\n\tassert.Contains(t, result, \"hello\")\n}\n";
        let new = "func TestReads(t *testing.T) {\n\tassert.Equal(t, content, result)\n}\n";

        // when
        let findings = diff_findings(old, new, "tests/reader_test.go");

        // then: the subject still has an assertion, so nothing was removed
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed"),
            "findings: {findings:?}"
        );
    }

    #[test]
    fn diff_findings_given_matcher_replaced_on_the_same_subject_reports_no_removal() {
        // given: the same expect subject switches matcher
        let old = "it('x', () => {\n  expect(add(1, 2)).toEqual(3);\n});\n";
        let new = "it('x', () => {\n  expect(add(1, 2)).toBe(3);\n});\n";

        // when
        let findings = diff_findings(old, new, "tests/app.test.js");

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed"),
            "findings: {findings:?}"
        );
    }

    #[test]
    fn diff_findings_given_emptiness_rephrased_as_len_reports_no_removal() {
        // given: the same collection checks written as is_empty() and as len()
        let old = "#[test]\nfn t() {\n    assert!(!comments.is_empty());\n    assert!(filtered.is_empty());\n}\n";
        let new = "#[test]\nfn t() {\n    assert_eq!(comments.len(), 1);\n    assert_eq!(filtered.len(), 0);\n}\n";

        // when
        let findings = diff_findings(old, new, "tests/app_test.rs");

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed"),
            "findings: {findings:?}"
        );
    }

    #[test]
    fn diff_findings_given_assertion_subject_absent_reports_high() {
        // given: a subject with no assertion after the edit
        let old = "def test_a():\n    assert add(1, 1) == 2\n    assert add(2, 2) == 4\n";
        let new = "def test_a():\n    assert add(1, 1) == 2\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");
        let removed: Vec<_> = findings
            .iter()
            .filter(|finding| finding.rule == "assertion-removed")
            .collect();

        // then
        assert_eq!(removed.len(), 1, "findings: {findings:?}");
        assert_eq!(removed[0].confidence, Confidence::High);
    }

    #[test]
    fn diff_findings_given_reindented_assertion_reports_no_removal() {
        // given: the same assertion with different internal whitespace
        let old = "def test_a():\n    assert add(1, 1) == 2\n";
        let new = "def test_a():\n        assert  add(1,  1)  ==  2\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then: whitespace normalization keeps the expression identity
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed")
        );
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

    #[test]
    fn classify_added_line_given_bare_expect_reports_incomplete_assertion() {
        // given
        let added = lines(&["    expect(value);"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "incomplete-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_complete_expect_reports_nothing() {
        // given
        let added = lines(&["    expect(value).toBe(3);"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "incomplete-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_unawaited_resolves_reports_unawaited_assertion() {
        // given
        let added = lines(&["    expect(promise).resolves.toBe(1);"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "unawaited-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_awaited_resolves_reports_nothing() {
        // given
        let added = lines(&["    await expect(promise).resolves.toBe(1);"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "unawaited-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_broad_exception_reports_broad_exception() {
        // given
        let added = lines(&["    with pytest.raises(Exception):"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "broad-exception")
        );
    }

    #[test]
    fn classify_added_line_given_specific_exception_reports_nothing() {
        // given
        let added = lines(&["    with pytest.raises(ValueError):"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "broad-exception")
        );
    }

    #[test]
    fn classify_test_body_given_assertionless_body_reports_no_assertion() {
        // given
        let body = lines(&["def test_a():", "    do_work()"]);

        // when
        let findings = classify_test_body("tests/test_app.py", 1, &body);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "no-assertion")
        );
    }

    #[test]
    fn classify_test_body_given_body_with_assertion_reports_no_assertion_nothing() {
        // given
        let body = lines(&["def test_a():", "    assert do_work() == 1"]);

        // when
        let findings = classify_test_body("tests/test_app.py", 1, &body);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "no-assertion")
        );
    }

    #[test]
    fn classify_test_body_given_nested_suite_reports_no_assertion_nothing() {
        // given
        let body = lines(&[
            "describe('x', () => {",
            "  it('y', () => { expect(1).toBe(1); });",
            "});",
        ]);

        // when
        let findings = classify_test_body("tests/app.test.js", 1, &body);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "no-assertion")
        );
    }

    #[test]
    fn classify_harness_line_given_failure_ignored_reports_gate_weakened() {
        // given
        let added = lines(&["      - run: cargo test || true"]);

        // when
        let finding = classify_harness_line(".github/workflows/ci.yml", &added[0]);

        // then
        assert_eq!(
            finding.map(|finding| finding.rule),
            Some("gate-weakened".to_owned())
        );
    }

    #[test]
    fn classify_harness_line_given_plain_command_reports_nothing() {
        // given
        let added = lines(&["      - run: cargo test"]);

        // when
        let finding = classify_harness_line(".github/workflows/ci.yml", &added[0]);

        // then
        assert!(finding.is_none());
    }

    /// (path, removed line, added line) strict→loose pairs that must fire `weakened-assertion`.
    const WEAKENING_PAIRS: &[(&str, &str, &str)] = &[
        (
            "tests/app.test.js",
            "expect(add(1, 2)).toStrictEqual(3)",
            "expect(add(1, 2)).toEqual(3)",
        ),
        (
            "tests/app.test.js",
            "expect(add(1, 2)).toEqual(3)",
            "expect(add(1, 2)).toBe(3)",
        ),
        (
            "tests/app.test.js",
            "expect(add(1, 2)).toEqual(3)",
            "expect(add(1, 2)).toBeDefined()",
        ),
        (
            "tests/app.test.js",
            "expect(result).toBe(expected)",
            "expect(result).toBeTruthy()",
        ),
        (
            "tests/test_app.py",
            "self.assertEqual(add(1, 2), 3)",
            "self.assertIsNotNone(add(1, 2))",
        ),
        (
            "tests/test_app.py",
            "assert add(1, 2) == 3",
            "assert add(1, 2) is not None",
        ),
        (
            "tests/test_app.py",
            "self.assertEqual(value, 3)",
            "self.assertTrue(value)",
        ),
    ];

    #[test]
    fn classify_diff_line_given_strict_to_loose_pair_reports_weakened_assertion() {
        // given
        let mut failures = Vec::new();

        // when
        for &(path, old, new) in WEAKENING_PAIRS {
            let added_texts = [new];
            let added = lines(&added_texts);
            let removed = [old];
            let diff = DiffContext {
                removed: &removed,
                removed_original: &removed,
            };
            let findings = classify_diff_line(path, &added, 0, &diff);
            if !findings
                .iter()
                .any(|finding| finding.rule == "weakened-assertion")
            {
                failures.push(format!("{path}: `{old}` -> `{new}` produced {findings:?}"));
            }
        }

        // then
        assert!(
            failures.is_empty(),
            "weakening pairs not detected: {failures:#?}"
        );
    }

    #[test]
    fn classify_diff_line_given_high_confidence_pair_reports_high() {
        // given
        let added = lines(&["expect(add(1, 2)).toBeDefined()"]);
        let removed = ["expect(add(1, 2)).toEqual(3)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/app.test.js", &added, 0, &diff);

        // then
        let weakened = findings
            .iter()
            .find(|finding| finding.rule == "weakened-assertion");
        assert_eq!(
            weakened.map(|finding| finding.confidence),
            Some(Confidence::High)
        );
    }

    #[test]
    fn classify_diff_line_given_unrelated_subject_reports_nothing() {
        // given
        let added = lines(&["expect(other).toBeDefined()"]);
        let removed = ["expect(add(1, 2)).toEqual(3)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/app.test.js", &added, 0, &diff);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "weakened-assertion")
        );
    }

    #[test]
    fn classify_diff_line_given_strict_to_strict_reports_nothing() {
        // given
        let added = lines(&["expect(x).toStrictEqual(3)"]);
        let removed = ["expect(x).toEqual(3)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/app.test.js", &added, 0, &diff);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "weakened-assertion")
        );
    }

    #[test]
    fn classify_diff_line_given_already_loose_before_reports_nothing() {
        // given
        let added = lines(&["expect(x).toBeDefined()"]);
        let removed = ["expect(x).toBeTruthy()"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/app.test.js", &added, 0, &diff);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "weakened-assertion")
        );
    }

    #[test]
    fn classify_diff_line_given_equality_replaced_by_snapshot_reports_snapshot_regenerated() {
        // given
        let added = lines(&["expect(render()).toMatchSnapshot()"]);
        let removed = ["expect(render()).toEqual({ a: 1 })"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/app.test.js", &added, 0, &diff);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "snapshot-regenerated")
        );
    }

    #[test]
    fn classify_diff_line_given_narrowed_precision_reports_tolerance_broadened() {
        // given
        let added = lines(&["self.assertAlmostEqual(add(1, 2), 3, places=1)"]);
        let removed = ["self.assertAlmostEqual(add(1, 2), 3, places=5)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/test_app.py", &added, 0, &diff);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "tolerance-broadened")
        );
    }

    #[test]
    fn classify_diff_line_given_longer_sleep_reports_tolerance_broadened() {
        // given
        let added = lines(&["time.sleep(5)"]);
        let removed = ["time.sleep(1)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/test_app.py", &added, 0, &diff);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "tolerance-broadened")
        );
    }

    #[test]
    fn classify_diff_line_given_widened_precision_reports_nothing() {
        // given
        let added = lines(&["self.assertAlmostEqual(add(1, 2), 3, places=7)"]);
        let removed = ["self.assertAlmostEqual(add(1, 2), 3, places=2)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/test_app.py", &added, 0, &diff);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "tolerance-broadened")
        );
    }

    #[test]
    fn classify_diff_line_given_widened_pytest_approx_reports_tolerance_broadened() {
        // given: a larger relative tolerance is looser
        let added = lines(&["assert value == pytest.approx(3.0, rel=1e-1)"]);
        let removed = ["assert value == pytest.approx(3.0, rel=1e-6)"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/test_app.py", &added, 0, &diff);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "tolerance-broadened")
        );
    }

    #[test]
    fn classify_diff_line_given_widened_approx_in_assert_equal_reports_tolerance_broadened() {
        // given: the unittest carrier (`assertEqual`) must resolve the same subject
        let added = lines(&["self.assertEqual(value, pytest.approx(3.0, rel=1e-1))"]);
        let removed = ["self.assertEqual(value, pytest.approx(3.0, rel=1e-6))"];
        let diff = DiffContext {
            removed: &removed,
            removed_original: &removed,
        };

        // when
        let findings = classify_diff_line("tests/test_app.py", &added, 0, &diff);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "tolerance-broadened")
        );
    }

    #[test]
    fn classify_added_line_given_loose_matcher_on_write_reports_low_confidence() {
        // given
        let added = lines(&["expect(x).toBeDefined()"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        let weakened = findings
            .iter()
            .find(|finding| finding.rule == "weakened-assertion");
        assert_eq!(
            weakened.map(|finding| finding.confidence),
            Some(Confidence::Low)
        );
    }

    #[test]
    fn classify_added_line_given_strict_matcher_on_write_reports_nothing() {
        // given
        let added = lines(&["expect(x).toEqual(3)"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "weakened-assertion")
        );
    }

    #[test]
    fn classify_added_line_given_snapshot_on_write_reports_low_confidence() {
        // given
        let added = lines(&["expect(x).toMatchSnapshot()"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        let snapshot = findings
            .iter()
            .find(|finding| finding.rule == "snapshot-regenerated");
        assert_eq!(
            snapshot.map(|finding| finding.confidence),
            Some(Confidence::Low)
        );
    }

    #[test]
    fn classify_added_line_given_tobecloseto_is_a_complete_assertion() {
        // given: a complete numeric assertion must not read as incomplete
        let added = lines(&["expect(x).toBeCloseTo(y, 2);"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "incomplete-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_ternary_inside_template_literal_is_not_conditional() {
        // given: the `? :` sits inside a template literal, not a runtime guard
        let added = lines(&["expect(render(`A ${flag ? 'yes' : 'no'}`)).toBe('A yes');"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "conditional-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_rerunning_identifier_is_not_retry_added() {
        // given: an identifier containing "rerun" is not a retry marker
        let added = lines(&["rerunning_service();"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "retry-added"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_ternary_operand_is_not_conditional() {
        // given: a ternary used as an `expect` operand, not a guard around the assertion
        let added = lines(&["expect(user.isAdmin ? a : b).toBeTruthy();"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "conditional-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_regex_optional_is_not_conditional() {
        // given: a regex `?:` is not a ternary
        let added = lines(&["expect(url).toMatch(/https?:\\/\\//);"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "conditional-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_unclosed_expect_is_not_incomplete() {
        // given: the first line of a prettier-wrapped `expect(\n …\n).toBe(3)`
        let added = lines(&["expect("]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "incomplete-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_resolves_in_string_is_not_unawaited() {
        // given: `.resolves.` inside a string literal is not an unawaited promise assertion
        let added = lines(&["it('documents .resolves. usage', () => {});"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "unawaited-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_test_body_given_pytest_raises_reports_no_assertion_nothing() {
        // given: a raises-style test whose expectation IS the assertion
        let body = lines(&["with pytest.raises(ValueError):", "    run()"]);

        // when
        let findings = classify_test_body("tests/test_app.py", 1, &body);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "no-assertion"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_inputstream_skip_is_not_skip_added() {
        // given: a benign IO `skip(n)` call, not a framework test skip
        let added = lines(&["inputStream.skip(10);"]);

        // when
        let findings = classify_added_line("src/test/java/AppTest.java", &added, 0);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "skip-added"),
            "{findings:?}"
        );
    }

    #[test]
    fn looks_like_commented_out_test_given_prose_is_false() {
        // then
        assert!(!looks_like_commented_out_test(
            "# assert that setup ran twice (see issue #12)"
        ));
    }

    #[test]
    fn classify_added_line_given_substring_stem_is_not_mock_of_sut() {
        // given: stem "lib" must not match the substring "library"
        let added = lines(&["mock.patch(\"library.Loader\")"]);

        // when
        let findings = classify_added_line("tests/test_lib.py", &added, 0);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "mock-of-sut"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_sut_mock_still_reports_mock_of_sut() {
        // given: the real system under test, referenced as a whole token
        let added = lines(&["mock.patch(\"app.Client\")"]);

        // when
        let findings = classify_added_line("tests/test_app.py", &added, 0);

        // then
        assert!(
            findings.iter().any(|finding| finding.rule == "mock-of-sut"),
            "{findings:?}"
        );
    }

    #[test]
    fn diff_findings_given_removed_assert_comment_reports_nothing() {
        // given: the removed line is a commented-out assertion, not a real one
        let old = "def test_a():\n    assert one() == 1\n    # assert two() == 2\n";
        let new = "def test_a():\n    assert one() == 1\n";

        // when
        let findings = diff_findings(old, new, "tests/test_app.py");

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "assertion-removed" || finding.rule == "gutted-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_method_cancel_is_not_skip_added() {
        // given: a method call ending in `.cancel()`, not the ScalaTest skip idiom
        let added = lines(&["request.cancel();"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "skip-added"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_receiver_cancel_with_message_is_not_skip_added() {
        // given: a double-quoted receiver cancellation (axios/AbortController), not a bare cancel
        let added = lines(&["source.cancel(\"Operation canceled\");"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "skip-added"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_scala_cancel_still_reports_skip_added() {
        // given: the idiomatic ScalaTest cancellation
        let added = lines(&["cancel(\"later\")"]);

        // when
        let findings = classify_added_line("src/test/scala/AppSpec.scala", &added, 0);

        // then
        assert!(
            findings.iter().any(|finding| finding.rule == "skip-added"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_catchhandlers_identifier_is_not_swallowed_error() {
        // given: an identifier containing "catch" is not a catch clause
        let added = lines(&["const catchHandlers = {};"]);

        // when
        let findings = classify_added_line("tests/app.test.js", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "swallowed-error"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_test_body_given_field_round_trip_reports_echo_test() {
        // given: a DTO is constructed with a value and the value is read back unchanged
        let body = lines(&[
            "def test_dto():",
            "    dto = Dto(name)",
            "    assert dto.name == name",
        ]);

        // when
        let findings = classify_test_body("tests/test_dto.py", 1, &body);

        // then
        assert!(
            findings.iter().any(|finding| finding.rule == "echo-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_test_body_given_setter_getter_reports_echo_test() {
        // given: a setter is read back through its getter on the next statement
        let body = lines(&[
            "it('stores', () => {",
            "  dto.set_name(name);",
            "  expect(dto.get_name()).toBe(name);",
            "});",
        ]);

        // when
        let findings = classify_test_body("tests/app.test.js", 1, &body);

        // then
        assert!(
            findings.iter().any(|finding| finding.rule == "echo-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_test_body_given_logic_between_write_and_read_reports_no_echo_test() {
        // given: another statement sits between the write and the read
        let body = lines(&[
            "def test_dto():",
            "    dto = Dto(name)",
            "    dto.normalize()",
            "    assert dto.name == name",
        ]);

        // when
        let findings = classify_test_body("tests/test_dto.py", 1, &body);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "echo-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_test_body_given_changed_value_reports_no_echo_test() {
        // given: the read-back value differs from the value written
        let body = lines(&[
            "def test_dto():",
            "    dto.name = name",
            "    assert dto.name == other",
        ]);

        // when
        let findings = classify_test_body("tests/test_dto.py", 1, &body);

        // then
        assert!(
            !findings.iter().any(|finding| finding.rule == "echo-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_test_body_given_unreliable_fragment_suppresses_whole_test_rules() {
        // given: a fragment whose captured test lacks its closing delimiter
        let body = lines(&["it('x', () => {", "  dto.name = name;", "});"]);

        // when
        let findings = classify_test_body_with_reliability("tests/app.test.js", 1, &body, false);

        // then: absence of an assertion is not trustworthy on an incomplete fragment
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "no-assertion" || finding.rule == "echo-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_mirrored_expression_reports_mirror_implementation() {
        // given: the expected value repeats the call's own arithmetic
        let added = lines(&["assert add(a, b) == a + b"]);

        // when
        let findings = classify_added_line("tests/test_math.py", &added, 0);

        // then
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "mirror-implementation-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_literal_expectation_reports_no_mirror_implementation() {
        // given: an expected literal is a real oracle, not a restatement
        let added = lines(&["assert add(a, b) == 3"]);

        // when
        let findings = classify_added_line("tests/test_math.py", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "mirror-implementation-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_unrelated_call_reports_no_mirror_implementation() {
        // given: both sides call code, so the expectation is not a mirror of the arguments
        let added = lines(&["assert parse(a) == render(a)"]);

        // when
        let findings = classify_added_line("tests/test_math.py", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "mirror-implementation-test"),
            "{findings:?}"
        );
    }

    #[test]
    fn classify_added_line_given_assertion_quoted_in_a_fixture_string_reports_nothing() {
        // given: the assertion is data inside a string literal, not code in this test
        let added = lines(&["let source = \"expect(add(a, b)).toBe(a + b);\";"]);

        // when
        let findings = classify_added_line("tests/detector.rs", &added, 0);

        // then
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "mirror-implementation-test"),
            "{findings:?}"
        );
    }
}
