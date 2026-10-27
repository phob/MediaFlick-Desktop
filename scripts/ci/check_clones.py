#!/usr/bin/env python3
"""Ratchet code copied between files: a change may not add a cross-file clone.

Usage: check_clones.py
Runs jscpd, pinned in ui/package.json and installed by `pnpm --dir ui install`, over the
production sources. A clone inside one file is left to review; one spanning two files is
a copied helper or block that wants one owner. The thresholds skip short look-alikes
(field lists, two-line wrappers) and keep copied functions and blocks.
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

MAX_CLONES = 10  # guard-ratchet
MIN_TOKENS = 60
MIN_LINES = 6
ROOT = Path(__file__).resolve().parents[2]
JSCPD = ROOT / "ui" / "node_modules" / ".bin" / "jscpd"
PATHS = ("ui/src", "src", "plugin")
IGNORE = (
    "**/node_modules/**",
    "**/bin/**",
    "**/obj/**",
    "**/ui/src/lib/api/wire.gen.ts",
    "**/ui/src/lib/api/wire/**",
    "**/tests/**",
    "**/tests.rs",
    "**/test_support.rs",
    "**/*.test.*",
    "**/test_*.py",
)


def main() -> int:
    if not JSCPD.exists():
        print(f"{JSCPD.relative_to(ROOT)} is missing. Fix: run `pnpm --dir ui install`.")
        return 2
    with tempfile.TemporaryDirectory() as output:
        subprocess.run(
            [str(JSCPD), *PATHS, "--absolute", "--silent", "--min-tokens", str(MIN_TOKENS), "--min-lines", str(MIN_LINES),
             "--ignore", ",".join(IGNORE), "--reporters", "json", "--output", output],
            cwd=ROOT, check=True, stdout=subprocess.DEVNULL,
        )
        report = json.loads((Path(output) / "jscpd-report.json").read_text())

    def where(fragment: dict) -> str:
        return f"{Path(fragment['name']).relative_to(ROOT)}:{fragment['start']}-{fragment['end']}"

    clones = sorted(
        f"{where(clone['firstFile'])} <> {where(clone['secondFile'])} ({clone['tokens']} tokens)"
        for clone in report["duplicates"]
        if clone["firstFile"]["name"] != clone["secondFile"]["name"]
    )
    if len(clones) <= MAX_CLONES:
        if len(clones) < MAX_CLONES:
            print(f"clones: {len(clones)} cross-file clones; lower MAX_CLONES in {Path(__file__).name} to {len(clones)}.")
        return 0
    print("\n".join(clones))
    print(
        f"clones: {len(clones)} cross-file clones, more than the {MAX_CLONES} already known (listed above).\n"
        "Fix: give the copied code one owner in its domain module and call it from both places;\n"
        "do not raise MAX_CLONES."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
