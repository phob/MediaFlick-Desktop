"""Regression checks for the Linux runtime's relocated dependency closure."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import bundle


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mediaflick-bundle-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.prefix = self.root / "prefix"
        (self.prefix / "lib").mkdir(parents=True)
        self.output = self.root / "output"
        self.sources = self.root / "sources"

    def compile(self, soname, code, *flags):
        path = self.prefix / "lib" / soname
        subprocess.run(
            ["cc", "-shared", "-fPIC", "-x", "c", "-", f"-Wl,-soname,{soname}",
             "-o", str(path), *flags], input=code, text=True, check=True,
        )
        return path

    def playback_libraries(self):
        self.compile("libfixture-codec.so.1", "int decode(void) { return 42; }")
        return self.compile(
            "libmpv.so.2", "extern int decode(void); int playback(void) { return decode(); }",
            f"-L{self.prefix / 'lib'}", "-l:libfixture-codec.so.1",
        )

    def test_private_dependency_still_loads_after_relocation_without_build_prefix(self):
        self.playback_libraries()
        bundle.bundle(self.prefix, self.output, self.sources)
        shutil.rmtree(self.prefix)
        moved = self.root / "AppDir/usr/bin/libmpv"
        moved.parent.mkdir(parents=True)
        shutil.move(self.output / "lib", moved)
        self.assertEqual({p.name for p in moved.iterdir()}, {"libmpv.so.2", "libfixture-codec.so.1"})
        # A fresh process prevents a previously loaded library hiding bad RPATHs.
        subprocess.run(
            ["python3", "-c", "import ctypes, sys; assert ctypes.CDLL(sys.argv[1]).playback() == 42",
             str(moved / "libmpv.so.2")],
            env={k: v for k, v in os.environ.items() if k != "LD_LIBRARY_PATH"}, check=True,
        )

    def test_missing_transitive_dependency_fails_packaging(self):
        self.playback_libraries()
        (self.prefix / "lib/libfixture-codec.so.1").unlink()
        with self.assertRaisesRegex(RuntimeError, "libfixture-codec"):
            bundle.bundle(self.prefix, self.output, self.sources)

    def test_host_driver_is_not_copied_or_traversed(self):
        # Its deliberately missing private dependency belongs to the host's
        # driver stack; bundling must stop at the stable libva interface.
        for interface in ("libva.so.2", "libvulkan.so.1", "libcuda.so.1", "libnvcuvid.so.1"):
            with self.subTest(interface=interface):
                self.compile("libfixture-driver.so.1", "int driver(void) { return 1; }")
                self.compile(
                    interface, "extern int driver(void); int gpu(void) { return driver(); }",
                    f"-L{self.prefix / 'lib'}", "-l:libfixture-driver.so.1",
                )
                self.compile(
                    "libmpv.so.2", "extern int gpu(void); int playback(void) { return gpu(); }",
                    f"-L{self.prefix / 'lib'}", f"-l:{interface}",
                )
                (self.prefix / "lib/libfixture-driver.so.1").unlink()
                bundle.bundle(self.prefix, self.output, self.sources)
                self.assertEqual([p.name for p in (self.output / "lib").iterdir()], ["libmpv.so.2"])
                self.assertEqual((self.output / "HOST-LIBRARIES.txt").read_text(), interface + "\n")

    def test_build_dependency_sources_are_recorded_without_a_shared_library(self):
        self.playback_libraries()
        subprocess_run = subprocess.run
        downloads = []

        def run(args, **kwargs):
            if args[0] == "apt-get":
                downloads.append(args)
                return subprocess.CompletedProcess(args, 0)
            return subprocess_run(args, **kwargs)

        # libc6-dev is present with the compiler; use its real package metadata
        # and notice, but intercept network downloads for this regression test.
        with patch.object(bundle.subprocess, "run", side_effect=run):
            bundle.bundle(self.prefix, self.output, self.sources, build_packages=["libc6-dev"])
        record = (self.output / "SYSTEM-PACKAGES.txt").read_text().strip().split("\t")
        binary, _, source, version = record
        self.assertTrue(binary.startswith("libc6-dev"))
        self.assertTrue((self.output / "licenses" / f"{binary.replace(':', '-')}.copyright").is_file())
        (download,) = downloads
        self.assertIn("source", download)
        self.assertIn(f"{source}={version}", download)


if __name__ == "__main__":
    unittest.main()
