from __future__ import annotations

from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_rust_file_size as check


class RustSourcePathsTests(unittest.TestCase):
    def test_finds_tracked_and_untracked_sources_but_skips_ignored_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet"], cwd=root, check=True)
            (root / ".gitignore").write_text("target/\n", encoding="utf-8")
            (root / "src").mkdir()
            (root / "src" / "tracked.rs").write_text("fn tracked() {}\n", encoding="utf-8")
            (root / "src" / "untracked.rs").write_text("fn untracked() {}\n", encoding="utf-8")
            (root / "target").mkdir()
            (root / "target" / "generated.rs").write_text("generated\n", encoding="utf-8")
            subprocess.run(
                [
                    "git",
                    "-c",
                    "core.autocrlf=false",
                    "add",
                    ".gitignore",
                    "src/tracked.rs",
                ],
                cwd=root,
                check=True,
            )

            paths = [path.relative_to(root).as_posix() for path in check.rust_source_paths(root)]

            self.assertEqual(paths, ["src/tracked.rs", "src/untracked.rs"])


class FileSizeReportTests(unittest.TestCase):
    def test_counts_blank_lines_and_mixed_line_endings(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sample.rs"
            path.write_bytes(b"first\r\n\r\nthird\n")

            self.assertEqual(check.physical_line_count(path), 3)

    def test_reports_review_files_and_rejects_only_hard_limit_violations(self) -> None:
        sizes = [
            check.SourceSize(Path("at-review-limit.rs"), 1_000),
            check.SourceSize(Path("review.rs"), 1_001),
            check.SourceSize(Path("at-hard-limit.rs"), 1_500),
            check.SourceSize(Path("too-large.rs"), 1_501),
        ]
        output = StringIO()

        with redirect_stdout(output):
            exit_code = check.report(sizes)

        self.assertEqual(exit_code, 1)
        self.assertNotIn("at-review-limit.rs", output.getvalue())
        self.assertIn("review: review.rs has 1001 physical lines", output.getvalue())
        self.assertIn("review: at-hard-limit.rs has 1500 physical lines", output.getvalue())
        self.assertIn("error: too-large.rs has 1501 physical lines", output.getvalue())

    def test_review_range_does_not_fail_the_check(self) -> None:
        output = StringIO()

        with redirect_stdout(output):
            exit_code = check.report([check.SourceSize(Path("review.rs"), 1_001)])

        self.assertEqual(exit_code, 0)
        self.assertIn("review: review.rs has 1001 physical lines", output.getvalue())
        self.assertIn("Rust file-size check passed", output.getvalue())


if __name__ == "__main__":
    unittest.main()
