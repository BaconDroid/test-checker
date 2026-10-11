# Implementation prompt — build `test-checker` from scratch on the `comment-checker` base

You are building a new Rust project. Work autonomously, in small verified steps, and land
changes through branch → PR → merge. Do NOT modify `comment-checker`.

## 0. Target and base

- **Target repo:** `BaconDroid/test-checker` (default branch `main`). **Start fresh:** delete its
  current `src/`, `tests/`, `benches/`, `npm/`, `.github/`, `Cargo.toml`, `Cargo.lock`, `README.md`,
  `AGENTS.md` and rebuild them following `comment-checker`. Ignore/discard the existing `test-checker`
  code entirely — it is NOT the base. Keep only the repo, its name, and its license if it fits. Keep
  this `docs/plans/` directory.
- **Base to copy from:** `BaconDroid/comment-checker` (standalone Rust project). Clone it
  (`git clone https://github.com/BaconDroid/comment-checker.git`) and **read it fully first**; the goal
  is to reproduce its structure, conventions, and quality bar for the *test* domain.
- **Workflow:** for each coherent change, create a branch, commit, push, open a PR, squash-merge, and
  sync `main`. Keep CI green at every step.

`comment-checker` is a PostToolUse hook for Claude Code/OpenCode: it reads a JSON hook payload on
stdin, detects comments/docstrings in the tool's written/edited content, and on findings writes a
human warning to **stderr** and exits **2** (else exits 0). `test-checker` must do the *same kind of
thing*, but detect **test-weakening** in test files instead of comments.

---

## 1. How `comment-checker` works (study the real code; this is the shape to mirror)

**Binary / CLI (`src/main.rs`, `src/cli.rs`)**
- `main` → `cli::run_from_env()`.
- Hand-rolled `std::env::args` parser (NO clap): accepts `--prompt <text>` and `--prompt=<text>`; all
  other args are ignored; reads the hook JSON from stdin (`serde_json::from_reader` on a locked stdin);
  prints the result message to stderr; returns the exit code.

**Hook API (`src/api.rs`)**
- Input model (serde derive): `HookInput { tool_name: String, tool_input: ToolInput }` and
  `ToolInput { file_path, content, new_string, old_string, edits: Vec<EditInput{old_string,new_string}> }`,
  with camelCase `#[serde(alias = "...")]`s and a null-tolerant `deserialize_with` so missing **or**
  `null` fields fall back to `Default` (upstream producers are inconsistent). Unknown fields are ignored.
- `evaluate(input)`: resolve the language from `file_path`; then:
  - `Edit` → detect on `old_string` and `new_string`, keep only findings new in `new_string`;
  - `MultiEdit` → same per edit;
  - otherwise (`Write`, …) → detect on `content`.
  - Then run the filters; if nothing remains → "Success"; else → "block" with the formatted message.
- JSON API (library face): `check_hook_input_json(input, prompt)` and
  `detect_comments_json(content, file_path, include_docstrings)`.

**Detector (`src/detector.rs`)**
- `language_for_path` → a grammar name via `tree-sitter-language-pack::detect_language`.
- Per-language `Runtime` cache (`RUNTIMES: LazyLock<Mutex<HashMap<String, Arc<Runtime>>>>`): the
  `Language` plus each compiled `tree_sitter::Query` (`comment_query`, `docstring_query`,
  `injection_query`) — compiled **once per process** and reused.
- Per-thread `Parser` cache (`PARSERS`); parsing runs through `parse_with_options` with a 50 ms
  progress-callback budget (abort pathological input).
- Extraction: (a) run the comment query and turn captures into `CommentInfo`s with real line numbers;
  (b) docstrings — query + `/**` prefix filter for js/ts/tsx/java, and a **cheap line scan** for
  Python (compiling the Python docstring query costs ~18 ms/process, which a per-edit hook pays every
  run — so Python uses the scan); (c) **injected languages** — run the pack's `get_injections_query`,
  take the injected language from an `@injection.language` capture or the pattern's
  `#set! injection.language` property, parse the captured content with that grammar, and offset line
  numbers.
- `determine_comment_type`: classify by node kind then text (`line_comment` → Line; `//` `#` `--` →
  Line; `/*` `(*` `<!--` → Block).

**Language map (`src/language.rs`)**: `language_for_path` (pack detection); per-name `comment_query`,
`docstring_query`, `docstring_prefix`. Language coverage = every language compiled by the pack (the
`TSLP_LANGUAGES` list in `.cargo/config.toml`, currently 31).

**Filters (`src/filters.rs`)**: `apply_filters` drops allowed comments — BDD (`given`/`when`/`then`),
linter directives (`# noqa`, `// @ts-ignore`, `#[allow]`, …), and shebangs; an `AgentMemoFilter`
flags "memo" comments as a code smell.

**Output (`src/output.rs`)**: `build_comments_xml` (with **XML escaping** of text and the `file`
attribute), and `format_hook_message` (optional agent-memo section + the standard warning + the
`<comments>` XML). `--prompt` replaces the default message, with `{{comments}}` substituted.

**Models (`src/models.rs`)**: `CommentInfo { text, line_number, file_path, comment_type, is_docstring,
metadata }` (`Serialize` + `Deserialize`, camelCase keys, `skip_serializing_if` empty metadata),
`CommentType`.

**Project shape**: `Cargo.toml` is a **single-package** manifest (no `[workspace]`, no `[features]`),
edition 2024, `rust-version = 1.85`, release profile `lto=fat`/`codegen-units=1`/`strip=symbols`/
`panic=abort`, and a strict `[lints]` block (`clippy all=deny`, `pedantic`/`nursery`/`cargo`=warn,
`unwrap_used`/`expect_used`/`panic`/`todo`/`unimplemented`/`unreachable`/`mem_forget`=deny, …). CI is
`.github/workflows/ci.yml`; release is `.github/workflows/release.yml`; the npm wrapper is under
`npm/comment-checker/`; benches use a plain `std::time::Instant` harness (no criterion).

---

## 2. The mission: the same thing, for tests

Reproduce every one of the capabilities above, in the test domain:

| comment-checker | test-checker (the analog) |
|---|---|
| Detects comments/docstrings in written/edited content | Detects **test-weakening** in written/edited **test** content |
| Runs tree-sitter **comment queries** to capture comment nodes | Runs per-language **test-body queries** to capture the test blocks/functions a change touches |
| Classifies a comment (`Line`/`Block`/`Docstring`) from node kind + text | **Classifies a change** as weakening via the rule catalog over the **added lines** of the test body |
| Differs `old_string` vs `new_string` to report only *new* comments | Differs old vs new test bodies to report only *newly introduced* weakening |
| Gates on file extension | Gates on extension **and** test identity (`is_test_file` from the payload, else test-path globs) |
| Drops allowed comments (BDD/directive/shebang) via filters | Drops test suppressions/opt-outs via the same filter architecture |
| `<comments file="…"><comment line-number="…">…` | `<findings file="…"><finding line-number="…" rule="…" confidence="…">…` |
| Exit 2 on findings, 0 otherwise; never panics; fail-open | Identical (exit 2/0, fail-open, no panic) |
| `detect_comments_json` library API | `detect_findings_json` library API |

Everything else — stdin contract, `--prompt`/placeholder, message prefix `[test-checker]`, JSON API,
Rust+tree-sitter engine, caches, parse budget, XML escaping, 31-language
coverage, tests/benches/CI/npm/release — mirrors `comment-checker` exactly. Two capabilities are easy
to lose in the mapping, so state them explicitly:

- **Injection handling is required, not optional.** Reuse `comment-checker`'s injection machinery (the
  pack's `get_injections_query`, the injected language taken from an `@injection.language` capture or
  the pattern's `#set! injection.language` property, re-parsing the captured span, and re-offsetting
  line numbers) for the **comment capture**, so a commented-out test inside an embedded/injected block
  (a JS/TS test inside `<script>`, a Python test inside a markdown fence, …) is caught. This applies
  to `commented-out-test` (§3b); the test-body capture itself runs on the host language only.
- **Keep a `detect(..., include_*)` toggle** so the library face matches `comment-checker`'s
  `detect(content, file_path, include_docstrings)` and
  `detect_comments_json(content, file_path, include_docstrings)`. Define the test-domain equivalent
  (e.g. `include_commented_out_tests` / `include_body_tests`), thread it through `evaluate()`, both
  JSON APIs and the filters exactly as `include_docstrings` is, and test its default in both states.

### 2a. What "parity" does and does NOT mean

**Parity means** the same hook contract, CLI, JSON API, output format, conventions, dependency set,
caches, parse budget, escaping, and repo scaffolding (benches/CI/npm/release), across the same 31
**compiled and parsed** languages.

**Parity does NOT mean:**

- **the same rules content.** comment-checker's signals are comment queries + allowed-pattern
  filters; test-checker's are a rule-classifier catalog over added lines. Same *engine shape*,
  different *content* — the catalog is deliberately new.
- **31 languages produce findings.** Comments are universal, tests are not. Every language is
  **parsed** (the same `TSLP_LANGUAGES`), but only languages with a real test framework/test-body
  query can yield a finding; `dockerfile`, `hcl`, `proto`, `toml`, `yaml`, `css`, `html`, `cue`, …
  parse cleanly and report nothing. Coverage tests must therefore assert **pipeline coverage per
  language** (grammar resolves, query compiles, capture runs, zero false positives), never *detection*
  per language.
- **every comment-checker feature has a test analog.** Docstring detection (query + `/**`/`(*`/`--`
  prefix filter + the Python line scan), `determine_comment_type` (`Line`/`Block`/`Docstring`) and the
  multiline "agent memo" message banner have **no test-domain equivalent** — drop them; do not
  re-invent them.
- **this document is the parity guarantee.** It is a spec. Parity is produced by reading
  `comment-checker` fully and mirroring it faithfully, step by step, verified per step.

---

## 3. What to detect (ruleset)

> **DRAFT — replace this list with your own signals.** The catalog below is a comprehensive starting
> point; keep the *architecture* (a declarative rule catalog of classifiers over the added lines)
> regardless of which signals you keep.

Each finding carries a `rule` id and a `confidence` (high/medium/low). Rules are classifiers over the
**added lines** of the captured test bodies (comments stripped), plus the before/after diff.

### 3a. Test-weakening signals

1. `skip-added` — a test/assertion disabled: `pytest.mark.skip`/`skipif`/`xfail`, `unittest.skip`,
   `.skip()`/`.todo()`, `t.Skip()`, `#[ignore]`, `xdescribe`/`xit`, `@Disabled`, …
2. `focus-added` — a focused test that would restrict the suite: `.only`, `fit`/`fdescribe`, `iit`.
3. `assertion-removed` — an existing assertion deleted with no replacement.
4. `gutted-test` — a test emptied or stripped of all assertions.
5. `tautological-assertion` — always-true assertions (`assert x == x`, `expect(true).toBe(true)`,
   `assertEqual(a, a)`).
6. `constant-assertion` — asserting a literal against a literal (`assert 1 == 1`).
7. `weakened-assertion` — a matcher loosened (`toEqual`→`toBeDefined`, `assertEqual`→`assertIsNotNone`,
   exact → truthy, equality → containment).
8. `swallowed-error` — a `try/catch` or `except` (or `assertRaises` misuse) around the assertion body
   that swallows failures (`catch {}`, `except: pass`).
9. `mock-of-sut` — mocking/stubbing the system-under-test itself, so the test asserts on a mock.
10. `over-mocking` — mock setup far outnumbering real assertions.
11. `duplicate-test` — a new test identical (or near-identical) to an existing one.
12. `expected-copied-from-actual` — expected value replaced by a copy of the actual/computed value.
13. `snapshot-regenerated` — a snapshot/approval fixture updated to match new output without review.
14. `tolerance-broadened` — an epsilon/tolerance/delta widened, or a timeout/sleep raised, to hide
    flakiness.
15. `conditional-assertion` — assertions guarded by a runtime condition / env flag so they may not run.
16. `retry-added` — `@retry`/`flaky`/`rerun` introduced to make a failing test pass.

### 3b. Signals adapted from `comment-checker` (reuse its patterns)

These reuse machinery `comment-checker` already has, adapted to the test domain:

17. `agent-memo-test` — a **test name or assertion message** that is an agent memo: reuse
    `comment-checker`'s `AgentMemoFilter` phrase catalog (e.g. `added`, `changed`/`convert`/`migrated`
    /`modified`/`moved`/`renamed`/`switched`/`updated` **from X to Y**, `now we/this/it`, `note:`,
    `previously`, `refactored`/`replaced`/`removed`, `this implements/adds/removes/changes/fixes`,
    `was changed`, and the `a -> b` arrow). A test that describes *what changed* instead of *what the
    behavior is* is a smell. Apply the phrase catalog to identifiers and assertion/message strings,
    not to comment text.
18. `commented-out-test` — a **comment whose body is commented-out test/assertion code** (`# assert x == y`,
   `// expect(x).toBe(y)`, `# def test_foo():`, `// it("…", …)`). This **reuses `comment-checker`'s
   comment detection**, which already covers **all 31 languages**, so a disabled-by-comment test is
   caught everywhere for free — including inside injected/embedded blocks via comment-checker's
   injection handling (see §2). (This is the direct analog of `comment-checker` flagging comments.)

### 3c. Allow-list / suppression predicates (the filter analog)

Mirror `comment-checker`'s filter architecture (allow-lists + a suppression marker), not as signals:

- **BDD allow-list**: `given`/`when`/`then`/`arrange`/`act`/`assert` (and `when & then`, `when&then`) —
  BDD-structured tests/comments are acceptable.
- **Directive/suppression allow-list**: the same directive prefixes (`noqa`, `ts-ignore`, `eslint-disable`,
  `clippy:`, `allow`/`deny`/`warn`/`forbid`, …) plus a test-level opt-out marker (e.g.
  `# test-checker: allow`) that suppresses findings for a file/line.
- **Framework-sanctioned exceptions**: a documented quarantine/retry policy can be allow-listed.

---

## 4. Architecture to build (mirror `comment-checker` exactly)

```
Cargo.toml            single package, edition 2024, the same profile + lints as comment-checker
src/
  main.rs             → cli::run_from_env()
  cli.rs              std arg parse (--prompt), stdin read, stderr, exit code
  api.rs              HookInput/ToolInput models; evaluate(); filters; format; JSON API
  detector.rs         test-body query capture + comment (commented-out test) capture + diff + rule evaluation; RUNTIMES/PARSERS caches; parse budget
  language.rs         detect_language → grammar; per-language test_query + test-path globs + plugin key
  rules.rs            the rule catalog: one classifier per rule over the added lines / diff
  filters.rs          allow-lists (BDD/directive) + suppression markers + apply_filters
  models.rs           Finding { file, line, rule, confidence, message }, Confidence, Edit
  output.rs           build_findings_xml (XML-escaped) + format_hook_message
tests/                api / cli / detector / filters / integration / models / output (+ a coverage table)
benches/              one bench target with a std::time::Instant harness
npm/test-checker/     cli.js, index.js, postinstall.js, package.json, *.test.js
.github/workflows/    ci.yml, release.yml (+ scripts)
README.md, AGENTS.md, LICENSE, .gitignore, .cargo/config.toml (TSLP_LANGUAGES), clippy.toml, deny.toml, rustfmt.toml, rust-toolchain.toml
```

---

## 5. Conventions you MUST follow (inherited from `comment-checker`)

- **Dependencies (exactly these four):** `serde` (data model + serialization), `serde_json` (wire I/O
  only), `tree-sitter` (parsing + matching via queries), `tree-sitter-language-pack` (language,
  grammar, identification, injection queries only). Do **not** add `clap`, `itoa`, `criterion`,
  `tempfile`, or any napi/wasm bindings. Do **not** use dynamic `serde_json::Value` modeling,
  `TreeCursor` walks, or the pack's `get_parser`/`process`/runtime download.
- **CLI:** hand-rolled `std::env::args`; `--prompt` + `{{findings}}`; ignore unknown args.
- **Hook contract:** JSON on stdin; **stderr** message; exit **2** on findings, **0** otherwise; never
  panics; every failure path degrades to no findings (**fail-open**). Message prefix `[test-checker]`.
- **Model:** typed `serde` derive with camelCase `alias`es + null tolerance + `skip_serializing_if`;
  `Serialize` **and** `Deserialize`.
- **Engine:** tree-sitter queries for capture; per-language `Runtime` + per-thread `Parser` caches
  (compile each query once per process); `parse_with_options` with a ~50 ms budget; query-compile cost
  matters (a per-edit hook pays it every process — hence e.g. the Python docstring line scan).
- **Output:** XML with correct escaping of text and attributes.
- **Lints:** the same strict `[lints]` block; `cargo clippy --all-targets --all-features -- -D warnings`
  must pass with **zero** warnings.
- **Tests:** follow the repo's test-authoring gate (in `AGENTS.md`): one primary owner per contract, no
  junk/near-duplicate tests, table-driven cases for cross-language coverage, and a coverage table that
  exercises **every one of the 31 languages**.
- **Formatting:** `cargo fmt --all -- --check` clean.

---

## 6. Naming / distribution (decide and stay consistent)

- Crate + binary: `test-checker`; message prefix `[test-checker]`; JSON key casing camelCase.
- `Cargo.toml` `repository` = `https://github.com/BaconDroid/test-checker`.
- npm wrapper under `npm/test-checker/`, package name `@bacondroid/test-checker` (or the scope you
  prefer — keep it consistent with the release workflow's registry).
- Release workflow publishes to this repo's GitHub Releases and its own Homebrew tap; do not reference
  upstream `code-yeongyu`.

---

## 7. Build order (small verified steps, each a branch → PR → merge)

1. **Scaffold:** wipe the repo, add `Cargo.toml` (4 deps, profiles, lints), `rust-toolchain.toml`,
   `rustfmt.toml`, `clippy.toml`, `deny.toml`, `.cargo/config.toml` (the same `TSLP_LANGUAGES`), `.gitignore`.
2. **Models + output:** `models.rs`, `output.rs` with XML escaping (+ tests).
3. **Language + rules catalog:** `language.rs` (extension→grammar, per-language `test_query`,
   test-path globs) and `rules.rs` (the catalog — 3a + 3b signals, one classifier each, + tests).
4. **Detector:** `detector.rs` (RUNTIMES/PARSERS caches, parse budget, test-body capture, commented-out
   test capture via the comment query, diff, evaluate) (+ tests).
5. **Filters + API + CLI:** `filters.rs` (allow-lists/suppression), `api.rs` (models/evaluate/format/JSON),
   `cli.rs`, `main.rs` (+ tests), then the **integration** and **cli** tests.
6. **Coverage:** the 31-language detection/classification coverage test.
7. **Benches:** the std-`Instant` bench harness.
8. **npm + release + CI:** port `comment-checker`'s `npm/`, `.github/` (CI + release + scripts), retargeted
   to `test-checker`; wire the npm tests into CI.
9. **Docs:** `README.md` (contract, rules, extensions, dev commands) and `AGENTS.md` (the same command
   list + test-authoring gate + a short "conventions" note), then a final whole-repo review pass.

---

## 8. Acceptance / verification

- `cargo fmt --all -- --check` clean; `cargo clippy -p test-checker --all-targets --all-features -- -D warnings`
  **zero warnings**; `cargo test --all-targets --all-features` green; `cargo bench --no-run` green;
  `node --test npm/test-checker/*.test.js` green.
- Behavior: a clean test edit → exit 0 + `[test-checker] Success: No problematic tests found`; a
  weakening edit → exit 2 + `<findings>` XML on stderr; an unsupported/non-test file → exit 0 skip.
- **Pipeline** coverage across **all 31 languages** (a table case per language: grammar resolves, the
  test-body query compiles, capture runs, and a clean fixture yields no findings). Only languages with
  a real test framework produce findings; assert *pipeline* coverage per language, not detection
  (see §2a).
- No dependency outside the four; no `--help`/clap; no bindings; single-package manifest.
- Every change landed via branch → PR → (squash) merge; final `main` CI green.

Report progress per step, with the exact verification commands and their results.
