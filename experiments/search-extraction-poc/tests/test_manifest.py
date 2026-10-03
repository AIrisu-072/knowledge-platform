import json
import pathlib
import unittest
import zipfile
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]


class ManifestContract(unittest.TestCase):
    def test_fixed_corpus_and_oracle(self):
        import sys

        sys.path.insert(0, str(ROOT))
        from run import verify_manifest

        rows = json.loads((ROOT / "manifest.json").read_text())
        self.assertGreaterEqual(len(rows), 20)
        self.assertEqual(
            set().union(*(set(row["format"] for row in rows),)),
            {"docx", "xlsx", "xlsm", "pptx", "pdf", "text", "csv", "html", "zip", "doc", "xls", "ppt"},
        )
        verify_manifest(rows)

    def test_formula_cache_never_supplies_positive_unit(self):
        rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}
        spreadsheet_ns = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
        cases = (
            ("xlsx-cache-gap", "B2", None, "MissingFormulaCache"),
            ("xlsx-cache-complete", "B2", "10", "UnsupportedStructure"),
            ("xlsx-cache-freshness-unknown", "2+2", "4", "UnsupportedStructure"),
        )
        for fixture_id, formula, cached_value, reason in cases:
            with self.subTest(fixture_id=fixture_id):
                row = rows[fixture_id]
                with zipfile.ZipFile(ROOT / row["file"]) as package:
                    xml = ET.fromstring(package.read("xl/worksheets/sheet1.xml"))
                formula_cell = next(cell for cell in xml.iter(spreadsheet_ns + "c") if cell.get("r") == "C4")
                self.assertEqual(formula_cell.findtext(spreadsheet_ns + "f"), formula)
                self.assertEqual(formula_cell.findtext(spreadsheet_ns + "v"), cached_value)
                if fixture_id == "xlsx-cache-complete":
                    source_cell = next(cell for cell in xml.iter(spreadsheet_ns + "c") if cell.get("r") == "B2")
                    self.assertEqual(source_cell.findtext(f"{spreadsheet_ns}is/{spreadsheet_ns}t"), "東京")
                self.assertNotIn(
                    {"Spreadsheet": {"sheet_ordinal": 0, "row": 3, "col": 2}},
                    [unit["locator"] for unit in row["expected_units"]],
                )
                self.assertEqual(row["coverage"], "Partial")
                omissions = row.get("known_omissions", [])
                self.assertEqual(len(omissions), 1)
                self.assertEqual(omissions[0]["package_path"], "xl/worksheets/sheet1.xml")
                self.assertEqual(omissions[0]["physical_child_path"], [1, 0])
                self.assertEqual(omissions[0]["reason"], reason)

    def test_formula_only_sheet_is_unsupported_without_units(self):
        rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}
        row = rows["xlsx-formula-only"]
        self.assertEqual((row["coverage"], row["expected_units"]), ("Unsupported", []))
        self.assertEqual(row["reasons"], ["UnsupportedStructure"])

    def test_rich_shared_string_without_formula_remains_supported(self):
        rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}
        row = rows["xlsx-shared-string-rich-formula-free"]
        self.assertEqual(row["coverage"], "Supported")
        self.assertEqual(row["known_omissions"] if "known_omissions" in row else [], [])
        self.assertIn("東京", [unit["text"] for unit in row["expected_units"]])

    def test_formula_cache_omission_survives_archive(self):
        rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}
        archive = rows["zip-modern-leaves"]
        self.assertEqual(archive["coverage"], "Partial")
        self.assertEqual(archive.get("known_omissions"), [{
            "member_chain": ["b.xlsx"],
            "package_path": "xl/worksheets/sheet1.xml",
            "physical_child_path": [1, 0],
            "reason": "UnsupportedStructure",
        }])
        self.assertNotIn(
            {"Archive": {"members": ["b.xlsx"], "inner": {"Spreadsheet": {"sheet_ordinal": 0, "row": 3, "col": 2}}}},
            [unit["locator"] for unit in archive["expected_units"]],
        )

    def test_formula_omission_preserves_nested_member_chain(self):
        rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}
        row = rows["zip-nested-formula"]
        self.assertEqual(row["coverage"], "Partial")
        self.assertEqual(row["known_omissions"][0]["member_chain"], ["inner.zip", "b.xlsx"])
        self.assertEqual(row["known_omissions"][0]["physical_child_path"], [1, 0])


if __name__ == "__main__":
    unittest.main()
