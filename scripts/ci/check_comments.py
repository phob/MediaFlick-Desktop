#!/usr/bin/env python3
"""Ban non-exempt code comments with a per-file and repo-wide git ratchet.

Usage:
  check_comments.py BASE [HEAD]                Fail if any file or the repo adds comments since merge-base(BASE, HEAD).
  check_comments.py --list [--json] [PATH...]  List remaining non-exempt comments in the working tree.
"""

from dataclasses import asdict, dataclass
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tokenize

ROOT = Path(__file__).resolve().parents[2]
C_FAMILY = {".rs", ".ts", ".tsx", ".js", ".mjs", ".cs", ".css"}
JS_EXTS = {".ts", ".tsx", ".js", ".mjs"}
HASH_EXTS = {".sh", ".ps1", ".py"}
SCANNED_EXTS = C_FAMILY | HASH_EXTS
EXEMPT_PATHS = {
    "ui/src/lib/api/wire.gen.ts",
    "distribution/libarchive-wasm/bundle/worker-bundle.js",
}
EXEMPT_PREFIXES = ("ui/src/lib/api/wire/",)
REGEX_PREFIX = set("(,=:[!&|?{};+-*%~^")
REGEX_KEYWORD = re.compile(r"\b(return|typeof|case|in|of|delete|void|throw|new|yield|await)$")
RUST_CHAR = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'")
RUST_RAW_PREFIX = re.compile(r"r(#*)$")
CS_VERBATIM_PREFIX = re.compile(r"[@$]{1,2}$")
SH_HEREDOC = re.compile(r"<<(-?)\s*(['\"]?)(\w+)\2")
PS_HERE_START = re.compile(r"@[\"']\s*$")
LICENSE_PATTERN = re.compile(r"SPDX-License-Identifier|Copyright\b|CC0-1\.0", re.IGNORECASE)
SHELLCHECK_DIRECTIVE = re.compile(r"^#\s*shellcheck\s+disable=")
VITE_IGNORE = re.compile(r"^/\*\s*@vite-ignore\s*\*/$")
RUST_SAFETY = re.compile(r"^//\s*SAFETY:")


@dataclass(frozen=True)
class CommentBlock:
    path: str
    start: int
    end: int
    kind: str
    text: str

    @property
    def lines(self) -> int:
        return self.end - self.start + 1


def is_exempt_path(path: str) -> bool:
    return path in EXEMPT_PATHS or path.startswith(EXEMPT_PREFIXES)


def is_exempt_block(ext: str, text: str) -> bool:
    stripped = text.strip()
    if ext == ".rs" and RUST_SAFETY.match(stripped):
        return True
    if ext in JS_EXTS and VITE_IGNORE.match(stripped):
        return True
    if ext == ".sh" and SHELLCHECK_DIRECTIVE.match(stripped):
        return True
    if LICENSE_PATTERN.search(stripped):
        return True
    return False


def line_number(src: str, index: int) -> int:
    return src.count("\n", 0, index) + 1


def is_own_line(src: str, index: int) -> bool:
    line_start = src.rfind("\n", 0, index) + 1
    return src[line_start:index].strip() == ""


def lex_c_family(src: str, ext: str) -> list[tuple[int, int, bool, str, str]]:
    comments: list[tuple[int, int, bool, str, str]] = []
    js = ext in JS_EXTS
    templates: list[int] = []
    brace_depth = 0
    last = ""
    i = 0
    n = len(src)

    def skip_quoted(pos: int, quote: str, multiline: bool) -> int:
        j = pos + 1
        while j < n and src[j] != quote:
            if src[j] == "\n" and not multiline:
                return j
            j += 2 if src[j] == "\\" else 1
        return j + 1

    def scan_template(pos: int) -> int:
        nonlocal brace_depth
        j = pos
        while j < n:
            ch = src[j]
            if ch == "\\":
                j += 2
            elif ch == "`":
                return j + 1
            elif ch == "$" and j + 1 < n and src[j + 1] == "{":
                templates.append(brace_depth)
                brace_depth = 0
                return j + 2
            else:
                j += 1
        return j

    def scan_regex(pos: int) -> int:
        j = pos + 1
        in_class = False
        while j < n and src[j] != "\n":
            ch = src[j]
            if ch == "\\":
                j += 2
            elif ch == "[":
                in_class = True
                j += 1
            elif ch == "]":
                in_class = False
                j += 1
            elif ch == "/" and not in_class:
                j += 1
                while j < n and "a" <= src[j] <= "z":
                    j += 1
                return j
            else:
                j += 1
        return -1

    if src.startswith("#!"):
        nl = src.find("\n")
        i = n if nl == -1 else nl + 1

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if (
            c == "/"
            and nxt == "/"
            and ext != ".css"
            and not (i >= 2 and src[i - 1] == ":" and (src[i - 2].isalnum() or src[i - 2] == "_"))
        ):
            end = src.find("\n", i)
            if end == -1:
                end = n
            head = src[i : i + 4]
            is_doc = ext in (".rs", ".cs") and len(head) >= 3 and head[2] in ("/", "!") and head[3:4] != "/"
            comments.append((i, end, is_own_line(src, i), "doc" if is_doc else "line", src[i:end]))
            i = end
            continue
        if c == "/" and nxt == "*":
            close = src.find("*/", i + 2)
            end = n if close == -1 else close + 2
            head = src[i : i + 4]
            is_doc = len(head) >= 4 and head[2] in ("*", "!") and head[3] != "/"
            comments.append((i, end, is_own_line(src, i), "doc" if is_doc else "block", src[i:end]))
            i = end
            continue
        if c == '"':
            if ext == ".rs":
                match = RUST_RAW_PREFIX.search(src[max(0, i - 64) : i])
                if match:
                    hashes = match.group(1)
                    close = src.find('"' + hashes, i + 1)
                    i = n if close == -1 else close + 1 + len(hashes)
                    last = '"'
                    continue
            if ext == ".cs" and src.startswith('"""', i):
                close = src.find('"""', i + 3)
                i = n if close == -1 else close + 3
                last = '"'
                continue
            if ext == ".cs":
                prefix = src[max(0, i - 2) : i]
                if CS_VERBATIM_PREFIX.search(prefix) and "@" in prefix:
                    j = i + 1
                    while j < n and not (src[j] == '"' and (j + 1 >= n or src[j + 1] != '"')):
                        j += 2 if src[j] == '"' else 1
                    i = j + 1
                    last = '"'
                    continue
            i = skip_quoted(i, '"', not js)
            last = '"'
            continue
        if c == "'":
            if ext == ".rs":
                match = RUST_CHAR.match(src, i)
                i = match.end() if match else i + 1
            else:
                i = skip_quoted(i, "'", False)
            last = "'"
            continue
        if js and c == "`":
            i = scan_template(i + 1)
            last = "`"
            continue
        if js and c == "/" and (
            last == ""
            or last in REGEX_PREFIX
            or bool(REGEX_KEYWORD.search(src[max(0, i - 12) : i].rstrip()))
        ):
            end = scan_regex(i)
            if end != -1:
                i = end
                last = "/"
                continue
        if js and templates:
            if c == "{":
                brace_depth += 1
            elif c == "}":
                if brace_depth == 0:
                    brace_depth = templates.pop()
                    i = scan_template(i + 1)
                    last = "`"
                    continue
                brace_depth -= 1
        if not c.isspace():
            last = c
        i += 1
    return comments


def lex_python(src: str) -> list[tuple[int, int, bool, str, str]]:
    comments: list[tuple[int, int, bool, str, str]] = []
    lines = src.splitlines(keepends=True)
    offsets: list[int] = [0]
    for line in lines:
        offsets.append(offsets[-1] + len(line))
    try:
        tokens = list(tokenize.generate_tokens(io.StringIO(src).readline))
    except tokenize.TokenError:
        return comments
    for tok in tokens:
        if tok.type != tokenize.COMMENT:
            continue
        lineno, col = tok.start
        if lineno == 1 and col == 0 and tok.string.startswith("#!"):
            continue
        start = offsets[lineno - 1] + col
        end = start + len(tok.string)
        comments.append((start, end, lines[lineno - 1][:col].strip() == "", "line", tok.string))
    return comments


def lex_shell_or_ps(src: str, ext: str) -> list[tuple[int, int, bool, str, str]]:
    comments: list[tuple[int, int, bool, str, str]] = []
    lines = src.split("\n")
    offset = 0
    state: dict[str, object] | None = None
    for idx, line in enumerate(lines):
        i = 0
        if idx == 0 and line.startswith("#!"):
            i = len(line)
        while i < len(line):
            if state and state["type"] == "heredoc":
                candidate = line.lstrip("\t") if state["strip"] else line
                if candidate == state["word"]:
                    state = state["resume"]
                i = len(line)
                break
            if state and state["type"] == "block":
                close_token = str(state["close"])
                close = line.find(close_token, i)
                buf = state["text"]
                assert isinstance(buf, list)
                buf.append(line)
                if close == -1:
                    i = len(line)
                    break
                comments.append(
                    (
                        int(state["index"]),
                        offset + close + len(close_token),
                        bool(state["own"]),
                        "block",
                        "\n".join(buf),
                    )
                )
                i = close + len(close_token)
                state = None
                continue
            if state and state["type"] == "string":
                close_token = str(state["close"])
                close = line.find(close_token, i)
                if close == -1:
                    i = len(line)
                    break
                i = close + len(close_token)
                state = None
                continue
            rest = line[i:]
            ch = line[i]
            if ext == ".ps1" and rest.startswith("<#"):
                close = line.find("#>", i + 2)
                own = line[:i].strip() == ""
                if close != -1:
                    comments.append((offset + i, offset + close + 2, own, "block", line[i : close + 2]))
                    i = close + 2
                else:
                    state = {"type": "block", "close": "#>", "index": offset + i, "own": own, "text": [line[i:]]}
                    i = len(line)
                continue
            if ext == ".ps1" and PS_HERE_START.match(rest):
                state = {"type": "string", "close": f"{rest[1]}@"}
                i = len(line)
                break
            if ch in ("'", '"'):
                j = i + 1
                while j < len(line) and line[j] != ch:
                    j += 2 if ch == '"' and line[j] in ("\\", "`") else 1
                if j >= len(line):
                    state = {"type": "string", "close": ch}
                    i = len(line)
                else:
                    i = j + 1
                continue
            if ch == "#" and (i == 0 or line[i - 1].isspace()):
                comments.append((offset + i, offset + len(line), line[:i].strip() == "", "line", line[i:]))
                break
            i += 1
        if ext == ".sh" and (not state or state["type"] != "heredoc"):
            match = SH_HEREDOC.search(line)
            last_idx = comments[-1][0] if comments else -1
            if match and not (last_idx >= offset and last_idx - offset < match.start()):
                state = {
                    "type": "heredoc",
                    "strip": match.group(1) == "-",
                    "word": match.group(3),
                    "resume": state if state and state["type"] == "string" else None,
                }
        offset += len(line) + 1
    return comments


def scan_source(path: str, source: str) -> list[CommentBlock]:
    ext = Path(path).suffix
    if ext not in SCANNED_EXTS or is_exempt_path(path):
        return []
    if ext in C_FAMILY:
        raw = lex_c_family(source, ext)
    elif ext == ".py":
        raw = lex_python(source)
    else:
        raw = lex_shell_or_ps(source, ext)

    grouped: list[dict[str, object]] = []
    for start_idx, end_idx, own, kind, text in raw:
        start = line_number(source, start_idx)
        end = line_number(source, end_idx)
        line_style = bool(re.match(r"^\s*(//|#(?!>))", text))
        if (
            grouped
            and grouped[-1]["line_style"]
            and line_style
            and grouped[-1]["own"]
            and own
            and grouped[-1]["kind"] == kind
            and grouped[-1]["end"] == start - 1
        ):
            grouped[-1]["end"] = end
            grouped[-1]["text"] = f"{grouped[-1]['text']}\n{text.strip()}"
            continue
        grouped.append(
            {
                "start": start,
                "end": end,
                "kind": kind,
                "own": own,
                "line_style": line_style,
                "text": text.strip() if own else text,
            }
        )

    blocks: list[CommentBlock] = []
    for item in grouped:
        text = str(item["text"])
        if is_exempt_block(ext, text):
            continue
        blocks.append(
            CommentBlock(
                path=path,
                start=int(item["start"]),
                end=int(item["end"]),
                kind=str(item["kind"]),
                text=text,
            )
        )
    return blocks


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def git_show(rev: str, path: str) -> str:
    result = subprocess.run(["git", "show", f"{rev}:{path}"], cwd=ROOT, capture_output=True)
    return result.stdout.decode("utf-8", errors="replace") if result.returncode == 0 else ""


def tracked_files(rev: str | None, prefixes: list[str]) -> list[str]:
    if rev is None:
        raw = git("ls-files", "-z", "--", *(prefixes or ["."]))
        names = [p for p in raw.split("\0") if p]
    else:
        raw = git("ls-tree", "-r", "-z", "--name-only", rev, "--", *(prefixes or ["."]))
        names = [p for p in raw.split("\0") if p]
    return [p for p in names if Path(p).suffix in SCANNED_EXTS and not is_exempt_path(p)]


def ratchet_failures(
    before_counts: dict[str, int],
    after_counts: dict[str, int],
    renames: dict[str, str],
) -> list[tuple[str, int, int]]:
    failures: list[tuple[str, int, int]] = []
    for path in sorted(after_counts):
        after = after_counts[path]
        before = before_counts.get(renames.get(path, path), 0)
        if after > before:
            failures.append((path, before, after))
    return failures


def run_list(args: list[str]) -> int:
    as_json = "--json" in args
    prefixes = [a for a in args if a != "--json"]
    blocks: list[CommentBlock] = []
    for rel in tracked_files(None, prefixes):
        full = ROOT / rel
        if not full.is_file():
            continue
        blocks.extend(scan_source(rel, full.read_text(encoding="utf-8", errors="replace")))
    total_lines = sum(b.lines for b in blocks)
    if as_json:
        payload = {
            "blocks": len(blocks),
            "lines": total_lines,
            "items": [asdict(b) for b in blocks],
        }
        print(json.dumps(payload, indent=2))
    else:
        for b in blocks:
            first = b.text.splitlines()[0]
            print(f"{b.path}:{b.start}: {first}")
        print(f"comments: {len(blocks)} non-exempt blocks ({total_lines} lines)")
    return 1 if blocks else 0


def run_ratchet(base_ref: str, head_ref: str) -> int:
    base = git("merge-base", base_ref, head_ref).strip()
    changed_raw = git("diff", "--name-status", "-M", f"{base}..{head_ref}")
    before_counts: dict[str, int] = {}
    after_counts: dict[str, int] = {}
    after_blocks: dict[str, list[CommentBlock]] = {}
    renames: dict[str, str] = {}

    for line in changed_raw.splitlines():
        if not line:
            continue
        parts = line.split("\t")
        status = parts[0]
        if status.startswith("D"):
            continue
        new_path = parts[-1]
        if Path(new_path).suffix not in SCANNED_EXTS or is_exempt_path(new_path):
            continue
        old_path = parts[1] if status.startswith(("R", "C")) else new_path
        if old_path != new_path:
            renames[new_path] = old_path
        if not status.startswith("A"):
            before_counts[old_path] = len(scan_source(old_path, git_show(base, old_path)))
        blocks = scan_source(new_path, git_show(head_ref, new_path))
        after_counts[new_path] = len(blocks)
        after_blocks[new_path] = blocks

    failures = ratchet_failures(before_counts, after_counts, renames)
    total_before = sum(before_counts.get(renames.get(p, p), 0) for p in after_counts)
    total_after = sum(after_counts.values())
    print(f"comments (changed files): {total_before} -> {total_after}")
    if not failures:
        return 0
    for path, before, after in failures:
        print(f"comments: {path}: {before} -> {after}")
        for b in after_blocks[path][:5]:
            print(f"  {b.path}:{b.start}: {b.text.splitlines()[0]}")
    print(
        f"comments ratchet: {len(failures)} file(s) added non-exempt comments since {base[:12]}.\n"
        "Fix: delete the comment, move non-obvious decisions to docs/adr/, or file an issue for the workaround.\n"
        "List remaining comments with `python3 scripts/ci/check_comments.py --list` or run `/delete-comment`."
    )
    return 1


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[1] == "--list":
        return run_list(argv[2:])
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    head = argv[2] if len(argv) == 3 else "HEAD"
    return run_ratchet(argv[1], head)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
