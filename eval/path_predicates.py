"""Path predicates shared by the eval and cleanup tooling.

Reads the committed ``eval/path-predicates.json``, which is generated from the predicate tables in
``src/language.rs``. Regenerate it with ``scripts/generate-path-predicates``; do not hand-edit it or
re-add a private regex copy here.
"""

from __future__ import annotations

import json
from pathlib import Path

TABLE = json.loads(
    (Path(__file__).resolve().parent / "path-predicates.json").read_text(encoding="utf-8")
)


def _matches_extension_rule(name: str) -> bool:
    stem, _, extension = name.rpartition(".")
    if not stem:
        stem, extension = name, ""
    for rule in TABLE["extension_test_name_rules"]:
        if extension not in rule["extensions"]:
            continue
        if any(name.startswith(prefix) for prefix in rule["filename_prefixes"]):
            return True
        if any(stem.endswith(suffix) for suffix in rule["filename_suffixes"]):
            return True
    return False


def is_test_path(file_path: str | Path) -> bool:
    normalized = str(file_path).replace("\\", "/").lower()
    name = normalized.rsplit("/", 1)[-1]
    return (
        any(segment in TABLE["test_path_segments"] for segment in normalized.split("/"))
        or any(name.startswith(prefix) for prefix in TABLE["test_filename_prefixes"])
        or any(infix in name for infix in TABLE["test_filename_infixes"])
        or _matches_extension_rule(name)
    )
