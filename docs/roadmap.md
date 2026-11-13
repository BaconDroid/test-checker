# Roadmap

Planned work, in blocks. Each block is independently shippable; entries inside a
block are ordered. Status is `now`, `next` or `later`. Finished work leaves this
file.

Cross-repo items are marked **[cc+tc]**: they change `src/engine.rs`,
`src/cli.rs`, `src/main.rs`, `parity/**` or `parity/vectors.json`, which are
byte-identical across both repos (`parity/check.py`), so they ship as one wave on
the same branch name in both repositories.

## Block A - Rule precision

Misfires found by the secondary corpus (`eval/README.md`).

| # | Item | Status |
|:-:|:-----|:------|
| A3 | Publish cited evidence in the verdict: `duplicate-test` reports the counterpart line it already computes, `test-removed` / `assertion-removed` report the removed subject and its old line instead of line 1 with an empty text. | next |
| A4 | Report withheld findings: exit 0 for a genuinely clean write, a distinct message when findings were filtered by the confidence gate or suppressed by config. | next |
| A5 | Matcher coverage: add testing-library matchers as the corpus surfaces them; the JS matcher set is gated per language (see `src/rules.rs`). | later |

## Block B - Cleanup-pass safety

| # | Item | Status |
|:-:|:-----|:------|
| B2 | Drop the blank line a removed span leaves behind without touching pre-existing blank lines. | later |

## Block C - Configuration

| # | Item | Status |
|:-:|:-----|:------|
| C1 | `TEST_CHECKER_MIN_CONFIDENCE` read in the domain layer, unknown or empty value falling back to `medium`, so a CI job can run `low` without the parity-locked CLI. | next |

## Block D - Evaluation

| # | Item | Status |
|:-:|:-----|:------|
| D1 | Truth labels for a slice of the secondary corpus, so precision stops being a hit-rate estimate. | later |
| D2 | Differential oracle against an independent test-smell implementation, gated on the "reference flags, we spare" group only. No trusted peer exists yet. | later |

## Block E - Infrastructure

| # | Item | Status |
|:-:|:-----|:------|
| E2 | Non-blocking mutation job (`.cargo/mutants.toml` already scopes `src/rules.rs`). | next |
| E3 | `npm run doctor` wrapper around `scripts/doctor.sh`, plus the setup skill. | later |

## Coordinated with the sibling

| # | Item | Status |
|:-:|:-----|:------|
| **[cc+tc]** X1 | Marker grammar for `comment-checker` gains rule and reason validation, comment-node anchoring, and inert-if-added; `parity/vectors.json`'s `suppression-marker` case changes with it. | later |

## Not adopted

| Idea | Why |
|:-----|:----|
| Semantic test-file admission in `comment-checker` | Every code file is comment-worthy; a "is this a test" gate has no comment-domain meaning. |
| Comment-checker's spare-unless-proven keyword posture | Inverted product stance: this repo flags unless justified, so a keyword would acquit a `no-assertion` test that merely names a reason. |
| File-wide in-source suppression marker | Retired in favour of the committed, reason-bearing `.test-checker.json`. |
