import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("prepare-release.py")
RELEASE_FILES = {
    "CHANGELOG.md": "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- Upcoming feature.\n",
    "Cargo.toml": '[package]\nname = "mediaflick-desktop"\nversion = "0.1.6"\n',
    "Cargo.lock": '[[package]]\nname = "mediaflick-desktop"\nversion = "0.1.6"\n',
}


OLDER_RELEASE = "\n## [0.1.6] - 2026-07-16\n\n### Fixed\n\n- Old fix.\n"
PREPARED_VERSION = 'version = "0.2.0"'


def write_release_files(root, files=RELEASE_FILES):
    for name, contents in files.items():
        (root / name).write_text(contents)


def run_script(root, *arguments):
    return subprocess.run([sys.executable, str(SCRIPT), *arguments], cwd=root, capture_output=True, text=True)


def read_files(root):
    return {path.name: path.read_text() for path in root.iterdir()}


class PrepareReleaseTests(unittest.TestCase):
    def test_test_draft_preserves_changelog_and_versions_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root)
            subprocess.run([sys.executable, str(SCRIPT), "--version", "0.2.0-test.1", "--test-draft", "--notes-out", "notes.md"], cwd=root, check=True, capture_output=True)
            self.assertEqual((root / "CHANGELOG.md").read_text(), RELEASE_FILES["CHANGELOG.md"])
            for name in ("Cargo.toml", "Cargo.lock"):
                self.assertIn('version = "0.2.0-test.1"', (root / name).read_text())
            self.assertTrue((root / "notes.md").is_file())

    def test_test_draft_rejects_stable_version_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root)
            result = subprocess.run([sys.executable, str(SCRIPT), "--version", "1.0.0", "--test-draft", "--notes-out", "notes.md"], cwd=root, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual({path.name: path.read_text() for path in root.iterdir()}, RELEASE_FILES)

    def test_check_writes_the_notes_the_release_pr_promoted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root, RELEASE_FILES | {"CHANGELOG.md": RELEASE_FILES["CHANGELOG.md"] + OLDER_RELEASE})
            promoted = run_script(root, "--version", "0.2.0", "--date", "2026-11-12", "--notes-out", "promoted.md")
            self.assertEqual(promoted.returncode, 0, promoted.stderr)
            prepared = read_files(root)

            checked = run_script(root, "--version", "v0.2.0", "--check", "--notes-out", "checked.md")
            self.assertEqual(checked.returncode, 0, checked.stderr)
            notes = (root / "checked.md").read_text()
            self.assertEqual(notes, prepared["promoted.md"])
            self.assertIn("- Upcoming feature.", notes)
            self.assertNotIn("Old fix", notes)
            self.assertEqual(read_files(root), prepared | {"checked.md": notes})

    def test_check_rejects_a_checkout_that_was_not_prepared(self):
        bumped = {name: RELEASE_FILES[name].replace('version = "0.1.6"', PREPARED_VERSION) for name in ("Cargo.toml", "Cargo.lock")}
        unprepared = {
            "nothing prepared": RELEASE_FILES,
            "Cargo.lock left behind": RELEASE_FILES | {"Cargo.toml": bumped["Cargo.toml"]},
            "changelog not promoted": RELEASE_FILES | bumped,
            "release section without entries": RELEASE_FILES | bumped | {"CHANGELOG.md": "# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-11-12\n\n### Added\n" + OLDER_RELEASE},
        }
        for name, files in unprepared.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                write_release_files(root, files)
                result = run_script(root, "--version", "0.2.0", "--check", "--notes-out", "notes.md")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("0.2.0", result.stderr)
                self.assertEqual(read_files(root), files)

    def test_check_cannot_be_combined_with_a_test_draft(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root)
            result = run_script(root, "--version", "0.2.0-test.1", "--check", "--test-draft", "--notes-out", "notes.md")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(read_files(root), RELEASE_FILES)


if __name__ == "__main__":
    unittest.main()
