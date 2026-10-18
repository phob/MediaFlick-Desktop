#!/usr/bin/env python3
"""Fail a change that deletes or relaxes a guard, so a relaxation is never silent.

Usage: check_guards.py BASE [HEAD]
Escape: MEDIAFLICK_GUARDS_CHANGED=1, which CI sets only from the PR's `guards-changed` label.

Lint configuration is compared by meaning: a new lint at full strength, a raised level
or a lowered threshold passes. Guard code (CI scripts, workflows, check recipes, Cargo configuration,
TypeScript and compiler-warning settings, the count behind the wire ratchets) has no such
reading, so any change to it counts, except a version bump. CI runs the default branch's copy of this script
from a pull_request_target workflow, so a PR cannot edit the check that judges it.
"""

import difflib
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
MSBUILD_WARNINGS = re.compile(
    r"TreatWarningsAsErrors|WarningsAsErrors|WarningsNotAsErrors|NoWarn|WarningLevel|AnalysisLevel|AnalysisMode"
    r"|EnforceCodeStyleInBuild|Nullable|Condition|<PropertyGroup|</PropertyGroup|<Import|<Project"
)
RATCHET = re.compile(r"guard-ratchet")
NUMBER = re.compile(r"\d[\d_]*")
RUST_ATTRIBUTE = re.compile(r"#!?\[")
RUST_LINT_LEVEL = re.compile(r"\b(allow|expect)\s*\(\s*(\w[\w:]*(?:\s*,\s*\w[\w:]*)*)")
RUST_STRING = re.compile(r'"(?:\\[\s\S]|[^"\\])*"')
RUST_COMMENT = re.compile(r"//[^\n]*|/\*[\s\S]*?\*/")
RUST_GAP = re.compile(r"(?:\s+|//[^\n]*|/\*[\s\S]*?\*/|\bpub\s*\([^)]*\))*")
RUST_WORD = re.compile(r"[A-Za-z_]\w*")
RUST_ITEMS = frozenset({"fn", "impl", "mod", "struct", "enum", "union", "trait", "const", "static", "type", "use", "macro_rules"})
RUST_QUALIFIERS = frozenset({"pub", "crate", "super", "self", "in", "async", "unsafe", "default", "extern"})
VERSION_PIN = re.compile(r"((?:uses:\s*[^@\s]+@)|(?:[\w-]*version[\w-]*:\s*))\S+", re.IGNORECASE)
UNTYPED_COUNT_ITEMS = re.compile(r"\bmod\s+untyped_count\b|\bfn\s+untyped\s*\(")
SCRIPT_SUPPRESSION = re.compile(r"(?:oxlint|eslint)-disable[^\n*]*|@ts-(?:ignore|expect-error|nocheck)")
CSHARP_SUPPRESSION = re.compile(r"#pragma\s+warning\s+disable[^\n]*|SuppressMessage\s*\([^)]*\)")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def show(rev: str, path: str) -> str:
    result = subprocess.run(["git", "show", f"{rev}:{path}"], capture_output=True)
    return result.stdout.decode("utf-8", errors="replace") if result.returncode == 0 else ""


def guard_lines(text: str, comments: tuple[str, ...]) -> Counter[str]:
    lines = (line.strip() for line in text.splitlines())
    return Counter(line for line in lines if line and not line.startswith(comments))


def changed_lines(before: str, after: str, comments: tuple[str, ...] = ("#", "//")) -> list[str]:
    """Any guard line added or removed; moving a line within the file is fine."""
    old, new = guard_lines(before, comments), guard_lines(after, comments)
    return [f"removed `{line}`" for line in sorted(old - new)] + [f"added `{line}`" for line in sorted(new - old)]


def rust_lines(before: str, after: str) -> list[str]:
    """Code lines of Rust in order, `#[...]` included, so moving a line counts too.

    A ratchet's number is left to raised_ratchets, so lowering one passes.
    """

    def code(text: str) -> list[str]:
        lines = (line.strip() for line in text.splitlines())
        return [NUMBER.sub("#", line, count=1) if RATCHET.search(line) else line for line in lines if line and not line.startswith("//")]

    old, new = code(before), code(after)
    findings = []
    for tag, old_start, old_end, new_start, new_end in difflib.SequenceMatcher(a=old, b=new, autojunk=False).get_opcodes():
        if tag != "equal":
            findings += [f"removed `{line}`" for line in old[old_start:old_end]] + [f"added `{line}`" for line in new[new_start:new_end]]
    return findings


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


def rust_declarations(pattern: re.Pattern[str]) -> Check:
    """Lines matching `pattern` and the outer attributes on the item each starts, so a `#[cfg]` that turns one off counts."""

    def keep(text: str) -> str:
        kept = [line for line in text.splitlines() if pattern.search(line)]
        for match in RUST_ATTRIBUTE.finditer(text):
            if match.group() == "#![" or "//" in text[text.rfind("\n", 0, match.start()) + 1 : match.start()]:
                continue
            end = attribute_end(text, match.end() - 1)
            item = RUST_GAP.match(text, end).end()
            while text.startswith("#[", item):
                item = RUST_GAP.match(text, attribute_end(text, item + 1)).end()
            line_end = text.find("\n", item)
            if pattern.search(text[item : line_end if line_end >= 0 else len(text)]):
                kept.append(" ".join(text[match.start() : end].split()))
        return "\n".join(kept)

    def check(before: str, after: str) -> list[str]:
        return rust_lines(keep(before), keep(after))

    return check


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
    Surface("MSBuild warning settings", "*Directory.Build.*", matching_lines(MSBUILD_WARNINGS)),
    Surface("Companion warning settings", "plugin/*.props", matching_lines(MSBUILD_WARNINGS)),
    Surface("Companion warning settings", "plugin/*.targets", matching_lines(MSBUILD_WARNINGS)),
    Surface("Companion warning settings", "plugin/*.csproj", matching_lines(MSBUILD_WARNINGS)),
    Surface("Analyzer severities", "*.editorconfig", matching_lines(re.compile(r"severity"))),
    Surface("Analyzer severities", "*.globalconfig", matching_lines(re.compile(r"severity"))),
    Surface("CI scripts", "scripts/ci/*", changed_lines),
    Surface("Cargo configuration", ".cargo/config*", changed_lines),
    Surface("Workflow", ".github/workflows/*", workflow_lines),
    Surface("Check recipes", "justfile", recipe_lines),
    Surface("Wire ratchet count", "src/shell/cef/api/wire/untyped_count.rs", rust_lines),
    Surface("Wire ratchet count", "src/shell/cef/api/wire.rs", rust_declarations(UNTYPED_COUNT_ITEMS)),
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
        return rust_suppressions(text)
    if suffix in (".ts", ".tsx", ".js", ".mjs"):
        return Counter(match.strip() for match in SCRIPT_SUPPRESSION.findall(text))
    if suffix == ".cs":
        return Counter(match.strip() for match in CSHARP_SUPPRESSION.findall(text))
    return Counter()


def attribute_end(text: str, start: int) -> int:
    """Index just past the bracket that closes the one at `start`, skipping string literals."""
    depth, index = 0, start
    while index < len(text):
        char = text[index]
        if char == '"':
            index += 1
            while index < len(text) and text[index] != '"':
                index += 2 if text[index] == "\\" else 1
        elif text.startswith(("//", "/*"), index):
            comment = RUST_COMMENT.match(text, index)
            index = comment.end() - 1 if comment else len(text)
        elif char in "[(":
            depth += 1
        elif char in "])":
            depth -= 1
            if depth == 0:
                return index + 1
        index += 1
    return len(text)


def attribute_target(text: str, index: int) -> str:
    """The kind of item an outer attribute ending at `index` applies to; fields and variants are `member`."""
    while True:
        index = RUST_GAP.match(text, index).end()
        if not text.startswith("#[", index):
            break
        index = attribute_end(text, index + 1)
    words = RUST_WORD.findall(RUST_STRING.sub('""', text[index : index + 200]))
    for position, word in enumerate(words):
        following = words[position + 1] if position + 1 < len(words) else ""
        if word in RUST_QUALIFIERS or word == "const" and following in ("fn", "unsafe", "async", "extern"):
            continue
        return word if word in RUST_ITEMS else "member"
    return "member"


def rust_suppressions(text: str) -> Counter[str]:
    """`expect(clippy::panic) on fn`: what is silenced and how far, so widening the scope counts as new."""
    found: Counter[str] = Counter()
    for match in RUST_ATTRIBUTE.finditer(text):
        if "//" in text[text.rfind("\n", 0, match.start()) + 1 : match.start()]:
            continue
        end = attribute_end(text, match.end() - 1)
        body = RUST_COMMENT.sub(" ", RUST_STRING.sub('""', text[match.end() - 1 : end]))
        for kind, names in RUST_LINT_LEVEL.findall(body):
            lints = ",".join(name for name in re.split(r"\s*,\s*", names) if name != "reason")
            scope = "module" if match.group() == "#![" else attribute_target(text, end)
            found[f"{kind}({lints}) on {scope}"] += 1
    return found


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

    findings = list(dict.fromkeys(findings))
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
