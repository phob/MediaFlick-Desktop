#!/usr/bin/env python3
"""Report large Rust source files and reject files above the hard limit."""

from __future__ import annotations

from dataclasses import dataclass
import os
from pathlib import Path
import subprocess
import sys


REVIEW_LINE_LIMIT = 1_000
HARD_LINE_LIMIT = 1_500


@dataclass(frozen=True)
class SourceSize:
    path: Path
    line_count: int


def repository_root() -> Path:
    return Path(__file__).resolve().parents[1]


def rust_source_paths(root: Path) -> list[Path]:
    """Return tracked and non-ignored untracked Rust sources."""
    try:
        result = subprocess.run(
            [
                "git",
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                "*.rs",
            ],
            cwd=root,
            capture_output=True,
            check=False,
        )
    except FileNotFoundError as error:
        raise RuntimeError("git is not installed or is not on PATH") from error

    if result.returncode != 0:
        detail = result.stderr.decode(errors="replace").strip()
        raise RuntimeError(detail or "git could not enumerate Rust source files")

    paths = {
        root / Path(os.fsdecode(raw_path))
        for raw_path in result.stdout.split(b"\0")
        if raw_path
    }
    return sorted((path for path in paths if path.is_file()), key=lambda path: path.as_posix())


def physical_line_count(path: Path) -> int:
    with path.open("rb") as source:
        return sum(1 for _ in source)


def source_sizes(root: Path) -> list[SourceSize]:
    return [
        SourceSize(path.relative_to(root), physical_line_count(path))
        for path in rust_source_paths(root)
    ]


def report(sizes: list[SourceSize]) -> int:
    violations = sorted(
        (size for size in sizes if size.line_count > HARD_LINE_LIMIT),
        key=lambda size: (-size.line_count, size.path.as_posix()),
    )
    reviews = sorted(
        (
            size
            for size in sizes
            if REVIEW_LINE_LIMIT < size.line_count <= HARD_LINE_LIMIT
        ),
        key=lambda size: (-size.line_count, size.path.as_posix()),
    )

    for size in violations:
        print(
            f"error: {size.path.as_posix()} has {size.line_count} physical lines; "
            f"the hard limit is {HARD_LINE_LIMIT}"
        )
    for size in reviews:
        print(
            f"review: {size.path.as_posix()} has {size.line_count} physical lines; "
            f"consider splitting files above {REVIEW_LINE_LIMIT}"
        )

    if violations:
        print(
            f"Rust file-size check failed: {len(violations)} file(s) exceed "
            f"{HARD_LINE_LIMIT} physical lines."
        )
        return 1

    print(
        f"Rust file-size check passed: {len(sizes)} file(s) checked; "
        f"none exceeds {HARD_LINE_LIMIT} physical lines."
    )
    return 0


def main() -> int:
    root = repository_root()
    try:
        sizes = source_sizes(root)
    except (OSError, RuntimeError) as error:
        print(f"Rust file-size check could not run: {error}", file=sys.stderr)
        return 2
    return report(sizes)


if __name__ == "__main__":
    raise SystemExit(main())
