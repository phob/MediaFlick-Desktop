"""Tests for plugin/package.py, the Companion's package and repository tooling.

Expected values come from Jellyfin's plugin repository format (a JSON list of
plugins, each with `versions` carrying version, targetAbi, sourceUrl, MD5
checksum and timestamp), Jellyfin's rule that `targetAbi` is the minimum
server version, and the operator's rules for the stable repository on
flick.media (2026-11-15): an entry is added only once its download is public,
earlier entries stay, and a published version is never replaced.

Ways adding a release to the repository could go wrong, each covered below:
- a version that is already listed (also as 0.2.2 vs 0.2.2.0) is replaced;
- earlier versions are dropped, so older servers lose a build they can load;
- versions are ordered as text, so 0.10.0 sorts below 0.9.0;
- an entry is merged into a catalog of another plugin or a malformed one;
- the checksum is not taken from the bytes at sourceUrl, so Jellyfin rejects
  the download, or an unreachable URL (a draft asset is 404 without
  authentication) still gets an entry;
- a failed add rewrites the manifest anyway.
"""

import hashlib
import http.server
import json
import random
import re
import subprocess
import sys
import tempfile
import threading
import unittest
import zipfile
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

import package

SCRIPT = Path(__file__).with_name("package.py")
GUID = "11d8f2bb-2b9d-4ce1-8c33-5a0f809dfd2f"


def published_plugin(root: Path) -> Path:
    publish = root / "publish"
    publish.mkdir()
    (publish / "Jellyfin.Plugin.MediaFlick.dll").write_bytes(b"assembly fixture")
    (publish / "LICENSE").write_text("license fixture")
    return publish


def build(root: Path, version: str) -> subprocess.CompletedProcess[str]:
    publish = root / "publish" if (root / "publish").exists() else published_plugin(root)
    return subprocess.run(
        [sys.executable, str(SCRIPT), "build", "--version", version, "--publish-dir", str(publish),
         "--output-dir", str(root / "package"), "--changelog", "Test build"],
        capture_output=True, text=True,
    )


@contextmanager
def serve(directory: Path) -> Iterator[str]:
    class Quiet(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(directory), **kwargs)

        def log_message(self, format, *args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Quiet)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


def entry(version: str) -> dict[str, str]:
    return {"version": version, "changelog": "c", "targetAbi": "12.0.0.0", "sourceUrl": f"https://example.org/{version}.zip",
            "checksum": "0" * 32, "timestamp": "2026-11-15T00:00:00+00:00"}


def versions(catalog: list[dict]) -> list[str]:
    return [listed["version"] for listed in catalog[0]["versions"]]


class BuildTests(unittest.TestCase):
    def test_package_holds_assembly_meta_and_license(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build(root, "0.2.1").check_returncode()
            with zipfile.ZipFile(root / "package" / "mediaflick-companion_0.2.1.zip") as archive:
                self.assertEqual(set(archive.namelist()), {"Jellyfin.Plugin.MediaFlick.dll", "meta.json", "LICENSE"})
                meta = json.loads(archive.read("meta.json"))
            self.assertEqual(meta["version"], "0.2.1")
            self.assertEqual(meta["guid"], GUID)
            self.assertEqual(meta["changelog"], "Test build")
            self.assertEqual(meta["targetAbi"], package.target_abi())

    def test_jellyfin_rejects_semver_prerelease_suffix(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertNotEqual(build(root, "0.2.1-test.1").returncode, 0)
            self.assertFalse((root / "package").exists())


class AbiTests(unittest.TestCase):
    def test_build_yaml_matches_the_compiled_floor(self):
        # Jellyfin loads a plugin only on servers at or above targetAbi, and the
        # plugin only knows the APIs of the Jellyfin.Controller it compiles against.
        plugin = Path(__file__).parent
        manifest = (plugin / "build.yaml").read_text(encoding="utf-8")
        abi = re.search(r'^targetAbi:\s*"([^"]+)"', manifest, re.MULTILINE)
        version = re.search(r'^version:\s*"([^"]+)"', manifest, re.MULTILINE)
        props = (plugin / "Directory.Build.props").read_text(encoding="utf-8")
        assembly = re.search(r"<AssemblyVersion>([^<]+)</AssemblyVersion>", props)
        assert abi and version and assembly, "build.yaml or Directory.Build.props lost a field"
        self.assertEqual(abi.group(1), package.target_abi())
        self.assertEqual(version.group(1), assembly.group(1))


class AddVersionTests(unittest.TestCase):
    def test_first_version_creates_the_plugin_record(self):
        catalog = package.add_version([], entry("0.2.2.0"))
        self.assertEqual(len(catalog), 1)
        record = catalog[0]
        self.assertEqual(record["guid"], GUID)
        self.assertEqual(record["name"], "MediaFlick Companion")
        for field in ("description", "overview", "owner", "category"):
            self.assertIsInstance(record[field], str)
        self.assertEqual(record["versions"], [entry("0.2.2.0")])

    def test_earlier_versions_stay_newest_first(self):
        catalog = package.add_version([], entry("0.2.2.0"))
        catalog = package.add_version(catalog, entry("0.10.0.0"))
        catalog = package.add_version(catalog, entry("0.9.1"))
        self.assertEqual(versions(catalog), ["0.10.0.0", "0.9.1", "0.2.2.0"])

    def test_a_listed_version_is_never_replaced(self):
        catalog = package.add_version([], entry("0.2.2.0"))
        for duplicate in ("0.2.2.0", "0.2.2"):
            with self.assertRaises(SystemExit):
                package.add_version(catalog, entry(duplicate))
        self.assertEqual(versions(catalog), ["0.2.2.0"])

    def test_another_plugin_or_malformed_catalog_is_refused(self):
        other = [{"guid": "00000000-0000-0000-0000-000000000000", "name": "Other", "versions": []}]
        for catalog in (other, [*package.add_version([], entry("0.2.2.0")), *other], [{"versions": []}]):
            with self.assertRaises(SystemExit):
                package.add_version(catalog, entry("0.3.0.0"))

    def test_order_holds_for_any_insertion_order(self):
        generator = random.Random(20261115)
        for _ in range(200):
            numbers = {tuple(generator.randint(0, 20) for _ in range(4)) for _ in range(generator.randint(1, 8))}
            shuffled = list(numbers)
            generator.shuffle(shuffled)
            catalog: list[dict] = []
            for parts in shuffled:
                catalog = package.add_version(catalog, entry(".".join(map(str, parts))))
            expected = [".".join(map(str, parts)) for parts in sorted(numbers, reverse=True)]
            self.assertEqual(versions(catalog), expected)


class AddCommandTests(unittest.TestCase):
    def add(self, url: str, manifest: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, str(SCRIPT), "add", "--source-url", url, "--manifest", str(manifest)],
                              capture_output=True, text=True)

    def test_entry_describes_the_bytes_at_source_url(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build(root, "0.2.2.0").check_returncode()
            manifest = root / "manifest.json"
            manifest.write_text("[]\n")
            archive = root / "package" / "mediaflick-companion_0.2.2.0.zip"
            with serve(root / "package") as base:
                url = f"{base}/mediaflick-companion_0.2.2.0.zip"
                self.add(url, manifest).check_returncode()
            with zipfile.ZipFile(archive) as zipped:
                meta = json.loads(zipped.read("meta.json"))
            listed = json.loads(manifest.read_text())[0]["versions"][0]
            self.assertEqual(listed, {
                "version": "0.2.2.0",
                "changelog": "Test build",
                "targetAbi": meta["targetAbi"],
                "sourceUrl": url,
                "checksum": hashlib.md5(archive.read_bytes()).hexdigest(),
                "timestamp": meta["timestamp"],
            })

    def test_unreachable_download_or_mismatched_name_leaves_the_manifest_alone(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build(root, "0.2.2.0").check_returncode()
            archive = root / "package" / "mediaflick-companion_0.2.2.0.zip"
            (root / "package" / "mediaflick-companion_0.3.0.0.zip").write_bytes(archive.read_bytes())
            manifest = root / "manifest.json"
            manifest.write_text("[]\n")
            with serve(root / "package") as base:
                for url in (f"{base}/mediaflick-companion_0.2.3.0.zip", f"{base}/mediaflick-companion_0.3.0.0.zip"):
                    self.assertNotEqual(self.add(url, manifest).returncode, 0, url)
            self.assertEqual(manifest.read_text(), "[]\n")


if __name__ == "__main__":
    unittest.main()
