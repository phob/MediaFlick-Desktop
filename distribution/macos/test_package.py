"""Exercise macOS package staging without requiring macOS signing tools."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("build-dmg.sh")


@unittest.skipUnless(os.name == "posix", "requires a Unix shell and filesystem")
class MacPackageTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="mediaflick-macos-package-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        distribution = self.root / "distribution/macos"
        distribution.mkdir(parents=True)
        shutil.copy2(SCRIPT, distribution)
        (distribution / "Info.plist.in").write_text("@APP_VERSION_FULL@")
        (distribution / "AppIcon.icns").write_bytes(b"icon fixture")
        for name in ("LICENSE", "THIRD-PARTY-NOTICES.md"):
            (self.root / name).write_text(name)
        self.runtime = self.root / "cef/152/cef_macos_arm64"
        (self.runtime / "Chromium Embedded Framework.framework/Resources").mkdir(parents=True)
        target = self.root / "build/cargo-target/release"
        target.mkdir(parents=True)
        binary = target / "mediaflick-desktop"
        binary.write_text("#!/bin/sh\nexit 0\n")
        binary.chmod(0o755)
        tools = self.root / "tools"
        tools.mkdir()
        for name in ("codesign", "hdiutil"):
            stub = tools / name
            stub.write_text("#!/bin/sh\nexit 0\n")
            stub.chmod(0o755)
        self.env = dict(os.environ, PATH=f"{tools}:{os.environ['PATH']}", CEF_PATH=str(self.root / "cef"), VERSION="0.2.0-test.1", TAG="v0.2.0-test.1", CARGO_TARGET_DIR="build/cargo-target")

    def run_package(self):
        return subprocess.run(["bash", "distribution/macos/build-dmg.sh"], cwd=self.root, env=self.env, capture_output=True, text=True)

    def test_credits_are_copied_from_selected_framework_cache(self):
        (self.runtime / "CREDITS.html").write_text("Chromium credits fixture")
        result = self.run_package()
        self.assertEqual(result.returncode, 0, result.stderr)
        credits = self.root / "dist/macos/MediaFlick Desktop.app/Contents/Resources/Licenses/CREDITS.html"
        self.assertEqual(credits.read_text(), "Chromium credits fixture")

    def test_missing_credits_still_rejects_package(self):
        result = self.run_package()
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
