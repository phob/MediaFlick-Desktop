"""Tests for check_feature_map.py against the real verify feature map, then and now.

Expected results come from outside the checker: the 2026-10-17 audit of the map found three
"Proven by" claims that cite run ids instead of evidence (features/playback.md, the three
segments claims), and the rule itself for the planted changes below.
"""

import re
import subprocess
import unittest
from pathlib import Path

from check_feature_map import SKILL, Problem, problems

ROOT = Path(__file__).resolve().parents[2]
AUDITED = "a70d2f1"  # main on 2026-10-17, the map the audit read
SKILL_PATH = SKILL.relative_to(ROOT).as_posix()
# A claim's first drive, found without the checker's parser.
FIRST_DRIVE = re.compile(r"[Pp]roven by `([a-z0-9-]+)`")


def at_revision(rev: str, directory: str, suffix: str) -> dict[str, str]:
    names = subprocess.run(
        ["git", "ls-tree", "--name-only", rev, f"{SKILL_PATH}/{directory}/"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.split()
    return {
        Path(name).name.removesuffix(suffix): subprocess.run(["git", "show", f"{rev}:{name}"], cwd=ROOT, check=True, capture_output=True, text=True).stdout
        for name in names
        if name.endswith(suffix)
    }


def on_disk(directory: str, suffix: str) -> dict[str, str]:
    return {path.name.removesuffix(suffix): path.read_text(encoding="utf-8") for path in (SKILL / directory).glob(f"*{suffix}")}


def lines(found: list[Problem]) -> set[tuple[str, int]]:
    return {(problem.feature, problem.line) for problem in found}


def claim_lines(features: dict[str, str]) -> list[tuple[str, int, str, int]]:
    """Each line holding a claim that starts with a drive name: the drive and where its backticked name starts."""
    return [
        (feature, number, match.group(1), match.start(1) - 1)
        for feature, text in features.items()
        for number, line in enumerate(text.splitlines(), start=1)
        for match in [FIRST_DRIVE.search(line)]
        if match
    ]


class AuditedMap(unittest.TestCase):
    def test_finds_exactly_the_three_run_id_claims(self) -> None:
        found = problems(at_revision(AUDITED, "features", ".md"), at_revision(AUDITED, "drives", ".mjs"))
        self.assertEqual(lines(found), {("playback", 37), ("playback", 38), ("playback", 39)}, found)


class PlantedChanges(unittest.TestCase):
    """Every claim in today's map, changed one way at a time."""

    def setUp(self) -> None:
        self.features = on_disk("features", ".md")
        self.drives = on_disk("drives", ".mjs")
        self.claims = claim_lines(self.features)
        self.assertGreater(len(self.claims), 100)

    def planted(self, feature: str, number: int, drive: str, at: int, replacement: str, before: tuple[int, str] = (0, "")) -> list[Problem]:
        """The claim's backticked first drive replaced, and `before` inserted at its offset; the rest of the map as it is."""
        text = self.features[feature].splitlines(keepends=True)
        line = text[number - 1]
        line = line[:at] + replacement + line[at + len(drive) + 2 :]
        offset, insert = before
        text[number - 1] = line[:offset] + insert + line[offset:]
        return problems({**self.features, feature: "".join(text)}, self.drives)

    def test_the_map_passes(self) -> None:
        self.assertEqual(problems(self.features, self.drives), [])

    def test_a_removed_drive_fails_every_claim_naming_it(self) -> None:
        for drive in sorted({drive for _, _, drive, _ in self.claims}):
            with self.subTest(drive=drive):
                remaining = {name: source for name, source in self.drives.items() if name != drive}
                expected = {(feature, number) for feature, number, first, _ in self.claims if first == drive}
                self.assertTrue(expected <= lines(problems(self.features, remaining)))

    def test_a_misspelled_drive_fails(self) -> None:
        for feature, number, drive, at in self.claims:
            with self.subTest(feature=feature, line=number):
                self.assertIn((feature, number), lines(self.planted(feature, number, drive, at, f"`{drive}x`")))

    def test_a_run_id_fails(self) -> None:
        for feature, number, drive, at in self.claims:
            with self.subTest(feature=feature, line=number):
                self.assertIn((feature, number), lines(self.planted(feature, number, drive, at, f"`{drive}` (`{drive}-20261014-093528`)")))

    def test_evidence_the_drive_does_not_write_fails(self) -> None:
        for feature, number, drive, at in self.claims:
            with self.subTest(feature=feature, line=number):
                self.assertIn((feature, number), lines(self.planted(feature, number, drive, at, f"`{drive}` (`zz-never-written.json`)")))
                after_version = f"`{drive}` on Jellyfin 10.11.0 (`zz-never-written.json`)"
                self.assertIn((feature, number), lines(self.planted(feature, number, drive, at, after_version)))

    def test_a_claim_without_a_drive_fails(self) -> None:
        for feature, number, drive, at in self.claims:
            with self.subTest(feature=feature, line=number):
                claim = at - len("Proven by ")
                planted = self.planted(feature, number, drive, at, f"`{drive}`", before=(claim, "Proven by the test server. "))
                self.assertIn((feature, number), lines(planted))

    def test_unwritten_evidence_with_any_extension_fails(self) -> None:
        # Review of PR #337: a `.txt` name fell outside the evidence pattern and was ignored.
        for feature, number, drive, at in self.claims:
            with self.subTest(feature=feature, line=number):
                self.assertIn((feature, number), lines(self.planted(feature, number, drive, at, f"`{drive}` (`zz-never-written.txt`)")))

    def test_harness_evidence_and_plain_terms_pass(self) -> None:
        for feature, number, drive, at in self.claims:
            with self.subTest(feature=feature, line=number):
                extra = f"`{drive}` (`steps.log`, `app.log`, `failure.png`, `--account`, `Skip Intro`, `/api/status`, Jellyfin `10.11.0`)"
                self.assertEqual(self.planted(feature, number, drive, at, extra), [])


class Commands(unittest.TestCase):
    """#341's whats-new claim cites a helper script with its arguments, a command and not evidence."""

    def test_a_script_run_with_arguments_is_not_evidence(self) -> None:
        claim = "*Proven by `whats-new` with the Companion (`jellyfin-version-servers.py 12.4+companion`).*\n"
        self.assertEqual(problems({"whats-new": claim}, {"whats-new": "export default async function () {}\n"}), [])

    def test_a_misspelled_drive_with_arguments_still_fails(self) -> None:
        claim = "*Proven by `whats-new` and `whats-nwe --account SERVER`.*\n"
        found = problems({"whats-new": claim}, {"whats-new": "export default async function () {}\n"})
        self.assertEqual(lines(found), {("whats-new", 1)}, found)


class ScreenshotNames(unittest.TestCase):
    """Review of PR #337: `segments.png` was accepted from a drive that writes only `segments.json`."""

    def test_a_screenshot_name_does_not_match_a_longer_file_name(self) -> None:
        found = problems({"playback": "*Proven by `segments` (`segments.png`).*\n"}, {"segments": 'ctx.writeEvidence("segments.json", {})\n'})
        self.assertEqual(lines(found), {("playback", 1)}, found)

    def test_a_screenshot_name_matches_its_capture(self) -> None:
        source = 'await ctx.screenshot("02-playing")\nawait ctx.snapshot(`${name}-bar`)\nctx.writeEvidence("cover.png", png)\n'
        claim = "*Proven by `playback` (`02-playing.png`, `02-playing.ax.txt`, `cover.png`).*\n"
        self.assertEqual(problems({"playback": claim}, {"playback": source}), [])


if __name__ == "__main__":
    unittest.main()
