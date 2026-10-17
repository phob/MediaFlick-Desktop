#!/usr/bin/env python3
"""Fail a change that deletes or relaxes a guard, so a relaxation is never silent.

Usage: check_guards.py BASE [HEAD]
Escape: MEDIAFLICK_GUARDS_CHANGED=1, which CI sets only from the PR's `guards-changed` label.

Lint configuration is compared by meaning: a new lint at full strength, a raised level
or a lowered threshold passes. Guard code (CI scripts, gating workflows, check recipes,
TypeScript and compiler-warning settings) has no such reading, so any change to it
counts, except an action version bump. CI runs the base branch's copy of this script
from a pull_request_target workflow, so a PR cannot edit the check that judges it.
"""

import fnmatch
import json
import os
import re
import subprocess
import sys
import tomllib
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass

Check = Callable[[str, str], list[str]]

RUST_LEVELS = {"allow": 0, "warn": 1, "deny": 2, "forbid": 3}
OXLINT_LEVELS = {"off": 0, "allow": 0, 0: 0, "warn": 1, 1: 1, "error": 2, "deny": 2, 2: 2}
UI_GUARD_SCRIPTS = ("build", "lint", "test", "test:unit", "test:ui")
GUARD_RECIPES = ("check-pr", "clippy", "clippy-target", "fmt-check", "rust-quality", "test", "ui")
MSBUILD_WARNINGS = re.compile(r"TreatWarningsAsErrors|WarningsAsErrors|WarningsNotAsErrors|NoWarn|WarningLevel|AnalysisLevel|AnalysisMode|EnforceCodeStyleInBuild|Nullable")
RATCHET = re.compile(r"guard-ratchet")
NUMBER = re.compile(r"\d[\d_]*")
RUST_SUPPRESSION = re.compile(r"(?<![\w.:])(allow|expect) ?\( ?(\w[\w:]*(?: ?, ?\w[\w:]*)*)")
RUST_STRING = re.compile(r'"(?:\\[\s\S]|[^"\\])*"')
RUST_COMMENT = re.compile(r"//[^\n]*")
RUST_ITEM = re.compile(r"\b(fn|impl|mod|struct|enum|trait|const|static|type|use|let|match|macro_rules)\b")
VERSION_PIN = re.compile(r"((?:uses:\s*[^@\s]+@)|(?:[\w-]*version[\w-]*:\s*))\S+", re.IGNORECASE)
SCRIPT_SUPPRESSION = re.compile(r"(?:oxlint|eslint)-disable[^\n*]*|@ts-(?:ignore|expect-error|nocheck)")
CSHARP_SUPPRESSION = re.compile(r"#pragma\s+warning\s+disable[^\n]*|SuppressMessage\s*\([^)]*\)")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def show(rev: str, path: str) -> str:
    result = subprocess.run(["git", "show", f"{rev}:{path}"], capture_output=True)
    return result.stdout.decode("utf-8", errors="replace") if result.returncode == 0 else ""


def guard_lines(text: str) -> Counter[str]:
    lines = (line.strip() for line in text.splitlines())
    return Counter(line for line in lines if line and not line.startswith(("#", "//")))


def changed_lines(before: str, after: str) -> list[str]:
    """Any guard line added or removed; moving a line within the file is fine."""
    old, new = guard_lines(before), guard_lines(after)
    return [f"removed `{line}`" for line in sorted(old - new)] + [f"added `{line}`" for line in sorted(new - old)]


def workflow_lines(before: str, after: str) -> list[str]:
    """Like changed_lines, but an action or tool version bump is not a relaxation."""
    return changed_lines(VERSION_PIN.sub(r"\1V", before), VERSION_PIN.sub(r"\1V", after))


def matching_lines(pattern: re.Pattern[str]) -> Check:
    def check(before: str, after: str) -> list[str]:
        def keep(text: str) -> str:
            return "\n".join(line for line in text.splitlines() if pattern.search(line))

        return changed_lines(keep(before), keep(after))

    return check


def recipe_lines(before: str, after: str) -> list[str]:
    def recipes(text: str) -> str:
        blocks, current = [], None
        for line in text.splitlines():
            if line[:1].isalpha() and ":=" not in line:
                name = re.match(r"[\w-]+", line)
                current = name.group() if name and name.group() in GUARD_RECIPES else None
            elif not line[:1].isspace():
                current = None
            if current:
                blocks.append(line)
        return "\n".join(blocks)

    return changed_lines(recipes(before), recipes(after))


def toml(text: str) -> dict:
    return tomllib.loads(text) if text else {}


def cargo_lints(before: str, after: str) -> list[str]:
    def levels(text: str) -> dict[str, object]:
        lints = toml(text).get("lints", {})
        return {f"{tool}::{name}": value for tool, table in lints.items() if isinstance(table, dict) for name, value in table.items()}

    def rank(value: object) -> int:
        level = value.get("level") if isinstance(value, dict) else value
        return RUST_LEVELS.get(str(level), -1)

    old, new = levels(before), levels(after)
    findings = [f"added `{lint}` below deny" for lint, value in new.items() if lint not in old and rank(value) < RUST_LEVELS["deny"]]
    for lint, value in old.items():
        if lint not in new:
            findings.append(f"removed lint `{lint}`")
        elif rank(new[lint]) < rank(value):
            findings.append(f"lowered `{lint}` from {value!r} to {new[lint]!r}")
        elif rank(new[lint]) == rank(value) and new[lint] != value:
            findings.append(f"changed `{lint}` from {value!r} to {new[lint]!r}")
    return findings


def clippy_config(before: str, after: str) -> list[str]:
    old, new = toml(before), toml(after)
    findings = [f"removed `{key}`" for key in old if key not in new]
    for key, value in new.items():
        previous = old.get(key)
        loosens = key.startswith("allow-") and value is True
        if key not in old:
            if loosens or isinstance(value, (int, float)) and not isinstance(value, bool):
                findings.append(f"added `{key} = {json.dumps(value)}`")
        elif value == previous:
            continue
        elif isinstance(value, list) and isinstance(previous, list):
            kept = {json.dumps(item, sort_keys=True) for item in value}
            findings += [f"dropped {item} from `{key}`" for item in previous if json.dumps(item, sort_keys=True) not in kept]
        elif isinstance(value, bool) and key.startswith("allow-"):
            if loosens:
                findings.append(f"turned on `{key}`")
        elif isinstance(value, (int, float)) and isinstance(previous, (int, float)) and not isinstance(value, bool):
            if value > previous:
                findings.append(f"raised `{key}` from {previous} to {value}")
        else:
            findings.append(f"changed `{key}` from {previous!r} to {value!r}")
    return findings


def oxlint_config(before: str, after: str) -> list[str]:
    old, new = (json.loads(text) if text else {} for text in (before, after))

    def level(setting: object) -> int:
        head = setting[0] if isinstance(setting, list) else setting
        return OXLINT_LEVELS.get(head, -1)

    findings = []
    for table in ("rules", "categories"):
        old_rules, new_rules = old.get(table, {}), new.get(table, {})
        findings += [f"added {table} `{rule}` below error" for rule, setting in new_rules.items() if rule not in old_rules and level(setting) < 2]
        for rule, setting in old_rules.items():
            if rule not in new_rules:
                findings.append(f"removed {table} `{rule}`")
            elif level(new_rules[rule]) < level(setting):
                findings.append(f"lowered {table} `{rule}` from {setting!r} to {new_rules[rule]!r}")
            elif level(new_rules[rule]) == level(setting) and new_rules[rule] != setting:
                findings.append(f"changed options of {table} `{rule}`")
    for key in ("plugins", "jsPlugins"):
        findings += [f"dropped `{item}` from `{key}`" for item in old.get(key, []) if item not in new.get(key, [])]
    findings += [f"ignores `{item}`" for item in new.get("ignorePatterns", []) if item not in old.get("ignorePatterns", [])]
    other = (old.keys() | new.keys()) - {"$schema", "rules", "categories", "plugins", "jsPlugins", "ignorePatterns"}
    findings += [f"changed `{key}`" for key in sorted(other) if old.get(key) != new.get(key)]
    return findings


def ui_scripts(before: str, after: str) -> list[str]:
    old, new = (json.loads(text).get("scripts", {}) if text else {} for text in (before, after))
    return [f"changed script `{name}` from {old[name]!r} to {new.get(name)!r}" for name in UI_GUARD_SCRIPTS if name in old and old[name] != new.get(name)]


@dataclass(frozen=True)
class Surface:
    name: str
    pattern: str
    check: Check


SURFACES = (
    Surface("Rust lint levels", "Cargo.toml", cargo_lints),
    Surface("Clippy configuration", "clippy.toml", clippy_config),
    Surface("Oxlint configuration", "ui/.oxlintrc.json", oxlint_config),
    Surface("UI check scripts", "ui/package.json", ui_scripts),
    Surface("TypeScript configuration", "ui/tsconfig*.json", changed_lines),
    Surface("Companion warning settings", "plugin/*.props", matching_lines(MSBUILD_WARNINGS)),
    Surface("Companion warning settings", "plugin/*.csproj", matching_lines(MSBUILD_WARNINGS)),
    Surface("Analyzer severities", "*.editorconfig", matching_lines(re.compile(r"severity"))),
    Surface("CI scripts", "scripts/ci/*", changed_lines),
    Surface("CI workflow", ".github/workflows/ci.yml", workflow_lines),
    Surface("PR checks workflow", ".github/workflows/pr-checks.yml", workflow_lines),
    Surface("Guard workflow", ".github/workflows/guard-integrity.yml", workflow_lines),
    Surface("Check recipes", "justfile", recipe_lines),
)


def ratchets(text: str) -> dict[str, tuple[int, str]]:
    """Lines tagged `guard-ratchet`, keyed by their text with the first number blanked."""
    found = {}
    for line in text.splitlines():
        number = NUMBER.search(line)
        if RATCHET.search(line) and number:
            found[NUMBER.sub("#", line.strip(), count=1)] = (int(number.group().replace("_", "")), line.strip())
    return found


def raised_ratchets(before: str, after: str) -> list[str]:
    old, new = ratchets(before), ratchets(after)
    findings = []
    for key, (count, line) in old.items():
        if key not in new:
            findings.append(f"removed ratchet `{line}`")
        elif new[key][0] > count:
            findings.append(f"raised ratchet `{line}` to {new[key][0]}")
    return findings


def suppressions(path: str, text: str) -> Counter[str]:
    """Lint suppressions in a file, keyed by what they silence so a moved one still matches."""
    suffix = os.path.splitext(path)[1]
    if suffix == ".rs":
        return Counter(rust_suppression(code, match) for code in [re.sub(r"\s+", " ", RUST_COMMENT.sub("", RUST_STRING.sub('""', text)))] for match in RUST_SUPPRESSION.finditer(code))
    if suffix in (".ts", ".tsx", ".js", ".mjs"):
        return Counter(match.strip() for match in SCRIPT_SUPPRESSION.findall(text))
    if suffix == ".cs":
        return Counter(match.strip() for match in CSHARP_SUPPRESSION.findall(text))
    return Counter()


def rust_suppression(code: str, match: re.Match[str]) -> str:
    """`expect(clippy::panic) on fn`: what is silenced and how far, so widening the scope counts as new."""
    lints = match.group(2).replace(" ", "").removesuffix(",reason")
    if code.rfind("#![", 0, match.start()) > code.rfind("#[", 0, match.start()):
        scope = "module"
    else:
        item = RUST_ITEM.search(code, match.end())
        scope = item.group() if item else "item"
    return f"{match.group(1)}({lints}) on {scope}"


def added_suppressions(contents: dict[str, tuple[str, str]]) -> list[tuple[str, str]]:
    before, after = Counter(), Counter()
    for path, (old, new) in contents.items():
        before += suppressions(path, old)
        after += suppressions(path, new)
    findings = []
    for key in sorted(after - before):
        paths = [path for path, (old, new) in contents.items() if suppressions(path, new)[key] > suppressions(path, old)[key]]
        findings.append((f"Lint suppression ({', '.join(paths)})", f"added `{key}`"))
    return findings


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    head = argv[2] if len(argv) == 3 else "HEAD"
    base = git("merge-base", argv[1], head).strip()
    span = f"{base}..{head}"

    paths = set()
    for line in git("diff", "--name-status", "-M", span).splitlines():
        paths.update(line.split("\t")[1:])

    contents = {path: (show(base, path), show(head, path)) for path in sorted(paths)}
    findings: list[tuple[str, str]] = []
    for path, (before, after) in contents.items():
        for surface in SURFACES:
            if fnmatch.fnmatchcase(path, surface.pattern):
                findings += [(f"{surface.name} ({path})", finding) for finding in surface.check(before, after)]
        findings += [(f"Ratchet ({path})", finding) for finding in raised_ratchets(before, after)]
    findings += added_suppressions(contents)

    if not findings:
        return 0
    for where, finding in findings:
        print(f"{where}: {finding}")
    if os.environ.get("MEDIAFLICK_GUARDS_CHANGED") == "1":
        print(f"guards: {len(findings)} findings above, accepted by the guards-changed label")
        return 0
    print(
        f"guards: {span} relaxes a guard or changes guard code ({len(findings)} findings).\n"
        "Fix: keep the guard and change the code instead. If the relaxation is intended,\n"
        "say why in the PR body and label the PR `guards-changed`; that label marks it for the operator's review."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
