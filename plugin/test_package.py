"""Check the ZIP and catalog that Jellyfin actually downloads."""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT = Path(__file__).with_name("package.py")


def published_plugin(root):
    publish = root / "publish"
    publish.mkdir()
    (publish / "Jellyfin.Plugin.MediaFlick.dll").write_bytes(b"assembly fixture")
    (publish / "LICENSE").write_text("license fixture")
    return publish


class PackageTests(unittest.TestCase):
    def test_catalog_matches_package(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            publish = published_plugin(root)
            output = root / "package"
            url = "https://example.org/packages/mediaflick-companion_0.2.1.zip"
            subprocess.run([sys.executable, str(SCRIPT), "--version", "0.2.1", "--publish-dir", str(publish), "--output-dir", str(output), "--source-url", url, "--changelog", "Test build"], check=True, capture_output=True)
            manifest = json.loads((output / "manifest.json").read_text())[0]
            release = manifest["versions"][0]
            archive = output / "mediaflick-companion_0.2.1.zip"
            self.assertEqual(release["checksum"], hashlib.md5(archive.read_bytes()).hexdigest())
            self.assertEqual(release["sourceUrl"], url)
            with zipfile.ZipFile(archive) as package:
                self.assertEqual(set(package.namelist()), {"Jellyfin.Plugin.MediaFlick.dll", "meta.json", "LICENSE"})
                meta = json.loads(package.read("meta.json"))
            self.assertEqual(meta["version"], release["version"])
            self.assertEqual(meta["guid"], manifest["guid"])
            self.assertEqual(meta["changelog"], "Test build")

    def test_jellyfin_rejects_semver_prerelease_suffix(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            publish = published_plugin(root)
            output = root / "package"
            result = subprocess.run([sys.executable, str(SCRIPT), "--version", "0.2.1-test.1", "--publish-dir", str(publish), "--output-dir", str(output), "--source-url", "https://example.org/test.zip"], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())

    def test_repository_preserves_versions_and_rejects_replacement(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "package"
            package.mkdir()
            repository = root / "repository"
            command = [sys.executable, str(SCRIPT.with_name("stage-test-repository.py")), "--package-dir", str(package), "--repository-dir", str(repository), "--source-sha", "abc123"]
            for version in ("0.2.1", "0.2.2"):
                manifest = [{"guid": "test-guid", "versions": [{"version": version}]}]
                (package / "manifest.json").write_text(json.dumps(manifest))
                (package / f"mediaflick-companion_{version}.zip").write_bytes(version.encode())
                subprocess.run(command, check=True, capture_output=True)
            catalog = repository / "manifest.json"
            before = catalog.read_bytes()
            self.assertEqual([entry["version"] for entry in json.loads(before)[0]["versions"]], ["0.2.2", "0.2.1"])
            self.assertEqual((repository / "packages/0.2.1/mediaflick-companion_0.2.1.zip").read_bytes(), b"0.2.1")
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(catalog.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
