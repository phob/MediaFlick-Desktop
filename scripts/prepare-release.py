#!/usr/bin/env python3
"""Prepare a changelog-driven release commit.

This promotes CHANGELOG.md's [Unreleased] section to the requested version,
opens a fresh [Unreleased] section, updates Cargo.toml/Cargo.lock package
versions, and writes release notes for GitHub Releases.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
from pathlib import Path

SEMVER_RE = re.compile(r"^v?(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$")
UNRELEASED_HEADING_RE = re.compile(r"^## \[Unreleased\]\s*$", re.MULTILINE)
SECOND_LEVEL_HEADING_RE = re.compile(r"^## \[", re.MULTILINE)

EMPTY_UNRELEASED_SECTION = """## [Unreleased]

### Breaking Changes

### Added

### Changed

### Fixed

### Removed
"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Prepare changelog and version files for a release.")
    parser.add_argument("--version", required=True, help="Version to release, with or without a leading v.")
    parser.add_argument("--date", default=dt.date.today().isoformat(), help="Release date, defaulting to today.")
    parser.add_argument("--notes-out", required=True, help="Path where extracted release notes should be written.")
    parser.add_argument("--test-draft", action="store_true", help="Prepare a disposable prerelease without consuming the changelog.")
    return parser.parse_args()


def normalize_version(version: str) -> str:
    match = SEMVER_RE.match(version.strip())
    if not match:
        raise SystemExit("Version must look like 1.2.3, 1.2.3-beta.1, or v1.2.3.")
    return match.group(1)


def find_unreleased_section(changelog: str) -> tuple[re.Match[str], int]:
    match = UNRELEASED_HEADING_RE.search(changelog)
    if not match:
        raise SystemExit("CHANGELOG.md is missing a '## [Unreleased]' section.")

    next_heading = SECOND_LEVEL_HEADING_RE.search(changelog, match.end())
    section_end = next_heading.start() if next_heading else len(changelog)
    return match, section_end


def strip_empty_subsections(section: str) -> str:
    section = section.strip()
    if not section:
        return ""

    parts = re.split(r"(?m)^(### .*)\s*$", section)
    kept: list[str] = []

    preamble = parts[0].strip()
    if preamble:
        kept.append(preamble)

    for index in range(1, len(parts), 2):
        heading = parts[index].strip()
        body = parts[index + 1].strip() if index + 1 < len(parts) else ""
        if body:
            kept.append(f"{heading}\n\n{body}")

    return "\n\n".join(kept).strip()


def promote_changelog(version: str, release_date: str) -> str:
    changelog_path = Path("CHANGELOG.md")
    changelog = changelog_path.read_text(encoding="utf-8")

    if re.search(rf"^## \[{re.escape(version)}\](?:\s+-\s+\d{{4}}-\d{{2}}-\d{{2}})?\s*$", changelog, re.MULTILINE):
        raise SystemExit(f"CHANGELOG.md already contains a section for {version}.")

    unreleased_heading, section_end = find_unreleased_section(changelog)
    raw_unreleased = changelog[unreleased_heading.end():section_end]
    release_notes = strip_empty_subsections(raw_unreleased)

    if not any(line.lstrip().startswith("- ") for line in release_notes.splitlines()):
        raise SystemExit("CHANGELOG.md [Unreleased] has no bullet entries to release. Run /cl before releasing.")

    replacement = f"{EMPTY_UNRELEASED_SECTION}\n\n## [{version}] - {release_date}\n\n{release_notes}\n"
    updated = changelog[:unreleased_heading.start()] + replacement + changelog[section_end:].lstrip("\n")
    changelog_path.write_text(updated, encoding="utf-8")
    return f"{release_notes}\n"


def replace_first_package_version(cargo_toml: str, version: str) -> str:
    package_match = re.search(r"(?m)^\[package\]\s*$", cargo_toml)
    if not package_match:
        raise SystemExit("Cargo.toml is missing a [package] section.")

    next_section = re.search(r"(?m)^\[", cargo_toml[package_match.end():])
    package_end = package_match.end() + next_section.start() if next_section else len(cargo_toml)
    package_section = cargo_toml[package_match.end():package_end]

    version_match = re.search(r'(?m)^version\s*=\s*"[^"]+"\s*$', package_section)
    if not version_match:
        raise SystemExit("Cargo.toml [package] section is missing a version field.")

    start = package_match.end() + version_match.start()
    end = package_match.end() + version_match.end()
    return f'{cargo_toml[:start]}version = "{version}"{cargo_toml[end:]}'


def update_cargo_files(version: str) -> None:
    cargo_toml_path = Path("Cargo.toml")
    cargo_toml_path.write_text(replace_first_package_version(cargo_toml_path.read_text(encoding="utf-8"), version), encoding="utf-8")

    cargo_lock_path = Path("Cargo.lock")
    cargo_lock = cargo_lock_path.read_text(encoding="utf-8")
    updated_lock, count = re.subn(
        r'(\[\[package\]\]\s*\nname = "mediaflick-desktop"\s*\nversion = ")[^"]+("\s*\n)',
        rf"\g<1>{version}\2",
        cargo_lock,
        count=1,
    )
    if count != 1:
        raise SystemExit('Cargo.lock is missing the mediaflick-desktop package entry.')
    cargo_lock_path.write_text(updated_lock, encoding="utf-8")


def main() -> None:
    args = parse_args()
    version = normalize_version(args.version)
    if args.test_draft:
        if "-" not in version.split("+", 1)[0]:
            raise SystemExit("A test draft requires a prerelease version such as 0.2.0-test.1.")
        release_notes = (
            f"# MediaFlick Desktop {version} — test draft\n\n"
            "Disposable build for manual testing. Keep this release as a draft; "
            "it is not a stable release or a 1.0 release candidate.\n\n"
            "The normal release changelog and source-branch version are unchanged. "
            "The bundled application reports the test version.\n\n"
            "## Manual checks\n\n"
            "- Fresh installation, launch, sign-in, and Quick Connect.\n"
            "- Upgrade an existing installation and verify settings, accounts, collections, and posters.\n"
            "- Movies and Series: browsing, search, details, and account switching.\n"
            "- Built-in libmpv and external mpv: start, pause, seek, resume, subtitles, "
            "segment skipping, next episode, and Jellyfin progress/watched state.\n"
            "- Settings Save, Reset, and Discard; restart and verify persistence.\n"
            "- Companion absent/present: discovery, calendar, requests, ratings, and collections.\n"
            "- Install and update Companion through its separate test repository, then restart Jellyfin.\n\n"
            "macOS requires external mpv and uses an ad-hoc signed app. "
            "Linux built-in playback requires X11 or XWayland.\n"
        )
    else:
        release_notes = promote_changelog(version, args.date)
    update_cargo_files(version)
    Path(args.notes_out).write_text(release_notes, encoding="utf-8")
    print(f"Prepared release v{version}")


if __name__ == "__main__":
    main()
