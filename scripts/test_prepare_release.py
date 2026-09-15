"""Exercise release preparation in an isolated source fixture."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("prepare-release.py")


class PrepareReleaseTests(unittest.TestCase):
    def test_test_draft_preserves_changelog_and_versions_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            changelog = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- Upcoming feature.\n"
            (root / "CHANGELOG.md").write_text(changelog)
            (root / "Cargo.toml").write_text('[package]\nname = "mediaflick-desktop"\nversion = "0.1.6"\n')
            (root / "Cargo.lock").write_text('[[package]]\nname = "mediaflick-desktop"\nversion = "0.1.6"\n')
            subprocess.run([sys.executable, str(SCRIPT), "--version", "0.2.0-test.1", "--test-draft", "--notes-out", "notes.md"], cwd=root, check=True, capture_output=True)
            self.assertEqual((root / "CHANGELOG.md").read_text(), changelog)
            for name in ("Cargo.toml", "Cargo.lock"):
                self.assertIn('version = "0.2.0-test.1"', (root / name).read_text())
            self.assertIn("Keep this release as a draft", (root / "notes.md").read_text())

    def test_test_draft_rejects_stable_version_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, str(SCRIPT), "--version", "1.0.0", "--test-draft", "--notes-out", "notes.md"], cwd=directory, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires a prerelease version", result.stderr)
            self.assertEqual(list(Path(directory).iterdir()), [])


if __name__ == "__main__":
    unittest.main()
