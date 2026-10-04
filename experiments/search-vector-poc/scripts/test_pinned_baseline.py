import tempfile
import unittest
from pathlib import Path

from pinned_baseline import digest_package_tree


class SnapshotTest(unittest.TestCase):
    def test_content_change_and_new_rust_input_change_tree_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory)
            (package / "src").mkdir()
            (package / "Cargo.toml").write_text("[package]\nname='example'\nversion='0.0.0'\n")
            source = package / "src" / "lib.rs"
            source.write_text("pub fn value() -> u8 { 1 }\n")
            original = digest_package_tree(package, None)
            source.write_text("pub fn value() -> u8 { 2 }\n")
            changed = digest_package_tree(package, None)
            self.assertNotEqual(original["tree_sha256"], changed["tree_sha256"])
            (package / "build.rs").write_text("fn main() {}\n")
            added = digest_package_tree(package, None)
            self.assertNotEqual(changed["tree_sha256"], added["tree_sha256"])


if __name__ == "__main__":
    unittest.main()
