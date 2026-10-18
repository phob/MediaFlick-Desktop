import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check_guards.py")
WIRE = "src/shell/cef/api/wire.rs"
COUNT = "src/shell/cef/api/wire/untyped_count.rs"
WIRE_SOURCE = """\
wire_types! {
    AboutInfo,
}

#[derive(Serialize, TS)]
struct ErrorBody {
    error: String,
}

impl ApiResponse {
    /// A response the UI reads without a generated type.
    pub(super) fn untyped(value: impl Serialize) -> Self {
        Self::json(200, value)
    }
}

#[cfg(test)]
const UNTYPED_RESPONSES: usize = 1; // guard-ratchet

#[cfg(test)]
mod guards;
#[cfg(test)]
mod untyped_count;
"""
COUNT_SOURCE = """\
use super::UNTYPED_RESPONSES;

/// Calls to the untyped escape, by name.
#[derive(Default)]
struct UntypedCalls {
    responses: usize,
}

impl Visit<'_> for UntypedCalls {
    fn visit_path(&mut self, path: &syn::Path) {
        if path.segments.last().is_some_and(|last| last.ident == "untyped") {
            self.responses += 1;
        }
    }
}

#[test]
fn untyped_responses_only_shrink() {
    let responses = count();
    assert!(responses <= UNTYPED_RESPONSES, "new untyped API bodies");
}
"""


class GuardCheck:
    """A throwaway repository with the wire sources on `main` and a branch to change them on."""

    def __init__(self, root: Path):
        self.root = root
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.email", "guard@example.invalid")
        self.git("config", "user.name", "guard")
        self.write(WIRE, WIRE_SOURCE)
        self.write(COUNT, COUNT_SOURCE)
        self.commit("base")
        self.git("checkout", "-q", "-b", "change")

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, path: str, text: str) -> None:
        (self.root / path).parent.mkdir(parents=True, exist_ok=True)
        (self.root / path).write_text(text)

    def edit(self, path: str, old: str, new: str) -> None:
        text = (self.root / path).read_text()
        if old not in text:
            raise AssertionError(f"{old!r} is not in {path}")
        self.write(path, text.replace(old, new, 1))

    def commit(self, message: str) -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", message)

    def run(self, labelled: bool = False) -> subprocess.CompletedProcess[str]:
        self.commit("change")
        env = {key: value for key, value in os.environ.items() if key != "MEDIAFLICK_GUARDS_CHANGED"}
        if labelled:
            env["MEDIAFLICK_GUARDS_CHANGED"] = "1"
        return subprocess.run([sys.executable, str(SCRIPT), "main"], cwd=self.root, env=env, capture_output=True, text=True)


class WireRatchetCountTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = GuardCheck(Path(directory.name))

    def assertBlocked(self, *expected: str):
        result = self.repo.run()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        for line in expected:
            self.assertIn(line, result.stdout, result.stderr)

    def assertAccepted(self):
        result = self.repo.run()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_changed_count_fails(self):
        self.repo.edit(COUNT, "self.responses += 1;", "self.responses += 0;")
        self.assertBlocked(f"Wire ratchet count ({COUNT}): removed `self.responses += 1;`")

    def test_a_loosened_comparison_fails(self):
        self.repo.edit(COUNT, "responses <= UNTYPED_RESPONSES", "responses <= UNTYPED_RESPONSES + 5")
        self.assertBlocked(f"Wire ratchet count ({COUNT}): added `assert!(responses <= UNTYPED_RESPONSES + 5")

    def test_an_ignored_count_test_fails(self):
        self.repo.edit(COUNT, "#[test]\n", "#[test]\n#[ignore]\n")
        self.assertBlocked(f"Wire ratchet count ({COUNT}): added `#[ignore]`")

    def test_a_dropped_test_attribute_fails(self):
        self.repo.edit(COUNT, "#[test]\n", "")
        self.assertBlocked(f"Wire ratchet count ({COUNT}): removed `#[test]`")

    def test_a_deleted_count_file_fails(self):
        (self.repo.root / COUNT).unlink()
        self.assertBlocked(f"Wire ratchet count ({COUNT}): removed `fn untyped_responses_only_shrink() {{`")

    def test_a_moved_count_file_fails(self):
        self.repo.git("mv", COUNT, "src/shell/cef/api/wire/elsewhere.rs")
        self.assertBlocked(f"Wire ratchet count ({COUNT}): removed `fn untyped_responses_only_shrink() {{`")

    def test_a_removed_count_module_fails(self):
        self.repo.edit(WIRE, "#[cfg(test)]\nmod untyped_count;\n", "")
        self.assertBlocked(f"Wire ratchet count ({WIRE}): removed `mod untyped_count;`")

    def test_a_disabled_count_module_fails(self):
        self.repo.edit(WIRE, "#[cfg(test)]\nmod untyped_count;", "#[cfg(any())]\nmod untyped_count;")
        self.assertBlocked(f"Wire ratchet count ({WIRE}): added `#[cfg(any())]`", f"Wire ratchet count ({WIRE}): removed `#[cfg(test)]`")

    def test_a_multiline_attribute_on_the_count_module_fails(self):
        self.repo.edit(WIRE, "#[cfg(test)]\nmod untyped_count;", "#[cfg(test)]\n#[cfg(not(\n    test\n))]\nmod untyped_count;")
        self.assertBlocked(f"Wire ratchet count ({WIRE}): added `#[cfg(not( test ))]`")

    def test_an_attribute_on_the_module_line_fails(self):
        self.repo.edit(WIRE, "mod untyped_count;", '#[path = "empty.rs"] mod untyped_count;')
        self.assertBlocked(f"Wire ratchet count ({WIRE}): added `#[path = \"empty.rs\"]`")

    def test_a_renamed_untyped_escape_fails(self):
        self.repo.edit(WIRE, "fn untyped(", "fn relay_json(")
        self.assertBlocked(f"Wire ratchet count ({WIRE}): removed `pub(super) fn untyped(value: impl Serialize) -> Self {{`")

    def test_the_label_accepts_a_changed_count(self):
        self.repo.edit(COUNT, "self.responses += 1;", "self.responses += 0;")
        result = self.repo.run(labelled=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("accepted by the guards-changed label", result.stdout)

    def test_a_reworded_comment_in_the_count_file_passes(self):
        self.repo.edit(COUNT, "/// Calls to the untyped escape, by name.", "/// Every call to the untyped escape, counted by name.")
        self.assertAccepted()

    def test_a_new_wire_type_passes(self):
        self.repo.edit(WIRE, "    AboutInfo,\n", "    AboutInfo,\n    AddProfileBody,\n")
        self.assertAccepted()

    def test_a_new_error_field_with_attributes_passes(self):
        self.repo.edit(WIRE, "    error: String,\n", '    error: String,\n    #[serde(skip_serializing_if = "Option::is_none")]\n    expired: Option<bool>,\n')
        self.assertAccepted()

    def test_a_lowered_ratchet_passes(self):
        self.repo.edit(WIRE, "UNTYPED_RESPONSES: usize = 1;", "UNTYPED_RESPONSES: usize = 0;")
        self.assertAccepted()


if __name__ == "__main__":
    unittest.main()
