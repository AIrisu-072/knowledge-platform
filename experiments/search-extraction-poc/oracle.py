"""Independent stdlib native-location oracle over the original fixture bytes."""
import csv
import io
import pathlib
import re
import stat
import unicodedata
import xml.etree.ElementTree as ET
import zipfile
from html.parser import HTMLParser

W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
S = '{http://schemas.openxmlformats.org/spreadsheetml/2006/main}'
A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
P = '{http://schemas.openxmlformats.org/presentationml/2006/main}'
R = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'


def u(kind, text, locator):
    return dict(kind=kind, text=unicodedata.normalize('NFC', text.replace('\r\n', '\n').replace('\r', '\n')), locator=locator, part_ordinal=0)


def readzip(raw):
    z = zipfile.ZipFile(io.BytesIO(raw))
    names = z.namelist()
    if len(names) != len(set(names)) or len(names) > 20000:
        raise ValueError('UnsupportedStructure')
    if len(set(unicodedata.normalize('NFC', n) for n in names)) != len(names):
        raise ValueError('UnsupportedStructure')
    for i in z.infolist():
        p = pathlib.PurePosixPath(i.filename)
        if i.flag_bits & 1:
            raise ValueError('Encrypted')
        if p.is_absolute() or '..' in p.parts or '\\' in i.filename or stat.S_ISLNK(i.external_attr >> 16):
            raise ValueError('UnsupportedStructure')
        if i.file_size > 65536 and i.compress_size and i.file_size / i.compress_size > 100:
            raise ValueError('ResourceLimit')
    return z


def docx(raw):
    z = readzip(raw)
    ct = ET.fromstring(z.read('[Content_Types].xml'))
    main = next(n for n in ct if n.get('PartName') == '/word/document.xml')
    if main.get('ContentType') != 'application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml':
        return [], 'Unsupported', ['UnsupportedStructure']
    body = ET.fromstring(z.read('word/document.xml')).find(W + 'body')
    def max_depth(n): return 1 + max((max_depth(c) for c in n), default=0)
    if max_depth(body) > 256: return [], 'Unsupported', ['ResourceLimit']
    if any(n.tag == W + 'altChunk' for n in body.iter()): return [], 'Unsupported', ['UnsupportedStructure']
    found = []

    def blocks(parent, steps, cell=False):
        for index, child in enumerate(parent):
            loc = steps + [{'BodyBlock' if not steps else 'CellBlock': index}]
            if child.tag == W + 'p':
                text = ''.join(n.text or '' for n in child.iter(W + 't'))
                style = child.find(f'{W}pPr/{W}pStyle')
                kind = 'TableCell' if cell else ('Heading' if style is not None and style.get(W + 'val', '').startswith('Heading') else 'Paragraph')
                if text: found.append(u(kind, text, {'Docx': {'steps': loc}}))
            elif child.tag == W + 'tbl':
                for r, row in enumerate(child.findall(W + 'tr')):
                    for c, tc in enumerate(row.findall(W + 'tc')):
                        blocks(tc, loc + [{'Row': r}, {'Cell': c}], True)
    blocks(body, [])
    omitted = any(n.startswith(('word/header', 'word/footer', 'word/footnotes')) for n in z.namelist())
    return found, 'Partial' if omitted else 'Supported', ['UnsupportedStructure'] if omitted else []


def spreadsheet(raw, macro):
    z = readzip(raw)
    ct = ET.fromstring(z.read('[Content_Types].xml'))
    wb = ET.fromstring(z.read('xl/workbook.xml'))
    rel = ET.fromstring(z.read('xl/_rels/workbook.xml.rels'))
    targets = {r.get('Id'): r.get('Target') for r in rel}
    shared = []
    if 'xl/sharedStrings.xml' in z.namelist():
        ct = ET.fromstring(z.read('[Content_Types].xml'))
        declaration = next((n.get('ContentType') for n in ct if n.get('PartName') == '/xl/sharedStrings.xml'), None)
        if declaration != 'application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml':
            return [], 'Unsupported', ['UnsupportedStructure']
        strings = ET.fromstring(z.read('xl/sharedStrings.xml'))
        for si in strings.findall(S + 'si'):
            if len(si) == 1 and si[0].tag == S + 't': shared.append(si[0].text or '')
            elif all(n.tag == S + 'r' for n in si):
                shared.append(''.join((run.find(S + 't').text or '') for run in si))
            else: return [], 'Unsupported', ['UnsupportedStructure']
    found = []; reasons = []
    for si, sheet in enumerate(wb.find(S + 'sheets')):
        target = targets[sheet.get(R + 'id')]
        name = 'xl/' + target.lstrip('/') if not target.startswith('/xl/') else target.lstrip('/')
        declared = next((n.get('ContentType') for n in ct if n.get('PartName') == '/' + name), None)
        if declared != 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml':
            return [], 'Unsupported', ['UnsupportedStructure']
        try:
            root = ET.fromstring(z.read(name))
        except (KeyError, ET.ParseError):
            return [], 'FailedPermanent', ['CorruptDocument']
        if root.tag != S + 'worksheet' or root.find(S + 'sheetData') is None:
            return [], 'FailedPermanent', ['CorruptDocument']
        if sheet.get('state') not in (None, 'visible'):
            if 'UnsupportedStructure' not in reasons: reasons.append('UnsupportedStructure')
            continue
        for c in root.iter(S + 'c'):
            if c.get('t') not in (None, 'n', 'inlineStr', 's'):
                return [], 'Unsupported', ['UnsupportedStructure']
            cell = c.get('r')
            m = re.fullmatch(r'([A-Z]+)([1-9][0-9]*)', cell)
            col = 0
            for letter in m.group(1): col = col * 26 + ord(letter) - 64
            row = int(m.group(2)) - 1; col -= 1
            value = c.find(S + 'v')
            if c.find(S + 'f') is not None:
                reason = 'MissingFormulaCache' if value is None or not value.text else 'UnsupportedStructure'
                if reason not in reasons: reasons.append(reason)
                continue
            if c.get('t') == 's':
                try: text = shared[int(value.text)]
                except (IndexError, ValueError, TypeError, AttributeError): return [], 'FailedPermanent', ['CorruptDocument']
            elif c.get('t') == 'inlineStr': text = ''.join(t.text or '' for t in c.iter(S + 't'))
            else: text = value.text or '' if value is not None else ''
            if text: found.append(u('Cell', text, {'Spreadsheet': {'sheet_ordinal': si, 'row': row, 'col': col}}))
    if macro: reasons.append('UnsupportedStructure')
    if reasons and not found:
        return [], 'Unsupported', [reasons[0]]
    return found, 'Partial' if reasons else 'Supported', reasons


def known_omissions(fmt, raw, members=()):
    """Locate supported-corpus omissions from raw package bytes, independently."""
    if fmt == 'zip':
        z = readzip(raw)
        omissions = []
        for name in sorted(z.namelist()):
            if name.endswith('.zip'):
                omissions.extend(known_omissions('zip', z.read(name), (*members, name)))
            else:
                fmt_inner = name.rsplit('.', 1)[-1]
                if fmt_inner in ('docx', 'xlsx', 'xlsm', 'pptx'):
                    omissions.extend(known_omissions(fmt_inner, z.read(name), (*members, name)))
        return omissions
    if fmt not in ('docx', 'xlsx', 'xlsm', 'pptx'):
        return []
    z = readzip(raw)
    if fmt == 'docx':
        return [dict(member_chain=list(members), package_path=name, physical_child_path=[], reason='UnsupportedStructure')
                for name in sorted(z.namelist()) if name.startswith(('word/header', 'word/footer', 'word/footnotes'))]
    package = [dict(member_chain=list(members), package_path=name, physical_child_path=[], reason='UnsupportedStructure')
               for name in sorted(z.namelist())
               if (fmt == 'xlsm' and name == 'xl/vbaProject.bin')
               or name.startswith(('xl/charts/', 'xl/drawings/', 'xl/comments'))
               or (fmt == 'pptx' and name.startswith(('ppt/notesSlides/', 'ppt/charts/')))]
    if fmt == 'pptx':
        return package
    workbook = ET.fromstring(z.read('xl/workbook.xml'))
    content_types = ET.fromstring(z.read('[Content_Types].xml'))
    relationships = ET.fromstring(z.read('xl/_rels/workbook.xml.rels'))
    targets = {rel.get('Id'): rel.get('Target') for rel in relationships}
    omissions = []
    for sheet in workbook.find(S + 'sheets'):
        target = targets[sheet.get(R + 'id')]
        path = 'xl/' + target.lstrip('/') if not target.startswith('/xl/') else target.lstrip('/')
        if sheet.get('state') not in (None, 'visible'):
            declared = next((n.get('ContentType') for n in content_types if n.get('PartName') == '/' + path), None)
            if declared != 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml':
                raise ValueError('UnsupportedStructure')
            try:
                root = ET.fromstring(z.read(path))
            except (KeyError, ET.ParseError) as exc:
                raise ValueError('CorruptDocument') from exc
            if root.tag != S + 'worksheet' or root.find(S + 'sheetData') is None:
                raise ValueError('CorruptDocument')
            omissions.append(dict(member_chain=list(members), package_path=path,
                                  physical_child_path=[], reason='UnsupportedStructure'))
            continue
        worksheet = ET.fromstring(z.read(path))
        data = worksheet.find(S + 'sheetData')
        for row_index, row in enumerate(data):
            for cell_index, cell in enumerate(row):
                if cell.find(S + 'f') is None:
                    continue
                cache = cell.find(S + 'v')
                omissions.append({
                    'member_chain': list(members),
                    'package_path': path,
                    'physical_child_path': [row_index, cell_index],
                    'reason': 'MissingFormulaCache' if cache is None or not cache.text else 'UnsupportedStructure',
                })
    return omissions + package


def pptx(raw):
    z = readzip(raw)
    prs = ET.fromstring(z.read('ppt/presentation.xml'))
    rel = ET.fromstring(z.read('ppt/_rels/presentation.xml.rels'))
    targets = {r.get('Id'): r.get('Target') for r in rel}
    found = []

    def shapes(tree, slide, prefix):
        for i, shape in enumerate(tree):
            path = prefix + [i]
            if shape.tag == P + 'grpSp':
                shapes(shape, slide, path)
            elif shape.tag == P + 'sp':
                tx = shape.find(P + 'txBody')
                if tx is not None:
                    for pi, para in enumerate(tx.findall(A + 'p')):
                        text = ''.join(t.text or '' for t in para.iter(A + 't'))
                        if text: found.append(u('ShapeText', text, {'Pptx': {'slide_ordinal': slide, 'shape_path': path, 'text_slot': {'ShapeParagraph': {'paragraph': pi}}}}))
            elif shape.tag == P + 'graphicFrame':
                for ri, row in enumerate(shape.iter(A + 'tr')):
                    for ci, cell in enumerate(row.findall(A + 'tc')):
                        for pi, para in enumerate(cell.iter(A + 'p')):
                            text = ''.join(t.text or '' for t in para.iter(A + 't'))
                            if text: found.append(u('TableCell', text, {'Pptx': {'slide_ordinal': slide, 'shape_path': path, 'text_slot': {'TableCellParagraph': {'row': ri, 'col': ci, 'paragraph': pi}}}}))
    for si, slide in enumerate(prs.find(P + 'sldIdLst')):
        target = targets[slide.get(R + 'id')]
        name = 'ppt/' + target.lstrip('/') if not target.startswith('/ppt/') else target.lstrip('/')
        root = ET.fromstring(z.read(name))
        shapes(root.find(f'{P}cSld/{P}spTree'), si, [])
    omitted = any(n.startswith(('ppt/notesSlides/', 'ppt/charts/')) for n in z.namelist())
    return found, 'Partial' if omitted else 'Supported', ['UnsupportedStructure'] if omitted else []


def pdf(raw):
    if b'%%EOF' not in raw: return [], 'FailedPermanent', ['CorruptDocument']
    if b'3 Tr' in raw: return [], 'Unsupported', ['UnsupportedStructure']
    # Source content stream/Tj + ToUnicode CMap, not PDFium output.
    mapping = {int(src, 16): bytes.fromhex(dst.decode()).decode('utf-16-be')
               for src, dst in re.findall(rb'<([0-9A-F]{4})> <([0-9A-F]{4})>', raw)}
    if not mapping: return [], 'FailedPermanent', ['CorruptDocument']
    found = []
    for page, hx in enumerate(re.findall(rb'BT .*? <([0-9A-F]+)> Tj ET', raw)):
        text = ''.join(mapping[int(hx[i:i + 4], 16)] for i in range(0, len(hx), 4))
        found.append(u('PageText', text, {'Pdf': {'page_index': page, 'char_start': 0, 'char_end': len(text)}}))
    if not found: return [], 'Unsupported', ['RequiresOcr']
    if len(found) > 1: return [], 'Unsupported', ['AmbiguousReadingOrder']
    return found, 'Supported', []


def text(raw, limits):
    try:
        decoded = raw.decode('utf-8-sig' if raw.startswith(b'\xef\xbb\xbf') else 'cp932' if limits.get('charset') == 'windows-31j' else 'utf-8')
    except UnicodeError:
        return [], 'Unsupported', ['UnsupportedEncoding']
    lines = decoded.replace('\r\n', '\n').replace('\r', '\n').split('\n')
    return [u('Line', line, {'Text': {'line_start': i, 'line_end': i + 1}}) for i, line in enumerate(lines) if line], 'Supported', []


def comma(raw):
    decoded = raw.decode('utf-8')
    if ';' in decoded.splitlines()[0]: return [], 'Unsupported', ['UnsupportedDialect']
    found = []
    csv.field_size_limit(1048578)
    for ri, row in enumerate(csv.reader(io.StringIO(decoded, newline=''), strict=True)):
        for ci, field in enumerate(row):
            if len(field.encode()) > 1048576: return [], 'Unsupported', ['ResourceLimit']
            if field: found.append(u('Field', field, {'Csv': {'record': ri, 'field': ci}}))
    return found, 'Supported', []


class DOM(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.root = ['document', [], {}]; self.stack = [self.root]

    def handle_starttag(self, tag, attrs):
        node = [tag, [], dict(attrs)]; self.stack[-1][1].append(node)
        if tag not in ('br', 'img', 'meta', 'link', 'input'): self.stack.append(node)

    def handle_endtag(self, tag):
        if len(self.stack) > 1 and self.stack[-1][0] == tag: self.stack.pop()

    def handle_data(self, data): self.stack[-1][1].append(['#text', data, {}])
    def handle_comment(self, data): self.stack[-1][1].append(['#comment', data, {}])


def html(raw):
    source = raw.decode('utf-8')
    if source.count('<div>') > 256: return [], 'Unsupported', ['ResourceLimit']
    parser = DOM(); parser.feed(source)
    document = next(n for n in parser.root[1] if n[0] == 'html')
    body = next(n for n in document[1] if n[0] == 'body')
    found = []

    def walk(node, path, heading=False, hidden=False):
        tag, children, attrs = node
        if tag in ('script', 'style', 'template'): return
        hidden = hidden or 'hidden' in attrs
        heading = heading or tag in ('h1', 'h2', 'h3', 'h4', 'h5', 'h6')
        if tag == '#text' and children.strip() and not hidden:
            found.append(u('Heading' if heading else 'Text', children, {'Html': {'text_node_path': path}}))
        elif isinstance(children, list):
            for i, child in enumerate(children): walk(child, path + [i], heading, hidden)
    for i, child in enumerate(body[1]): walk(child, [i])
    hidden = ' hidden' in source or '<script' in source or '<style' in source
    if hidden: return [], 'Unsupported', ['DynamicVisibility']
    return found, 'Supported', []


def archive_units(raw, members=None, meter=None):
    members = members or []
    meter = meter if meter is not None else {'entries': 0, 'expanded': 0}
    try: z = readzip(raw)
    except ValueError as e: return [], 'Unsupported', [str(e)]
    meter['entries'] += len(z.infolist())
    meter['expanded'] += sum(i.file_size for i in z.infolist())
    if meter['entries'] > 20000 or meter['expanded'] > 536870912:
        return [], 'Unsupported', ['ResourceLimit']
    found = []
    reasons = []
    for info in sorted(z.infolist(), key=lambda i: i.filename):
        if info.is_dir(): continue
        data = z.read(info)
        chain = members + [info.filename]
        if len(chain) > 3: return [], 'Unsupported', ['ResourceLimit']
        if info.filename.endswith('.zip'):
            inner, cov, inner_reasons = archive_units(data, chain, meter)
            if cov not in ('Supported', 'Partial'): return [], cov, inner_reasons
            found += inner
        else:
            ext = info.filename.rsplit('.', 1)[-1]
            fmt = {'txt': 'text', 'csv': 'csv', 'html': 'html', 'docx': 'docx',
                   'xlsx': 'xlsx', 'xlsm': 'xlsm', 'pptx': 'pptx', 'pdf': 'pdf'}.get(ext)
            if fmt is None: return [], 'Unsupported', ['UnsupportedFormat']
            units, cov, inner_reasons = inspect(fmt, data, {})
            if cov not in ('Supported', 'Partial'): return [], cov, inner_reasons
            found += [u(x['kind'], x['text'], {'Archive': {'members': chain, 'inner': x['locator']}}) for x in units]
        for reason in inner_reasons:
            if reason not in reasons: reasons.append(reason)
    return found, 'Partial' if reasons else 'Supported', reasons


def inspect(fmt, raw, limits):
    if fmt == 'docx': return docx(raw)
    if fmt in ('xlsx', 'xlsm'): return spreadsheet(raw, fmt == 'xlsm')
    if fmt == 'pptx': return pptx(raw)
    if fmt == 'pdf': return pdf(raw)
    if fmt == 'text': return text(raw, limits)
    if fmt == 'csv': return comma(raw)
    if fmt == 'html': return html(raw)
    if fmt == 'zip': return archive_units(raw)
    if fmt in ('doc', 'xls', 'ppt'):
        assert raw.startswith(bytes.fromhex('D0CF11E0A1B11AE1'))
        return [], 'Unsupported', ['UnsupportedFormat']
    raise ValueError(fmt)
