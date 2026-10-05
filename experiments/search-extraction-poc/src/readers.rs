use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::Component;
use std::sync::OnceLock;

use encoding_rs::SHIFT_JIS;
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use quick_xml::{Reader, XmlVersion, events::Event};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;
use zip::{CompressionMethod, ZipArchive};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub kind: String,
    pub text: String,
    pub locator: Value,
    pub part_ordinal: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownOmission {
    pub member_chain: Vec<String>,
    pub package_path: String,
    pub physical_child_path: Vec<u32>,
    pub reason: String,
}

fn package_omission(package_path: &str) -> KnownOmission {
    KnownOmission {
        member_chain: vec![],
        package_path: package_path.to_owned(),
        physical_child_path: vec![],
        reason: "UnsupportedStructure".into(),
    }
}

#[derive(Clone, Debug)]
pub struct Extracted {
    pub units: Vec<Unit>,
    pub coverage: &'static str,
    pub reasons: Vec<&'static str>,
    pub known_omissions: Vec<KnownOmission>,
}

impl Extracted {
    fn supported(units: Vec<Unit>) -> Self {
        Self {
            units,
            coverage: "Supported",
            reasons: vec![],
            known_omissions: vec![],
        }
    }
    fn partial_with_omissions(
        units: Vec<Unit>,
        reasons: Vec<&'static str>,
        known_omissions: Vec<KnownOmission>,
    ) -> Self {
        Self {
            units,
            coverage: "Partial",
            reasons,
            known_omissions,
        }
    }
    fn unsupported(reason: &'static str) -> Self {
        Self {
            units: vec![],
            coverage: "Unsupported",
            reasons: vec![reason],
            known_omissions: vec![],
        }
    }
    fn failed(reason: &'static str) -> Self {
        Self {
            units: vec![],
            coverage: "FailedPermanent",
            reasons: vec![reason],
            known_omissions: vec![],
        }
    }
}

fn norm(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").nfc().collect()
}
fn unit(kind: &str, text: &str, locator: Value) -> Unit {
    Unit {
        kind: kind.into(),
        text: norm(text),
        locator,
        part_ordinal: 0,
    }
}

pub fn inspect(fmt: &str, raw: &[u8], limits: &Value) -> Extracted {
    if raw.len() > 268_435_456 {
        return Extracted::unsupported("ResourceLimit");
    }
    let mut meter = ZipMeter::default();
    let result = match fmt {
        "text" => text(raw, limits),
        "csv" => csv(raw),
        "html" => html(raw),
        "docx" => docx(raw, &mut meter),
        "xlsx" | "xlsm" => spreadsheet(raw, fmt == "xlsm", &mut meter),
        "pptx" => pptx(raw, &mut meter),
        "pdf" => pdf(raw),
        "zip" => archive(raw, &mut meter),
        "doc" | "xls" | "ppt" => Ok(Extracted::unsupported("UnsupportedFormat")),
        _ => Ok(Extracted::unsupported("UnsupportedFormat")),
    };
    let mut extracted = result.unwrap_or_else(|reason| match reason {
        "MalformedArchive" | "CorruptDocument" | "TextExtractionFailed" => {
            Extracted::failed(reason)
        }
        other => Extracted::unsupported(other),
    });
    if extracted.units.len() > 100_000 || extracted.units.iter().any(|u| u.text.len() > 1_048_576) {
        extracted = Extracted::unsupported("ResourceLimit");
    }
    extracted
}

fn text(raw: &[u8], limits: &Value) -> Result<Extracted, &'static str> {
    let decoded = if limits.get("charset").and_then(Value::as_str) == Some("windows-31j") {
        SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(raw)
            .ok_or("UnsupportedEncoding")?
            .into_owned()
    } else {
        let bytes = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
        std::str::from_utf8(bytes)
            .map_err(|_| "UnsupportedEncoding")?
            .to_owned()
    };
    let mut units = vec![];
    for (i, line) in norm(&decoded).split('\n').enumerate() {
        if !line.is_empty() {
            units.push(unit(
                "Line",
                line,
                json!({"Text":{"line_start":i,"line_end":i+1}}),
            ));
        }
    }
    Ok(Extracted::supported(units))
}

fn csv(raw: &[u8]) -> Result<Extracted, &'static str> {
    let decoded = std::str::from_utf8(raw).map_err(|_| "UnsupportedEncoding")?;
    if decoded.lines().next().unwrap_or("").contains(';') {
        return Err("UnsupportedDialect");
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(false)
        .from_reader(raw);
    let mut units = vec![];
    for (ri, row) in reader.records().enumerate() {
        if ri >= 1_000_000 {
            return Err("ResourceLimit");
        }
        let row = row.map_err(|_| "CorruptDocument")?;
        for (ci, field) in row.iter().enumerate() {
            if field.len() > 1_048_576 {
                return Err("ResourceLimit");
            }
            if !field.is_empty() {
                units.push(unit(
                    "Field",
                    field,
                    json!({"Csv":{"record":ri,"field":ci}}),
                ));
            }
        }
    }
    Ok(Extracted::supported(units))
}

fn html(raw: &[u8]) -> Result<Extracted, &'static str> {
    let source = std::str::from_utf8(raw).map_err(|_| "UnsupportedEncoding")?;
    let dom = parse_document(RcDom::default(), Default::default()).one(source);
    fn find_body(h: &Handle) -> Option<Handle> {
        if let NodeData::Element { name, .. } = &h.data
            && name.local.as_ref() == "body"
        {
            return Some(h.clone());
        }
        for child in h.children.borrow().iter() {
            if let Some(b) = find_body(child) {
                return Some(b);
            }
        }
        None
    }
    struct State {
        units: Vec<Unit>,
        omitted: bool,
        nodes: usize,
    }
    fn walk(
        h: &Handle,
        path: Vec<usize>,
        heading: bool,
        hidden: bool,
        state: &mut State,
        depth: usize,
    ) -> Result<(), &'static str> {
        state.nodes += 1;
        if state.nodes > 2_000_000 || depth > 256 {
            return Err("ResourceLimit");
        }
        let mut heading = heading;
        let mut hidden = hidden;
        match &h.data {
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if ["script", "style", "template"].contains(&tag) {
                    state.omitted = true;
                    return Ok(());
                }
                if tag == "link"
                    && attrs
                        .borrow()
                        .iter()
                        .any(|a| a.name.local.as_ref() == "rel" && a.value.as_ref() == "stylesheet")
                {
                    state.omitted = true;
                }
                if attrs
                    .borrow()
                    .iter()
                    .any(|a| a.name.local.as_ref() == "style")
                {
                    state.omitted = true;
                    hidden = true;
                }
                heading |= matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
                if attrs
                    .borrow()
                    .iter()
                    .any(|a| a.name.local.as_ref() == "hidden")
                {
                    hidden = true;
                    state.omitted = true;
                }
            }
            NodeData::Text { contents } => {
                let value = contents.borrow();
                if !hidden && !value.trim().is_empty() {
                    state.units.push(unit(
                        if heading { "Heading" } else { "Text" },
                        &value,
                        json!({"Html":{"text_node_path":path}}),
                    ));
                }
                return Ok(());
            }
            _ => {}
        }
        for (i, child) in h.children.borrow().iter().enumerate() {
            let mut next = path.clone();
            next.push(i);
            walk(child, next, heading, hidden, state, depth + 1)?;
        }
        Ok(())
    }
    let body = find_body(&dom.document).ok_or("CorruptDocument")?;
    let mut state = State {
        units: vec![],
        omitted: false,
        nodes: 0,
    };
    for (i, child) in body.children.borrow().iter().enumerate() {
        walk(child, vec![i], false, false, &mut state, 1)?;
    }
    Ok(if state.omitted {
        // Visibility can change outside this DOM traversal; without a
        // locatable unread range, frozen Partial would be a false claim.
        Extracted::unsupported("DynamicVisibility")
    } else {
        Extracted::supported(state.units)
    })
}

#[derive(Clone, Debug)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.local() == name)
    }
    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.local() == name)
    }
    fn desc_text(&self, name: &str) -> String {
        let mut out = if self.local() == name {
            self.text.clone()
        } else {
            String::new()
        };
        for child in &self.children {
            out.push_str(&child.desc_text(name));
        }
        out
    }
}

fn parse_xml(raw: &[u8]) -> Result<Node, &'static str> {
    if raw.len() > 67_108_864 {
        return Err("ResourceLimit");
    }
    let mut reader = Reader::from_reader(raw);
    let mut stack: Vec<Node> = vec![];
    let mut root = None;
    let mut nodes = 0usize;
    loop {
        match reader.read_event().map_err(|_| "CorruptDocument")? {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let empty = matches!(&event, Event::Empty(_));
                let e = match &event {
                    Event::Start(e) | Event::Empty(e) => e,
                    _ => unreachable!(),
                };
                nodes += 1;
                if nodes > 2_000_000 || stack.len() >= 256 {
                    return Err("ResourceLimit");
                }
                let name = e.name().as_ref().to_owned();
                let mut attrs = BTreeMap::new();
                for attr in e.attributes().with_checks(true) {
                    let attr = attr.map_err(|_| "CorruptDocument")?;
                    let key = attr.key.as_ref().to_owned();
                    let value = attr
                        .normalized_value(XmlVersion::Explicit1_0)
                        .map_err(|_| "CorruptDocument")?
                        .into_owned();
                    attrs.insert(key, value);
                }
                let node = Node {
                    name,
                    attrs,
                    children: vec![],
                    text: String::new(),
                };
                // An empty element has no separate End event.
                if empty {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else if root.replace(node).is_some() {
                        return Err("CorruptDocument");
                    }
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or("CorruptDocument")?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err("CorruptDocument");
                }
            }
            Event::Text(e) => {
                let value =
                    quick_xml::escape::unescape(e.as_ref()).map_err(|_| "CorruptDocument")?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&value);
                }
            }
            Event::CData(e) => {
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(e.as_ref());
                }
            }
            Event::DocType(_) => return Err("UnsupportedStructure"),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("CorruptDocument");
    }
    root.ok_or("CorruptDocument")
}

fn safe_name(name: &str) -> bool {
    let p = std::path::Path::new(name);
    !name.is_empty()
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && name.nfc().collect::<String>() == name
        && p.components().all(|c| matches!(c, Component::Normal(_)))
        && !name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
}

#[derive(Default)]
struct ZipMeter {
    entries: usize,
    expanded: u64,
}

fn zip_parts(raw: &[u8], meter: &mut ZipMeter) -> Result<BTreeMap<String, Vec<u8>>, &'static str> {
    // zip-rs' name table can shadow duplicate entries. Walk the bounded central
    // directory separately so ambiguity is rejected before any extraction.
    fn u16le(raw: &[u8], pos: usize) -> Result<usize, &'static str> {
        Ok(u16::from_le_bytes(
            raw.get(pos..pos + 2)
                .ok_or("MalformedArchive")?
                .try_into()
                .unwrap(),
        ) as usize)
    }
    fn u32le(raw: &[u8], pos: usize) -> Result<usize, &'static str> {
        Ok(u32::from_le_bytes(
            raw.get(pos..pos + 4)
                .ok_or("MalformedArchive")?
                .try_into()
                .unwrap(),
        ) as usize)
    }
    let eocd = raw
        .windows(4)
        .rposition(|x| x == b"PK\x05\x06")
        .ok_or("MalformedArchive")?;
    let declared = u16le(raw, eocd + 10)?;
    if declared > 20_000 {
        return Err("ResourceLimit");
    }
    meter.entries = meter.entries.checked_add(declared).ok_or("ResourceLimit")?;
    if meter.entries > 20_000 {
        return Err("ResourceLimit");
    }
    let central_size = u32le(raw, eocd + 12)?;
    let mut at = u32le(raw, eocd + 16)?;
    let central_end = at.checked_add(central_size).ok_or("MalformedArchive")?;
    if central_end > eocd {
        return Err("MalformedArchive");
    }
    let mut central_names = BTreeSet::new();
    for _ in 0..declared {
        if raw.get(at..at + 4) != Some(&b"PK\x01\x02"[..]) {
            return Err("MalformedArchive");
        }
        let flags = u16le(raw, at + 8)?;
        if flags & 1 != 0 {
            return Err("Encrypted");
        }
        let n = u16le(raw, at + 28)?;
        let extra = u16le(raw, at + 30)?;
        let comment = u16le(raw, at + 32)?;
        let name = std::str::from_utf8(raw.get(at + 46..at + 46 + n).ok_or("MalformedArchive")?)
            .map_err(|_| "UnsupportedStructure")?;
        if !safe_name(name) || !central_names.insert(name.to_owned()) {
            return Err("UnsupportedStructure");
        }
        at = at
            .checked_add(46 + n + extra + comment)
            .ok_or("MalformedArchive")?;
    }
    if at != central_end {
        return Err("MalformedArchive");
    }
    let mut zip = ZipArchive::new(Cursor::new(raw)).map_err(|_| "MalformedArchive")?;
    if zip.len() != declared {
        return Err("UnsupportedStructure");
    }
    let mut names = BTreeSet::new();
    let mut intervals = vec![];
    // Preflight the whole central directory before decompressing any entry.
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|_| "MalformedArchive")?;
        let name = f.name().to_owned();
        if f.encrypted() {
            return Err("Encrypted");
        }
        if !safe_name(&name) || !names.insert(name) {
            return Err("UnsupportedStructure");
        }
        if f.unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("UnsupportedStructure");
        }
        if !matches!(
            f.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err("UnsupportedCodec");
        }
        if f.size() > 67_108_864 {
            return Err("ResourceLimit");
        }
        meter.expanded = meter
            .expanded
            .checked_add(f.size())
            .ok_or("ResourceLimit")?;
        if meter.expanded > 536_870_912 {
            return Err("ResourceLimit");
        }
        if f.size() > 65536 && f.compressed_size() > 0 && f.size() / f.compressed_size() > 100 {
            return Err("ResourceLimit");
        }
        let a = f.data_start().ok_or("MalformedArchive")?;
        let b = a
            .checked_add(f.compressed_size())
            .ok_or("MalformedArchive")?;
        if b > raw.len() as u64 {
            return Err("MalformedArchive");
        }
        intervals.push((a, b));
    }
    intervals.sort_unstable();
    for pair in intervals.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("UnsupportedStructure");
        }
    }
    let mut parts = BTreeMap::new();
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|_| "MalformedArchive")?;
        let name = f.name().to_owned();
        let expected_size = f.size();
        let mut bytes = Vec::new();
        f.take(67_108_865)
            .read_to_end(&mut bytes)
            .map_err(|_| "MalformedArchive")?;
        if bytes.len() as u64 != expected_size {
            return Err("MalformedArchive");
        }
        parts.insert(name, bytes);
    }
    Ok(parts)
}

fn part<'a>(parts: &'a BTreeMap<String, Vec<u8>>, name: &str) -> Result<&'a [u8], &'static str> {
    parts.get(name).map(Vec::as_slice).ok_or("CorruptDocument")
}

fn opc_target(source_part: &str, target: &str) -> Result<String, &'static str> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains("\\")
        || target.contains("//")
        || target.contains('%')
        || target.chars().any(char::is_control)
    {
        return Err("UnsupportedStructure");
    }
    let mut path: Vec<&str> = source_part.split('/').collect();
    path.pop();
    for component in target.split('/') {
        match component {
            "." => {}
            ".." if path.pop().is_some() => {}
            ".." | "" => return Err("UnsupportedStructure"),
            other => path.push(other),
        }
    }
    let resolved = path.join("/");
    if !safe_name(&resolved) {
        return Err("UnsupportedStructure");
    }
    Ok(resolved)
}

fn opc_relationships(
    parts: &BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<Vec<(String, String, String)>, &'static str> {
    let root = parse_xml(part(parts, path)?)?;
    if root.local() != "Relationships" {
        return Err("CorruptDocument");
    }
    let mut seen = BTreeSet::new();
    let mut relationships = vec![];
    for node in &root.children {
        if node.local() != "Relationship" || node.attr("TargetMode") == Some("External") {
            return Err("UnsupportedStructure");
        }
        let id = node.attr("Id").ok_or("CorruptDocument")?;
        let kind = node.attr("Type").ok_or("CorruptDocument")?;
        let target = node.attr("Target").ok_or("CorruptDocument")?;
        if !seen.insert(id) {
            return Err("UnsupportedStructure");
        }
        relationships.push((id.to_owned(), kind.to_owned(), target.to_owned()));
    }
    Ok(relationships)
}

fn check_ooxml(
    parts: &BTreeMap<String, Vec<u8>>,
    main: &str,
    content_type: &str,
) -> Result<(), &'static str> {
    let ct = parse_xml(part(parts, "[Content_Types].xml")?)?;
    if ct.local() != "Types" {
        return Err("CorruptDocument");
    }
    let declared = ct
        .children("Override")
        .find(|n| n.attr("PartName").is_some_and(|p| p == format!("/{main}")))
        .and_then(|n| n.attr("ContentType"))
        .ok_or("CorruptDocument")?;
    if declared != content_type {
        return Err("UnsupportedStructure");
    }
    let rel = parse_xml(part(parts, "_rels/.rels")?)?;
    let target = rel
        .children("Relationship")
        .find(|r| {
            r.attr("Type")
                .is_some_and(|t| t.ends_with("/officeDocument"))
        })
        .and_then(|r| r.attr("Target"))
        .ok_or("CorruptDocument")?;
    if target.trim_start_matches('/') != main {
        return Err("UnsupportedStructure");
    }
    Ok(())
}

fn docx(raw: &[u8], meter: &mut ZipMeter) -> Result<Extracted, &'static str> {
    let parts = zip_parts(raw, meter)?;
    let mut known_omissions = vec![];
    for name in parts.keys() {
        if matches!(
            name.as_str(),
            "[Content_Types].xml"
                | "_rels/.rels"
                | "word/document.xml"
                | "word/_rels/document.xml.rels"
        ) {
            continue;
        }
        if name.starts_with("word/header")
            || name.starts_with("word/footer")
            || name.starts_with("word/footnotes")
        {
            known_omissions.push(package_omission(name));
        } else {
            return Err("UnsupportedStructure");
        }
    }
    check_ooxml(
        &parts,
        "word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    )?;
    let xml = parse_xml(part(&parts, "word/document.xml")?)?;
    if xml.local() != "document" || xml.attr("xmlns:w").is_none() {
        return Err("CorruptDocument");
    }
    let body = xml.child("body").ok_or("CorruptDocument")?;
    let section = body.children.last().filter(|node| node.local() == "sectPr");
    if body
        .children
        .iter()
        .take(body.children.len().saturating_sub(1))
        .any(|node| node.local() == "sectPr")
    {
        return Err("UnsupportedStructure");
    }
    if parts.contains_key("word/_rels/document.xml.rels") {
        let relationships = opc_relationships(&parts, "word/_rels/document.xml.rels")?;
        let section = section.ok_or("UnsupportedStructure")?;
        if section.children.is_empty()
            || section
                .children
                .iter()
                .any(|node| node.local() != "headerReference")
        {
            return Err("UnsupportedStructure");
        }
        let types = parse_xml(part(&parts, "[Content_Types].xml")?)?;
        let mut used = BTreeSet::new();
        for reference in &section.children {
            let id = reference.attr("r:id").ok_or("CorruptDocument")?;
            if !used.insert(id) {
                return Err("UnsupportedStructure");
            }
            let (_, kind, target) = relationships
                .iter()
                .find(|(candidate, _, _)| candidate == id)
                .ok_or("CorruptDocument")?;
            if kind != "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" {
                return Err("UnsupportedStructure");
            }
            let path = opc_target("word/document.xml", target)?;
            if !path.starts_with("word/header") || !known_omissions.iter().any(|o| o.package_path == path) {
                return Err("UnsupportedStructure");
            }
            let declared = types
                .children("Override")
                .find(|node| node.attr("PartName") == Some(format!("/{path}").as_str()))
                .and_then(|node| node.attr("ContentType"));
            if declared != Some("application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml") {
                return Err("UnsupportedStructure");
            }
            let header = parse_xml(part(&parts, &path)?)?;
            if header.local() != "hdr" {
                return Err("CorruptDocument");
            }
        }
        if used.len() != relationships.len() {
            return Err("UnsupportedStructure");
        }
    } else if section.is_some() {
        return Err("UnsupportedStructure");
    }
    fn walk_blocks(
        parent: &Node,
        steps: Vec<Value>,
        cell: bool,
        out: &mut Vec<Unit>,
    ) -> Result<(), &'static str> {
        for (i, block) in parent.children.iter().enumerate() {
            let mut path = steps.clone();
            path.push(if steps.is_empty() {
                json!({"BodyBlock":i})
            } else {
                json!({"CellBlock":i})
            });
            match block.local() {
                "sectPr" if steps.is_empty() && i + 1 == parent.children.len() => {}
                "p" => {
                    for child in &block.children {
                        match child.local() {
                            "pPr" => {}
                            "r" if child.children.iter().all(|x| x.local() == "t") => {}
                            _ => return Err("UnsupportedStructure"),
                        }
                    }
                    let text = block.desc_text("t");
                    if !text.is_empty() {
                        let heading = block
                            .child("pPr")
                            .and_then(|x| x.child("pStyle"))
                            .and_then(|x| x.attr("w:val"))
                            .is_some_and(|x| x.starts_with("Heading"));
                        out.push(unit(
                            if cell {
                                "TableCell"
                            } else if heading {
                                "Heading"
                            } else {
                                "Paragraph"
                            },
                            &text,
                            json!({"Docx":{"steps":path}}),
                        ));
                    }
                }
                "tbl" => {
                    if block.children.iter().any(|x| x.local() != "tr") {
                        return Err("UnsupportedStructure");
                    }
                    for (r, row) in block.children("tr").enumerate() {
                        if row.children.iter().any(|x| x.local() != "tc") {
                            return Err("UnsupportedStructure");
                        }
                        for (c, tc) in row.children("tc").enumerate() {
                            let mut sub = path.clone();
                            sub.extend([json!({"Row":r}), json!({"Cell":c})]);
                            walk_blocks(tc, sub, true, out)?;
                        }
                    }
                }
                _ => return Err("UnsupportedStructure"),
            }
        }
        Ok(())
    }
    let mut units = vec![];
    walk_blocks(body, vec![], false, &mut units)?;
    Ok(if !known_omissions.is_empty() {
        if units.is_empty() {
            return Err("UnsupportedStructure");
        }
        Extracted::partial_with_omissions(units, vec!["UnsupportedStructure"], known_omissions)
    } else {
        Extracted::supported(units)
    })
}

fn spreadsheet(
    raw: &[u8],
    macro_enabled: bool,
    meter: &mut ZipMeter,
) -> Result<Extracted, &'static str> {
    let parts = zip_parts(raw, meter)?;
    let mut package_omissions = vec![];
    for name in parts.keys() {
        if matches!(
            name.as_str(),
            "[Content_Types].xml"
                | "_rels/.rels"
                | "xl/workbook.xml"
                | "xl/_rels/workbook.xml.rels"
                | "xl/sharedStrings.xml"
        ) || (name.starts_with("xl/worksheets/")
            && name.ends_with(".xml")
            && !name.contains("/_rels/"))
        {
            continue;
        }
        if (macro_enabled && name == "xl/vbaProject.bin")
            || name.starts_with("xl/charts/")
            || name.starts_with("xl/drawings/")
            || name.starts_with("xl/comments")
        {
            package_omissions.push(package_omission(name));
        } else {
            return Err("UnsupportedStructure");
        }
    }
    check_ooxml(
        &parts,
        "xl/workbook.xml",
        if macro_enabled {
            "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
        } else {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
        },
    )?;
    let wb = parse_xml(part(&parts, "xl/workbook.xml")?)?;
    if wb.local() != "workbook" {
        return Err("CorruptDocument");
    }
    if wb.children.iter().any(|n| n.local() != "sheets") {
        return Err("UnsupportedStructure");
    }
    let rel = parse_xml(part(&parts, "xl/_rels/workbook.xml.rels")?)?;
    let content_types = parse_xml(part(&parts, "[Content_Types].xml")?)?;
    let targets: BTreeMap<_, _> = rel
        .children("Relationship")
        .filter_map(|n| Some((n.attr("Id")?.to_owned(), n.attr("Target")?.to_owned())))
        .collect();
    let shared = if let Some(bytes) = parts.get("xl/sharedStrings.xml") {
        let ct = parse_xml(part(&parts, "[Content_Types].xml")?)?;
        let declared = ct
            .children("Override")
            .find(|n| n.attr("PartName") == Some("/xl/sharedStrings.xml"))
            .and_then(|n| n.attr("ContentType"));
        if declared
            != Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml")
        {
            return Err("UnsupportedStructure");
        }
        let xml = parse_xml(bytes)?;
        if xml.local() != "sst" {
            return Err("CorruptDocument");
        }
        let mut strings = Vec::new();
        for si in xml.children("si") {
            if strings.len() >= 100_000 {
                return Err("ResourceLimit");
            }
            let value = if si.children.len() == 1 && si.children[0].local() == "t" {
                si.children[0].text.clone()
            } else if si.children.iter().all(|n| n.local() == "r") {
                let mut out = String::new();
                for run in &si.children {
                    if run
                        .children
                        .iter()
                        .any(|n| !matches!(n.local(), "rPr" | "t"))
                    {
                        return Err("UnsupportedStructure");
                    }
                    let text = run.child("t").ok_or("CorruptDocument")?;
                    out.push_str(&text.text);
                }
                out
            } else {
                return Err("UnsupportedStructure");
            };
            strings.push(value);
        }
        Some(strings)
    } else {
        None
    };
    let mut units = vec![];
    let mut reasons = vec![];
    let mut known_omissions = vec![];
    let mut referenced_sheets = BTreeSet::new();
    for (si, sheet) in wb
        .child("sheets")
        .ok_or("CorruptDocument")?
        .children("sheet")
        .enumerate()
    {
        let target = targets
            .get(sheet.attr("r:id").ok_or("CorruptDocument")?)
            .ok_or("CorruptDocument")?;
        let path = format!("xl/{}", target.trim_start_matches('/'));
        if !safe_name(&path) || !referenced_sheets.insert(path.clone()) {
            return Err("UnsupportedStructure");
        }
        let declared = content_types
            .children("Override")
            .find(|n| n.attr("PartName").is_some_and(|p| p == format!("/{path}")))
            .and_then(|n| n.attr("ContentType"));
        if declared
            != Some("application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml")
        {
            return Err("UnsupportedStructure");
        }
        let xml = parse_xml(part(&parts, &path)?)?;
        if xml.local() != "worksheet" || xml.child("sheetData").is_none() {
            return Err("CorruptDocument");
        }
        if !matches!(sheet.attr("state"), None | Some("visible")) {
            if !reasons.contains(&"UnsupportedStructure") {
                reasons.push("UnsupportedStructure");
            }
            known_omissions.push(package_omission(&path));
            continue;
        }
        if xml.children.iter().any(|n| n.local() != "sheetData") {
            return Err("UnsupportedStructure");
        }
        let data = xml.child("sheetData").ok_or("CorruptDocument")?;
        if data.children.iter().any(|n| n.local() != "row") {
            return Err("UnsupportedStructure");
        }
        for (row_index, row) in data.children.iter().enumerate() {
            if row.children.iter().any(|n| n.local() != "c") {
                return Err("UnsupportedStructure");
            }
            for (cell_index, cell) in row.children.iter().enumerate() {
                if cell
                    .children
                    .iter()
                    .any(|n| !matches!(n.local(), "f" | "v" | "is"))
                {
                    return Err("UnsupportedStructure");
                }
                if !matches!(
                    cell.attr("t"),
                    None | Some("n") | Some("inlineStr") | Some("s")
                ) {
                    return Err("UnsupportedStructure");
                }
                let addr = cell.attr("r").ok_or("CorruptDocument")?;
                let mut chars = addr.chars().peekable();
                let mut col = 0u32;
                while chars.peek().is_some_and(|x| x.is_ascii_uppercase()) {
                    col = col
                        .checked_mul(26)
                        .and_then(|v| v.checked_add(chars.next().unwrap() as u32 - 64))
                        .ok_or("ResourceLimit")?;
                }
                let row: u32 = chars
                    .collect::<String>()
                    .parse()
                    .map_err(|_| "CorruptDocument")?;
                if row == 0 || col == 0 {
                    return Err("CorruptDocument");
                }
                let value = cell.child("v");
                if cell.child("f").is_some() {
                    let reason = if value.is_none_or(|v| v.text.is_empty()) {
                        "MissingFormulaCache"
                    } else {
                        "UnsupportedStructure"
                    };
                    if !reasons.contains(&reason) {
                        reasons.push(reason);
                    }
                    known_omissions.push(KnownOmission {
                        member_chain: vec![],
                        package_path: path.clone(),
                        physical_child_path: vec![row_index as u32, cell_index as u32],
                        reason: reason.into(),
                    });
                    continue;
                }
                let text = match cell.attr("t") {
                    Some("inlineStr") => cell.desc_text("t"),
                    Some("s") => {
                        let index: usize = value
                            .ok_or("CorruptDocument")?
                            .text
                            .parse()
                            .map_err(|_| "CorruptDocument")?;
                        shared
                            .as_ref()
                            .ok_or("CorruptDocument")?
                            .get(index)
                            .ok_or("CorruptDocument")?
                            .clone()
                    }
                    _ => value.map(|v| v.text.clone()).unwrap_or_default(),
                };
                if !text.is_empty() {
                    units.push(unit(
                        "Cell",
                        &text,
                        json!({"Spreadsheet":{"sheet_ordinal":si,"row":row-1,"col":col-1}}),
                    ));
                }
            }
        }
    }
    if parts.keys().any(|name| {
        name.starts_with("xl/worksheets/")
            && !name.contains("/_rels/")
            && !referenced_sheets.contains(name)
    }) {
        return Err("UnsupportedStructure");
    }
    if !package_omissions.is_empty() && !reasons.contains(&"UnsupportedStructure") {
        reasons.push("UnsupportedStructure");
    }
    known_omissions.extend(package_omissions);
    Ok(if reasons.is_empty() {
        Extracted::supported(units)
    } else if units.is_empty() {
        Extracted::unsupported(reasons[0])
    } else {
        Extracted::partial_with_omissions(units, reasons, known_omissions)
    })
}

fn pptx(raw: &[u8], meter: &mut ZipMeter) -> Result<Extracted, &'static str> {
    let parts = zip_parts(raw, meter)?;
    let mut known_omissions = vec![];
    for name in parts.keys() {
        if matches!(
            name.as_str(),
            "[Content_Types].xml"
                | "_rels/.rels"
                | "ppt/presentation.xml"
                | "ppt/_rels/presentation.xml.rels"
        ) || (name.starts_with("ppt/slides/")
            && name.ends_with(".xml")
            && !name.contains("/_rels/"))
            || (name.starts_with("ppt/slides/_rels/") && name.ends_with(".xml.rels"))
        {
            continue;
        }
        if name.starts_with("ppt/notesSlides/") || name.starts_with("ppt/charts/") {
            known_omissions.push(package_omission(name));
        } else {
            return Err("UnsupportedStructure");
        }
    }
    check_ooxml(
        &parts,
        "ppt/presentation.xml",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
    )?;
    let prs = parse_xml(part(&parts, "ppt/presentation.xml")?)?;
    let rel = parse_xml(part(&parts, "ppt/_rels/presentation.xml.rels")?)?;
    let content_types = parse_xml(part(&parts, "[Content_Types].xml")?)?;
    let targets: BTreeMap<_, _> = rel
        .children("Relationship")
        .filter_map(|n| Some((n.attr("Id")?.to_owned(), n.attr("Target")?.to_owned())))
        .collect();
    fn shapes(
        tree: &Node,
        slide: usize,
        prefix: Vec<usize>,
        units: &mut Vec<Unit>,
        omitted: &mut bool,
    ) {
        for (i, shape) in tree.children.iter().enumerate() {
            let mut path = prefix.clone();
            path.push(i);
            match shape.local() {
                "grpSp" => shapes(shape, slide, path, units, omitted),
                "sp" => {
                    if let Some(tx) = shape.child("txBody") {
                        for (pi, para) in tx.children("p").enumerate() {
                            if para.children.iter().any(|n| n.local() != "r") {
                                *omitted = true;
                                continue;
                            }
                            let text = para.desc_text("t");
                            if !text.is_empty() {
                                units.push(unit("ShapeText", &text,
                            json!({"Pptx":{"slide_ordinal":slide,"shape_path":path,"text_slot":{"ShapeParagraph":{"paragraph":pi}}}})));
                            }
                        }
                    }
                }
                "graphicFrame" => {
                    let graphic = shape
                        .child("graphic")
                        .and_then(|n| n.child("graphicData"))
                        .and_then(|n| n.child("tbl"));
                    if let Some(table) = graphic {
                        for (ri, row) in table.children("tr").enumerate() {
                            for (ci, cell) in row.children("tc").enumerate() {
                                if let Some(tx) = cell.child("txBody") {
                                    for (pi, para) in tx.children("p").enumerate() {
                                        if para.children.iter().any(|n| n.local() != "r") {
                                            *omitted = true;
                                            continue;
                                        }
                                        let text = para.desc_text("t");
                                        if !text.is_empty() {
                                            units.push(unit("TableCell", &text,
                                            json!({"Pptx":{"slide_ordinal":slide,"shape_path":path,"text_slot":{"TableCellParagraph":{"row":ri,"col":ci,"paragraph":pi}}}})));
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        *omitted = true;
                    }
                }
                _ => *omitted = true,
            }
        }
    }
    let mut units = vec![];
    let mut omitted = false;
    let mut referenced_slides = BTreeSet::new();
    let mut referenced_slide_rels = BTreeSet::new();
    for (si, slide) in prs
        .child("sldIdLst")
        .ok_or("CorruptDocument")?
        .children("sldId")
        .enumerate()
    {
        let target = targets
            .get(slide.attr("r:id").ok_or("CorruptDocument")?)
            .ok_or("CorruptDocument")?;
        let path = format!("ppt/{}", target.trim_start_matches('/'));
        if !safe_name(&path) || !referenced_slides.insert(path.clone()) {
            return Err("UnsupportedStructure");
        }
        let (directory, filename) = path.rsplit_once('/').ok_or("CorruptDocument")?;
        let rel_path = format!("{directory}/_rels/{filename}.rels");
        if parts.contains_key(&rel_path) {
            referenced_slide_rels.insert(rel_path.clone());
            for (_, kind, target) in opc_relationships(&parts, &rel_path)? {
                if kind != "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" {
                    return Err("UnsupportedStructure");
                }
                let note_path = opc_target(&path, &target)?;
                if !note_path.starts_with("ppt/notesSlides/") {
                    return Err("UnsupportedStructure");
                }
                let declared = content_types
                    .children("Override")
                    .find(|node| node.attr("PartName") == Some(format!("/{note_path}").as_str()))
                    .and_then(|node| node.attr("ContentType"));
                if declared != Some("application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml") {
                    return Err("UnsupportedStructure");
                }
                let note = parse_xml(part(&parts, &note_path)?)?;
                if note.local() != "notes" {
                    return Err("CorruptDocument");
                }
                if !known_omissions.iter().any(|item| item.package_path == note_path) {
                    return Err("UnsupportedStructure");
                }
            }
        }
        let xml = parse_xml(part(&parts, &path)?)?;
        let tree = xml
            .child("cSld")
            .and_then(|n| n.child("spTree"))
            .ok_or("CorruptDocument")?;
        shapes(tree, si, vec![], &mut units, &mut omitted);
    }
    if parts.keys().any(|name| {
        name.starts_with("ppt/slides/")
            && !name.contains("/_rels/")
            && !referenced_slides.contains(name)
    }) || parts.keys().any(|name| {
        name.starts_with("ppt/slides/_rels/") && !referenced_slide_rels.contains(name)
    }) {
        return Err("UnsupportedStructure");
    }
    if omitted || (!known_omissions.is_empty() && units.is_empty()) {
        return Err("UnsupportedStructure");
    }
    Ok(if !known_omissions.is_empty() {
        Extracted::partial_with_omissions(units, vec!["UnsupportedStructure"], known_omissions)
    } else {
        Extracted::supported(units)
    })
}

fn pdf(raw: &[u8]) -> Result<Extracted, &'static str> {
    use pdfium_render::prelude::*;
    static PDFIUM: OnceLock<Result<Pdfium, ()>> = OnceLock::new();
    let structure = lopdf::Document::load_mem(raw).map_err(|_| "CorruptDocument")?;
    if structure.is_encrypted() {
        return Err("Encrypted");
    }
    let mut operations = 0usize;
    let mut has_image = false;
    for page_id in structure.get_pages().into_values() {
        let content = structure
            .get_page_content_with_limit(page_id, 67_108_864)
            .map_err(|_| "ResourceLimit")?;
        let decoded = lopdf::content::Content::decode(&content).map_err(|_| "CorruptDocument")?;
        operations = operations
            .checked_add(decoded.operations.len())
            .ok_or("ResourceLimit")?;
        if operations > 1_000_000 {
            return Err("ResourceLimit");
        }
        for op in decoded.operations {
            match op.operator.as_str() {
                "BT" | "Tf" | "Tm" | "Tj" | "ET" | "q" | "cm" | "Q" => {}
                "Do" => has_image = true,
                _ => return Err("UnsupportedStructure"),
            }
        }
    }
    let engine = PDFIUM
        .get_or_init(|| {
            let dir = std::env::var("PDFIUM_DYNAMIC_LIB_PATH").map_err(|_| ())?;
            let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
            let library = Pdfium::bind_to_library(path).map_err(|_| ())?;
            Ok(Pdfium::new(library))
        })
        .as_ref()
        .map_err(|_| "TextExtractionFailed")?;
    let document = engine
        .load_pdf_from_byte_slice(raw, None)
        .map_err(|_| "CorruptDocument")?;
    if document.pages().len() > 1024 {
        return Err("ResourceLimit");
    }
    let mut units = vec![];
    for (pi, page) in document.pages().iter().enumerate() {
        let text = page.text().map_err(|_| "TextExtractionFailed")?;
        let chars = text.chars();
        let mut content = String::new();
        let mut start = None;
        let mut end = 0;
        for i in 0..chars.len() {
            if let Some(ch) = chars
                .get(i)
                .map_err(|_| "TextExtractionFailed")?
                .unicode_char()
            {
                if !ch.is_whitespace() {
                    if start.is_none() {
                        start = Some(i);
                    }
                    end = i + 1;
                }
                content.push(ch);
            }
        }
        let content = content.trim();
        if let Some(start) = start {
            units.push(unit(
                "PageText",
                content,
                json!({"Pdf":{"page_index":pi,"char_start":start,"char_end":end}}),
            ));
        }
    }
    Ok(if units.is_empty() {
        Extracted::unsupported("RequiresOcr")
    } else if has_image {
        Extracted::unsupported("UnsupportedStructure")
    } else if units.len() > 1 {
        // A page-local character range does not locate the unresolved global
        // reading order, so this cannot satisfy frozen Partial semantics.
        Extracted::unsupported("AmbiguousReadingOrder")
    } else {
        Extracted::supported(units)
    })
}

fn archive(raw: &[u8], meter: &mut ZipMeter) -> Result<Extracted, &'static str> {
    fn recurse(
        raw: &[u8],
        members: &[String],
        units: &mut Vec<Unit>,
        reasons: &mut Vec<&'static str>,
        known_omissions: &mut Vec<KnownOmission>,
        meter: &mut ZipMeter,
    ) -> Result<(), &'static str> {
        let parts = zip_parts(raw, meter)?;
        for (name, data) in parts {
            let mut chain = members.to_vec();
            chain.push(name.clone());
            if chain.len() > 3 {
                return Err("ResourceLimit");
            }
            let inner = if name.ends_with(".zip") {
                recurse(&data, &chain, units, reasons, known_omissions, meter)?;
                continue;
            } else if name.ends_with(".txt") {
                text(&data, &Value::Null)?
            } else if name.ends_with(".csv") {
                csv(&data)?
            } else if name.ends_with(".html") {
                html(&data)?
            } else if name.ends_with(".docx") {
                docx(&data, meter)?
            } else if name.ends_with(".xlsx") {
                spreadsheet(&data, false, meter)?
            } else if name.ends_with(".xlsm") {
                spreadsheet(&data, true, meter)?
            } else if name.ends_with(".pptx") {
                pptx(&data, meter)?
            } else if name.ends_with(".pdf") {
                pdf(&data)?
            } else {
                return Err("UnsupportedFormat");
            };
            if !matches!(inner.coverage, "Supported" | "Partial") {
                return Err(inner.reasons[0]);
            }
            for reason in inner.reasons {
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            }
            for mut omission in inner.known_omissions {
                omission.member_chain = chain.clone();
                known_omissions.push(omission);
            }
            for u in inner.units {
                units.push(unit(
                    &u.kind,
                    &u.text,
                    json!({"Archive":{"members":chain,"inner":u.locator}}),
                ));
            }
        }
        Ok(())
    }
    let mut units = vec![];
    let mut reasons = vec![];
    let mut known_omissions = vec![];
    recurse(
        raw,
        &[],
        &mut units,
        &mut reasons,
        &mut known_omissions,
        meter,
    )?;
    Ok(if reasons.is_empty() {
        Extracted::supported(units)
    } else if units.is_empty() {
        Extracted::unsupported(reasons[0])
    } else {
        Extracted::partial_with_omissions(units, reasons, known_omissions)
    })
}
