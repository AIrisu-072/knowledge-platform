"""Next Q01 gate: actual Office package relationship topology.

Separate from the closed 45-case bounded candidate suite. Run explicitly with
P1_QUALIFIER_BIN set to a freshly built Linux binary; these RED cases must not
be counted as admitted until independent raw oracles and fixes are reviewed.
"""

import hashlib
import io
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile

from office_oracle import inspect_docx, inspect_pptx, inspect_xlsx


ROOT = pathlib.Path(__file__).resolve().parents[1]
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_REL = "http://schemas.openxmlformats.org/package/2006/relationships"


def edit_zip(raw, changes=None, additions=None, missing=()):
    changes = changes or {}
    additions = additions or {}
    out = io.BytesIO()
    with zipfile.ZipFile(io.BytesIO(raw)) as src, zipfile.ZipFile(out, "w") as dst:
        for entry in src.infolist():
            if entry.filename not in missing:
                data = src.read(entry)
                if entry.filename in changes:
                    data = changes[entry.filename](data)
                dst.writestr(entry, data)
        for name, data in additions.items():
            dst.writestr(name, data)
    return out.getvalue()


def before_end(data, tag, insertion):
    close = ("</" + tag + ">").encode()
    assert data.count(close) == 1
    return data.replace(close, insertion + close)


def related_docx_header(raw):
    header = b'<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>header text</w:t></w:r></w:p></w:hdr>'
    override = b'<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>'
    rels = f'<Relationships xmlns="{PKG_REL}"><Relationship Id="rIdH" Type="{REL}/header" Target="header1.xml"/></Relationships>'.encode()
    section = f'<w:sectPr><w:headerReference xmlns:r="{REL}" w:type="default" r:id="rIdH"/></w:sectPr>'.encode()
    return edit_zip(raw, {
        "[Content_Types].xml": lambda data: before_end(data, "Types", override),
        "word/document.xml": lambda data: before_end(data, "w:body", section),
    }, {"word/_rels/document.xml.rels": rels, "word/header1.xml": header})


def related_pptx_notes(raw):
    notes = b'<p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree/></p:cSld></p:notes>'
    override = b'<Override PartName="/ppt/notesSlides/notesSlide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>'
    rels = f'<Relationships xmlns="{PKG_REL}"><Relationship Id="rIdN" Type="{REL}/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>'.encode()
    return edit_zip(raw, {"[Content_Types].xml": lambda data: before_end(data, "Types", override)},
                    {"ppt/slides/_rels/slide1.xml.rels": rels, "ppt/notesSlides/notesSlide1.xml": notes})


def related_xlsx_shared_strings(raw, target="sharedStrings.xml"):
    rel = f'<Relationship Id="rIdShared" Type="{REL}/sharedStrings" Target="{target}"/>'.encode()
    return edit_zip(raw, {"xl/_rels/workbook.xml.rels": lambda data: before_end(data, "Relationships", rel)})


@unittest.skipUnless(os.environ.get("P1_QUALIFIER_BIN"), "set P1_QUALIFIER_BIN")
class OfficeRelationshipAdmission(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = {row["id"]: row for row in json.loads((ROOT / "manifest.json").read_text())}

    def run_case(self, fixture_id, raw):
        source = self.rows[fixture_id]
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory)
            (path / "fixture").write_bytes(raw)
            row = dict(source, file="fixture", sha256=hashlib.sha256(raw).hexdigest())
            process = subprocess.run(
                [os.environ["P1_QUALIFIER_BIN"]],
                input=json.dumps({"rows": [row], "root": str(path)}).encode(),
                capture_output=True, check=False,
            )
            self.assertIn(process.returncode, (0, 1), process.stderr.decode(errors="replace"))
            return json.loads(process.stdout)

    def test_referenced_docx_header_is_located_partial(self):
        raw = related_docx_header((ROOT / self.rows["docx-main"]["file"]).read_bytes())
        with zipfile.ZipFile(io.BytesIO(raw)) as z:
            self.assertIn("word/header1.xml", z.namelist())
            self.assertEqual(ET.fromstring(z.read("word/_rels/document.xml.rels"))[0].get("Target"), "header1.xml")
            self.assertEqual(ET.fromstring(z.read("word/header1.xml")).tag.rsplit("}", 1)[-1], "hdr")
        units, coverage, reasons, omissions = inspect_docx(raw)
        self.assertEqual((units, coverage, reasons, omissions),
                         (self.rows["docx-main"]["expected_units"], "Partial", ["UnsupportedStructure"], ["word/header1.xml"]))
        result = self.run_case("docx-main", raw)
        self.assertEqual(result["coverage"], "Partial", result)
        self.assertIn("word/header1.xml", [o["package_path"] for o in result["known_omissions"]], result)

    def test_missing_referenced_docx_header_cannot_claim_partial(self):
        raw = related_docx_header((ROOT / self.rows["docx-main"]["file"]).read_bytes())
        raw = edit_zip(raw, missing={"word/header1.xml"})
        self.assertEqual(inspect_docx(raw), ([], "FailedPermanent", ["CorruptDocument"], []))
        result = self.run_case("docx-main", raw)
        self.assertNotEqual(result["coverage"], "Partial", result)
        self.assertEqual(result["matched_units"], 0, result)

    def test_wrong_header_content_type_cannot_claim_partial(self):
        raw = related_docx_header((ROOT / self.rows["docx-main"]["file"]).read_bytes())
        raw = edit_zip(raw, {"[Content_Types].xml": lambda data: data.replace(
            b'PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"',
            b'PartName="/word/header1.xml" ContentType="application/x-synthetic-invalid+xml"')})
        self.assertEqual(inspect_docx(raw), ([], "Unsupported", ["UnsupportedStructure"], []))
        result = self.run_case("docx-main", raw)
        self.assertNotEqual(result["coverage"], "Partial", result)
        self.assertEqual(result["matched_units"], 0, result)

    def test_referenced_pptx_notes_are_located_partial(self):
        raw = related_pptx_notes((ROOT / self.rows["pptx-group-table"]["file"]).read_bytes())
        with zipfile.ZipFile(io.BytesIO(raw)) as z:
            self.assertIn("ppt/notesSlides/notesSlide1.xml", z.namelist())
            self.assertEqual(ET.fromstring(z.read("ppt/slides/_rels/slide1.xml.rels"))[0].get("Target"), "../notesSlides/notesSlide1.xml")
        units, coverage, reasons, omissions = inspect_pptx(raw)
        self.assertEqual((units, coverage, reasons, omissions),
                         (self.rows["pptx-group-table"]["expected_units"], "Partial", ["UnsupportedStructure"], ["ppt/notesSlides/notesSlide1.xml"]))
        result = self.run_case("pptx-group-table", raw)
        self.assertEqual(result["coverage"], "Partial", result)
        self.assertIn("ppt/notesSlides/notesSlide1.xml", [o["package_path"] for o in result["known_omissions"]], result)

    def test_missing_referenced_pptx_notes_cannot_claim_partial(self):
        raw = related_pptx_notes((ROOT / self.rows["pptx-group-table"]["file"]).read_bytes())
        raw = edit_zip(raw, missing={"ppt/notesSlides/notesSlide1.xml"})
        self.assertEqual(inspect_pptx(raw), ([], "FailedPermanent", ["CorruptDocument"], []))
        result = self.run_case("pptx-group-table", raw)
        self.assertNotEqual(result["coverage"], "Partial", result)
        self.assertEqual(result["matched_units"], 0, result)

    def test_wrong_notes_content_type_cannot_claim_partial(self):
        raw = related_pptx_notes((ROOT / self.rows["pptx-group-table"]["file"]).read_bytes())
        raw = edit_zip(raw, {"[Content_Types].xml": lambda data: data.replace(
            b'PartName="/ppt/notesSlides/notesSlide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"',
            b'PartName="/ppt/notesSlides/notesSlide1.xml" ContentType="application/x-synthetic-invalid+xml"')})
        self.assertEqual(inspect_pptx(raw), ([], "Unsupported", ["UnsupportedStructure"], []))
        result = self.run_case("pptx-group-table", raw)
        self.assertNotEqual(result["coverage"], "Partial", result)
        self.assertEqual(result["matched_units"], 0, result)

    def test_notes_relationship_cannot_escape_package(self):
        raw = related_pptx_notes((ROOT / self.rows["pptx-group-table"]["file"]).read_bytes())
        raw = edit_zip(raw, {"ppt/slides/_rels/slide1.xml.rels": lambda data: data.replace(
            b'Target="../notesSlides/notesSlide1.xml"', b'Target="../../../../escape.xml"')})
        self.assertEqual(inspect_pptx(raw), ([], "Unsupported", ["UnsupportedStructure"], []))
        result = self.run_case("pptx-group-table", raw)
        self.assertNotEqual(result["coverage"], "Partial", result)
        self.assertEqual(result["matched_units"], 0, result)

    def test_valid_shared_string_relationship_keeps_supported_cells(self):
        raw = related_xlsx_shared_strings((ROOT / self.rows["xlsx-shared-string-rich-formula-free"]["file"]).read_bytes())
        with zipfile.ZipFile(io.BytesIO(raw)) as z:
            self.assertIn("xl/sharedStrings.xml", z.namelist())
            self.assertEqual(ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))[-1].get("Target"), "sharedStrings.xml")
        units, coverage, reasons, omissions = inspect_xlsx(raw)
        self.assertEqual((units, coverage, reasons, omissions),
                         (self.rows["xlsx-shared-string-rich-formula-free"]["expected_units"], "Supported", [], []))
        result = self.run_case("xlsx-shared-string-rich-formula-free", raw)
        self.assertEqual(result["coverage"], "Supported", result)
        self.assertEqual(result["missed_units"], 0, result)

    def test_broken_shared_string_relationship_cannot_be_supported(self):
        raw = related_xlsx_shared_strings((ROOT / self.rows["xlsx-shared-string-rich-formula-free"]["file"]).read_bytes(),
                                          target="missing-shared-strings.xml")
        with zipfile.ZipFile(io.BytesIO(raw)) as z:
            self.assertIn("xl/sharedStrings.xml", z.namelist())
            self.assertNotIn("xl/missing-shared-strings.xml", z.namelist())
            self.assertEqual(ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))[-1].get("Target"), "missing-shared-strings.xml")
        self.assertEqual(inspect_xlsx(raw), ([], "Unsupported", ["UnsupportedStructure"], []))
        result = self.run_case("xlsx-shared-string-rich-formula-free", raw)
        self.assertNotEqual(result["coverage"], "Supported", result)
        self.assertEqual(result["matched_units"], 0, result)

    def test_wrong_shared_string_relationship_type_cannot_be_supported(self):
        raw = related_xlsx_shared_strings((ROOT / self.rows["xlsx-shared-string-rich-formula-free"]["file"]).read_bytes())
        raw = edit_zip(raw, {"xl/_rels/workbook.xml.rels": lambda data: data.replace(
            (REL + "/sharedStrings").encode(), (REL + "/syntheticWrongType").encode())})
        self.assertEqual(inspect_xlsx(raw), ([], "Unsupported", ["UnsupportedStructure"], []))
        result = self.run_case("xlsx-shared-string-rich-formula-free", raw)
        self.assertNotEqual(result["coverage"], "Supported", result)
        self.assertEqual(result["matched_units"], 0, result)


if __name__ == "__main__":
    unittest.main()
