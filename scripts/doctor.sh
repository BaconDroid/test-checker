#!/usr/bin/env bash
# Standalone install doctor for test-checker.
#
# This script is intentionally not wired into the CLI. Run it from the
# directory whose .claude/settings.json should be checked:
#
#   scripts/doctor.sh
#
# It only depends on the test-checker binary being available on PATH.

set -u

BAD_PAYLOAD='{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    @pytest.mark.skip\n    assert add(1, 2) == 3\n"}}'
GOOD_PAYLOAD='{"tool_name":"Write","tool_input":{"file_path":"tests/test_app.py","content":"def test_sums():\n    assert add(1, 2) == 3\n"}}'

bin_path="$(command -v test-checker 2>/dev/null || true)"
if [ -z "$bin_path" ]; then
    printf '%s\n' \
        "doctor: FAIL: 'test-checker' was not found on PATH." \
        "  Install it with 'npm install -g @bacondroid/test-checker' or a Homebrew tap," \
        "  or add the directory containing the binary to PATH." \
        "  Verify with: command -v test-checker && test-checker --version" >&2
    exit 1
fi

printf 'doctor: binary: %s\n' "$bin_path"
hook_summary="no .claude/settings.json in $PWD"

# 1. --version must succeed and be reported verbatim.
version_status=0
version_output="$("$bin_path" --version 2>&1)" || version_status=$?
if [ "$version_status" -ne 0 ]; then
    printf 'doctor: FAIL: --version exited %s.\n' "$version_status" >&2
    printf '%s\n' "$version_output" >&2
    printf '%s\n' \
        "  Reinstall the binary ('npm install -g @bacondroid/test-checker') or rebuild it" \
        "  ('cargo build --release') and make sure the resulting binary is on PATH." >&2
    exit 1
fi
printf '%s\n' "$version_output"

# 2. Optional Claude Code hook wiring check.
if [ -f .claude/settings.json ]; then
    if grep -q 'test-checker' .claude/settings.json 2>/dev/null; then
        hook_summary=".claude/settings.json references test-checker"
        printf 'doctor: hook settings: %s\n' "$hook_summary"
    else
        printf '%s\n' \
            "doctor: FAIL: .claude/settings.json exists but does not reference test-checker." \
            "  Add a PostToolUse hook for Write|Edit|MultiEdit that invokes test-checker," \
            "  then rerun this doctor from the same directory." >&2
        exit 1
    fi
else
    printf 'doctor: note: %s; skipping hook-wiring check (not a failure).\n' "$hook_summary"
fi

# Run one gate and show the binary's stderr verbatim. Never use set -e here:
# the expected failure (exit 2) is data, not a script failure.
run_gate() {
    local label="$1"
    local expected="$2"
    local payload="$3"
    local gate_status=0
    local gate_stderr

    gate_stderr="$(printf '%s' "$payload" | "$bin_path" 2>&1 >/dev/null)" || gate_status=$?

    printf '\n[doctor] %s (expected exit %s)\n' "$label" "$expected"
    if [ -n "$gate_stderr" ]; then
        printf '%s\n' "$gate_stderr"
    else
        printf '(binary wrote no stderr)\n'
    fi
    printf '[doctor] actual exit: %s\n' "$gate_status"

    if [ "$gate_status" -ne "$expected" ]; then
        printf 'doctor: FAIL: %s expected exit %s but got %s.\n' \
            "$label" "$expected" "$gate_status" >&2
        printf '%s\n' \
            "  Inspect the gate output above. test-checker must exit 2 when it reports" \
            "  findings and 0 for clean input. Rebuild ('cargo build --release') or" \
            "  reinstall ('npm install -g @bacondroid/test-checker') the binary, then" \
            "  rerun this doctor from the same directory." >&2
        return 1
    fi

    return 0
}

if ! run_gate "known-bad skip-added payload" 2 "$BAD_PAYLOAD"; then
    exit 1
fi

if ! run_gate "known-good clean payload" 0 "$GOOD_PAYLOAD"; then
    exit 1
fi

printf '\ndoctor: OK\n'
printf '  binary: %s\n' "$bin_path"
printf '  version: %s\n' "$version_output"
printf '  hook settings: %s\n' "$hook_summary"
printf '  skip-added gate: exit 2 as expected\n'
printf '  clean gate: exit 0 as expected\n'
exit 0
