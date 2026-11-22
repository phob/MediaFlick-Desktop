import importlib.util
import random
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("prepare-release.py")


def release_toml(version, channel):
    return f'version = "{version}"\nchannel = "{channel}"\n'


SCHEMA_SOURCE = "src/library/db/migrate.rs"


def schema_source(version):
    return f"pub(super) const FIRST_RELEASED_SCHEMA: i32 = 23;\n\npub const SCHEMA_VERSION: i32 = {version};\n"


def frozen_schema(version):
    return f"fixtures/library-db/released/schema-{version}.sql"


RELEASE_FILES = {
    "CHANGELOG.md": "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- Upcoming feature.\n",
    "Cargo.toml": '[package]\nname = "mediaflick-desktop"\nversion = "0.1.6"\n',
    "Cargo.lock": '[[package]]\nname = "mediaflick-desktop"\nversion = "0.1.6"\n',
    "release.toml": release_toml("0.1.6", "latest"),
    SCHEMA_SOURCE: schema_source(23),
    frozen_schema(23): "CREATE TABLE credentials (id INTEGER PRIMARY KEY);\n",
}


OLDER_RELEASE = "\n## [0.1.6] - 2026-07-16\n\n### Fixed\n\n- Old fix.\n"
PREPARED_VERSION = 'version = "0.2.0"'


def write_release_files(root, files=RELEASE_FILES):
    for name, contents in files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(contents)


def run_script(root, *arguments):
    return subprocess.run([sys.executable, str(SCRIPT), *arguments], cwd=root, capture_output=True, text=True)


def read_files(root):
    return {path.relative_to(root).as_posix(): path.read_text() for path in root.rglob("*") if path.is_file() and ".git" not in path.relative_to(root).parts}


def read_release(root):
    return tomllib.loads((root / "release.toml").read_text())


def git(root, *arguments):
    subprocess.run(["git", "-c", "user.name=test", "-c", "user.email=test@example.invalid", *arguments], cwd=root, check=True, capture_output=True)


def commit_base(root, files):
    write_release_files(root, files)
    git(root, "init", "-q")
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "base")


def outputs(stdout):
    return dict(line.split("=", 1) for line in stdout.splitlines())


def load_script():
    spec = importlib.util.spec_from_file_location("prepare_release", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def prepared_files(version, channel="prerelease", base=RELEASE_FILES):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        write_release_files(root, base)
        result = run_script(root, "--version", version, "--date", "2026-11-15", "--notes-out", "notes.md")
        if result.returncode != 0:
            raise AssertionError(result.stderr)
        files = {name: (root / name).read_text() for name in base}
    return files | {"release.toml": release_toml(version, channel)}


class PrepareReleaseTests(unittest.TestCase):
    def test_test_draft_preserves_changelog_and_versions_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root)
            subprocess.run([sys.executable, str(SCRIPT), "--version", "0.2.0-test.1", "--test-draft", "--notes-out", "notes.md"], cwd=root, check=True, capture_output=True)
            self.assertEqual((root / "CHANGELOG.md").read_text(), RELEASE_FILES["CHANGELOG.md"])
            self.assertEqual((root / "release.toml").read_text(), RELEASE_FILES["release.toml"])
            for name in ("Cargo.toml", "Cargo.lock"):
                self.assertIn('version = "0.2.0-test.1"', (root / name).read_text())
            self.assertTrue((root / "notes.md").is_file())

    def test_test_draft_rejects_stable_version_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root)
            result = subprocess.run([sys.executable, str(SCRIPT), "--version", "1.0.0", "--test-draft", "--notes-out", "notes.md"], cwd=root, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(read_files(root), RELEASE_FILES)

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
            "release.toml left at the old version": prepared_files("0.2.0") | {"release.toml": RELEASE_FILES["release.toml"]},
            "release.toml missing": {name: text for name, text in prepared_files("0.2.0").items() if name != "release.toml"},
            "version already flipped to latest": prepared_files("0.2.0", "latest"),
        }
        for name, files in unprepared.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                write_release_files(root, files)
                result = run_script(root, "--version", "0.2.0", "--check", "--notes-out", "notes.md")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("0.2.0", result.stderr)
                self.assertEqual(read_files(root), files)

    def test_check_requires_the_published_schema_to_be_frozen(self):
        prepared = prepared_files("0.2.0")
        for name, files in {
            "no frozen schema": {path: text for path, text in prepared.items() if path != frozen_schema(23)},
            "a newer schema than the frozen one": prepared | {SCHEMA_SOURCE: schema_source(24)},
        }.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                write_release_files(root, files)
                result = run_script(root, "--version", "0.2.0", "--check", "--notes-out", "notes.md")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("schema", result.stderr)
                self.assertIn("fixtures/library-db/released/schema-", result.stderr)
                self.assertEqual(read_files(root), files)

    def test_check_accepts_a_newer_schema_once_it_is_frozen(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = prepared_files("0.2.0") | {SCHEMA_SOURCE: schema_source(24), frozen_schema(24): "-- 24\n"}
            write_release_files(root, files)
            result = run_script(root, "--version", "0.2.0", "--check", "--notes-out", "notes.md")
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_check_cannot_be_combined_with_a_test_draft(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root)
            result = run_script(root, "--version", "0.2.0-test.1", "--check", "--test-draft", "--notes-out", "notes.md")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(read_files(root), RELEASE_FILES)


class ReleaseFileTests(unittest.TestCase):

    def prepare(self, base, version):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_release_files(root, base)
            result = run_script(root, "--version", version, "--date", "2026-11-15", "--notes-out", "notes.md")
            return result, read_files(root)

    def test_prepare_writes_the_new_version_as_a_prerelease(self):
        result, files = self.prepare(RELEASE_FILES, "v0.2.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(tomllib.loads(files["release.toml"]), {"version": "0.2.0", "channel": "prerelease"})

    def test_prepare_moves_forward_by_number_not_by_text(self):
        base = {name: text.replace("0.1.6", "0.9.0") for name, text in RELEASE_FILES.items()}
        result, files = self.prepare(base, "0.10.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(tomllib.loads(files["release.toml"]), {"version": "0.10.0", "channel": "prerelease"})

    def test_prepare_refuses_before_writing_anything(self):
        refused = {
            "the version main is already at": (RELEASE_FILES, "0.1.6"),
            "an older version": (RELEASE_FILES, "0.1.5"),
            "a version 0.9.0 sorts after as text": ({name: text.replace("0.1.6", "0.10.0") for name, text in RELEASE_FILES.items()}, "0.9.0"),
            "a suffixed version": (RELEASE_FILES, "0.2.0-rc.1"),
        }
        for name, (base, version) in refused.items():
            with self.subTest(name):
                result, files = self.prepare(base, version)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(files, base)

    def plan(self, base, merged, since="HEAD"):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            commit_base(root, base)
            for name in base:
                if name not in merged:
                    (root / name).unlink()
            write_release_files(root, merged)
            return run_script(root, "--transition", since)

    def assert_action(self, result, action, version):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(outputs(result.stdout), {"action": action, "version": version})

    def test_a_merge_that_leaves_release_toml_alone_releases_nothing(self):
        prerelease = prepared_files("0.2.0")
        for name, (base, merged) in {
            "Latest, nothing changed": (RELEASE_FILES, RELEASE_FILES),
            "prerelease published, another PR merges": (prerelease, prerelease | {"CHANGELOG.md": prerelease["CHANGELOG.md"] + "\n<!-- edit -->\n"}),
            "Latest, changelog edited": (RELEASE_FILES, RELEASE_FILES | {"CHANGELOG.md": RELEASE_FILES["CHANGELOG.md"] + "- Another.\n"}),
        }.items():
            with self.subTest(name):
                self.assert_action(self.plan(base, merged), "none", tomllib.loads(merged["release.toml"])["version"])

    def test_a_release_pr_publishes_a_prerelease(self):
        self.assert_action(self.plan(RELEASE_FILES, prepared_files("0.2.0")), "prerelease", "0.2.0")

    def test_a_release_pr_without_its_frozen_schema_publishes_nothing(self):
        merged = {path: text for path, text in prepared_files("0.2.0").items() if path != frozen_schema(23)}
        result = self.plan(RELEASE_FILES, merged)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertNotIn("action=", result.stdout)
        self.assertIn(frozen_schema(23), result.stderr)

    def test_a_pr_that_changes_only_the_channel_makes_it_latest(self):
        prerelease = prepared_files("0.2.0")
        self.assert_action(self.plan(prerelease, prerelease | {"release.toml": release_toml("0.2.0", "latest")}), "latest", "0.2.0")

    def test_introducing_release_toml_releases_nothing(self):
        without = {name: text for name, text in RELEASE_FILES.items() if name != "release.toml"}
        self.assert_action(self.plan(without, RELEASE_FILES), "none", "0.1.6")

    def test_compares_against_where_the_change_left_base(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prerelease = prepared_files("0.2.0")
            commit_base(root, prerelease)
            git(root, "branch", "-M", "main")
            git(root, "checkout", "-q", "-b", "feature")
            write_release_files(root, {"README.md": "feature\n"})
            git(root, "add", "-A")
            git(root, "commit", "-q", "-m", "feature")
            git(root, "checkout", "-q", "main")
            write_release_files(root, {"release.toml": release_toml("0.2.0", "latest")})
            git(root, "commit", "-q", "-am", "flip")
            git(root, "checkout", "-q", "feature")
            self.assert_action(run_script(root, "--transition", "main"), "none", "0.2.0")

    def test_rejected_changes(self):
        prerelease = prepared_files("0.2.0")
        latest = prepared_files("0.2.0", "latest")
        rejected = {
            "new version straight to latest": (RELEASE_FILES, latest),
            "latest back to prerelease": (latest, prerelease),
            "older version": (prerelease, prepared_files("0.1.5", base=RELEASE_FILES | {name: text.replace("0.1.6", "0.1.4") for name, text in RELEASE_FILES.items() if name != "CHANGELOG.md"})),
            "release.toml bumped without the release": (RELEASE_FILES, RELEASE_FILES | {"release.toml": release_toml("0.2.0", "prerelease")}),
            "Cargo.toml bumped without release.toml": (RELEASE_FILES, prerelease | {"release.toml": RELEASE_FILES["release.toml"]}),
            "changelog not promoted": (RELEASE_FILES, prerelease | {"CHANGELOG.md": RELEASE_FILES["CHANGELOG.md"]}),
            "release.toml deleted": (RELEASE_FILES, {name: text for name, text in RELEASE_FILES.items() if name != "release.toml"}),
            "unknown channel": (RELEASE_FILES, RELEASE_FILES | {"release.toml": release_toml("0.1.6", "stable")}),
            "channel missing": (RELEASE_FILES, RELEASE_FILES | {"release.toml": 'version = "0.1.6"\n'}),
            "unknown key": (RELEASE_FILES, RELEASE_FILES | {"release.toml": RELEASE_FILES["release.toml"] + 'draft = true\n'}),
            "version with a v": (RELEASE_FILES, RELEASE_FILES | {"release.toml": release_toml("v0.1.6", "latest")}),
            "not TOML": (RELEASE_FILES, RELEASE_FILES | {"release.toml": "version = 0.1.6\n"}),
        }
        for name, (base, merged) in rejected.items():
            with self.subTest(name):
                result = self.plan(base, merged)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertNotIn("action=", result.stdout)

    def test_an_unknown_base_is_an_error_not_a_missing_file(self):
        result = self.plan(RELEASE_FILES, RELEASE_FILES, since="no-such-revision")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("action=", result.stdout)

    def test_transitions_follow_the_rules_for_every_pair(self):
        script = load_script()
        generator = random.Random(20261115)

        def number(version):
            return tuple(int(part) for part in version.split("."))

        def expected(before, after):
            if before is None or before == after:
                return "none"
            if number(after.version) > number(before.version):
                return "prerelease" if after.channel == "prerelease" else None
            if after.version == before.version and before.channel == "prerelease":
                return "latest"
            return None

        def release():
            version = ".".join(str(generator.choice((0, 1, 2, 9, 10, 11))) for _ in range(3))
            return script.Release(version=version, channel=generator.choice(("prerelease", "latest")))

        for _ in range(2000):
            before, after = (None if generator.random() < 0.05 else release()), release()
            if generator.random() < 0.3 and before is not None:
                after = script.Release(version=before.version, channel=after.channel)
            want = expected(before, after)
            with self.subTest(before=before, after=after):
                if want is None:
                    with self.assertRaises(SystemExit):
                        script.transition(before, after)
                else:
                    self.assertEqual(script.transition(before, after), want)


if __name__ == "__main__":
    unittest.main()
