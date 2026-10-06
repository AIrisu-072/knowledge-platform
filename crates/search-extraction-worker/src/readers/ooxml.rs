//! Bounded ZIP package traversal and a strict XML element tree shared by the
//! OOXML readers and the explicit ZIP container reader.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::Component;

use quick_xml::{Reader, XmlVersion, events::Event};
use search_core::knowledge_unit::BudgetKey;
use search_extraction_core::{BudgetMeter, CoverageReason};
use unicode_normalization::is_nfc;
use zip::{CompressionMethod, ZipArchive};

use super::{ReadResult, corrupt, malformed_archive, resource_limit, structure, unsupported};

const MAX_EXPANSION_RATIO: u64 = 100;

#[derive(Clone, Debug)]
pub(super) struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    pub(super) children: Vec<Node>,
    pub(super) text: String,
}

impl Node {
    pub(super) fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    pub(super) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }

    pub(super) fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.local() == name)
    }

    pub(super) fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children
            .iter()
            .filter(move |child| child.local() == name)
    }

    pub(super) fn desc_text(&self, name: &str) -> String {
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

pub(super) fn parse_xml(raw: &[u8], meter: &mut BudgetMeter) -> ReadResult<Node> {
    let mut reader = Reader::from_reader(raw);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event().map_err(|_| corrupt())? {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let empty = matches!(&event, Event::Empty(_));
                let element = match &event {
                    Event::Start(element) | Event::Empty(element) => element,
                    _ => unreachable!("matched start or empty element"),
                };
                meter.charge(BudgetKey::XmlNodes, 1)?;
                meter.observe_peak(BudgetKey::XmlDepth, stack.len() as u64 + 1)?;
                let name: String = element.name().as_ref().to_owned();
                let mut attrs = BTreeMap::new();
                for attr in element.attributes().with_checks(true) {
                    let attr = attr.map_err(|_| corrupt())?;
                    let key: String = attr.key.as_ref().to_owned();
                    let value = attr
                        .normalized_value(XmlVersion::Explicit1_0)
                        .map_err(|_| corrupt())?
                        .into_owned();
                    attrs.insert(key, value);
                }
                let node = Node {
                    name,
                    attrs,
                    children: Vec::new(),
                    text: String::new(),
                };
                // An empty element has no separate End event.
                if empty {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else if root.replace(node).is_some() {
                        return Err(corrupt());
                    }
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(corrupt)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(corrupt());
                }
            }
            Event::Text(text) => {
                let value = quick_xml::escape::unescape(text.as_ref()).map_err(|_| corrupt())?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&value);
                }
            }
            Event::CData(data) => {
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(data.as_ref());
                }
            }
            Event::DocType(_) => return Err(structure()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(corrupt());
    }
    root.ok_or_else(corrupt)
}

pub(super) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && is_nfc(name)
        && std::path::Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !name
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// A member name, or a content-free directory entry (`docs/`) whose path is
/// otherwise safe.
fn safe_entry_name(name: &str) -> bool {
    safe_name(name.strip_suffix('/').unwrap_or(name))
}

fn u16le(raw: &[u8], at: usize) -> ReadResult<usize> {
    let end = at.checked_add(2).ok_or_else(malformed_archive)?;
    let bytes = raw.get(at..end).ok_or_else(malformed_archive)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]) as usize)
}

fn u32le(raw: &[u8], at: usize) -> ReadResult<usize> {
    let end = at.checked_add(4).ok_or_else(malformed_archive)?;
    let bytes = raw.get(at..end).ok_or_else(malformed_archive)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
}

/// Fully preflight the central directory, then inflate each member with its
/// declared size enforced. Duplicate, unsafe, overlapping, encrypted, linked or
/// unexpected-codec entries fail before any member is decompressed.
pub(super) fn zip_parts(
    raw: &[u8],
    meter: &mut BudgetMeter,
    depth: u64,
) -> ReadResult<BTreeMap<String, Vec<u8>>> {
    meter.observe_peak(BudgetKey::ZipDepth, depth)?;
    // zip-rs' name table can shadow duplicate entries, so walk the central
    // directory independently before trusting the library view.
    let eocd = raw
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or_else(malformed_archive)?;
    let declared = u16le(raw, eocd + 10)?;
    meter.charge(BudgetKey::ZipEntries, declared as u64)?;
    let central_size = u32le(raw, eocd + 12)?;
    let mut at = u32le(raw, eocd + 16)?;
    let central_end = at.checked_add(central_size).ok_or_else(malformed_archive)?;
    if central_end > eocd {
        return Err(malformed_archive());
    }
    let mut central_names = BTreeSet::new();
    for _ in 0..declared {
        if raw.get(at..at + 4) != Some(&b"PK\x01\x02"[..]) {
            return Err(malformed_archive());
        }
        if u16le(raw, at + 8)? & 1 != 0 {
            return Err(unsupported(CoverageReason::Encrypted));
        }
        let name_len = u16le(raw, at + 28)?;
        let extra = u16le(raw, at + 30)?;
        let comment = u16le(raw, at + 32)?;
        let name_end = (at + 46)
            .checked_add(name_len)
            .ok_or_else(malformed_archive)?;
        let name = std::str::from_utf8(raw.get(at + 46..name_end).ok_or_else(malformed_archive)?)
            .map_err(|_| structure())?;
        if !safe_entry_name(name) || !central_names.insert(name.to_owned()) {
            return Err(structure());
        }
        at = at
            .checked_add(46 + name_len + extra + comment)
            .ok_or_else(malformed_archive)?;
    }
    if at != central_end {
        return Err(malformed_archive());
    }
    let mut zip = ZipArchive::new(Cursor::new(raw)).map_err(|_| malformed_archive())?;
    if zip.len() != declared {
        return Err(structure());
    }
    let mut names = BTreeSet::new();
    let mut intervals = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let file = zip.by_index_raw(index).map_err(|_| malformed_archive())?;
        let name = file.name().to_owned();
        if file.encrypted() {
            return Err(unsupported(CoverageReason::Encrypted));
        }
        if !safe_entry_name(&name) || (name.ends_with('/') && file.size() != 0) {
            return Err(structure());
        }
        if !names.insert(name) {
            return Err(structure());
        }
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(structure());
        }
        if !matches!(
            file.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(unsupported(CoverageReason::UnsupportedCodec));
        }
        meter.observe_peak(BudgetKey::ZipEntryBytes, file.size())?;
        meter.charge(BudgetKey::ZipTotalBytes, file.size())?;
        if file.size() > 65_536
            && file.compressed_size() > 0
            && file.size() / file.compressed_size() > MAX_EXPANSION_RATIO
        {
            return Err(resource_limit());
        }
        let start = file.data_start().ok_or_else(malformed_archive)?;
        let end = start
            .checked_add(file.compressed_size())
            .ok_or_else(malformed_archive)?;
        if end > raw.len() as u64 {
            return Err(malformed_archive());
        }
        intervals.push((start, end));
    }
    intervals.sort_unstable();
    if intervals.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(structure());
    }
    let mut parts = BTreeMap::new();
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(|_| malformed_archive())?;
        let name = file.name().to_owned();
        if name.ends_with('/') {
            continue;
        }
        let expected = file.size();
        let mut bytes = Vec::new();
        file.take(expected.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| malformed_archive())?;
        if bytes.len() as u64 != expected {
            return Err(malformed_archive());
        }
        parts.insert(name, bytes);
    }
    Ok(parts)
}

pub(super) fn part<'a>(parts: &'a BTreeMap<String, Vec<u8>>, name: &str) -> ReadResult<&'a [u8]> {
    parts.get(name).map(Vec::as_slice).ok_or_else(corrupt)
}

/// Resolve a relative OPC relationship target inside the package.
pub(super) fn opc_target(source_part: &str, target: &str) -> ReadResult<String> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.contains("//")
        || target.contains('%')
        || target.chars().any(char::is_control)
    {
        return Err(structure());
    }
    let mut path: Vec<&str> = source_part.split('/').collect();
    path.pop();
    for component in target.split('/') {
        match component {
            "." => {}
            ".." if path.pop().is_some() => {}
            ".." | "" => return Err(structure()),
            other => path.push(other),
        }
    }
    let resolved = path.join("/");
    if !safe_name(&resolved) {
        return Err(structure());
    }
    Ok(resolved)
}

pub(super) struct Relationship {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) target: String,
}

pub(super) fn opc_relationships(
    parts: &BTreeMap<String, Vec<u8>>,
    path: &str,
    meter: &mut BudgetMeter,
) -> ReadResult<Vec<Relationship>> {
    let root = parse_xml(part(parts, path)?, meter)?;
    if root.local() != "Relationships" {
        return Err(corrupt());
    }
    let mut seen = BTreeSet::new();
    let mut relationships = Vec::with_capacity(root.children.len());
    for node in &root.children {
        if node.local() != "Relationship" || node.attr("TargetMode") == Some("External") {
            return Err(structure());
        }
        let id = node.attr("Id").ok_or_else(corrupt)?;
        let kind = node.attr("Type").ok_or_else(corrupt)?;
        let target = node.attr("Target").ok_or_else(corrupt)?;
        if !seen.insert(id.to_owned()) {
            return Err(structure());
        }
        relationships.push(Relationship {
            id: id.to_owned(),
            kind: kind.to_owned(),
            target: target.to_owned(),
        });
    }
    Ok(relationships)
}

pub(super) fn declared_content_type<'a>(types: &'a Node, part_name: &str) -> Option<&'a str> {
    types
        .children("Override")
        .find(|node| {
            node.attr("PartName")
                .is_some_and(|name| name.strip_prefix('/') == Some(part_name))
        })
        .and_then(|node| node.attr("ContentType"))
}

/// The declared main part and the package relationship must agree with the
/// requested format before any body part is read.
pub(super) fn check_package(
    parts: &BTreeMap<String, Vec<u8>>,
    main: &str,
    content_type: &str,
    meter: &mut BudgetMeter,
) -> ReadResult<Node> {
    let types = parse_xml(part(parts, "[Content_Types].xml")?, meter)?;
    if types.local() != "Types" {
        return Err(corrupt());
    }
    if declared_content_type(&types, main).ok_or_else(corrupt)? != content_type {
        return Err(structure());
    }
    let relationships = parse_xml(part(parts, "_rels/.rels")?, meter)?;
    let target = relationships
        .children("Relationship")
        .find(|node| {
            node.attr("Type")
                .is_some_and(|kind| kind.ends_with("/officeDocument"))
        })
        .and_then(|node| node.attr("Target"))
        .ok_or_else(corrupt)?;
    if target.trim_start_matches('/') != main {
        return Err(structure());
    }
    Ok(types)
}
