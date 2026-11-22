#!/usr/bin/env python3
import re
import sys
from dataclasses import dataclass
from pathlib import Path

SKILL = Path(__file__).resolve().parents[2] / ".claude" / "skills" / "verify-mediaflick"
CLAUSE = re.compile(r"(?<!\bis )(?<!\bare )proven by((?:`[^`\n]*`|\.(?!\s|$)|[^`*;.\n])*)", re.IGNORECASE | re.MULTILINE)
NAME = re.compile(r"`([^`\n]*)`")
DRIVE_LIKE = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*")
EVIDENCE = re.compile(r"\d\d[a-z]?-[\w-]+(?:\.png|\.ax\.txt)?|[\w-]+(?:\.ax)?\.[a-z][a-z0-9]{1,4}")
SCREENSHOT = re.compile(r"\.(?:png|ax\.txt)$")
HARNESS_FILES = frozenset({
    "steps.log", "app.log", "cef.log", "doctor.json", "doctor-relaunch.json", "result.json", "guard.json",
    "pids.json", "account.json", "failure.png", "failure.ax.txt",
})


@dataclass(frozen=True)
class Problem:
    feature: str
    line: int
    message: str


def problems(features: dict[str, str], drives: dict[str, str]) -> list[Problem]:
    found = []
    for feature, text in sorted(features.items()):
        for clause in CLAUSE.finditer(text):
            line = text.count("\n", 0, clause.start()) + 1
            names = [(name.split()[0], len(name.split()) > 1) for name in NAME.findall(clause.group(1)) if name.strip()]
            named = [name for name, _ in names if name in drives]
            if not named:
                found.append(Problem(feature, line, f'"{clause.group(0).strip()}" names no drive in drives/.'))
                continue
            for name, command in names:
                if name in drives or name in HARNESS_FILES:
                    continue
                if not command and EVIDENCE.fullmatch(name):
                    spellings = {name, SCREENSHOT.sub("", name)}
                    written = re.compile(rf"(?<![\w.-])(?:{'|'.join(map(re.escape, spellings))})(?![\w.-])")
                    if not any(written.search(drives[drive]) for drive in named):
                        found.append(Problem(feature, line, f"no drive among {', '.join(named)} writes `{name}`."))
                elif DRIVE_LIKE.fullmatch(name):
                    found.append(Problem(feature, line, f"`{name}` is neither a drive in drives/ nor an evidence file."))
    return found


def main() -> int:
    features = {path.stem: path.read_text(encoding="utf-8") for path in (SKILL / "features").glob("*.md")}
    drives = {path.stem: path.read_text(encoding="utf-8") for path in (SKILL / "drives").glob("*.mjs")}
    found = problems(features, drives)
    for problem in found:
        print(f"{(SKILL / 'features' / problem.feature).relative_to(SKILL.parents[2])}.md:{problem.line}: {problem.message}")
    if found:
        print(
            "Fix: name the drive that proves the claim and the evidence file it writes (a screenshot, snapshot\n"
            "or `ctx.writeEvidence` name from its source), not a run id. A renamed or removed drive takes its claims along."
        )
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
