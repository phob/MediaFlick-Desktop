#!/usr/bin/env python3
"""Fail a change that touches shipped code without a changelog.d fragment.

Usage: check_changelog_fragment.py BASE [HEAD]
Escape: MEDIAFLICK_NO_CHANGELOG=1, which CI sets only from the PR's `no-changelog` label.
"""

import os
import re
import subprocess
import sys

CODE_PREFIXES = ("src/", "ui/src/", "plugin/")
# Dependency manifests are exempt, as Cargo.toml and ui/package.json are by lying outside the prefixes.
DEPENDENCY_SUFFIXES = (".csproj", ".props", "packages.lock.json")
FRAGMENT = re.compile(r"^changelog\.d/[^/]+\.md$")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    base, head = argv[1], argv[2] if len(argv) == 3 else "HEAD"
    span = f"{git('merge-base', base, head).strip()}..{head}"

    changed = [line.split("\t") for line in git("diff", "--name-status", "--no-renames", span).splitlines()]
    code = [path for _, path in changed if path.startswith(CODE_PREFIXES) and not path.endswith(DEPENDENCY_SUFFIXES)]
    fragments = [path for status, path in changed if status in ("A", "M") and FRAGMENT.match(path)]
    if not code or fragments:
        return 0
    if os.environ.get("MEDIAFLICK_NO_CHANGELOG") == "1":
        print(f"changelog: {len(code)} code files changed, fragment waived by the no-changelog label")
        return 0

    print(f"changelog: {span} changes {len(code)} files under src/, ui/src/ or plugin/ but adds no changelog.d/*.md")
    for path in code[:5]:
        print(f"  {path}")
    print(
        "Fix: add changelog.d/<topic>-<6 hex>.md describing what users see (AGENTS.md, 'Changelog and releases').\n"
        "No user-visible change? Label the PR `no-changelog`."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
