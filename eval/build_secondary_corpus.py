#!/usr/bin/env python3
"""Build, regenerate, and measure the secondary eval corpus from three OSS repositories.

The corpus is a precision sample of real test-file changes, not a labelled truth set. For each
selected commit that modifies a test file, the recipe emits the same two payloads `replay.py` uses:

* `Write` - the committed file content as `content`.
* `Edit` - the parent content as `old_string` and the committed content as `new_string`.

Only the manifest (`eval/secondary-manifest.json`) is tracked: it records the repository, the commit
SHA, the file path, and the payload kind for every entry. The payloads themselves embed third-party
source and stay in the scratch directory (`$TMPDIR/opencode/corpus-src`), never in this checkout.

Commands:

    # Clone/reuse the sources, select commits, and record the manifest (maintainer only).
    python3 eval/build_secondary_corpus.py select --manifest eval/secondary-manifest.json

    # Regenerate payloads from the manifest into the scratch directory.
    python3 eval/build_secondary_corpus.py build --manifest eval/secondary-manifest.json

    # Replay every payload through a binary and report hit/exit/latency distributions.
    python3 eval/build_secondary_corpus.py measure --binary target/release/test-checker

`select` is deliberately not reproducible over time: it picks the newest qualifying commits at the
clone's HEAD. `build` and `measure` reproduce exactly what the manifest records.

The selection filters follow each repository's history shape. `fd` keeps `tests/**.rs`, `netbox`
keeps only `test_*.py`, and `cal.diy` drops generated Playwright and Prisma artifacts. A file whose
content cannot be read on both sides (added, deleted, binary, oversized) is skipped and counted in
the manifest, so the expected counts stay auditable.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import re
import statistics
import subprocess
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = ROOT / "eval" / "secondary-manifest.json"
DEFAULT_SCRATCH = Path(os.environ.get("TMPDIR", "/tmp")) / "opencode" / "corpus-src"
MANIFEST_SCHEMA = 1
MAX_BLOB_BYTES = 1024 * 1024
MAX_FILES_PER_COMMIT = 2
LOG_LIMIT = 5000
EXTENSIONS = {
    "py", "js", "ts", "tsx", "jsx", "go", "rs", "java", "rb", "php", "c",
    "cpp", "cs", "kt", "swift", "scala", "lua", "ex", "exs",
}
RULE = re.compile(r'rule="([^"]+)"')
TEST_DECL = re.compile(
    r"(?m)^\s*(?:#\[(?:tokio::)?test\]|fn\s+test\w*|def\s+test\w*|"
    r"(?:it|test|describe)(?:\.\w+)?\s*\()"
)

CALDIY_PATHSPECS = (
    ":(glob)**/*.test.ts",
    ":(glob)**/*.test.tsx",
    ":(glob)**/*.test.js",
    ":(glob)**/*.test.jsx",
    ":(glob)**/*.spec.ts",
    ":(glob)**/*.spec.tsx",
    ":(glob)**/tests/**",
    ":(glob)**/__tests__/**",
)
CALDIY_EXCLUDES = re.compile(
    r"(^|/)(node_modules|test-results|playwright-report|blob-report|\.next)/"
    r"|(^|/)packages/prisma/"
    r"|(^|/)__snapshots__/"
    r"|\.snap$"
)


@dataclass(frozen=True)
class RepoSpec:
    key: str
    name: str
    url: str
    license: str
    language: str
    target_payloads: int
    clone_args: tuple[str, ...]
    pathspecs: tuple[str, ...]
    test_path: Callable[[str], bool]
    test_path_source: str
    excludes: re.Pattern[str] | None
    caveats: tuple[str, ...]


def netbox_test_path(path: str) -> bool:
    name = path.rsplit("/", 1)[-1]
    return replay.is_test_path(path) and name.startswith("test_") and name.endswith(".py")


REPOS = (
    RepoSpec(
        key="fd",
        name="sharkdp/fd",
        url="https://github.com/sharkdp/fd",
        license="MIT OR Apache-2.0",
        language="Rust",
        target_payloads=120,
        clone_args=(),
        pathspecs=("tests",),
        test_path=replay.is_test_path,
        test_path_source="eval/path-predicates.json",
        excludes=None,
        caveats=(
            "low diversity: tests/tests.rs supplies almost every modified test file",
            "Write/Edit pairs only; added or deleted test files are skipped",
        ),
    ),
    RepoSpec(
        key="netbox",
        name="netbox-community/netbox",
        url="https://github.com/netbox-community/netbox",
        license="Apache-2.0",
        language="Python",
        target_payloads=120,
        clone_args=(),
        pathspecs=(":(glob)**/test_*.py",),
        test_path=netbox_test_path,
        test_path_source="eval/path-predicates.json narrowed to basename test_*.py",
        excludes=None,
        caveats=(
            "only test_*.py is sampled; tests/ helpers and non-test paths are excluded",
            "Write/Edit pairs only; added or deleted test files are skipped",
        ),
    ),
    RepoSpec(
        key="caldiy",
        name="calcom/cal.diy",
        url="https://github.com/calcom/cal.diy",
        license="MIT",
        language="TypeScript",
        target_payloads=110,
        clone_args=("--filter=blob:none",),
        pathspecs=CALDIY_PATHSPECS,
        test_path=replay.is_test_path,
        test_path_source="eval/path-predicates.json",
        excludes=CALDIY_EXCLUDES,
        caveats=(
            "blobless clone; git fetches blobs on demand",
            "generated Playwright reports and packages/prisma artifacts are excluded",
            "Write/Edit pairs only; added or deleted test files are skipped",
        ),
    ),
)
REPOS_BY_KEY = {spec.key: spec for spec in REPOS}


def git(repo: Path, *args: str) -> str:
    environment = dict(os.environ)
    environment["GIT_TERMINAL_PROMPT"] = "0"
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        text=True,
        check=True,
        env=environment,
    ).stdout


def blob(repo: Path, revision: str, path: str) -> bytes | None:
    environment = dict(os.environ)
    environment["GIT_TERMINAL_PROMPT"] = "0"
    process = subprocess.run(
        ["git", "-C", str(repo), "show", f"{revision}:{path}"],
        capture_output=True,
        check=False,
        env=environment,
    )
    return process.stdout if process.returncode == 0 else None


def ensure_checkout(spec: RepoSpec, scratch: Path) -> Path:
    destination = scratch / spec.key
    if destination.is_dir():
        return destination
    scratch.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    environment["GIT_TERMINAL_PROMPT"] = "0"
    subprocess.run(
        ["git", "clone", "--quiet", *spec.clone_args, spec.url, str(destination)],
        check=True,
        env=environment,
    )
    return destination


def checkout_from_manifest(repo: dict, scratch: Path) -> Path:
    destination = scratch / repo["key"]
    if destination.is_dir():
        return destination
    scratch.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    environment["GIT_TERMINAL_PROMPT"] = "0"
    subprocess.run(
        ["git", "clone", "--quiet", *repo.get("clone_args", []), repo["url"], str(destination)],
        check=True,
        env=environment,
    )
    return destination


def newest_changed_files(repo: Path, pathspecs: tuple[str, ...]) -> list[tuple[str, str]]:
    output = git(
        repo, "log", "--format=__COMMIT__%H", "--name-only", "-n", str(LOG_LIMIT), "--", *pathspecs
    )
    entries: list[tuple[str, str]] = []
    commit = None
    for line in output.splitlines():
        if line.startswith("__COMMIT__"):
            commit = line[len("__COMMIT__") :]
        elif line.strip() and commit:
            entries.append((commit, line.strip()))
    return entries


def decode(data: bytes | None) -> str | None:
    if data is None or len(data) > MAX_BLOB_BYTES or b"\x00" in data:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def select_repo(spec: RepoSpec, repo: Path) -> dict:
    payloads: list[dict[str, str]] = []
    files: collections.Counter[str] = collections.Counter()
    skipped: collections.Counter[str] = collections.Counter()
    commits: set[str] = set()
    per_commit: collections.Counter[str] = collections.Counter()
    for commit, path in newest_changed_files(repo, spec.pathspecs):
        if len(payloads) >= spec.target_payloads:
            break
        if per_commit[commit] >= MAX_FILES_PER_COMMIT:
            continue
        if not spec.test_path(path):
            continue
        if spec.excludes is not None and spec.excludes.search(path):
            skipped["excluded_artifact"] += 1
            continue
        if path.rsplit(".", 1)[-1] not in EXTENSIONS:
            continue
        new = decode(blob(repo, commit, path))
        old = decode(blob(repo, f"{commit}^", path))
        if new is None:
            skipped["deleted_in_commit" if old is not None else "unreadable_new"] += 1
            continue
        if old is None:
            skipped["added_in_commit" if new is not None else "unreadable_parent"] += 1
            continue
        payloads.append({"kind": "write", "commit": commit, "path": path})
        payloads.append({"kind": "edit", "commit": commit, "path": path})
        commits.add(commit)
        per_commit[commit] += 1
        files[path] += 1

    counts = {
        "test_commits": len(commits),
        "payloads": len(payloads),
        "write": sum(1 for entry in payloads if entry["kind"] == "write"),
        "edit": sum(1 for entry in payloads if entry["kind"] == "edit"),
        "files": dict(files.most_common()),
        "skipped": dict(sorted(skipped.items())),
    }
    return {
        "key": spec.key,
        "name": spec.name,
        "url": spec.url,
        "license": spec.license,
        "language": spec.language,
        "head": git(repo, "rev-parse", "HEAD").strip(),
        "clone_args": list(spec.clone_args),
        "selection": {
            "target_payloads": spec.target_payloads,
            "pathspecs": list(spec.pathspecs),
            "test_path_source": spec.test_path_source,
            "excludes_regex": spec.excludes.pattern if spec.excludes else None,
            "order": "newest-first, modified files only",
            "max_files_per_commit": MAX_FILES_PER_COMMIT,
        },
        "caveats": list(spec.caveats),
        "counts": counts,
        "payloads": payloads,
    }


def entry_payload(repo: Path, entry: dict) -> dict[str, object]:
    path = entry["path"]
    commit = entry["commit"]
    new = decode(blob(repo, commit, path))
    if new is None:
        raise RuntimeError(f"{entry.get('repo', '?')}: unreadable {commit}:{path}")
    if entry["kind"] == "write":
        return {"tool_name": "Write", "tool_input": {"file_path": path, "content": new}}
    old = decode(blob(repo, f"{commit}^", path))
    if old is None:
        raise RuntimeError(f"{entry.get('repo', '?')}: unreadable parent of {commit}:{path}")
    return {
        "tool_name": "Edit",
        "tool_input": {"file_path": path, "old_string": old, "new_string": new},
    }


def adds_tests(repo: Path, entry: dict, cache: dict[tuple[str, str], bool]) -> bool:
    key = (entry["commit"], entry["path"])
    if key in cache:
        return cache[key]
    new = decode(blob(repo, entry["commit"], entry["path"])) or ""
    old = decode(blob(repo, f"{entry['commit']}^", entry["path"])) or ""
    cache[key] = len(TEST_DECL.findall(new)) > len(TEST_DECL.findall(old))
    return cache[key]


def load_manifest(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or data.get("schema") != MANIFEST_SCHEMA:
        raise SystemExit(
            f"build_secondary_corpus: {path} is not a schema {MANIFEST_SCHEMA} manifest"
        )
    return data


def write_manifest(path: Path, repos: list[dict]) -> None:
    manifest = {
        "schema": MANIFEST_SCHEMA,
        "description": (
            "Secondary eval corpus of real test-file changes from three OSS repositories. "
            "Payloads are not tracked; regenerate them with "
            "`python3 eval/build_secondary_corpus.py build`."
        ),
        "generated_with": "eval/build_secondary_corpus.py select",
        "payload_shape": "Write: committed content; Edit: parent -> commit on the same file",
        "repos": repos,
    }
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def run_select(args: argparse.Namespace) -> int:
    scratch = Path(args.scratch).expanduser()
    selected = []
    for spec in REPOS:
        if args.repo and spec.key not in args.repo:
            continue
        repo = ensure_checkout(spec, scratch)
        record = select_repo(spec, repo)
        selected.append(record)
        counts = record["counts"]
        print(
            f"{spec.key}: {counts['payloads']} payloads from {counts['test_commits']} commits "
            f"(target {spec.target_payloads}); skipped {counts['skipped'] or '{}'}"
        )
    if args.manifest:
        write_manifest(Path(args.manifest), selected)
        print(f"wrote {args.manifest}")
    return 0


def run_build(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    dump = (
        Path(args.dump).expanduser()
        if args.dump
        else Path(args.scratch).expanduser() / "secondary-corpus"
    )
    resolved = dump.resolve()
    if resolved == ROOT or ROOT in resolved.parents:
        print(
            "build_secondary_corpus: refusing to write payloads inside the checkout; "
            "pick a scratch path outside it",
            file=sys.stderr,
        )
        return 2
    dump.mkdir(parents=True, exist_ok=True)
    counts: collections.Counter[str] = collections.Counter()
    for repo_record in manifest["repos"]:
        repo = checkout_from_manifest(repo_record, Path(args.scratch).expanduser())
        path = dump / f"{repo_record['key']}.jsonl"
        with path.open("w", encoding="utf-8") as handle:
            for entry in repo_record["payloads"]:
                payload = entry_payload(repo, {**entry, "repo": repo_record["key"]})
                line = {
                    "repo": repo_record["key"],
                    "kind": entry["kind"],
                    "commit": entry["commit"],
                    "path": entry["path"],
                    "payload": payload,
                }
                handle.write(json.dumps(line) + "\n")
                counts[repo_record["key"]] += 1
        print(f"{path}: {counts[repo_record['key']]} payloads")
    print(f"payloads written under {dump} (outside the checkout)")
    return 0


def p95(samples: list[float]) -> float:
    if not samples:
        return 0.0
    ordered = sorted(samples)
    return ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]


def run_measure(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    scratch = Path(args.scratch).expanduser()
    rows = []
    for repo_record in manifest["repos"]:
        repo = checkout_from_manifest(repo_record, scratch)
        exits: collections.Counter[int] = collections.Counter()
        rules: collections.Counter[str] = collections.Counter()
        adding_rules: collections.Counter[str] = collections.Counter()
        samples: list[float] = []
        kinds: collections.Counter[str] = collections.Counter()
        kind_rules: dict[str, collections.Counter[str]] = {
            "write": collections.Counter(),
            "edit": collections.Counter(),
        }
        kind_exits: dict[str, collections.Counter[int]] = {
            "write": collections.Counter(),
            "edit": collections.Counter(),
        }
        cache: dict[tuple[str, str], bool] = {}
        adding_payloads = 0
        for entry in repo_record["payloads"]:
            entry = {**entry, "repo": repo_record["key"]}
            payload = entry_payload(repo, entry)
            adding = adds_tests(repo, entry, cache)
            if adding:
                adding_payloads += 1
            kinds[entry["kind"]] += 1
            start = time.perf_counter()
            process = subprocess.run(
                [args.binary, "--min-confidence", "low"],
                input=json.dumps(payload).encode(),
                capture_output=True,
                check=False,
            )
            samples.append((time.perf_counter() - start) * 1000.0)
            exits[process.returncode] += 1
            text = (process.stderr or process.stdout).decode()
            found = RULE.findall(text)
            rules.update(found)
            kind_rules[entry["kind"]].update(found)
            kind_exits[entry["kind"]][process.returncode] += 1
            if adding:
                adding_rules.update(found)
        rows.append(
            {
                "key": repo_record["key"],
                "name": repo_record["name"],
                "payloads": len(repo_record["payloads"]),
                "write": kinds["write"],
                "edit": kinds["edit"],
                "adding_payloads": adding_payloads,
                "exits": dict(sorted(exits.items())),
                "rules": dict(rules.most_common()),
                "adding_rules": dict(adding_rules.most_common()),
                "kinds": {
                    kind: {
                        "payloads": kinds[kind],
                        "exit2": kind_exits[kind].get(2, 0),
                        "rules": dict(kind_rules[kind].most_common()),
                    }
                    for kind in ("write", "edit")
                },
                "p95_ms": round(p95(samples), 2),
                "p50_ms": round(statistics.median(samples), 2) if samples else 0.0,
            }
        )

    print(
        "| repo | payloads | write | edit | exit 0 | exit 1 | exit 2 "
        "| exit2 write | exit2 edit | p50 ms | p95 ms |"
    )
    print("|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|")
    for row in rows:
        exits = row["exits"]
        print(
            f"| {row['key']} ({row['name']}) | {row['payloads']} | {row['write']} | {row['edit']} "
            f"| {exits.get(0, 0)} | {exits.get(1, 0)} | {exits.get(2, 0)} "
            f"| {row['kinds']['write']['exit2']} | {row['kinds']['edit']['exit2']} "
            f"| {row['p50_ms']} | {row['p95_ms']} |"
        )
    for row in rows:
        print(f"\n== {row['key']} ({row['name']}) ==")
        print(f"test-adding payloads: {row['adding_payloads']}/{row['payloads']}")
        if not row["rules"]:
            print("(no findings)")
            continue
        print("| rule | hits | write | edit | on test-adding |")
        print("|:--|--:|--:|--:|--:|")
        for rule, hits in row["rules"].items():
            print(
                f"| {rule} | {hits} | {row['kinds']['write']['rules'].get(rule, 0)} "
                f"| {row['kinds']['edit']['rules'].get(rule, 0)} "
                f"| {row['adding_rules'].get(rule, 0)} |"
            )

    if args.json:
        Path(args.json).write_text(json.dumps({"repos": rows}, indent=2) + "\n", encoding="utf-8")
        print(f"\nwrote {args.json}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    select = subparsers.add_parser(
        "select", help="pick newest qualifying commits and write the manifest"
    )
    select.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    select.add_argument("--scratch", default=str(DEFAULT_SCRATCH))
    select.add_argument("--repo", nargs="+", choices=sorted(REPOS_BY_KEY))
    select.set_defaults(func=run_select)

    build = subparsers.add_parser("build", help="regenerate payloads from the manifest")
    build.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    build.add_argument("--scratch", default=str(DEFAULT_SCRATCH))
    build.add_argument("--dump", help="output directory outside the checkout")
    build.set_defaults(func=run_build)

    measure = subparsers.add_parser("measure", help="replay the corpus through a binary")
    measure.add_argument("--binary", default=str(ROOT / "target" / "release" / "test-checker"))
    measure.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    measure.add_argument("--scratch", default=str(DEFAULT_SCRATCH))
    measure.add_argument("--json", help="write the per-repo statistics as JSON")
    measure.set_defaults(func=run_measure)

    args = parser.parse_args()
    if args.command == "measure":
        binary = Path(args.binary)
        if not binary.is_file():
            print(f"build_secondary_corpus: binary not found: {binary}", file=sys.stderr)
            return 2
        args.binary = str(binary)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
