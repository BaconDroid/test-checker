#!/usr/bin/env python3
"""D0 baseline: assert the binary's exit-code contract and measure per-invocation wall time.

Run from the repository root. This is an off-contract eval helper (see eval/README.md), not part
of the parity-locked suite.
"""
from __future__ import annotations

import json
import math
import statistics
import subprocess
import sys
import time
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

BUDGET_P95_MS = 15.0
BASELINE_FILE = Path(__file__).resolve().parent / "baseline.json"


def load_baseline() -> dict:
    """The recorded release p95 and regression tolerance (the W0-1 relative gate)."""
    try:
        return json.loads(BASELINE_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise RuntimeError(f"cannot load baseline {BASELINE_FILE}") from error


def run(binary: Path, payload: str) -> subprocess.CompletedProcess[bytes]:
    # `binary` is this harness's own argument (a locally built executable), never untrusted input,
    # so invoking it is the tool's purpose rather than a command-injection sink.
    return subprocess.run(  # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
        [str(binary)], input=payload.encode(), capture_output=True, check=False
    )


RUN_INVOCATIONS = 210
DISCARD_FIRST = 10


def measure_once(binary: Path, payload: str) -> tuple[float, float]:
    """One run: RUN_INVOCATIONS calls, discard the first DISCARD_FIRST, return (median, p95) ms."""
    samples = []
    for _ in range(RUN_INVOCATIONS):
        start = time.perf_counter()
        run(binary, payload)
        samples.append((time.perf_counter() - start) * 1000.0)
    samples = samples[DISCARD_FIRST:]
    samples.sort()
    return statistics.median(samples), samples[int(len(samples) * 0.95)]


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


def main() -> int:
    binary = (
        Path(sys.argv[1])
        if len(sys.argv) > 1
        else ROOT / "target" / "release" / "test-checker"
    )
    if not binary.is_file():
        print(f"baseline: binary not found: {binary}", file=sys.stderr)
        return 2

    failures = 0
    for name, payload, expected in CASES:
        result = run(binary, payload)
        status = "ok" if result.returncode == expected else "FAIL"
        if result.returncode != expected:
            failures += 1
        print(f"  [{status}] {name}: exit {result.returncode} (expected {expected})")

    trigger = next(payload for name, payload, _ in CASES if name.startswith("trigger"))
    profile = "debug" if "debug" in binary.parts else "release"
    # Three runs; take the run with the lowest p95 to cut noise.
    p50, p95 = min((measure_once(binary, trigger) for _ in range(3)), key=lambda pair: pair[1])

    if "--record" in sys.argv:
        baseline = load_baseline()
        baseline.setdefault("profiles", {})[profile] = {"p95_ms": round(p95, 2)}
        BASELINE_FILE.write_text(json.dumps(baseline, indent=2) + "\n", encoding="utf-8")
        print(f"\nrecorded {profile} p95 {p95:.2f} ms in {BASELINE_FILE.name}")
        return 1 if failures else 0

    if profile == "debug":
        print(
            f"\nper-invocation (startup included): p50 {p50:.2f} ms, p95 {p95:.2f} ms "
            "[debug profile: not gated]"
        )
        return 1 if failures else 0

    try:
        baseline = load_baseline()
    except RuntimeError as error:
        print(f"\nbaseline: {error}", file=sys.stderr)
        return 1

    # Relative gate: fail when p95 regresses > tolerance vs the recorded baseline (per profile).
    recorded = baseline.get("profiles", {}).get(profile, {}).get("p95_ms")
    recorded = (
        float(recorded)
        if isinstance(recorded, (int, float)) and math.isfinite(recorded) and recorded > 0
        else None
    )
    tolerance = baseline.get("tolerance_percent", 25.0)
    tolerance = (
        float(tolerance)
        if isinstance(tolerance, (int, float)) and math.isfinite(tolerance) and tolerance > 0
        else None
    )
    regression = ((p95 - recorded) / recorded * 100.0) if recorded else 0.0
    # Fail-closed: a missing/invalid baseline or tolerance must fail, never disable the gate.
    regressed = recorded is None or tolerance is None or regression > tolerance
    flags = []
    if recorded is None:
        flags.append("no usable baseline")
    if tolerance is None:
        flags.append("no usable tolerance")
    if p95 >= BUDGET_P95_MS:
        flags.append(f"p95 over target {BUDGET_P95_MS:.0f} ms")
    if recorded is not None and tolerance is not None and regression > tolerance:
        flags.append(f"p95 regressed {regression:+.0f}% > {tolerance:.0f}%")
    status = "; ".join(flags) if flags else "ok"
    print(f"\nper-invocation (startup included): p50 {p50:.2f} ms, p95 {p95:.2f} ms [{status}]")

    if failures or regressed:
        print(
            f"\nbaseline: {failures} contract failure(s), regressed={regressed}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
