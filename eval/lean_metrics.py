#!/usr/bin/env python3
"""Replay AI-generated task diffs and report the "lean" product metrics, before vs after the hooks.

No corpus is shipped (see `LEAN-METRICS.md`). This harness defines the corpus schema and computes
what the data can support. Fields the corpus does not provide are reported as `null`, never invented.

Corpus schema (JSON):
{
  "tasks": [
    {
      "task_id": "t1",
      "hook_enabled": true,          // false = "before the hooks"
      "files": [
        {"path": "src/a.py", "old": "...", "new": "...",
         "revisions": 1,             // optional: agent revisions after an exit 2
         "kept": 3, "kept_fp": 0}    // optional: human labels on kept comments/tests
      ]
    }
  ]
}

Usage:
  python3 eval/lean_metrics.py --corpus tasks.json
  python3 eval/lean_metrics.py --corpus tasks.json --cc target/release/comment-checker \
      --tc ../test-checker/target/release/test-checker
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

COMMENT_TOKENS = ("#", "//", "/*", "*", "--")
TEST_DECL = re.compile(
    r"(\bdef\s+test\w*\s*\()|(\bfunc\s+Test\w*\s*\()|(\bfn\s+\w+\s*\()"
    r"|(\b(?:it|test)\s*\(\s*['\"])|(\bvoid\s+\w+\s*\()|(\bfun\s+\w+\s*\()"
)


def added_lines(old: str, new: str) -> list[str]:
    matcher = difflib.SequenceMatcher(a=old.splitlines(), b=new.splitlines())
    added: list[str] = []
    for tag, _i1, _i2, j1, j2 in matcher.get_opcodes():
        if tag in ("insert", "replace"):
            added.extend(new.splitlines()[j1:j2])
    return added


def count_comments(lines: list[str]) -> int:
    return sum(
        1
        for line in lines
        if line.strip() and line.strip().startswith(COMMENT_TOKENS)
    )


def count_tests(lines: list[str]) -> int:
    return sum(1 for line in lines if TEST_DECL.search(line))


def resolve_executable(value: str) -> str:
    # The path is operator-supplied (CLI flag), never remote input.
    candidate = shutil.which(value) or value
    path = Path(candidate).resolve()
    if not path.is_file() or not os.access(path, os.X_OK):
        raise SystemExit(f"lean_metrics: not an executable file: {value}")
    return str(path)


def hook_blocks(binary: str, path: str, content: str) -> bool:
    payload = json.dumps(
        {"tool_name": "Write", "tool_input": {"file_path": path, "content": content}}
    ).encode()
    try:
        # The binary path is operator-supplied (CLI flag), never remote input; list form + shell=False.
        # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit
        proc = subprocess.run([binary], input=payload, capture_output=True, shell=False)
    except OSError:
        return False
    return proc.returncode == 2


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--cc", default=None, help="comment-checker binary (optional)")
    parser.add_argument("--tc", default=None, help="test-checker binary (optional)")
    args = parser.parse_args()
    cc = resolve_executable(args.cc) if args.cc else None
    tc = resolve_executable(args.tc) if args.tc else None

    raw = json.loads(Path(args.corpus).read_text(encoding="utf-8"))
    tasks = raw.get("tasks", [])
    if not isinstance(tasks, list):
        print("lean_metrics: corpus.tasks must be a list", file=sys.stderr)
        return 2

    groups: dict[str, dict[str, int]] = {}
    per_task: list[dict[str, object]] = []
    for task in tasks:
        enabled = bool(task.get("hook_enabled"))
        comments = tests = blocked = revisions = kept = kept_fp = 0
        for item in task.get("files", []):
            lines = added_lines(str(item.get("old", "")), str(item.get("new", "")))
            comments += count_comments(lines)
            tests += count_tests(lines)
            revisions += int(item.get("revisions", 0) or 0)
            kept += int(item.get("kept", 0) or 0)
            kept_fp += int(item.get("kept_fp", 0) or 0)
            if cc and hook_blocks(cc, str(item.get("path", "a.py")), str(item.get("new", ""))):
                blocked = 1
            if tc and hook_blocks(tc, str(item.get("path", "a.py")), str(item.get("new", ""))):
                blocked = 1
        per_task.append(
            {
                "task_id": task.get("task_id", ""),
                "hook_enabled": enabled,
                "comments_added": comments,
                "tests_added": tests,
                "blocked": bool(blocked) if (cc or tc) else None,
                "revisions": revisions,
                "kept": kept,
                "kept_fp": kept_fp,
            }
        )
        key = "after" if enabled else "before"
        group = groups.setdefault(
            key,
            {
                "tasks": 0,
                "comments_added": 0,
                "tests_added": 0,
                "blocked": 0,
                "revisions": 0,
                "kept": 0,
                "kept_fp": 0,
            },
        )
        group["tasks"] += 1
        group["comments_added"] += comments
        group["tests_added"] += tests
        group["blocked"] += blocked
        group["revisions"] += revisions
        group["kept"] += kept
        group["kept_fp"] += kept_fp

    summary: dict[str, dict[str, Any]] = {}
    for key, group in groups.items():
        count = group["tasks"] or 1
        summary[key] = {
            "tasks": group["tasks"],
            "comments_added_per_task": group["comments_added"] / count,
            "tests_added_per_task": group["tests_added"] / count,
            "block_rate": group["blocked"] / count if (cc or tc) else None,
            "revisions_per_task": group["revisions"] / count,
            "false_positive_rate_on_kept": (
                group["kept_fp"] / group["kept"] if group["kept"] else None
            ),
        }

    print(json.dumps({"summary": summary, "tasks": per_task}, indent=2))
    for key in ("before", "after"):
        if key not in summary:
            continue
        s = summary[key]
        fp = s["false_positive_rate_on_kept"]
        block = s["block_rate"]
        fp_text = "n/a" if fp is None else f"{float(fp):.2%}"
        block_text = "n/a" if block is None else f"{float(block):.2%}"
        print(
            f"{key}: {s['tasks']} task(s), comments/task "
            f"{float(s['comments_added_per_task']):.2f}, tests/task "
            f"{float(s['tests_added_per_task']):.2f}, block-rate {block_text}, "
            f"revisions/task {float(s['revisions_per_task']):.2f}, FP-on-kept {fp_text}",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
