#!/usr/bin/env python3
"""Ratchet the UI rules the code does not meet yet: a change may not add a violation.

Usage: check_ui_ratchet.py BASE [HEAD]
Runs the rules in ui/lint/ratchet.json over ui/src as committed at the merge base
and at HEAD, both with this checkout's rules, so there is no baseline file to edit.
A rule's count may fall or stay put; it may not rise. Needs `pnpm --dir ui install`.
"""

import io
import json
import subprocess
import sys
import tarfile
import tempfile
from collections import Counter
from pathlib import Path

UI = Path(__file__).resolve().parents[2] / "ui"
CONFIG = UI / "lint" / "ratchet.json"
OXLINT = UI / "node_modules" / ".bin" / "oxlint"
FIXES = {
    "typescript(no-non-null-assertion)": "narrow the value instead; a query on an id that may be missing uses"
    " `optionalIdQuery` in ui/src/lib/query-client.ts (never `skipToken` on a shared key)",
    "ui(no-query-data-empty-fallback)": "render the query's pending and error states instead of an empty value",
    "ui(no-palette-color)": "use a theme color from ui/src/app.css, such as `scrim` for a dark overlay",
}


def git(*args: str) -> bytes:
    return subprocess.run(["git", *args], check=True, capture_output=True).stdout


def violations(rev: str) -> Counter[str]:
    """Each ratcheted rule's violation count in ui/src at `rev`."""
    with tempfile.TemporaryDirectory() as root:
        with tarfile.open(fileobj=io.BytesIO(git("archive", "--format=tar", rev, "ui/src"))) as archive:
            archive.extractall(root, filter="data")
        result = subprocess.run(
            [OXLINT, "--config", CONFIG, "--format", "json", "ui/src"], cwd=root, capture_output=True, text=True
        )
    if result.returncode not in (0, 1):
        sys.exit(f"oxlint failed at {rev}:\n{result.stderr}")
    return Counter(diagnostic["code"] for diagnostic in json.loads(result.stdout)["diagnostics"])


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    head = argv[2] if len(argv) == 3 else "HEAD"
    base = git("merge-base", argv[1], head).decode().strip()

    before, after = violations(base), violations(head)
    raised = []
    for rule in sorted(before.keys() | after.keys()):
        print(f"{rule}: {before[rule]} -> {after[rule]}")
        if after[rule] > before[rule]:
            raised.append(rule)
    for rule in raised:
        print(f"ui ratchet: {rule} rose since {base[:12]}. Fix: {FIXES.get(rule, 'remove the new violations')}.")
    if raised:
        print("List them with `pnpm --dir ui exec oxlint -c lint/ratchet.json src`.")
    return 1 if raised else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
