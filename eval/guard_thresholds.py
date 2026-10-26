#!/usr/bin/env python3
"""Recompute the refactor-guard threshold table from the shipped binary.

The binary prints one `GUARD_METRICS {json}` line per edit when
`TEST_CHECKER_GUARD_METRICS=1` is set (see `src/detector.rs`). That makes the
20/40/60% table reproducible without an instrumented copy.

The metric per payload is:
  demote(threshold) = (old_lines >= 20 and removed_lines/old_lines > threshold)
                      or (removed_tests > 0 and added_tests >= removed_tests)
  blocking = removal findings on a payload that is not demoted

Usage:
  python3 eval/guard_thresholds.py --binary /path/to/test-checker
  python3 eval/guard_thresholds.py --binary ... --repos /path/to/tc /path/to/cc
  python3 eval/guard_thresholds.py --binary ... --truth eval/replay_removal_truth.json

`--truth` is a JSON file with `cases: [{ "key": ... }]` for replay payloads that
are known genuine deletions; the key is `sha256(kind|path|old|new)[:16]`.
Without it, the replay TP recall is reported as `n/a`.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay

THRESHOLDS = (20, 40, 60)
REMOVAL_RULES = ("test-removed", "assertion-removed")
FINDING = re.compile(r'<finding line-number="\d+" rule="([^"]+)" confidence="([^"]+)"')
DEFAULT_REPOS = [
    "/var/home/bacon/Projects/test-checker",
    "/var/home/bacon/Projects/comment-checker",
]


def payload_key(kind: str, path: str, payload: dict) -> str:
    tool = payload["tool_input"]
    old = tool.get("old_string", "")
    new = tool.get("new_string", tool.get("content", ""))
    digest = hashlib.sha256(f"{kind}|{path}|{old}|{new}".encode()).hexdigest()
    return digest[:16]


def run(binary: str, payload: dict) -> tuple[dict | None, list[tuple[str, str]]]:
    env = dict(os.environ)
    env["TEST_CHECKER_GUARD_METRICS"] = "1"
    process = subprocess.run(
        [binary, "--min-confidence", "low"],
        input=json.dumps(payload).encode(),
        capture_output=True,
        env=env,
    )
    text = (process.stderr or process.stdout).decode()
    metrics = None
    for line in text.split("\n"):
        if line.startswith("GUARD_METRICS "):
            metrics = json.loads(line[len("GUARD_METRICS ") :])
            break
    return metrics, FINDING.findall(text)


def demote(metrics: dict, threshold: int) -> bool:
    old_lines = metrics["old_lines"]
    ratio = metrics["removed_lines"] / old_lines if old_lines else 0.0
    by_lines = old_lines >= 20 and ratio * 100 > threshold
    by_tests = metrics["removed_tests"] > 0 and metrics["added_tests"] >= metrics["removed_tests"]
    return by_lines or by_tests


def removal_count(metrics: dict) -> int:
    return metrics["test_removed"] + metrics["assertion_removed"]


def replay_rows(binary: str, repos: list[str]) -> list[dict]:
    rows = []
    for repo in repos:
        for kind, path, payload in replay.replay_payloads(Path(repo)):
            metrics, findings = run(binary, payload)
            if metrics is None:
                continue
            metrics.update(kind=kind, path=path, key=payload_key(kind, path, payload))
            metrics["removal"] = removal_count(metrics)
            rows.append(metrics)
    return rows


def corpus_rows(binary: str, corpus_path: str) -> list[dict]:
    rows = []
    cases = json.loads(Path(corpus_path).read_text())
    for case in cases:
        if "old_content" not in case:
            continue
        kind, path = "edit", case["path"]
        payload = {
            "tool_name": "Edit",
            "tool_input": {
                "file_path": path,
                "old_string": case["old_content"],
                "new_string": case["content"],
            },
        }
        metrics, findings = run(binary, payload)
        if metrics is None:
            continue
        metrics.update(
            kind=kind,
            path=path,
            key=payload_key(kind, path, payload),
            expected=case.get("expected"),
            high={rule for rule, confidence in findings if confidence == "high"},
        )
        metrics["removal"] = removal_count(metrics)
        rows.append(metrics)
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--repos", nargs="+", default=DEFAULT_REPOS)
    parser.add_argument("--corpus", default="eval/corpus.json")
    parser.add_argument("--truth", help="JSON with cases[].key for replay TP payloads")
    args = parser.parse_args()

    replay_data = replay_rows(args.binary, args.repos)
    corpus_data = corpus_rows(args.binary, args.corpus)
    truth = set()
    if args.truth:
        truth = {case["key"] for case in json.loads(Path(args.truth).read_text())["cases"]}

    print(f"replay payloads with guard metrics: {len(replay_data)}")
    print(f"recall-corpus edit cases: {len(corpus_data)}")
    print(f"threshold  total  demoted  blocking  tp_recall  fp_blocking")
    for threshold in THRESHOLDS:
        demoted = [row for row in replay_data if demote(row, threshold)]
        total = sum(row["removal"] for row in replay_data)
        demoted_findings = sum(row["removal"] for row in demoted)
        blocking = total - demoted_findings
        if truth:
            tp_total = sum(row["removal"] for row in replay_data if row["key"] in truth)
            tp_blocking = sum(
                row["removal"]
                for row in replay_data
                if row["key"] in truth and not demote(row, threshold)
            )
            tp_recall = f"{tp_blocking}/{tp_total}"
        else:
            tp_blocking = 0
            tp_recall = "n/a"
        print(
            f"{threshold:>8}%  {total:>5}  {demoted_findings:>7}  {blocking:>8}"
            f"  {tp_recall:>9}  {blocking - tp_blocking:>11}"
        )

    print("\nrecall corpus (expected removal rule still High):")
    for threshold in THRESHOLDS:
        kept = [
            row
            for row in corpus_data
            if not demote(row, threshold) and row["expected"] in row["high"]
        ]
        print(f"  {threshold:>3}%: {len(kept)}/{len(corpus_data)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
