"""Independent stdlib-only OPC relationship oracle for the v2 admission corpus."""

import io
import pathlib
import posixpath
import sys
import xml.etree.ElementTree as ET
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from oracle import inspect as old_content_oracle  # fixed raw-text/locator oracle only

REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
R = '{' + REL + '}'


def fail(coverage, reason):
    return [], coverage, [reason], []


def target_path(source, target):
    if not target or target.startswith('/') or '://' in target or '\\' in target:
        raise ValueError('UnsupportedStructure')
    path = posixpath.normpath(posixpath.join(posixpath.dirname(source), target))
    if path.startswith('../') or path == '..' or path.startswith('/'):
        raise ValueError('UnsupportedStructure')
    return path


def content_types(package):
    types = ET.fromstring(package.read('[Content_Types].xml'))
    return {node.get('PartName', '').lstrip('/'): node.get('ContentType') for node in types}


def relationships(package, rels_path):
    root = ET.fromstring(package.read(rels_path))
    seen = set()
    result = []
    for rel in root:
        rid = rel.get('Id')
        if not rid or rid in seen:
            raise ValueError('UnsupportedStructure')
        seen.add(rid)
        if rel.get('TargetMode') == 'External':
            raise ValueError('UnsupportedStructure')
        result.append((rid, rel.get('Type'), rel.get('Target')))
    return result


def inspect_docx(raw):
    with zipfile.ZipFile(io.BytesIO(raw)) as package:
        types = content_types(package)
        document = ET.fromstring(package.read('word/document.xml'))
        refs = list(document.iter(W + 'headerReference'))
        if not refs:
            return fail('Unsupported', 'UnsupportedStructure')
        try:
            rels = {rid: (kind, target) for rid, kind, target in relationships(package, 'word/_rels/document.xml.rels')}
        except KeyError:
            return fail('FailedPermanent', 'CorruptDocument')
        omissions = []
        for ref in refs:
            kind, target = rels.get(ref.get(R + 'id'), (None, None))
            if kind != REL + '/header':
                return fail('Unsupported', 'UnsupportedStructure')
            try:
                path = target_path('word/document.xml', target)
            except ValueError:
                return fail('Unsupported', 'UnsupportedStructure')
            if types.get(path) != 'application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml':
                return fail('Unsupported', 'UnsupportedStructure')
            try:
                header = ET.fromstring(package.read(path))
            except (KeyError, ET.ParseError):
                return fail('FailedPermanent', 'CorruptDocument')
            if header.tag != W + 'hdr':
                return fail('FailedPermanent', 'CorruptDocument')
            omissions.append(path)
    units, coverage, reasons = old_content_oracle('docx', raw, {})
    assert coverage == 'Partial' and reasons == ['UnsupportedStructure']
    return units, coverage, reasons, omissions


def inspect_pptx(raw):
    with zipfile.ZipFile(io.BytesIO(raw)) as package:
        types = content_types(package)
        try:
            rels = relationships(package, 'ppt/slides/_rels/slide1.xml.rels')
        except KeyError:
            return fail('FailedPermanent', 'CorruptDocument')
        notes = [(rid, target) for rid, kind, target in rels if kind == REL + '/notesSlide']
        if len(notes) != 1:
            return fail('Unsupported', 'UnsupportedStructure')
        try:
            path = target_path('ppt/slides/slide1.xml', notes[0][1])
        except ValueError:
            return fail('Unsupported', 'UnsupportedStructure')
        if types.get(path) != 'application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml':
            return fail('Unsupported', 'UnsupportedStructure')
        try:
            root = ET.fromstring(package.read(path))
        except (KeyError, ET.ParseError):
            return fail('FailedPermanent', 'CorruptDocument')
        if root.tag != '{http://schemas.openxmlformats.org/presentationml/2006/main}notes':
            return fail('FailedPermanent', 'CorruptDocument')
    units, coverage, reasons = old_content_oracle('pptx', raw, {})
    assert coverage == 'Partial' and reasons == ['UnsupportedStructure']
    return units, coverage, reasons, [path]


def inspect_xlsx(raw):
    with zipfile.ZipFile(io.BytesIO(raw)) as package:
        types = content_types(package)
        rels = relationships(package, 'xl/_rels/workbook.xml.rels')
        shared = [(rid, target) for rid, kind, target in rels if kind == REL + '/sharedStrings']
        if len(shared) != 1:
            return fail('Unsupported', 'UnsupportedStructure')
        try:
            path = target_path('xl/workbook.xml', shared[0][1])
        except ValueError:
            return fail('Unsupported', 'UnsupportedStructure')
        if path != 'xl/sharedStrings.xml' or types.get(path) != 'application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml':
            return fail('Unsupported', 'UnsupportedStructure')
        try:
            root = ET.fromstring(package.read(path))
        except (KeyError, ET.ParseError):
            return fail('FailedPermanent', 'CorruptDocument')
        if root.tag != '{http://schemas.openxmlformats.org/spreadsheetml/2006/main}sst':
            return fail('FailedPermanent', 'CorruptDocument')
    units, coverage, reasons = old_content_oracle('xlsx', raw, {})
    assert coverage == 'Supported' and reasons == []
    return units, coverage, reasons, []
