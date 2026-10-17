#!/usr/bin/env python3
"""Ratchet source file length: a change may not grow a file past, or further past, the limit.

Usage: check_file_size.py BASE [HEAD]
Both counts come from git, so there is no baseline file to edit. Files already
over the limit at BASE may shrink or stay put; they may not grow.
"""

import subprocess
import sys

LIMIT = 1_000  # guard-ratchet
SUFFIXES = (".rs", ".ts", ".tsx", ".cs", ".js", ".mjs", ".py")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def line_count(rev: str, path: str) -> int:
    result = subprocess.run(["git", "show", f"{rev}:{path}"], capture_output=True)
    return result.stdout.count(b"\n") if result.returncode == 0 else 0


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    head = argv[2] if len(argv) == 3 else "HEAD"
    base = git("merge-base", argv[1], head).strip()

    failures = []
    for line in git("diff", "--name-status", "-M", f"{base}..{head}").splitlines():
        status, *paths = line.split("\t")
        new_path = paths[-1]
        if status.startswith("D") or not new_path.endswith(SUFFIXES):
            continue
        after = line_count(head, new_path)
        before = line_count(base, paths[0])
        if after > LIMIT and after > before:
            failures.append((new_path, before, after))

    for path, before, after in failures:
        print(f"{path}: {before} -> {after} lines (limit {LIMIT}).")
    if failures:
        print(
            "Fix: move the new code into its owning module, or split the file along a seam\n"
            "(types, tests in a sibling tests.rs, one submodule per concern) until it no longer grows past the limit."
        )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
