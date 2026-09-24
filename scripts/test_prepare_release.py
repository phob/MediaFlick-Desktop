"""Exercise release preparation in an isolated source fixture."""

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


def write_release_files(root):
    for name, contents in RELEASE_FILES.items():
        (root / name).write_text(contents)


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


if __name__ == "__main__":
    unittest.main()
