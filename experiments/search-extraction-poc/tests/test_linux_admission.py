"""Executable Linux admission probes against the freshly built PoC binary.

Run with P1_QUALIFIER_BIN set to the absolute Linux binary path. The fixed
manifest suite remains usable without a Rust build; these tests are then
skipped rather than mistaken for admission evidence.
"""

import hashlib
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = os.environ.get("P1_QUALIFIER_BIN")
PDFIUM_LINUX_SHA256 = "f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64"


def add_zip_member(raw, name, data):
    out = io.BytesIO()
    with zipfile.ZipFile(io.BytesIO(raw)) as src, zipfile.ZipFile(out, "w") as dst:
        for member in src.infolist():
            dst.writestr(member, src.read(member))
        dst.writestr(name, data)
    return out.getvalue()


def replace_zip_member(raw, name, change):
    out = io.BytesIO()
    with zipfile.ZipFile(io.BytesIO(raw)) as src, zipfile.ZipFile(out, "w") as dst:
        for member in src.infolist():
            data = src.read(member)
            dst.writestr(member, change(data) if member.filename == name else data)
    return out.getvalue()


def remove_zip_member(raw, name):
    out = io.BytesIO()
    with zipfile.ZipFile(io.BytesIO(raw)) as src, zipfile.ZipFile(out, "w") as dst:
        for member in src.infolist():
            if member.filename != name:
                dst.writestr(member, src.read(member))
    return out.getvalue()


@unittest.skipUnless(BIN, "set P1_QUALIFIER_BIN to run Linux admission probes")
class LinuxAdmission(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}
        cls.binary = pathlib.Path(BIN)
        if not cls.binary.is_file():
            raise AssertionError(f"missing Linux qualifier: {cls.binary}")

    def inspect(self, fixture_id, raw=None):
        source = self.rows[fixture_id]
        if raw is None:
            raw = (ROOT / source["file"]).read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "fixture").write_bytes(raw)
            row = {
                "id": fixture_id,
                "format": source["format"],
                "file": "fixture",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "expected_units": source["expected_units"],
                "known_omissions": source.get("known_omissions", []),
                "coverage": source["coverage"],
                "reasons": source["reasons"],
                "limits": source["limits"],
            }
            process = subprocess.run(
                [str(self.binary)], input=json.dumps({"rows": [row], "root": str(root)}).encode(),
                capture_output=True, env=os.environ, check=False,
            )
            self.assertIn(process.returncode, (0, 1), process.stderr.decode(errors="replace"))
            return json.loads(process.stdout)

    def test_linux_pdfium_pin_matches_exact_installed_library(self):
        for fixture_id in ("pdf-single-page", "zip-nested-pdf"):
            with self.subTest(fixture_id=fixture_id):
                result = self.inspect(fixture_id)
                self.assertEqual(result["native_pin"], PDFIUM_LINUX_SHA256)
                self.assertTrue(result["qualified"], result)

    def test_binary_does_not_attest_an_external_scan_it_never_ran(self):
        result = self.inspect("text-bom")
        self.assertEqual(result["license_security"], "not-executed", result)

    def test_unlocated_pdf_order_and_html_visibility_gaps_are_not_partial(self):
        for fixture_id, reason in (("pdf-two-page-vertical", "AmbiguousReadingOrder"),
                                   ("html-hidden-script", "DynamicVisibility")):
            with self.subTest(fixture_id=fixture_id):
                result = self.inspect(fixture_id)
                self.assertEqual(result["coverage"], "Unsupported", result)
                self.assertEqual(result["reason"], [reason], result)
                self.assertEqual(result["matched_units"], 0, result)

    def test_docx_related_unread_story_cannot_claim_supported(self):
        raw = (ROOT / self.rows["docx-main"]["file"]).read_bytes()
        story = b'<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>hidden story</w:t></w:r></w:p></w:hdr>'
        # A relationship makes this a reader-visible package part, not junk ZIP data.
        raw = add_zip_member(raw, "word/_rels/document.xml.rels", b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/></Relationships>')
        raw = add_zip_member(raw, "word/header2.xml", story)
        result = self.inspect("docx-main", raw)
        self.assertNotEqual(result["coverage"], "Supported", result)
        if result["coverage"] == "Partial":
            self.assertTrue(result["known_omissions"], result)

    def test_xlsx_related_pivot_cannot_claim_supported(self):
        raw = (ROOT / self.rows["xlsx-shared-string-rich-formula-free"]["file"]).read_bytes()
        raw = add_zip_member(raw, "xl/worksheets/_rels/sheet1.xml.rels", b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdP" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable1.xml"/></Relationships>')
        raw = add_zip_member(raw, "xl/pivotTables/pivotTable1.xml", b'<pivotTableDefinition name="synthetic"/>')
        result = self.inspect("xlsx-shared-string-rich-formula-free", raw)
        self.assertNotEqual(result["coverage"], "Supported", result)
        if result["coverage"] == "Partial":
            self.assertTrue(result["known_omissions"], result)

    def test_unreferenced_office_part_cannot_claim_supported(self):
        cases = (
            ("xlsx-shared-string-rich-formula-free", "xl/worksheets/orphan.xml", b'<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>orphan</v></c></row></sheetData></worksheet>'),
            ("pptx-group-table", "ppt/slides/orphan.xml", b'<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>'),
        )
        for fixture_id, name, data in cases:
            with self.subTest(fixture_id=fixture_id):
                raw = (ROOT / self.rows[fixture_id]["file"]).read_bytes()
                result = self.inspect(fixture_id, add_zip_member(raw, name, data))
                self.assertNotEqual(result["coverage"], "Supported", result)

    def test_unlocated_pptx_shape_cannot_borrow_a_notes_omission(self):
        raw = (ROOT / self.rows["pptx-notes-omitted"]["file"]).read_bytes()
        raw = replace_zip_member(raw, "ppt/slides/slide1.xml", lambda data: data.replace(
            b"</p:spTree>", b"<p:pic/></p:spTree>"))
        result = self.inspect("pptx-notes-omitted", raw)
        self.assertEqual(result["coverage"], "Unsupported", result)
        self.assertEqual(result["reason"], ["UnsupportedStructure"], result)

    def test_known_package_omissions_are_physically_located(self):
        cases = (
            ("docx-header-omitted", "word/header1.xml"),
            ("pptx-notes-omitted", "ppt/notesSlides/notesSlide1.xml"),
            ("xlsm-macro-cache-gap", "xl/vbaProject.bin"),
            ("xlsx-hidden-sheet", "xl/worksheets/sheet2.xml"),
        )
        for fixture_id, package_path in cases:
            with self.subTest(fixture_id=fixture_id):
                result = self.inspect(fixture_id)
                if result["coverage"] == "Partial":
                    self.assertIn(package_path, [item["package_path"] for item in result["known_omissions"]], result)

    def test_hidden_sheet_omission_requires_existing_valid_declared_part(self):
        sys.path.insert(0, str(ROOT))
        from oracle import inspect as oracle_inspect

        raw = (ROOT / self.rows["xlsx-hidden-sheet"]["file"]).read_bytes()
        bad_type = replace_zip_member(raw, "[Content_Types].xml", lambda data: data.replace(
            b'PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"',
            b'PartName="/xl/worksheets/sheet2.xml" ContentType="application/x-synthetic-invalid+xml"'))
        cases = (
            ("missing", remove_zip_member(raw, "xl/worksheets/sheet2.xml"), "FailedPermanent", "CorruptDocument"),
            ("malformed", replace_zip_member(raw, "xl/worksheets/sheet2.xml", lambda _: b"<worksheet"), "FailedPermanent", "CorruptDocument"),
            ("wrong-content-type", bad_type, "Unsupported", "UnsupportedStructure"),
        )
        for label, mutated, coverage, reason in cases:
            with self.subTest(label=label):
                oracle_units, oracle_coverage, oracle_reasons = oracle_inspect("xlsx", mutated, {})
                self.assertEqual((oracle_units, oracle_coverage, oracle_reasons), ([], coverage, [reason]))
                result = self.inspect("xlsx-hidden-sheet", mutated)
                self.assertEqual(result["coverage"], coverage, result)
                self.assertEqual(result["reason"], [reason], result)
                self.assertEqual(result["matched_units"], 0, result)


if __name__ == "__main__":
    unittest.main()
