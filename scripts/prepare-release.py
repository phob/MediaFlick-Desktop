#!/usr/bin/env python3
"""Prepare a changelog-driven release, or check that a checkout holds one.

Run without a mode in the release-notes PR: it promotes CHANGELOG.md's
[Unreleased] section to the requested version, opens a fresh [Unreleased]
section, updates the Cargo.toml and Cargo.lock package versions, and writes the
release notes.

`--check` is what the release workflow runs on the merged result: it changes no
release file, fails unless the checkout already is that release, and writes the
same notes. `--test-draft` only stamps a prerelease version for a disposable
build.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
from pathlib import Path

SEMVER_RE = re.compile(r"^v?(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$")
UNRELEASED_HEADING_RE = re.compile(r"^## \[Unreleased\]\s*$", re.MULTILINE)
SECOND_LEVEL_HEADING_RE = re.compile(r"^## \[", re.MULTILINE)
PACKAGE_VERSION_RE = re.compile(r'(?m)^version\s*=\s*"([^"]+)"\s*$')
LOCK_VERSION_RE = re.compile(r'(\[\[package\]\]\s*\nname = "mediaflick-desktop"\s*\nversion = ")([^"]+)("\s*\n)')

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
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--test-draft", action="store_true", help="Prepare a disposable prerelease without consuming the changelog.")
    mode.add_argument("--check", action="store_true", help="Verify that the checkout already holds this release and write its notes; change no release file.")
    return parser.parse_args()


def normalize_version(version: str) -> str:
    match = SEMVER_RE.match(version.strip())
    if not match:
        raise SystemExit("Version must look like 1.2.3, 1.2.3-beta.1, or v1.2.3.")
    return match.group(1)


def release_heading(changelog: str, version: str) -> re.Match[str] | None:
    return re.search(rf"^## \[{re.escape(version)}\](?:\s+-\s+\d{{4}}-\d{{2}}-\d{{2}})?\s*$", changelog, re.MULTILINE)


def section_end(changelog: str, heading: re.Match[str]) -> int:
    next_heading = SECOND_LEVEL_HEADING_RE.search(changelog, heading.end())
    return next_heading.start() if next_heading else len(changelog)


def has_entries(release_notes: str) -> bool:
    return any(line.lstrip().startswith("- ") for line in release_notes.splitlines())


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

    if release_heading(changelog, version):
        raise SystemExit(f"CHANGELOG.md already contains a section for {version}.")

    unreleased_heading = UNRELEASED_HEADING_RE.search(changelog)
    if not unreleased_heading:
        raise SystemExit("CHANGELOG.md is missing a '## [Unreleased]' section.")
    unreleased_end = section_end(changelog, unreleased_heading)
    release_notes = strip_empty_subsections(changelog[unreleased_heading.end():unreleased_end])

    if not has_entries(release_notes):
        raise SystemExit("CHANGELOG.md [Unreleased] has no bullet entries to release. Run /cl before releasing.")

    replacement = f"{EMPTY_UNRELEASED_SECTION}\n\n## [{version}] - {release_date}\n\n{release_notes}\n"
    updated = changelog[:unreleased_heading.start()] + replacement + changelog[unreleased_end:].lstrip("\n")
    changelog_path.write_text(updated, encoding="utf-8")
    return f"{release_notes}\n"


def package_version(cargo_toml: str) -> re.Match[str]:
    """The `version` line of Cargo.toml's [package] section, with the version as group 1."""
    package_match = re.search(r"(?m)^\[package\]\s*$", cargo_toml)
    if not package_match:
        raise SystemExit("Cargo.toml is missing a [package] section.")

    next_section = re.compile(r"(?m)^\[").search(cargo_toml, package_match.end())
    version_match = PACKAGE_VERSION_RE.search(cargo_toml, package_match.end(), next_section.start() if next_section else len(cargo_toml))
    if not version_match:
        raise SystemExit("Cargo.toml [package] section is missing a version field.")
    return version_match


def lock_version(cargo_lock: str) -> re.Match[str]:
    """Cargo.lock's entry for this package, with the version as group 2."""
    match = LOCK_VERSION_RE.search(cargo_lock)
    if not match:
        raise SystemExit("Cargo.lock is missing the mediaflick-desktop package entry.")
    return match


def update_cargo_files(version: str) -> None:
    cargo_toml_path = Path("Cargo.toml")
    cargo_toml = cargo_toml_path.read_text(encoding="utf-8")
    toml_version = package_version(cargo_toml)
    cargo_toml_path.write_text(f'{cargo_toml[:toml_version.start()]}version = "{version}"{cargo_toml[toml_version.end():]}', encoding="utf-8")

    cargo_lock_path = Path("Cargo.lock")
    cargo_lock = cargo_lock_path.read_text(encoding="utf-8")
    locked = lock_version(cargo_lock)
    cargo_lock_path.write_text(f"{cargo_lock[:locked.start(2)]}{version}{cargo_lock[locked.end(2):]}", encoding="utf-8")


def check_prepared(version: str) -> str:
    """The notes of a release the checkout already holds; exits when it does not hold it."""
    hint = f"Merge the release-notes PR that runs `prepare-release.py --version {version}` first."
    for name, found in (
        ("Cargo.toml", package_version(Path("Cargo.toml").read_text(encoding="utf-8")).group(1)),
        ("Cargo.lock", lock_version(Path("Cargo.lock").read_text(encoding="utf-8")).group(2)),
    ):
        if found != version:
            raise SystemExit(f"{name} is at {found}, not {version}. {hint}")

    changelog = Path("CHANGELOG.md").read_text(encoding="utf-8")
    heading = release_heading(changelog, version)
    if not heading:
        raise SystemExit(f"CHANGELOG.md has no section for {version}. {hint}")
    release_notes = strip_empty_subsections(changelog[heading.end():section_end(changelog, heading)])
    if not has_entries(release_notes):
        raise SystemExit(f"CHANGELOG.md's section for {version} has no bullet entries.")
    return f"{release_notes}\n"


def main() -> None:
    args = parse_args()
    version = normalize_version(args.version)
    if args.check:
        Path(args.notes_out).write_text(check_prepared(version), encoding="utf-8")
        print(f"Checkout holds release v{version}")
        return
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
