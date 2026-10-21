#!/usr/bin/env python3
"""D0 baseline: assert the exit-code contract and gate the ANALYSIS/VERSION p95 ratio.

Run from the repository root. This is an off-contract eval helper (see eval/README.md), not part of
the parity-locked suite.

Absolute p95 drifts with machine load, so the gate compares the analysis-case p95 with the
`--version` p95 measured immediately after it in the same session. Five alternating sessions give
five ratios; the gate uses their median. A run is "over" when that ratio exceeds the recorded ratio
by more than `tolerance_percent`. The gate fails only when at least two of the last three runs are
over, so one noisy run does not fail a build. Per-run ratios live in `eval/baseline-history.json`
(local state, not tracked).

`eval/baseline.json` records `built_from`, the commit the baseline was measured on. The harness
takes the build commit from `BASELINE_BUILT_FROM` or `git rev-parse HEAD` and exits 2 when it cannot
prove the binary under test came from HEAD. The commit that records the baseline may be followed by
a metadata-only commit that changes only `eval/baseline.json`. The 15 ms p95 target stays a warning.
"""
from __future__ import annotations

import json
import math
import os
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (name, payload, expected exit code)
CASES = [
    (
        "clean",
        '{"tool_name":"Write","tool_input":{"file_path":"tests/a_test.py",'
        '"content":"def test_a():\\n    assert x == 1\\n"}}',
        0,
    ),
    (
        "trigger-skip",
        '{"tool_name":"Write","tool_input":{"file_path":"tests/a_test.py",'
        '"content":"def test_a():\\n    @pytest.mark.skip\\n    assert x == 1\\n"}}',
        2,
    ),
    ("invalid-json", "{not json", 0),
    ("empty-stdin", "", 0),
    ("unknown-ext", '{"tool_name":"Write","tool_input":{"file_path":"a.zzz","content":"x = 1\\n"}}', 0),
    (
        "non-test-path",
        '{"tool_name":"Write","tool_input":{"file_path":"src/app.py",'
        '"content":"def f():\\n    @pytest.mark.skip\\n    assert x == 1\\n"}}',
        0,
    ),
]

CLEAN_FIXTURE = ROOT / "eval" / "fixtures" / "clean_test.py"
if CLEAN_FIXTURE.is_file():
    CASES.append(
        (
            "write-with-cwd",
            json.dumps(
                {
                    "tool_name": "Write",
                    "cwd": str(ROOT),
                    "tool_input": {
                        "file_path": "eval/fixtures/clean_test.py",
                        "content": CLEAN_FIXTURE.read_text(encoding="utf-8"),
                    },
                }
            ),
            0,
        )
    )

BUDGET_P95_MS = 15.0
SESSIONS = 5
TOLERANCE_PERCENT = 35.0
HISTORY_LIMIT = 3
OVER_LIMIT = 2
BASELINE_FILE = Path(__file__).resolve().parent / "baseline.json"
HISTORY_FILE = Path(__file__).resolve().parent / "baseline-history.json"

# The only tracked file a follow-up commit may change without invalidating `built_from`.
RECORD_ONLY_FILES = {"eval/baseline.json"}


class ProvenanceError(RuntimeError):
    """The binary under test could not be tied to the current HEAD."""


class HistoryError(RuntimeError):
    """The run-history file is unreadable or malformed."""


def load_baseline() -> dict:
    """The recorded analysis/version p95 ratio, tolerance, and build commit."""
    try:
        data = json.loads(BASELINE_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise RuntimeError(f"cannot load baseline {BASELINE_FILE}") from error
    if not isinstance(data, dict):
        raise RuntimeError(f"baseline {BASELINE_FILE} is not a JSON object")
    return data


def load_history() -> dict:
    try:
        data = json.loads(HISTORY_FILE.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return {}
    except (OSError, ValueError) as error:
        raise HistoryError(f"cannot load run history {HISTORY_FILE}") from error
    if not isinstance(data, dict):
        raise HistoryError(f"run history {HISTORY_FILE} is not a JSON object")
    return data


def write_json(path: Path, data: dict) -> None:
    temporary = path.with_name(f"{path.name}.tmp")
    temporary.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def run(
    binary: Path,
    payload: str,
    env: dict[str, str] | None = None,
    args: tuple[str, ...] = (),
) -> subprocess.CompletedProcess[bytes]:
    # `binary` is this harness's own argument (a locally built executable), never untrusted input,
    # so invoking it is the tool's purpose rather than a command-injection sink.
    return subprocess.run(  # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
        [str(binary), *args], input=payload.encode(), capture_output=True, check=False, env=env
    )


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=False
    )


def full_commit(rev: str) -> str | None:
    result = git("rev-parse", "--verify", f"{rev}^{{commit}}")
    return result.stdout.strip() if result.returncode == 0 else None


def git_head() -> str | None:
    return full_commit("HEAD")


def build_commit() -> str:
    """The commit the binary under test was built from.

    CI or a fresh build may set `BASELINE_BUILT_FROM`; otherwise the checkout's HEAD is the answer.
    An explicit commit that is not HEAD means a stale binary, which the gate must not measure.
    """
    head = git_head()
    declared = os.environ.get("BASELINE_BUILT_FROM", "").strip()
    if declared:
        declared_commit = full_commit(declared) or declared
        if head is not None and declared_commit != head:
            raise ProvenanceError(
                f"BASELINE_BUILT_FROM={declared_commit} but HEAD is {head}: the binary under test "
                "was not built from HEAD. Rebuild from HEAD, or unset BASELINE_BUILT_FROM."
            )
        return declared_commit
    if head is None:
        raise ProvenanceError(
            "cannot determine the build commit: git rev-parse HEAD failed and BASELINE_BUILT_FROM "
            "is unset. Build from a git checkout or set BASELINE_BUILT_FROM to the commit the "
            "binary was built from."
        )
    return head


def recorded_from_head(built_from: str, head: str) -> tuple[bool, str]:
    """True when HEAD is the recorded commit or only rewrote the recorded baseline file."""
    if built_from == head:
        return True, ""
    if git("merge-base", "--is-ancestor", built_from, head).returncode != 0:
        return False, f"built_from {built_from[:12]} is not an ancestor of HEAD {head[:12]}"
    changed_result = git("diff", "--name-only", built_from, head)
    if changed_result.returncode != 0:
        return False, f"cannot diff built_from {built_from[:12]} against HEAD {head[:12]}"
    changed = {line.strip() for line in changed_result.stdout.splitlines() if line.strip()}
    if "eval/baseline.json" in changed and changed <= RECORD_ONLY_FILES:
        return True, (
            f"note: HEAD {head[:12]} only rewrote eval/baseline.json since "
            f"built_from {built_from[:12]}"
        )
    return False, (
        f"HEAD {head[:12]} differs from built_from {built_from[:12]} outside "
        "eval/baseline.json"
    )


RUN_INVOCATIONS = 210
DISCARD_FIRST = 10

Timed = list[tuple[str, dict[str, str] | None, tuple[str, ...]]]


def measure_once(binary: Path, cases: Timed) -> tuple[float, float]:
    """One run: RUN_INVOCATIONS iterations of the worst timed case, return (median, p95) ms."""
    samples = []
    for _ in range(RUN_INVOCATIONS):
        worst = 0.0
        for payload, env, args in cases:
            start = time.perf_counter()
            run(binary, payload, env, args)
            worst = max(worst, (time.perf_counter() - start) * 1000.0)
        samples.append(worst)
    samples = samples[DISCARD_FIRST:]
    samples.sort()
    return statistics.median(samples), samples[int(len(samples) * 0.95)]


@dataclass(frozen=True)
class Session:
    index: int
    analysis_p50_ms: float
    analysis_p95_ms: float
    version_p95_ms: float
    ratio: float


def measure_sessions(binary: Path, analysis_cases: Timed) -> list[Session]:
    """Five sessions; each measures analysis first, then `--version` in the same session."""
    version_cases: Timed = [("", None, ("--version",))]
    sessions = []
    for index in range(1, SESSIONS + 1):
        analysis_p50, analysis_p95 = measure_once(binary, analysis_cases)
        _, version_p95 = measure_once(binary, version_cases)
        if version_p95 <= 0:
            raise RuntimeError("--version p95 measured as zero; cannot form a ratio")
        ratio = analysis_p95 / version_p95
        session = Session(index, analysis_p50, analysis_p95, version_p95, ratio)
        sessions.append(session)
        print(
            f"  session {index}/{SESSIONS}: analysis p95 {analysis_p95:.2f} ms, "
            f"--version p95 {version_p95:.2f} ms, ratio {ratio:.2f}"
        )
    return sessions


def median(values: list[float]) -> float:
    return statistics.median(values)


def spread(values: list[float]) -> tuple[float, float]:
    middle = median(values)
    low, high = min(values), max(values)
    relative = (high - low) / middle if middle > 0 else 0.0
    return high - low, relative


def report_sessions(sessions: list[Session]) -> tuple[float, float]:
    ratios = [session.ratio for session in sessions]
    versions = [session.version_p95_ms for session in sessions]
    analyses = [session.analysis_p95_ms for session in sessions]
    ratio_median = median(ratios)
    ratio_spread, ratio_relative = spread(ratios)
    version_spread, version_relative = spread(versions)
    analysis_spread, analysis_relative = spread(analyses)

    print(f"\nratios: {', '.join(f'{value:.2f}' for value in ratios)}")
    print(
        f"  median {ratio_median:.2f}; spread {ratio_spread:.2f} "
        f"({ratio_relative:.1%}) across {SESSIONS} sessions"
    )
    print(
        f"--version p95: {', '.join(f'{value:.2f}' for value in versions)} ms; "
        f"spread {version_spread:.2f} ms ({version_relative:.1%})"
    )
    print(
        f"analysis p95: {', '.join(f'{value:.2f}' for value in analyses)} ms; "
        f"spread {analysis_spread:.2f} ms ({analysis_relative:.1%})"
    )
    analysis_median = median(analyses)
    if analysis_median >= BUDGET_P95_MS:
        print(
            f"  warning: analysis p95 {analysis_median:.2f} ms is over the "
            f"{BUDGET_P95_MS:.0f} ms target (warning only)"
        )
    return ratio_median, analysis_median


def check_contract(binary: Path) -> int:
    failures = 0
    for name, payload, expected in CASES:
        result = run(binary, payload)
        status = "ok" if result.returncode == expected else "FAIL"
        if result.returncode != expected:
            failures += 1
        print(f"  [{status}] {name}: exit {result.returncode} (expected {expected})")
    return failures


def analysis_cases() -> Timed:
    trigger = next(payload for name, payload, _ in CASES if name.startswith("trigger"))
    cases: Timed = [(trigger, None, ())]
    cwd_case = next((payload for name, payload, _ in CASES if name == "write-with-cwd"), None)
    if cwd_case is not None:
        cases.append((cwd_case, {**os.environ, "TEST_CHECKER_WRITE_BASELINE": "1"}, ()))
    return cases


def record(binary: Path, profile: str) -> int:
    built_from = build_commit()
    failures = check_contract(binary)
    sessions = measure_sessions(binary, analysis_cases())
    ratio, _ = report_sessions(sessions)

    baseline = load_baseline()
    profiles = baseline.setdefault("profiles", {})
    if not isinstance(profiles, dict):
        print(f"baseline: 'profiles' in {BASELINE_FILE} is not an object", file=sys.stderr)
        return 2
    profiles[profile] = {"ratio": round(ratio, 4)}
    baseline["tolerance_percent"] = TOLERANCE_PERCENT
    baseline["built_from"] = built_from
    write_json(BASELINE_FILE, baseline)

    # A new baseline invalidates the run history it replaces.
    history = load_history()
    history[profile] = []
    write_json(HISTORY_FILE, history)

    print(
        f"\nrecorded {profile} ratio {ratio:.2f} (median of {SESSIONS} sessions), "
        f"tolerance {TOLERANCE_PERCENT:.0f}%, built_from {built_from} in {BASELINE_FILE.name}"
    )
    return 1 if failures else 0


def gate(binary: Path, profile: str) -> int:
    built_from = build_commit()
    baseline = load_baseline()

    recorded_from = baseline.get("built_from")
    if not isinstance(recorded_from, str) or not recorded_from:
        print(
            f"baseline: {BASELINE_FILE} has no usable built_from; re-record on this build "
            "with --record.",
            file=sys.stderr,
        )
        return 2
    recorded_commit = full_commit(recorded_from) or recorded_from
    matches, note = recorded_from_head(recorded_commit, built_from)
    if not matches:
        print(
            f"baseline: refusing to compare: {note}. Rebuild from HEAD and re-record, or restore "
            "the commit the baseline was recorded on.",
            file=sys.stderr,
        )
        return 2
    if note:
        print(f"baseline: {note}")

    recorded = baseline.get("profiles", {}).get(profile, {}).get("ratio")
    recorded = (
        float(recorded)
        if isinstance(recorded, (int, float)) and math.isfinite(recorded) and recorded > 0
        else None
    )
    tolerance = baseline.get("tolerance_percent")
    tolerance = (
        float(tolerance)
        if isinstance(tolerance, (int, float)) and math.isfinite(tolerance) and tolerance > 0
        else None
    )
    if recorded is None or tolerance is None:
        print(
            f"baseline: {BASELINE_FILE} has no usable ratio/tolerance for profile {profile}; "
            "re-record.",
            file=sys.stderr,
        )
        return 1

    failures = check_contract(binary)
    sessions = measure_sessions(binary, analysis_cases())
    ratio, _ = report_sessions(sessions)

    threshold = recorded * (1.0 + tolerance / 100.0)
    over = ratio > threshold

    try:
        history = load_history()
    except HistoryError as error:
        print(f"\nbaseline: {error}; delete it to start a fresh history.", file=sys.stderr)
        return 2
    runs = history.get(profile, [])
    if not isinstance(runs, list):
        runs = []
    runs = [entry for entry in runs if isinstance(entry, dict)]
    runs.append(
        {
            "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "built_from": built_from,
            "ratio": round(ratio, 4),
            "threshold": round(threshold, 4),
            "over": over,
            "ratios": [round(session.ratio, 4) for session in sessions],
            "version_p95_ms": [round(session.version_p95_ms, 3) for session in sessions],
        }
    )
    runs = runs[-HISTORY_LIMIT:]
    history[profile] = runs
    write_json(HISTORY_FILE, history)

    over_count = sum(1 for entry in runs if entry.get("over") is True)
    gate_failed = over_count >= OVER_LIMIT
    print(
        f"\ngate {profile}: recorded ratio {recorded:.2f}, tolerance {tolerance:.0f}%, "
        f"threshold {threshold:.2f}; this run {ratio:.2f} -> {'over' if over else 'within'}"
    )
    recent = ", ".join(f"{entry.get('ratio', 0):.2f}" for entry in runs)
    print(
        f"history: {over_count} of last {len(runs)} run(s) over threshold [{recent}]: "
        f"{'FAIL' if gate_failed else 'PASS'}"
    )

    if failures or gate_failed:
        print(
            f"\nbaseline: {failures} contract failure(s), gate_failed={gate_failed} "
            f"({over_count}/{len(runs)} over threshold)",
            file=sys.stderr,
        )
        return 1
    return 0


def main() -> int:
    binary = (
        Path(sys.argv[1])
        if len(sys.argv) > 1 and not sys.argv[1].startswith("--")
        else ROOT / "target" / "release" / "test-checker"
    )
    if not binary.is_file():
        print(f"baseline: binary not found: {binary}", file=sys.stderr)
        return 2

    profile = "debug" if "debug" in binary.parts else "release"

    try:
        if "--record" in sys.argv:
            return record(binary, profile)
        # The debug profile is measured for information, but only release is gated.
        if profile == "debug":
            built_from = build_commit()
            print(f"baseline: debug profile, built_from {built_from} [not gated]")
            failures = check_contract(binary)
            sessions = measure_sessions(binary, analysis_cases())
            report_sessions(sessions)
            return 1 if failures else 0
        return gate(binary, profile)
    except ProvenanceError as error:
        print(f"baseline: {error}", file=sys.stderr)
        return 2
    except (RuntimeError, HistoryError) as error:
        print(f"baseline: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
