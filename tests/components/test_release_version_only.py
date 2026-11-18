"""Only exact release metadata can bypass a second full test suite."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
from release_version_only import release_version


MANIFEST = """[package]
name = "eska"
version.workspace = true

[workspace]
members = [".", "crates/*"]

[workspace.package]
version = "0.35.0"

[workspace.dependencies]
eska-test = { path = "crates/eska-test", version = "=0.35.0" }

[dependencies]
cli = { version = "1.2.3", features = ["std"] }
"""

LOCK = """version = 4

[[package]]
name = "cli"
version = "1.2.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "original"

[[package]]
name = "eska"
version = "0.35.0"
dependencies = [
 "cli",
 "eska-test 0.35.0",
]

[[package]]
name = "eska-test"
version = "0.35.0"

[[package]]
name = "eska-test"
version = "1.2.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "external"
"""

HEADER = "# История выпусков\n\n"
HISTORY = "## [0.35.0](previous) - 2026-11-18\n\n- Уже выпущено.\n"
ENTRY = (
    "## [0.35.1](https://github.com/odineska/eska/compare/v0.35.0...v0.35.1) - 2026-11-18\n"
    "\n- Описание нового выпуска.\n\n"
)


class VersionOnlyTests(unittest.TestCase):
    def setUp(self):
        self.changed = ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"]
        self.summary = ""
        self.previous = {
            "Cargo.toml": MANIFEST,
            "Cargo.lock": LOCK,
            "CHANGELOG.md": HEADER + HISTORY,
            "crates/eska-test/Cargo.toml": '[package]\nname = "eska-test"\nversion.workspace = true\n',
        }
        self.current = {
            "Cargo.toml": MANIFEST.replace("0.35.0", "0.35.1"),
            "Cargo.lock": LOCK.replace("0.35.0", "0.35.1"),
            "CHANGELOG.md": HEADER + ENTRY + HISTORY,
        }

    def git(self, operation, *args):
        if operation == "diff":
            return "\0".join(self.changed) + "\0" if args[0] == "--name-only" else self.summary
        if operation == "ls-tree":
            return "crates/eska-test/Cargo.toml\0crates/eska-test/src/lib.rs\0"
        revision, path = args[0].split(":", maxsplit=1)
        return (self.previous if revision == "previous" else self.current)[path]

    def version(self):
        return release_version("previous", "current", self.git)

    def test_patch_accepts_workspace_versions_and_versioned_local_reference(self):
        self.assertEqual(self.version(), "0.35.1")

    def test_patch_rejects_any_other_file_or_mode_change(self):
        self.changed.append("src/lib.rs")
        self.assertIsNone(self.version())
        self.changed.pop()
        self.summary = " mode change 100644 => 100755 Cargo.toml\n"
        self.assertIsNone(self.version())

    def test_patch_rejects_external_changes_and_inconsistent_version_pins(self):
        for filename, old, new in (
            ("Cargo.toml", 'version = "=0.35.1"', 'version = "=0.35.0"'),
            ("Cargo.toml", '["std"]', '["derive"]'),
            ("Cargo.toml", '"1.2.3"', '"1.2.4"'),
            ("Cargo.lock", 'checksum = "original"', 'checksum = "changed"'),
            ("Cargo.lock", 'version = "1.2.3"', 'version = "1.2.4"'),
            ("Cargo.lock", '"eska-test 0.35.1"', '"eska-test 0.35.0"'),
            ("Cargo.lock", ' "cli",\n', ""),
        ):
            with self.subTest(filename=filename, old=old):
                content = self.current[filename]
                self.current[filename] = content.replace(old, new, 1)
                self.assertIsNone(self.version())
                self.current[filename] = content

    def test_patch_rejects_rewriting_history_and_wrong_changelog_version(self):
        for content in (
            HEADER + ENTRY + HISTORY.replace("Уже", "Давно"),
            HEADER + ENTRY.replace("v0.35.0", "v0.34.0") + HISTORY,
            HEADER + ENTRY.replace("0.35.1", "0.35.2") + HISTORY,
            HEADER + ENTRY + "## Дополнительный релиз\n" + HISTORY,
        ):
            with self.subTest(content=content):
                self.current["CHANGELOG.md"] = content
                self.assertIsNone(self.version())


if __name__ == "__main__":
    unittest.main()
