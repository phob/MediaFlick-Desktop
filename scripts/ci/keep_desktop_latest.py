#!/usr/bin/env python3
"""Keep a desktop release as GitHub's Latest release.

Usage: keep_desktop_latest.py OWNER/REPO

The desktop updater (src/maintenance/updater.rs) and the flick.media download
buttons (website/public/site.js) both read `releases/latest`. A Companion
release, or any other tag, marked Latest would stop every desktop update. When
Latest is not a `v<digits>.<digits>.<digits>` tag, this script puts Latest back
on the highest published, non-prerelease desktop release and exits 1, so the
run fails red even though it repaired the state. It needs `gh` with a token
that can edit releases.
"""

import json
import re
import subprocess
import sys
from dataclasses import dataclass

DESKTOP_TAG = re.compile(r"v([0-9]+)\.([0-9]+)\.([0-9]+)")


@dataclass(frozen=True)
class Release:
    id: int
    tag: str
    draft: bool
    prerelease: bool


@dataclass(frozen=True)
class Keep:
    latest: str


@dataclass(frozen=True)
class Restore:
    latest: str | None
    target: Release


@dataclass(frozen=True)
class NoCandidate:
    latest: str | None


Decision = Keep | Restore | NoCandidate


def desktop_version(tag: str) -> tuple[int, int, int] | None:
    match = DESKTOP_TAG.fullmatch(tag)
    if match is None:
        return None
    major, minor, patch = (int(part) for part in match.groups())
    return major, minor, patch


def is_desktop_tag(tag: str) -> bool:
    return desktop_version(tag) is not None


def decide(latest: str | None, releases: list[Release]) -> Decision:
    if latest is not None and is_desktop_tag(latest):
        return Keep(latest)
    candidates = [
        (version, release)
        for release in releases
        if not release.draft and not release.prerelease and (version := desktop_version(release.tag)) is not None
    ]
    if not candidates:
        return NoCandidate(latest)
    return Restore(latest, max(candidates, key=lambda candidate: candidate[0])[1])


def gh(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["gh", "api", "-H", "Accept: application/vnd.github+json", *args], capture_output=True, text=True)


def latest_tag(repo: str) -> str | None:
    result = gh(f"repos/{repo}/releases/latest", "--jq", ".tag_name")
    if result.returncode != 0:
        if "HTTP 404" in result.stderr:
            return None
        raise SystemExit(f"reading the latest release failed: {result.stderr.strip()}")
    return result.stdout.removesuffix("\n")


def all_releases(repo: str) -> list[Release]:
    result = gh(
        "--paginate",
        f"repos/{repo}/releases?per_page=100",
        "--jq",
        ".[] | {id, tag: .tag_name, draft, prerelease}",
    )
    if result.returncode != 0:
        raise SystemExit(f"listing releases failed: {result.stderr.strip()}")
    releases = []
    for line in result.stdout.splitlines():
        row = json.loads(line)
        if not (
            isinstance(row.get("id"), int)
            and isinstance(row.get("tag"), str)
            and isinstance(row.get("draft"), bool)
            and isinstance(row.get("prerelease"), bool)
        ):
            raise SystemExit(f"unexpected release in the GitHub answer: {line}")
        releases.append(Release(id=row["id"], tag=row["tag"], draft=row["draft"], prerelease=row["prerelease"]))
    return releases


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    repo = sys.argv[1]
    latest = latest_tag(repo)
    match decide(latest, all_releases(repo)):
        case Keep(tag):
            print(f"Latest is the desktop release {tag}.")
            return 0
        case NoCandidate(tag):
            print(f"::error::Latest is {tag or 'unset'}, and no published desktop release can take it back.")
            return 1
        case Restore(tag, target):
            result = gh("-X", "PATCH", f"repos/{repo}/releases/{target.id}", "-f", "make_latest=true")
            if result.returncode != 0:
                raise SystemExit(f"marking {target.tag} as Latest failed: {result.stderr.strip()}")
            now = latest_tag(repo)
            print(
                f"::error::Latest was {tag or 'unset'}, which is not a desktop release. "
                f"Latest is now {now}. Publish Companion releases with 'Set as the latest release' unchecked."
            )
            return 1


if __name__ == "__main__":
    raise SystemExit(main())
