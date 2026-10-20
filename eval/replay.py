#!/usr/bin/env python3
"""Precision audit: replay real test-file changes and scan both repos' test sources.

Two independent checks, printed side by side for a `--before`/`--after` binary pair:

1. **Replay** — 320 payloads built from the two repositories' git history. For the 80 most recent
   commits per repo that touch a test-path file (both parent and commit carry the file), one
   `Write` payload carries the committed content and one `Edit` payload carries the parent ->
   commit pair. 2 repos x 80 commits x 2 payload kinds = 320.
2. **Audit** — every tracked `.rs`/`.js`/`.py` file in both repos except the eval harness
   (`eval/baseline.py`), sent as a `Write` payload: 58 files at this snapshot.

Findings are counted per rule over the rendered XML; the exit-code-2 payload/file count is reported
too. This harness is read-only.

Usage:
    python3 eval/replay.py --before /tmp/before --after /tmp/after
    python3 eval/replay.py --after target/debug/test-checker
"""

from __future__ import annotations

import argparse
import collections
import json
import re
import subprocess
import sys
from pathlib import Path

COMMITS_PER_REPO = 80
AUDIT_EXCLUDES = {"eval/baseline.py"}
TEST_PATH = re.compile(
    r"(^|/)(tests?|__tests__|spec|specs)/"
    r"|(^|/)test[_-][^/]*\.[^.]+$"
    r"|_test\.[^.]+$|\.test\.[^.]+$|\.spec\.[^.]+$|_spec\.[^.]+$"
)
RULE = re.compile(r'rule="([^"]+)"')


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def blob(repo: Path, revision: str, path: str) -> str | None:
    try:
        return git(repo, "show", f"{revision}:{path}")
    except subprocess.CalledProcessError:
        return None


def replay_payloads(repo: Path) -> list[tuple[str, str, dict[str, object]]]:
    """(label, file_path, payload) for the repo's 80 most recent test-file commits."""
    commits = git(repo, "log", "--format=%H", "-n", "400").split()
    payloads: list[tuple[str, str, dict[str, object]]] = []
    events = 0
    for commit in commits:
        if events >= COMMITS_PER_REPO:
            break
        try:
            changed = git(
                repo, "diff-tree", "--no-commit-id", "--name-only", "-r", f"{commit}^", commit
            ).split()
        except subprocess.CalledProcessError:
            continue
        for chosen in changed:
            if events >= COMMITS_PER_REPO:
                break
            if not TEST_PATH.search(chosen):
                continue
            if chosen.rsplit(".", 1)[-1] not in {
                "py", "js", "ts", "tsx", "jsx", "go", "rs", "java", "rb", "php", "c",
                "cpp", "cs", "kt", "swift", "scala", "lua", "ex", "exs",
            }:
                continue
            new = blob(repo, commit, chosen)
            old = blob(repo, f"{commit}^", chosen)
            if new is None or old is None:
                continue
            events += 1
            payloads.append(
                (
                    "write",
                    chosen,
                    {"tool_name": "Write", "tool_input": {"file_path": chosen, "content": new}},
                )
            )
            payloads.append(
                (
                    "edit",
                    chosen,
                    {
                        "tool_name": "Edit",
                        "tool_input": {
                            "file_path": chosen,
                            "old_string": old,
                            "new_string": new,
                        },
                    },
                )
            )
    return payloads


def audit_files(repo: Path) -> list[tuple[str, str, dict[str, object]]]:
    files = []
    for path in git(repo, "ls-files").split():
        if path in AUDIT_EXCLUDES:
            continue
        if path.rsplit(".", 1)[-1] not in {"rs", "js", "py"}:
            continue
        content = blob(repo, "HEAD", path)
        if content is None:
            continue
        files.append(
            (
                "audit",
                path,
                {"tool_name": "Write", "tool_input": {"file_path": path, "content": content}},
            )
        )
    return files


def run(binary: str, payloads: list[tuple[str, str, dict[str, object]]]) -> dict[str, object]:
    rules: collections.Counter[str] = collections.Counter()
    blocked = 0
    for _label, _path, payload in payloads:
        process = subprocess.run(
            [binary, "--min-confidence", "low"],
            input=json.dumps(payload).encode(),
            capture_output=True,
            check=False,
        )
        text = (process.stderr or process.stdout).decode()
        found = RULE.findall(text)
        if process.returncode == 2:
            blocked += 1
        rules.update(found)
    return {
        "payloads": len(payloads),
        "exit2": blocked,
        "findings": sum(rules.values()),
        "per_rule": dict(sorted(rules.items())),
    }


def show(name: str, result: dict[str, object]) -> None:
    print(
        f"{name}: {result['payloads']} payloads, {result['exit2']} exit 2, "
        f"{result['findings']} findings"
    )
    per_rule = result["per_rule"]
    assert isinstance(per_rule, dict)
    if not per_rule:
        print("  (no findings)")
    for rule, count in per_rule.items():
        print(f"  {rule}: {count}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", help="pre-change binary")
    parser.add_argument("--after", required=False, help="post-change binary")
    parser.add_argument(
        "--repos",
        nargs="+",
        default=[
            "/var/home/bacon/Projects/test-checker",
            "/var/home/bacon/Projects/comment-checker",
        ],
    )
    args = parser.parse_args()
    binaries = [("before", args.before), ("after", args.after or args.before)]
    if not any(path for _, path in binaries):
        parser.error("supply --after (and optionally --before)")

    replay: list[tuple[str, str, dict[str, object]]] = []
    audit: list[tuple[str, str, dict[str, object]]] = []
    for repo in map(Path, args.repos):
        replay.extend(replay_payloads(repo))
        audit.extend(audit_files(repo))

    print(
        f"corpus: {len(replay)} replay payloads from {len(args.repos)} repos, "
        f"{len(audit)} audit files"
    )
    results = {}
    for label, binary in binaries:
        if not binary:
            continue
        results[label] = {
            "replay": run(binary, replay),
            "audit": run(binary, audit),
        }
    for label in ("before", "after"):
        if label not in results:
            continue
        print(f"\n== {label} ==")
        show("replay", results[label]["replay"])
        show("audit", results[label]["audit"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
