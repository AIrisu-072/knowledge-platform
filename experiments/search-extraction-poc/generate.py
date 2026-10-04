#!/usr/bin/env python3
"""Deterministic, stdlib-only synthetic fixture generator; no customer bytes."""
import hashlib
import io
import json
import pathlib
import stat
import struct
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent
FIX = ROOT / "fixtures"
FIX.mkdir(exist_ok=True)
ROWS = []
XML = '<?xml version="1.0" encoding="UTF-8"?>'
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
S = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'


def unit(kind, text, locator, part_ordinal=0):
    return dict(kind=kind, text=text, locator=locator, part_ordinal=part_ordinal)


def put(id, fmt, raw, expected_units, coverage="Supported", reasons=None, *, limits=None, known_omissions=None):
    name = f"{id}.{fmt}"
    (FIX / name).write_bytes(raw)
    row = dict(id=id, format=fmt, file=f"fixtures/{name}",
               sha256=hashlib.sha256(raw).hexdigest(), origin="synthetic:generate.py",
               license="CC0-1.0", expected_units=expected_units,
               coverage=coverage, reasons=reasons or [], limits=limits or {})
    if known_omissions is not None:
        row['known_omissions'] = known_omissions
    ROWS.append(row)


def archive(parts):
    out = io.BytesIO()
    with zipfile.ZipFile(out, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        for name, data in parts:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            z.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=6)
    return out.getvalue()


def content_types(kinds):
    items = ''.join(f'<Override PartName="/{name}" ContentType="{typ}"/>' for name, typ in kinds)
    return (XML + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>' + items + '</Types>').encode()


def docx():
    body = (f'<w:document xmlns:w="{W}"><w:body>'
            '<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>東京</w:t></w:r></w:p>'
            '<w:p><w:r><w:t>ＡＢＣが、</w:t></w:r><w:r><w:t>同文。</w:t></w:r></w:p>'
            '<w:tbl><w:tr><w:tc><w:p><w:r><w:t>表一</w:t></w:r></w:p>'
            '<w:tbl><w:tr><w:tc><w:p><w:r><w:t>内側</w:t></w:r></w:p></w:tc></w:tr></w:tbl>'
            '</w:tc></w:tr></w:tbl></w:body></w:document>').encode()
    parts = [('[Content_Types].xml', content_types([('word/document.xml', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml')])),
             ('_rels/.rels', (XML + f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>').encode()),
             ('word/document.xml', XML.encode() + body)]
    return archive(parts)


def xlsx(macro=False, cache=False, *, formula=True, formula_text='B2', cache_value='10'):
    workbook = (XML + f'<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="第一" sheetId="1" r:id="rId1"/><sheet name="第二" sheetId="2" r:id="rId2"/></sheets></workbook>').encode()
    rels = (XML + f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{R}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>').encode()
    formula_xml = f'<f>{formula_text}</f>' if formula else ''
    value_xml = f'<v>{cache_value}</v>' if cache or not formula else ''
    first = (XML + f'<worksheet xmlns="{S}"><sheetData><row r="2"><c r="B2" t="inlineStr"><is><t>東京</t></is></c><c r="D2" t="inlineStr"><is><t>ＡＢＣ</t></is></c></row><row r="4"><c r="C4">{formula_xml}{value_xml}</c></row></sheetData></worksheet>').encode()
    second = (XML + f'<worksheet xmlns="{S}"><sheetData><row r="7"><c r="E7" t="inlineStr"><is><t>同文。</t></is></c></row></sheetData></worksheet>').encode()
    kind = 'application/vnd.ms-excel.sheet.macroEnabled.main+xml' if macro else 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml'
    parts = [('[Content_Types].xml', content_types([('xl/workbook.xml', kind), ('xl/worksheets/sheet1.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml'), ('xl/worksheets/sheet2.xml', 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml')])),
             ('_rels/.rels', (XML + f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/officeDocument" Target="xl/workbook.xml"/></Relationships>').encode()),
             ('xl/workbook.xml', workbook), ('xl/_rels/workbook.xml.rels', rels),
             ('xl/worksheets/sheet1.xml', first), ('xl/worksheets/sheet2.xml', second)]
    if macro:
        parts.append(('xl/vbaProject.bin', b'FAKE-SYNTHETIC-VBA-NOT-EXECUTABLE'))
    return archive(parts)


def pptx():
    presentation = (XML + f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="rId1"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>').encode()
    rels = (XML + f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/slide" Target="slides/slide1.xml"/><Relationship Id="rId2" Type="{R}/slide" Target="slides/slide2.xml"/></Relationships>').encode()
    sp = lambda text: f'<p:sp><p:txBody><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>'
    table = ('<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr>'
             '<a:tc><a:txBody><a:p><a:r><a:t>表セル</a:t></a:r></a:p></a:txBody></a:tc>'
             '</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>')
    first = (XML + f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{sp("東京")}<p:grpSp>{sp("群の文")}</p:grpSp>{table}</p:spTree></p:cSld></p:sld>').encode()
    second = (XML + f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{sp("同文。")} </p:spTree></p:cSld></p:sld>').encode()
    return archive([('[Content_Types].xml', content_types([('ppt/presentation.xml', 'application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml')])),
                    ('_rels/.rels', (XML + f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/officeDocument" Target="ppt/presentation.xml"/></Relationships>').encode()),
                    ('ppt/presentation.xml', presentation), ('ppt/_rels/presentation.xml.rels', rels),
                    ('ppt/slides/slide1.xml', first), ('ppt/slides/slide2.xml', second)])


def pdf(pages, invisible=False):
    # Type-0 Japanese CID font with a synthetic ToUnicode map, no external font binary.
    cmap = ('/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n'
            '/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n'
            '/CMapName /Synthetic def /CMapType 2 def\n'
            '1 begincodespacerange <0000> <FFFF> endcodespacerange\n'
            '6 beginbfchar <0001> <6771> <0002> <4EAC> <0003> <5927> <0004> <962A> <0005> <0041> <0006> <0042> endbfchar\n'
            'endcmap CMapName currentdict /CMap defineresource pop end end').encode()
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'']
    kids = []
    for i, page_text in enumerate(pages):
        page_no = len(objects) + 1
        content_no = page_no + 1
        kids.append(f'{page_no} 0 R')
        image_resources = ' /XObject << /Im0 0 0 R >>' if not page_text else ''
        objects.append(f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 0 0 R >>{image_resources} >> /Contents {content_no} 0 R >>'.encode())
        content = (f'BT {"3 Tr " if invisible else ""}/F1 18 Tf {"0 1 -1 0 220 50" if i else "1 0 0 1 30 200"} Tm <{page_text}> Tj ET').encode() if page_text else b'q 100 0 0 100 30 30 cm /Im0 Do Q'
        objects.append(b'<< /Length ' + str(len(content)).encode() + b' >>\nstream\n' + content + b'\nendstream')
    font_no = len(objects) + 1
    cid_no = font_no + 1
    cmap_no = font_no + 2
    objects.append(f'<< /Type /Font /Subtype /Type0 /BaseFont /HeiseiKakuGo-W5 /Encoding /Identity-H /DescendantFonts [{cid_no} 0 R] /ToUnicode {cmap_no} 0 R >>'.encode())
    objects.append(b'<< /Type /Font /Subtype /CIDFontType0 /BaseFont /HeiseiKakuGo-W5 /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 6 >> /DW 1000 >>')
    objects.append(b'<< /Length ' + str(len(cmap)).encode() + b' >>\nstream\n' + cmap + b'\nendstream')
    if any(not text for text in pages):
        image_no = len(objects) + 1
        objects.append(b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\n\x80\nendstream')
        objects = [o.replace(b'/Im0 0 0 R', f'/Im0 {image_no} 0 R'.encode()) for o in objects]
    objects[1] = f'<< /Type /Pages /Kids [{" ".join(kids)}] /Count {len(kids)} >>'.encode()
    objects = [o.replace(b'/F1 0 0 R', f'/F1 {font_no} 0 R'.encode()) for o in objects]
    out = bytearray(b'%PDF-1.7\n%\xE2\xE3\xCF\xD3\n')
    offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(out)); out += f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n'
    xref = len(out)
    out += f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode()
    for off in offsets[1:]: out += f'{off:010d} 00000 n \n'.encode()
    out += f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
    return bytes(out)


def D(steps): return {'Docx': {'steps': steps}}
def B(i): return {'BodyBlock': i}
def Rw(i): return {'Row': i}
def C(i): return {'Cell': i}
def CB(i): return {'CellBlock': i}
def Sh(s, r, c): return {'Spreadsheet': {'sheet_ordinal': s, 'row': r, 'col': c}}
def Ppt(s, path, slot): return {'Pptx': {'slide_ordinal': s, 'shape_path': path, 'text_slot': slot}}
def Pdf(p, a, b): return {'Pdf': {'page_index': p, 'char_start': a, 'char_end': b}}
def T(a, b): return {'Text': {'line_start': a, 'line_end': b}}
def Csv(r, f): return {'Csv': {'record': r, 'field': f}}
def Html(path): return {'Html': {'text_node_path': path}}
def Arc(members, inner): return {'Archive': {'members': members, 'inner': inner}}


doc_units = [unit('Heading', '東京', D([B(0)])), unit('Paragraph', 'ＡＢＣが、同文。', D([B(1)])),
             unit('TableCell', '表一', D([B(2), Rw(0), C(0), CB(0)])),
             unit('TableCell', '内側', D([B(2), Rw(0), C(0), CB(1), Rw(0), C(0), CB(0)]))]
put('docx-main', 'docx', docx(), doc_units)
with zipfile.ZipFile(io.BytesIO(docx())) as source:
    doc_parts = [(n, source.read(n)) for n in source.namelist()]
put('docx-header-omitted', 'docx', archive(doc_parts + [('word/header1.xml', XML.encode() + f'<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>ヘッダー</w:t></w:r></w:p></w:hdr>'.encode())]),
    doc_units, 'Partial', ['UnsupportedStructure'],
    known_omissions=[dict(member_chain=[], package_path='word/header1.xml', physical_child_path=[], reason='UnsupportedStructure')])
spoof_parts = [(n, data.replace(b'wordprocessingml.document.main+xml', b'spreadsheetml.sheet.main+xml') if n == '[Content_Types].xml' else data) for n, data in doc_parts]
put('docx-spoof-content-type', 'docx', archive(spoof_parts), [], 'Unsupported', ['UnsupportedStructure'])
deep_document = XML + f'<w:document xmlns:w="{W}"><w:body>' + '<w:nest>' * 260 + '</w:nest>' * 260 + '</w:body></w:document>'
put('docx-xml-depth', 'docx', archive([(n, deep_document.encode() if n == 'word/document.xml' else data) for n, data in doc_parts]),
    [], 'Unsupported', ['ResourceLimit'])
unknown_document = next(data for n, data in doc_parts if n == 'word/document.xml').replace(
    b'</w:body>', b'<w:altChunk xmlns:r="urn:synthetic" r:id="rIdX"/></w:body>')
put('docx-unknown-block', 'docx', archive([(n, unknown_document if n == 'word/document.xml' else data) for n, data in doc_parts]),
    [], 'Unsupported', ['UnsupportedStructure'])
sheet_units = [unit('Cell', '東京', Sh(0, 1, 1)), unit('Cell', 'ＡＢＣ', Sh(0, 1, 3)), unit('Cell', '同文。', Sh(1, 6, 4))]
def formula_omission(reason, members=None):
    return dict(member_chain=members or [], package_path='xl/worksheets/sheet1.xml',
                physical_child_path=[1, 0], reason=reason)
def package_omission(path, members=None):
    return dict(member_chain=members or [], package_path=path,
                physical_child_path=[], reason='UnsupportedStructure')

missing_formula = formula_omission('MissingFormulaCache')
unverified_formula = formula_omission('UnsupportedStructure')
put('xlsx-cache-gap', 'xlsx', xlsx(), sheet_units, 'Partial', ['MissingFormulaCache'],
    known_omissions=[missing_formula])
# B2 contains 東京 while C4 formula = B2 still carries the contradictory old cached 10.
put('xlsx-cache-complete', 'xlsx', xlsx(cache=True), sheet_units, 'Partial', ['UnsupportedStructure'],
    known_omissions=[unverified_formula])
# A plausible cached value is also unverified: the reader never evaluates formulas.
put('xlsx-cache-freshness-unknown', 'xlsx', xlsx(cache=True, formula_text='2+2', cache_value='4'),
    sheet_units, 'Partial', ['UnsupportedStructure'], known_omissions=[unverified_formula])
with zipfile.ZipFile(io.BytesIO(xlsx(cache=True))) as source:
    formula_only_parts = [(name, source.read(name)) for name in source.namelist()]
formula_only_parts = [(name, data.replace(b'<c r="B2" t="inlineStr"><is><t>\xe6\x9d\xb1\xe4\xba\xac</t></is></c>', b'')
                       .replace(b'<c r="D2" t="inlineStr"><is><t>\xef\xbc\xa1\xef\xbc\xa2\xef\xbc\xa3</t></is></c>', b'')
                       .replace(b'<c r="E7" t="inlineStr"><is><t>\xe5\x90\x8c\xe6\x96\x87\xe3\x80\x82</t></is></c>', b'')
                       if name.startswith('xl/worksheets/') else data)
                      for name, data in formula_only_parts]
put('xlsx-formula-only', 'xlsx', archive(formula_only_parts), [], 'Unsupported', ['UnsupportedStructure'])
with zipfile.ZipFile(io.BytesIO(xlsx())) as source:
    hidden_parts = [(n, source.read(n).replace(b'name="\xe7\xac\xac\xe4\xba\x8c"', b'name="\xe7\xac\xac\xe4\xba\x8c" state="hidden"')
                     if n == 'xl/workbook.xml' else source.read(n)) for n in source.namelist()]
put('xlsx-hidden-sheet', 'xlsx', archive(hidden_parts), sheet_units[:2], 'Partial',
    ['MissingFormulaCache', 'UnsupportedStructure'],
    known_omissions=[missing_formula, package_omission('xl/worksheets/sheet2.xml')])
with zipfile.ZipFile(io.BytesIO(xlsx(cache=True))) as source:
    shared_parts = [(n, source.read(n)) for n in source.namelist()]
shared_parts = [(n, data.replace(b'<c r="B2" t="inlineStr"><is><t>\xe6\x9d\xb1\xe4\xba\xac</t></is></c>', b'<c r="B2" t="s"><v>0</v></c>')
                 if n == 'xl/worksheets/sheet1.xml' else data) for n, data in shared_parts]
shared_override = b'<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>'
shared_parts = [(n, data.replace(b'</Types>', shared_override + b'</Types>') if n == '[Content_Types].xml' else data) for n, data in shared_parts]
shared_xml = XML.encode() + f'<sst xmlns="{S}"><si><r><t>東</t></r><r><t>京</t></r></si></sst>'.encode()
put('xlsx-shared-string-rich', 'xlsx', archive(shared_parts + [('xl/sharedStrings.xml', shared_xml)]),
    sheet_units, 'Partial', ['UnsupportedStructure'], known_omissions=[unverified_formula])
with zipfile.ZipFile(io.BytesIO(xlsx(cache=True, formula=False))) as source:
    formula_free_parts = [(n, source.read(n)) for n in source.namelist()]
formula_free_parts = [(n, data.replace(b'<c r="B2" t="inlineStr"><is><t>\xe6\x9d\xb1\xe4\xba\xac</t></is></c>',
                                       b'<c r="B2" t="s"><v>0</v></c>')
                       if n == 'xl/worksheets/sheet1.xml' else data) for n, data in formula_free_parts]
formula_free_parts = [(n, data.replace(b'</Types>', shared_override + b'</Types>')
                       if n == '[Content_Types].xml' else data) for n, data in formula_free_parts]
put('xlsx-shared-string-rich-formula-free', 'xlsx', archive(formula_free_parts + [('xl/sharedStrings.xml', shared_xml)]),
    sheet_units[:2] + [unit('Cell', '10', Sh(0, 3, 2))] + sheet_units[2:])
bad_shared = [(n, data.replace(b'<c r="B2" t="s"><v>0</v></c>', b'<c r="B2" t="s"><v>9</v></c>')
               if n == 'xl/worksheets/sheet1.xml' else data) for n, data in shared_parts]
put('xlsx-shared-index-bad', 'xlsx', archive(bad_shared + [('xl/sharedStrings.xml', shared_xml)]),
    [], 'FailedPermanent', ['CorruptDocument'])
put('xlsm-macro-cache-gap', 'xlsm', xlsx(True), sheet_units, 'Partial',
    ['MissingFormulaCache', 'UnsupportedStructure'],
    known_omissions=[missing_formula, package_omission('xl/vbaProject.bin')])
ppt_units = [unit('ShapeText', '東京', Ppt(0, [0], {'ShapeParagraph': {'paragraph': 0}})),
             unit('ShapeText', '群の文', Ppt(0, [1, 0], {'ShapeParagraph': {'paragraph': 0}})),
             unit('TableCell', '表セル', Ppt(0, [2], {'TableCellParagraph': {'row': 0, 'col': 0, 'paragraph': 0}})),
             unit('ShapeText', '同文。', Ppt(1, [0], {'ShapeParagraph': {'paragraph': 0}}))]
put('pptx-group-table', 'pptx', pptx(), ppt_units)
with zipfile.ZipFile(io.BytesIO(pptx())) as source:
    ppt_parts = [(n, source.read(n)) for n in source.namelist()]
put('pptx-notes-omitted', 'pptx', archive(ppt_parts + [('ppt/notesSlides/notesSlide1.xml', b'<notes>synthetic omitted notes</notes>')]),
    ppt_units, 'Partial', ['UnsupportedStructure'],
    known_omissions=[package_omission('ppt/notesSlides/notesSlide1.xml')])
put('pdf-single-page', 'pdf', pdf(['00010002']), [unit('PageText', '東京', Pdf(0, 0, 2))])
put('pdf-two-page-vertical', 'pdf', pdf(['00010002', '00030004']),
    [], 'Unsupported', ['AmbiguousReadingOrder'])
put('pdf-image-only', 'pdf', pdf(['']), [], 'Unsupported', ['RequiresOcr'])
put('pdf-invisible-text', 'pdf', pdf(['00010002'], invisible=True), [], 'Unsupported', ['UnsupportedStructure'])
put('pdf-corrupt', 'pdf', b'%PDF-1.7\ninvalid\n', [], 'FailedPermanent', ['CorruptDocument'])
put('text-bom', 'text', b'\xef\xbb\xbf' + '東京\r\nＡＢＣが、\r\n'.encode(),
    [unit('Line', '東京', T(0, 1)), unit('Line', 'ＡＢＣが、', T(1, 2))])
put('text-cp932', 'text', '大阪\r\n同文。'.encode('cp932'),
    [unit('Line', '大阪', T(0, 1)), unit('Line', '同文。', T(1, 2))], limits={'charset': 'windows-31j'})
put('text-invalid', 'text', b'\xff\xff', [], 'Unsupported', ['UnsupportedEncoding'])
put('csv-quoted-newline', 'csv', '見出し,値\r\n東京,"大阪\r\n京都"\r\n'.encode(),
    [unit('Field', '見出し', Csv(0, 0)), unit('Field', '値', Csv(0, 1)), unit('Field', '東京', Csv(1, 0)), unit('Field', '大阪\n京都', Csv(1, 1))])
put('csv-ambiguous', 'csv', 'a;b,c\n'.encode(), [], 'Unsupported', ['UnsupportedDialect'])
put('csv-big-field', 'csv', ('x' * 1048577).encode(), [], 'Unsupported', ['ResourceLimit'])
html = '<!doctype html><html><head><title>題</title></head><body><h1>東京</h1><!--skip--><p>ＡＢＣが、<span>同文。</span></p><p hidden>非表示</p><script>危険</script></body></html>'
put('html-hidden-script', 'html', html.encode(),
    [], 'Unsupported', ['DynamicVisibility'])
put('html-deep', 'html', ('<html><body>' + '<div>' * 260 + '深い' + '</div>' * 260 + '</body></html>').encode(),
    [], 'Unsupported', ['ResourceLimit'])
put('html-simple', 'html', '<!doctype html><html><body><h1>大阪</h1><p>同文。</p></body></html>'.encode(),
    [unit('Heading', '大阪', Html([0, 0])), unit('Text', '同文。', Html([1, 0]))])
inner = archive([('a.txt', '東京\n'.encode()), ('b.csv', '見出し,大阪\n'.encode())])
put('zip-nested', 'zip', archive([('inner.zip', inner)]),
    [unit('Line', '東京', Arc(['inner.zip', 'a.txt'], T(0, 1))),
     unit('Field', '見出し', Arc(['inner.zip', 'b.csv'], Csv(0, 0))),
     unit('Field', '大阪', Arc(['inner.zip', 'b.csv'], Csv(0, 1)))])
def wrap(name, units):
    return [unit(x['kind'], x['text'], Arc([name], x['locator'])) for x in units]

html_simple_units = [unit('Heading', '大阪', Html([0, 0])), unit('Text', '同文。', Html([1, 0]))]
modern_parts = [('a.docx', docx()), ('b.xlsx', xlsx(cache=True)), ('c.pptx', pptx()),
                ('d.pdf', pdf(['00010002'])),
                ('e.html', '<!doctype html><html><body><h1>大阪</h1><p>同文。</p></body></html>'.encode()),
                ('f.txt', '東京\n'.encode()), ('g.csv', '見出し,大阪\n'.encode())]
modern_units = (wrap('a.docx', doc_units) + wrap('b.xlsx', sheet_units)
                + wrap('c.pptx', ppt_units) + wrap('d.pdf', [unit('PageText', '東京', Pdf(0, 0, 2))])
                + wrap('e.html', html_simple_units) + wrap('f.txt', [unit('Line', '東京', T(0, 1))])
                + wrap('g.csv', [unit('Field', '見出し', Csv(0, 0)), unit('Field', '大阪', Csv(0, 1))]))
put('zip-modern-leaves', 'zip', archive(modern_parts), modern_units,
    'Partial', ['UnsupportedStructure'],
    known_omissions=[formula_omission('UnsupportedStructure', ['b.xlsx'])])
put('zip-partial-xlsm', 'zip', archive(modern_parts + [('h.xlsm', xlsx(True))]),
    modern_units + wrap('h.xlsm', sheet_units), 'Partial', ['UnsupportedStructure', 'MissingFormulaCache'],
    known_omissions=[formula_omission('UnsupportedStructure', ['b.xlsx']),
                     formula_omission('MissingFormulaCache', ['h.xlsm']),
                     package_omission('xl/vbaProject.bin', ['h.xlsm'])])
put('zip-nested-pdf', 'zip', archive([('inner.zip', archive([('a.pdf', pdf(['00010002']))]))]),
    [unit('PageText', '東京', Arc(['inner.zip', 'a.pdf'], Pdf(0, 0, 2)))])
put('zip-nested-formula', 'zip', archive([('inner.zip', archive([('a.txt', b'plain'), ('b.xlsx', xlsx(cache=True))]))]),
    [unit('Line', 'plain', Arc(['inner.zip', 'a.txt'], T(0, 1)))]
    + [unit(x['kind'], x['text'], Arc(['inner.zip', 'b.xlsx'], x['locator'])) for x in sheet_units],
    'Partial', ['UnsupportedStructure'],
    known_omissions=[formula_omission('UnsupportedStructure', ['inner.zip', 'b.xlsx'])])
put('zip-bad-inner', 'zip', archive([('a.txt', b'good'), ('b.xlsx', archive(bad_shared + [('xl/sharedStrings.xml', shared_xml)]))]),
    [], 'FailedPermanent', ['CorruptDocument'])
put('zip-path', 'zip', archive([('../escape.txt', b'x')]), [], 'Unsupported', ['UnsupportedStructure'])
put('zip-duplicate', 'zip', archive([('a.txt', b'x'), ('a.txt', b'y')]), [], 'Unsupported', ['UnsupportedStructure'])
put('zip-nfc-collision', 'zip', archive([('é.txt', b'x'), ('e\u0301.txt', b'y')]), [], 'Unsupported', ['UnsupportedStructure'])
symlink = io.BytesIO()
with zipfile.ZipFile(symlink, 'w') as z:
    info = zipfile.ZipInfo('link.txt'); info.create_system = 3; info.external_attr = (stat.S_IFLNK | 0o777) << 16
    z.writestr(info, 'target')
put('zip-symlink', 'zip', symlink.getvalue(), [], 'Unsupported', ['UnsupportedStructure'])
put('zip-bomb', 'zip', archive([('huge.txt', b'x' * (1024 * 1024))]), [], 'Unsupported', ['ResourceLimit'])
# Change only the central-directory general-purpose bit to make an encrypted fixture.
encrypted = bytearray(archive([('secret.txt', b'abc')]))
cen = encrypted.find(b'PK\x01\x02')
encrypted[cen + 8:cen + 10] = struct.pack('<H', 1)
put('zip-encrypted', 'zip', bytes(encrypted), [], 'Unsupported', ['Encrypted'])
for ext in ('doc', 'xls', 'ppt'):
    put('legacy-magic', ext, bytes.fromhex('D0CF11E0A1B11AE1') + b'SYNTHETIC', [], 'Unsupported', ['UnsupportedFormat'])

(ROOT / 'manifest.json').write_text(json.dumps(ROWS, ensure_ascii=False, indent=2) + '\n')
(ROOT / 'expected.json').write_text(json.dumps({r['id'] + '.' + r['format']: r['expected_units'] for r in ROWS}, ensure_ascii=False, indent=2) + '\n')
print(f'{len(ROWS)} synthetic fixtures')
