"""Regression checks for the Linux runtime's relocated dependency closure."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

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
        with self.assertRaisesRegex(RuntimeError, "Unresolved dependency libfixture-codec"):
            bundle.bundle(self.prefix, self.output, self.sources)

    def test_host_driver_is_not_copied_or_traversed(self):
        # Its deliberately missing private dependency belongs to the host's
        # driver stack; bundling must stop at the stable libva interface.
        self.compile("libfixture-driver.so.1", "int driver(void) { return 1; }")
        self.compile(
            "libva.so.2", "extern int driver(void); int va(void) { return driver(); }",
            f"-L{self.prefix / 'lib'}", "-l:libfixture-driver.so.1",
        )
        self.compile(
            "libmpv.so.2", "extern int va(void); int playback(void) { return va(); }",
            f"-L{self.prefix / 'lib'}", "-l:libva.so.2",
        )
        (self.prefix / "lib/libfixture-driver.so.1").unlink()
        bundle.bundle(self.prefix, self.output, self.sources)
        self.assertEqual([p.name for p in (self.output / "lib").iterdir()], ["libmpv.so.2"])
        self.assertEqual((self.output / "HOST-LIBRARIES.txt").read_text(), "libva.so.2\n")


if __name__ == "__main__":
    unittest.main()
