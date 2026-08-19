#!/usr/bin/env python3
"""Run deterministic Rust checks when a Codex session changed Rust inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


RELEVANT_FILES = {
    "Cargo.lock",
    "Cargo.toml",
    "build.rs",
    "clippy.toml",
    "justfile",
    "rust-toolchain.toml",
}
RELEVANT_DIRECTORIES = ("benches", "examples", "src", "tests")
MAX_FAILURE_CHARS = 12_000


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def input_digest(root: Path) -> str:
    digest = hashlib.sha256()
    paths = [root / name for name in sorted(RELEVANT_FILES)]
    for directory in RELEVANT_DIRECTORIES:
        base = root / directory
        if base.exists():
            paths.extend(sorted(base.rglob("*.rs")))

    for path in paths:
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix().encode()
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        contents = path.read_bytes()
        digest.update(len(contents).to_bytes(8, "big"))
        digest.update(contents)
    return digest.hexdigest()


def state_path(session_id: str) -> Path:
    session_hash = hashlib.sha256(session_id.encode()).hexdigest()
    state_directory = Path(tempfile.gettempdir()) / "mediaflick-codex-rust-gate"
    state_directory.mkdir(parents=True, exist_ok=True)
    return state_directory / f"{session_hash}.json"


def read_event() -> dict[str, object]:
    if sys.stdin.isatty():
        return {}
    payload = sys.stdin.read().strip()
    if not payload:
        return {}
    try:
        value = json.loads(payload)
    except json.JSONDecodeError:
        return {}
    return value if isinstance(value, dict) else {}


def save_baseline(path: Path, digest: str) -> None:
    path.write_text(json.dumps({"digest": digest}), encoding="utf-8")


def load_baseline(path: Path) -> str | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return None
    digest = value.get("digest") if isinstance(value, dict) else None
    return digest if isinstance(digest, str) else None


def run_quality_gate(root: Path) -> tuple[bool, str]:
    try:
        result = subprocess.run(
            ["just", "rust-quality"],
            cwd=root,
            capture_output=True,
            check=False,
            text=True,
            timeout=840,
        )
    except FileNotFoundError:
        return False, "`just` is not installed or is not on PATH."
    except subprocess.TimeoutExpired as error:
        output = f"{error.stdout or ''}\n{error.stderr or ''}".strip()
        return False, f"Rust quality checks timed out.\n{output}"

    output = f"{result.stdout}\n{result.stderr}".strip()
    return result.returncode == 0, output


def failure_reason(output: str) -> str:
    if len(output) > MAX_FAILURE_CHARS:
        half = MAX_FAILURE_CHARS // 2
        output = f"{output[:half]}\n... output truncated ...\n{output[-half:]}"
    return (
        "Rust quality checks failed. Fix every formatter or Clippy diagnostic, "
        "then run `just rust-quality` again before stopping.\n\n"
        f"{output}"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    event = read_event()
    root = repository_root()
    current_digest = input_digest(root)
    session_id = str(event.get("session_id") or "manual")
    baseline_path = state_path(session_id)
    event_name = event.get("hook_event_name")

    if event_name == "SessionStart":
        save_baseline(baseline_path, current_digest)
        return 0

    if not args.force and load_baseline(baseline_path) == current_digest:
        print("{}")
        return 0

    succeeded, output = run_quality_gate(root)
    if succeeded:
        save_baseline(baseline_path, current_digest)
        print("{}")
        return 0

    print(json.dumps({"decision": "block", "reason": failure_reason(output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
