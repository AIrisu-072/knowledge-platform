//! Bounded tagged-PDF interpretation. Indirect identities and MCID numbers are
//! validation inputs only; neither is part of the semantic projection.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde_json::{Value, json};

use super::{MAX_PDF_CONTENT_OPERATIONS, MAX_PDF_OBJECT_DEPTH, PdfDecodeBudget, failure};
use crate::{WorkerFailure, WorkerFailureCode};

const MAX_STRUCTURE_NODES: usize = 100_000;
const MAX_STRUCTURE_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CMAP_BYTES: usize = 128 * 1024;
const MAX_CMAP_MAPPINGS: usize = 16_384;

type LeafKey = (ObjectId, i64);

#[derive(Debug, Default)]
pub(super) struct PageStructure {
    pub(super) projection: Option<Value>,
    pub(super) native_text: Option<String>,
}

#[derive(Debug)]
enum Node {
    Element {
        role: String,
        language: Option<String>,
        actual_text: Option<String>,
        children: Vec<usize>,
    },
    Leaf {
        key: LeafKey,
        owner: ObjectId,
    },
}

#[derive(Debug, Default)]
struct LeafContent {
    text: String,
    tag: String,
    language: Option<String>,
    paints: Vec<Value>,
}

pub(super) struct StructureInspector<'a> {
    document: &'a Document,
    role_map: Option<&'a Dictionary>,
    nodes: Vec<Node>,
    roots: Vec<usize>,
    leaves: BTreeMap<LeafKey, usize>,
    contents: BTreeMap<LeafKey, LeafContent>,
    pages_seen: BTreeSet<ObjectId>,
    visited: BTreeSet<ObjectId>,
    nodes_seen: usize,
    text_bytes: usize,
    projection_visits: Cell<usize>,
}

impl<'a> StructureInspector<'a> {
    pub(super) fn new(document: &'a Document) -> Result<Self, WorkerFailure> {
        let mut inspector = Self {
            document,
            role_map: None,
            nodes: Vec::new(),
            roots: Vec::new(),
            leaves: BTreeMap::new(),
            contents: BTreeMap::new(),
            pages_seen: BTreeSet::new(),
            visited: BTreeSet::new(),
            nodes_seen: 0,
            text_bytes: 0,
            projection_visits: Cell::new(0),
        };
        let catalog = dictionary(document, required(&document.trailer, b"Root")?)?;
        if catalog.has(b"OCProperties") {
            return Err(unsupported("optional content is unsupported"));
        }
        let Some(root_object) = optional(catalog, b"StructTreeRoot") else {
            return Ok(inspector);
        };
        let (root_id, root_object) = resolve(document, root_object)?;
        let root = root_object
            .as_dict()
            .map_err(|_| malformed("invalid structure root"))?;
        check_keys(
            root,
            &[
                b"Type",
                b"K",
                b"ParentTree",
                b"ParentTreeNextKey",
                b"RoleMap",
            ],
        )?;
        check_type(root, b"StructTreeRoot")?;
        if let Some(roles) = optional(root, b"RoleMap") {
            let roles = dictionary(document, roles)?;
            if roles.len() > MAX_STRUCTURE_NODES {
                return Err(limit("role map exceeds the node limit"));
            }
            inspector.role_map = Some(roles);
        }
        if let Some(children) = optional(root, b"K") {
            let root_id = root_id.ok_or_else(|| malformed("structure root must be indirect"))?;
            inspector.roots = inspector.walk_children(children, root_id, None, 0)?;
        }
        inspector.validate_page_order()?;
        inspector.validate_parent_tree(root)?;
        Ok(inspector)
    }

    fn charge_node(&mut self, depth: usize) -> Result<(), WorkerFailure> {
        if depth >= MAX_PDF_OBJECT_DEPTH {
            return Err(limit("structure exceeds the depth limit"));
        }
        self.nodes_seen = self.nodes_seen.saturating_add(1);
        if self.nodes_seen > MAX_STRUCTURE_NODES {
            return Err(limit("structure exceeds the node limit"));
        }
        Ok(())
    }

    fn walk_children(
        &mut self,
        object: &'a Object,
        parent: ObjectId,
        page: Option<ObjectId>,
        depth: usize,
    ) -> Result<Vec<usize>, WorkerFailure> {
        self.charge_node(depth)?;
        let (id, object) = resolve(self.document, object)?;
        match object {
            Object::Null => Ok(Vec::new()),
            Object::Array(children) => {
                if let Some(id) = id {
                    if !self.visited.insert(id) {
                        return Err(malformed("structure has cyclic or duplicate child arrays"));
                    }
                }
                let mut result = Vec::new();
                for child in children {
                    result.extend(self.walk_children(child, parent, page, depth + 1)?);
                }
                Ok(result)
            }
            Object::Integer(mcid) => self.add_leaf(page, *mcid, parent).map(|index| vec![index]),
            Object::Dictionary(element) => {
                if optional(element, b"Type").and_then(|value| value.as_name().ok())
                    == Some(b"OBJR")
                {
                    return Err(unsupported("structure object references are unsupported"));
                }
                if optional(element, b"Type").and_then(|value| value.as_name().ok()) == Some(b"MCR")
                {
                    check_keys(element, &[b"Type", b"Pg", b"MCID"])?;
                    let page = page_reference(self.document, optional(element, b"Pg"))?.or(page);
                    let mcid = required(element, b"MCID")?
                        .as_i64()
                        .map_err(|_| malformed("invalid MCR MCID"))?;
                    return self.add_leaf(page, mcid, parent).map(|index| vec![index]);
                }
                let id = id.ok_or_else(|| malformed("structure element must be indirect"))?;
                if !self.visited.insert(id) {
                    return Err(malformed("structure has cyclic or duplicate ownership"));
                }
                check_keys(
                    element,
                    &[b"Type", b"S", b"P", b"Pg", b"K", b"ActualText", b"Lang"],
                )?;
                check_type(element, b"StructElem")?;
                if reference(required(element, b"P")?)? != parent {
                    return Err(malformed("structure parent does not match child ownership"));
                }
                let role = self.role(
                    required(element, b"S")?
                        .as_name()
                        .map_err(|_| malformed("invalid structure role"))?,
                )?;
                let language = optional(element, b"Lang").map(text_string).transpose()?;
                let actual_text = optional(element, b"ActualText")
                    .map(text_string)
                    .transpose()?;
                if language
                    .as_ref()
                    .is_some_and(|language| language.len() > 256)
                {
                    return Err(unsupported("structure language tag is unsupported"));
                }
                for value in [&language, &actual_text].into_iter().flatten() {
                    self.charge_text(value.len())?;
                }
                let page = page_reference(self.document, optional(element, b"Pg"))?.or(page);
                let children = match optional(element, b"K") {
                    Some(children) => self.walk_children(children, id, page, depth + 1)?,
                    None => Vec::new(),
                };
                let index = self.nodes.len();
                self.nodes.push(Node::Element {
                    role,
                    language,
                    actual_text,
                    children,
                });
                Ok(vec![index])
            }
            _ => Err(malformed("structure child has an invalid type")),
        }
    }

    fn add_leaf(
        &mut self,
        page: Option<ObjectId>,
        mcid: i64,
        owner: ObjectId,
    ) -> Result<usize, WorkerFailure> {
        let page = page.ok_or_else(|| malformed("structure leaf has no page"))?;
        if mcid < 0 || mcid as u64 >= MAX_STRUCTURE_NODES as u64 {
            return Err(malformed("structure MCID is outside the supported range"));
        }
        let key = (page, mcid);
        let index = self.nodes.len();
        if self.leaves.insert(key, index).is_some() {
            return Err(malformed("MCID has duplicate structure ownership"));
        }
        self.nodes.push(Node::Leaf { key, owner });
        Ok(index)
    }

    fn role(&self, original: &[u8]) -> Result<String, WorkerFailure> {
        let mut role = original;
        let mut seen = BTreeSet::new();
        for _ in 0..MAX_PDF_OBJECT_DEPTH {
            if !seen.insert(role.to_vec()) {
                return Err(malformed("cyclic structure role map"));
            }
            if supported_role(role) {
                // Standard roles cannot be redefined without introducing ambiguity.
                if self.role_map.is_some_and(|map| map.has(role)) {
                    return Err(unsupported(
                        "remapped standard structure role is unsupported",
                    ));
                }
                return Ok(String::from_utf8_lossy(role).into_owned());
            }
            if matches!(
                role,
                b"Table" | b"TR" | b"TH" | b"TD" | b"THead" | b"TBody" | b"TFoot"
            ) {
                return Err(unsupported("table structure is unsupported"));
            }
            let Some(mapped) = self.role_map.and_then(|map| optional(map, role)) else {
                return Err(unsupported("unknown structure role is unsupported"));
            };
            role = resolve(self.document, mapped)?
                .1
                .as_name()
                .map_err(|_| malformed("invalid mapped structure role"))?;
        }
        Err(limit("role map exceeds the depth limit"))
    }

    fn validate_page_order(&self) -> Result<(), WorkerFailure> {
        let page_order: BTreeMap<_, _> = self
            .document
            .get_pages()
            .into_iter()
            .map(|(number, id)| (id, number))
            .collect();
        let mut previous = None;
        // Arena leaves were appended during the structure's ordered depth-first walk.
        for node in &self.nodes {
            if let Node::Leaf { key: (page, _), .. } = node {
                let number = page_order
                    .get(page)
                    .ok_or_else(|| malformed("structure page is outside the page tree"))?;
                if previous.is_some_and(|previous| number < previous) {
                    return Err(unsupported("structure reading order crosses page order"));
                }
                previous = Some(number);
            }
        }
        Ok(())
    }

    fn validate_parent_tree(&mut self, root: &Dictionary) -> Result<(), WorkerFailure> {
        let mut entries = BTreeMap::new();
        if let Some(tree) = optional(root, b"ParentTree") {
            let mut visited = BTreeSet::new();
            read_number_tree(
                self.document,
                tree,
                0,
                &mut visited,
                &mut self.nodes_seen,
                &mut entries,
            )?;
        }
        let pages: BTreeSet<_> = self.document.get_pages().into_values().collect();
        let mut page_keys = BTreeMap::new();
        let mut key_pages = BTreeMap::new();
        for (page, _) in self.leaves.keys() {
            if !pages.contains(page) {
                return Err(malformed(
                    "structure references a page outside the page tree",
                ));
            }
            if page_keys.contains_key(page) {
                continue;
            }
            let dictionary = self
                .document
                .objects
                .get(page)
                .ok_or_else(|| malformed("missing structure page"))?
                .as_dict()
                .map_err(|_| malformed("invalid structure page"))?;
            let key = required(dictionary, b"StructParents")?
                .as_i64()
                .map_err(|_| malformed("invalid page StructParents"))?;
            if key < 0 || key_pages.insert(key, *page).is_some() {
                return Err(malformed("duplicate or invalid page StructParents"));
            }
            page_keys.insert(*page, key);
        }
        for ((page, mcid), index) in &self.leaves {
            let Node::Leaf { owner, .. } = self.nodes[*index] else {
                unreachable!()
            };
            let key = page_keys[page];
            let entry = entries
                .get(&key)
                .and_then(|array: &Vec<Option<ObjectId>>| array.get(*mcid as usize))
                .copied()
                .flatten();
            if entry != Some(owner) {
                return Err(malformed("ParentTree disagrees with MCID ownership"));
            }
        }
        for (key, values) in entries {
            if !key_pages.contains_key(&key) {
                return Err(malformed("ParentTree has an unmatched page key"));
            }
            for (mcid, owner) in values.into_iter().enumerate() {
                if let Some(owner) = owner {
                    let page = key_pages
                        .get(&key)
                        .ok_or_else(|| malformed("ParentTree has an unowned entry"))?;
                    let index = self
                        .leaves
                        .get(&(*page, mcid as i64))
                        .ok_or_else(|| malformed("ParentTree has an unmatched MCID"))?;
                    if !matches!(self.nodes[*index], Node::Leaf { owner: expected, .. } if expected == owner)
                    {
                        return Err(malformed("ParentTree has conflicting MCID ownership"));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn inspect_page(
        &mut self,
        page_id: ObjectId,
        operations: &[Operation],
        resources: Option<&'a Dictionary>,
        budget: &mut PdfDecodeBudget,
    ) -> Result<PageStructure, WorkerFailure> {
        if !self.pages_seen.insert(page_id) {
            return Err(malformed("structure page was inspected more than once"));
        }
        if operations.len() > MAX_PDF_CONTENT_OPERATIONS {
            return Err(limit("marked content exceeds the operation limit"));
        }
        // Validate syntax before ownership: a missing EMC can otherwise look
        // like a valid-but-unsupported nested MCID sequence.
        validate_marked_delimiters(operations)?;
        let has_marks = operations.iter().any(|operation| {
            matches!(
                operation.operator.as_str(),
                "BDC" | "BMC" | "EMC" | "DP" | "MP"
            )
        });
        if self.roots.is_empty() && !has_marks {
            return Ok(PageStructure::default());
        }
        let mut font_cache = BTreeMap::new();
        let mut current_font: Option<Vec<u8>> = None;
        let mut font_stack = Vec::new();
        let mut marks: Vec<Mark> = Vec::new();
        let mut native_text = String::new();
        let mut vector_index = 0usize;
        let mut path_segments = 0usize;
        let mut image_index = 0usize;
        for operation in operations {
            match operation.operator.as_str() {
                "BMC" => {
                    let [Object::Name(tag)] = operation.operands.as_slice() else {
                        return Err(malformed("invalid BMC operands"));
                    };
                    if tag != b"Artifact" {
                        return Err(unsupported("unmapped BMC structure is unsupported"));
                    }
                    if marks.iter().any(|mark| mark.key.is_some()) {
                        return Err(unsupported("artifact inside MCID ownership is ambiguous"));
                    }
                    push_mark(
                        &mut marks,
                        Mark::new(None, None, None, native_text.len(), true),
                    )?;
                }
                "BDC" => {
                    let [Object::Name(tag), properties] = operation.operands.as_slice() else {
                        return Err(malformed("invalid BDC operands"));
                    };
                    if tag == b"OC" {
                        return Err(unsupported("optional content is unsupported"));
                    }
                    let properties = property_dictionary(self.document, resources, properties)?;
                    let artifact = tag == b"Artifact";
                    if artifact && marks.iter().any(|mark| mark.key.is_some()) {
                        return Err(unsupported("artifact inside MCID ownership is ambiguous"));
                    }
                    if artifact {
                        validate_artifact_properties(properties)?;
                    } else {
                        check_keys(properties, &[b"MCID", b"ActualText", b"Lang"])?;
                    }
                    let actual_text = optional(properties, b"ActualText")
                        .map(text_string)
                        .transpose()?;
                    let language = optional(properties, b"Lang").map(text_string).transpose()?;
                    let language = language
                        .or_else(|| marks.iter().rev().find_map(|mark| mark.language.clone()));
                    if language
                        .as_ref()
                        .is_some_and(|language| language.len() > 256)
                    {
                        return Err(unsupported("marked language tag is unsupported"));
                    }
                    self.charge_text(
                        actual_text
                            .as_ref()
                            .map_or(0, String::len)
                            .saturating_add(language.as_ref().map_or(0, String::len)),
                    )?;
                    let mcid = optional(properties, b"MCID")
                        .map(|value| value.as_i64().map_err(|_| malformed("invalid marked MCID")))
                        .transpose()?;
                    let key = if let Some(mcid) = mcid {
                        let key = (page_id, mcid);
                        if marks.iter().any(|mark| mark.key.is_some() || mark.artifact) {
                            return Err(unsupported("nested MCID ownership is ambiguous"));
                        }
                        if !self.leaves.contains_key(&key) {
                            return Err(malformed("marked MCID has no structure leaf"));
                        }
                        if self
                            .contents
                            .insert(
                                key,
                                LeafContent {
                                    tag: self.role(tag)?,
                                    language: language.clone(),
                                    ..Default::default()
                                },
                            )
                            .is_some()
                        {
                            return Err(malformed("MCID occurs more than once in page content"));
                        }
                        Some(key)
                    } else {
                        if !artifact && optional(properties, b"Lang").is_some() {
                            return Err(unsupported(
                                "unowned marked language override is unsupported",
                            ));
                        }
                        if !artifact && tag != b"Span" {
                            return Err(unsupported(
                                "marked structure without MCID is unsupported",
                            ));
                        }
                        None
                    };
                    push_mark(
                        &mut marks,
                        Mark::new(key, actual_text, language, native_text.len(), artifact),
                    )?;
                }
                "EMC" => {
                    if !operation.operands.is_empty() {
                        return Err(malformed("invalid EMC operands"));
                    }
                    let mark = marks
                        .pop()
                        .ok_or_else(|| malformed("EMC has no matching marked-content start"))?;
                    if let Some(actual_text) = mark.actual_text {
                        if actual_text != native_text[mark.text_start..] {
                            return Err(unsupported(
                                "replacement text differs from independently decoded native text",
                            ));
                        }
                    }
                }
                "DP" | "MP" => return Err(unsupported("marked-content points are unsupported")),
                "q" => {
                    if font_stack.len() >= MAX_PDF_OBJECT_DEPTH {
                        return Err(limit("tagged graphics state exceeds the depth limit"));
                    }
                    font_stack.push(current_font.clone());
                }
                "Q" => {
                    current_font = font_stack
                        .pop()
                        .ok_or_else(|| malformed("unbalanced tagged graphics state"))?
                }
                "Tf" => {
                    let [Object::Name(name), _] = operation.operands.as_slice() else {
                        return Err(malformed("invalid tagged font selection"));
                    };
                    if !font_cache.contains_key(name) {
                        self.charge_node(0)?;
                        let resources =
                            resources.ok_or_else(|| malformed("tagged font has no resources"))?;
                        let fonts = dictionary(self.document, required(resources, b"Font")?)?;
                        let font = dictionary(self.document, required(fonts, name)?)?;
                        let encoding = bounded_font_encoding(self.document, font, budget)?;
                        font_cache.insert(name.clone(), encoding);
                    }
                    current_font = Some(name.clone());
                }
                "Tj" | "TJ" | "'" | "\"" => {
                    let font = current_font
                        .as_ref()
                        .and_then(|name| font_cache.get(name))
                        .ok_or_else(|| malformed("tagged text has no selected font"))?;
                    let strings = text_operands(operation)?;
                    for bytes in strings {
                        font.preflight(
                            bytes,
                            MAX_STRUCTURE_TEXT_BYTES.saturating_sub(self.text_bytes),
                        )?;
                        let text = Document::decode_text(&font.encoding, bytes)
                            .map_err(|_| malformed("tagged native text cannot be decoded"))?;
                        if text.contains('\u{fffd}') || text.contains('\0') {
                            return Err(unsupported(
                                "tagged native text has unresolved character mappings",
                            ));
                        }
                        self.charge_text(text.len())?;
                        native_text.push_str(&text);
                        if let Some(key) = marks.iter().find_map(|mark| mark.key) {
                            self.contents
                                .get_mut(&key)
                                .ok_or_else(|| malformed("missing marked-content owner"))?
                                .text
                                .push_str(&text);
                        }
                    }
                }
                "l" | "c" | "h" => path_segments = path_segments.saturating_add(1),
                "re" => path_segments = path_segments.saturating_add(4),
                "n" => path_segments = 0,
                "S" | "f" | "F" | "f*" | "B" | "B*" => {
                    if path_segments > 0 {
                        self.record_paint(&marks, "vector", vector_index)?;
                        vector_index += 1;
                    }
                    path_segments = 0;
                }
                "s" | "b" | "b*" => {
                    return Err(unsupported(
                        "tagged close-and-paint operator is unsupported",
                    ));
                }
                "Do" => {
                    let [Object::Name(name)] = operation.operands.as_slice() else {
                        return Err(malformed("invalid tagged XObject paint"));
                    };
                    let resources =
                        resources.ok_or_else(|| malformed("tagged XObject has no resources"))?;
                    let objects = dictionary(self.document, required(resources, b"XObject")?)?;
                    let object = resolve(self.document, required(objects, name)?)?
                        .1
                        .as_stream()
                        .map_err(|_| malformed("invalid tagged XObject"))?;
                    if required(&object.dict, b"Subtype")?
                        .as_name()
                        .map_err(|_| malformed("invalid tagged XObject subtype"))?
                        != b"Image"
                    {
                        return Err(unsupported("tagged Form traversal is unsupported"));
                    }
                    self.record_paint(&marks, "image", image_index)?;
                    image_index += 1;
                }
                _ => {}
            }
        }
        if !marks.is_empty() {
            return Err(malformed("marked content has an unmatched start"));
        }
        if !font_stack.is_empty() {
            return Err(malformed("unbalanced tagged graphics state"));
        }
        let mut projection = Vec::new();
        for &index in &self.roots {
            if let Some(value) = self.project_node(index, page_id)? {
                projection.push(value);
            }
        }
        Ok(PageStructure {
            projection: if projection.is_empty() {
                None
            } else {
                Some(Value::Array(projection))
            },
            native_text: Some(native_text),
        })
    }

    fn record_paint(
        &mut self,
        marks: &[Mark],
        kind: &str,
        index: usize,
    ) -> Result<(), WorkerFailure> {
        if let Some(key) = marks.iter().find_map(|mark| mark.key) {
            let leaf = self
                .contents
                .get_mut(&key)
                .ok_or_else(|| malformed("missing marked paint owner"))?;
            if leaf.paints.len() >= MAX_STRUCTURE_NODES {
                return Err(limit("marked paints exceed the node limit"));
            }
            leaf.paints.push(json!({ "kind": kind, "index": index }));
        }
        Ok(())
    }

    fn project_node(&self, index: usize, page: ObjectId) -> Result<Option<Value>, WorkerFailure> {
        let visits = self.projection_visits.get().saturating_add(1);
        self.projection_visits.set(visits);
        if visits > MAX_PDF_CONTENT_OPERATIONS {
            return Err(limit("structure projection exceeds the work limit"));
        }
        match &self.nodes[index] {
            Node::Leaf { key, .. } => {
                if key.0 != page {
                    return Ok(None);
                }
                let content = self
                    .contents
                    .get(key)
                    .ok_or_else(|| malformed("structure leaf has no marked content"))?;
                let mut result = json!({ "text": content.text, "tag": content.tag });
                if let Some(language) = &content.language {
                    result["language"] = json!(language);
                }
                if !content.paints.is_empty() {
                    result["paints"] = json!(content.paints);
                }
                Ok(Some(result))
            }
            Node::Element {
                role,
                language,
                children,
                ..
            } => {
                let mut projected = Vec::new();
                for &child in children {
                    if let Some(value) = self.project_node(child, page)? {
                        projected.push(value);
                    }
                }
                if projected.is_empty() {
                    return Ok(None);
                }
                let mut result = json!({ "role": role, "children": projected });
                if let Some(language) = language {
                    result["language"] = json!(language);
                }
                Ok(Some(result))
            }
        }
    }

    fn charge_text(&mut self, count: usize) -> Result<(), WorkerFailure> {
        self.text_bytes = self.text_bytes.saturating_add(count);
        if self.text_bytes > MAX_STRUCTURE_TEXT_BYTES {
            return Err(limit("structure text exceeds the byte limit"));
        }
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), WorkerFailure> {
        if self.leaves.len() != self.contents.len() {
            return Err(malformed("structure and marked content are incomplete"));
        }
        for node in &self.nodes {
            if let Node::Element {
                actual_text: Some(actual_text),
                children,
                ..
            } = node
            {
                let mut text = String::new();
                for &child in children {
                    self.append_native_text(child, &mut text)?;
                }
                if actual_text != &text {
                    return Err(unsupported(
                        "structure replacement text differs from native text",
                    ));
                }
            }
        }
        Ok(())
    }

    fn append_native_text(&self, index: usize, text: &mut String) -> Result<(), WorkerFailure> {
        let visits = self.projection_visits.get().saturating_add(1);
        self.projection_visits.set(visits);
        if visits > MAX_PDF_CONTENT_OPERATIONS {
            return Err(limit(
                "structure replacement validation exceeds the work limit",
            ));
        }
        match &self.nodes[index] {
            Node::Leaf { key, .. } => {
                let content = self
                    .contents
                    .get(key)
                    .ok_or_else(|| malformed("unconsumed structure leaf"))?;
                if text.len().saturating_add(content.text.len()) > MAX_STRUCTURE_TEXT_BYTES {
                    return Err(limit("replacement text exceeds the byte limit"));
                }
                text.push_str(&content.text);
            }
            Node::Element { children, .. } => {
                for &child in children {
                    self.append_native_text(child, text)?;
                }
            }
        }
        Ok(())
    }
}

struct Mark {
    key: Option<LeafKey>,
    actual_text: Option<String>,
    language: Option<String>,
    text_start: usize,
    artifact: bool,
}

impl Mark {
    fn new(
        key: Option<LeafKey>,
        actual_text: Option<String>,
        language: Option<String>,
        text_start: usize,
        artifact: bool,
    ) -> Self {
        Self {
            key,
            actual_text,
            language,
            text_start,
            artifact,
        }
    }
}

fn validate_marked_delimiters(operations: &[Operation]) -> Result<(), WorkerFailure> {
    if operations.len() > MAX_PDF_CONTENT_OPERATIONS {
        return Err(limit("marked content exceeds the operation limit"));
    }
    let mut depth = 0usize;
    for operation in operations {
        match operation.operator.as_str() {
            "BMC" | "BDC" => {
                if depth >= MAX_PDF_OBJECT_DEPTH {
                    return Err(limit("marked content exceeds the depth limit"));
                }
                depth += 1;
            }
            "EMC" => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| malformed("EMC has no matching marked-content start"))?;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(malformed("marked content has an unmatched start"));
    }
    Ok(())
}

fn push_mark(marks: &mut Vec<Mark>, mark: Mark) -> Result<(), WorkerFailure> {
    if marks.len() >= MAX_PDF_OBJECT_DEPTH {
        return Err(limit("marked content exceeds the depth limit"));
    }
    marks.push(mark);
    Ok(())
}

fn bounded_font_encoding<'a>(
    document: &'a Document,
    font: &'a Dictionary,
    budget: &mut PdfDecodeBudget,
) -> Result<FontDecoder<'a>, WorkerFailure> {
    let subtype = required(font, b"Subtype")?
        .as_name()
        .map_err(|_| malformed("invalid tagged font subtype"))?;
    let encoding_name = optional(font, b"Encoding")
        .map(|value| {
            resolve(document, value).and_then(|(_, value)| {
                value
                    .as_name()
                    .map_err(|_| unsupported("complex tagged font encoding is unsupported"))
            })
        })
        .transpose()?;
    if !matches!(
        encoding_name,
        None | Some(
            b"StandardEncoding"
                | b"WinAnsiEncoding"
                | b"MacRomanEncoding"
                | b"MacExpertEncoding"
                | b"Identity-H"
        )
    ) {
        return Err(unsupported("tagged font encoding is unsupported"));
    }
    if subtype == b"Type0" && encoding_name != Some(b"Identity-H") {
        return Err(unsupported("tagged composite font encoding is unsupported"));
    }
    if font.has(b"ToUnicode") && encoding_name.is_some() && encoding_name != Some(b"Identity-H") {
        return Err(unsupported(
            "combined simple encoding and ToUnicode is unsupported",
        ));
    }
    let requires_unicode =
        encoding_name == Some(b"Identity-H") || encoding_name.is_none() && font.has(b"ToUnicode");
    let mut cmap_bytes = 0usize;
    let mut code_bytes = 1usize;
    if let Some(cmap) = optional(font, b"ToUnicode") {
        let stream = resolve(document, cmap)?
            .1
            .as_stream()
            .map_err(|_| malformed("invalid tagged ToUnicode stream"))?;
        let decoded = stream
            .get_plain_content_with_limit(budget.remaining().min(MAX_CMAP_BYTES))
            .map_err(font_error)?;
        cmap_bytes = decoded.len();
        budget.charge(cmap_bytes, "tagged font CMap")?;
        let (width, mappings) = validate_cmap(&decoded)?;
        code_bytes = width;
        // Bound the helper's reverse-map allocation as well as decompression.
        budget.charge(mappings.saturating_mul(128), "tagged font mappings")?;
    } else if requires_unicode {
        return Err(malformed("tagged composite font has no ToUnicode mapping"));
    }
    if encoding_name == Some(b"Identity-H") && code_bytes != 2 {
        return Err(unsupported(
            "Identity-H CMap must use two-byte source codes",
        ));
    }
    let encoding = font
        .get_font_encoding_with_limit(document, budget.remaining())
        .map_err(font_error)?;
    // The helper is lenient for invalid CMaps; do not accept its fallback.
    if requires_unicode && !matches!(encoding, lopdf::Encoding::UnicodeMapEncoding(_)) {
        return Err(malformed("tagged font CMap could not be decoded strictly"));
    }
    if cmap_bytes > 0 {
        budget.charge(cmap_bytes, "tagged font encoding")?;
    }
    Ok(FontDecoder {
        encoding,
        code_bytes,
    })
}

fn font_error(error: lopdf::Error) -> WorkerFailure {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => {
            limit("tagged font exceeds the decode budget")
        }
        _ => malformed("tagged font cannot be decoded"),
    }
}

fn text_operands(operation: &Operation) -> Result<Vec<&[u8]>, WorkerFailure> {
    match (operation.operator.as_str(), operation.operands.as_slice()) {
        ("Tj" | "'", [Object::String(bytes, _)]) => Ok(vec![bytes]),
        (
            "\"",
            [
                Object::Integer(_) | Object::Real(_),
                Object::Integer(_) | Object::Real(_),
                Object::String(bytes, _),
            ],
        ) => Ok(vec![bytes]),
        ("TJ", [Object::Array(items)]) => items
            .iter()
            .filter_map(|item| match item {
                Object::String(bytes, _) => Some(Ok(bytes.as_slice())),
                Object::Integer(_) | Object::Real(_) => None,
                _ => Some(Err(malformed("invalid tagged TJ operand"))),
            })
            .collect(),
        _ => Err(malformed("invalid tagged text operands")),
    }
}

fn property_dictionary<'a>(
    document: &'a Document,
    resources: Option<&'a Dictionary>,
    object: &'a Object,
) -> Result<&'a Dictionary, WorkerFailure> {
    if let Object::Name(name) = object {
        let resources =
            resources.ok_or_else(|| malformed("named marked properties have no resources"))?;
        let properties = dictionary(document, required(resources, b"Properties")?)?;
        dictionary(document, required(properties, name)?)
    } else {
        dictionary(document, object)
    }
}

fn validate_artifact_properties(properties: &Dictionary) -> Result<(), WorkerFailure> {
    check_keys(
        properties,
        &[
            b"Type",
            b"Subtype",
            b"BBox",
            b"Attached",
            b"ActualText",
            b"Lang",
        ],
    )?;
    let kind = optional(properties, b"Type")
        .map(|value| {
            value
                .as_name()
                .map_err(|_| malformed("invalid artifact Type"))
        })
        .transpose()?;
    if kind.is_some_and(|kind| kind != b"Pagination") {
        return Err(unsupported("artifact Type is unsupported"));
    }
    let subtype = optional(properties, b"Subtype")
        .map(|value| {
            value
                .as_name()
                .map_err(|_| malformed("invalid artifact Subtype"))
        })
        .transpose()?;
    if subtype.is_some_and(|subtype| !matches!(subtype, b"Header" | b"Footer"))
        || subtype.is_some() && kind != Some(b"Pagination")
    {
        return Err(unsupported("artifact Subtype is unsupported"));
    }
    if let Some(attached) = optional(properties, b"Attached") {
        let attached = attached
            .as_array()
            .map_err(|_| malformed("invalid artifact Attached array"))?;
        let [Object::Name(edge)] = attached.as_slice() else {
            return Err(unsupported(
                "artifact attachment must have one supported edge",
            ));
        };
        if !matches!(
            (subtype, edge.as_slice()),
            (Some(b"Header"), b"Top") | (Some(b"Footer"), b"Bottom")
        ) {
            return Err(unsupported("artifact attachment is unsupported"));
        }
    }
    if let Some(bounds) = optional(properties, b"BBox") {
        let bounds = bounds
            .as_array()
            .map_err(|_| malformed("invalid artifact BBox"))?;
        if bounds.len() != 4
            || bounds.iter().any(|value| match value {
                Object::Integer(_) => false,
                Object::Real(value) => !value.is_finite(),
                _ => true,
            })
        {
            return Err(malformed(
                "artifact BBox must contain four finite coordinates",
            ));
        }
        // Bounds-bearing artifact grouping has not been qualified in this subset.
        return Err(unsupported("explicit artifact bounds are unsupported"));
    }
    Ok(())
}

fn read_number_tree(
    document: &Document,
    object: &Object,
    depth: usize,
    visited: &mut BTreeSet<ObjectId>,
    nodes: &mut usize,
    entries: &mut BTreeMap<i64, Vec<Option<ObjectId>>>,
) -> Result<Option<(i64, i64)>, WorkerFailure> {
    if depth >= MAX_PDF_OBJECT_DEPTH {
        return Err(limit("ParentTree exceeds the depth limit"));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_STRUCTURE_NODES {
        return Err(limit("ParentTree exceeds the node limit"));
    }
    let (id, object) = resolve(document, object)?;
    if let Some(id) = id {
        if !visited.insert(id) {
            return Err(malformed("ParentTree has cyclic or duplicate children"));
        }
    }
    let tree = object
        .as_dict()
        .map_err(|_| malformed("invalid ParentTree node"))?;
    check_keys(tree, &[b"Nums", b"Kids", b"Limits"])?;
    let mut range: Option<(i64, i64)> = None;
    match (optional(tree, b"Nums"), optional(tree, b"Kids")) {
        (Some(nums), None) => {
            let nums = resolve(document, nums)?
                .1
                .as_array()
                .map_err(|_| malformed("invalid ParentTree Nums"))?;
            if nums.len() % 2 != 0 {
                return Err(malformed("ParentTree Nums is not paired"));
            }
            let mut previous = None;
            for pair in nums.chunks_exact(2) {
                *nodes = nodes.saturating_add(1);
                if *nodes > MAX_STRUCTURE_NODES {
                    return Err(limit("ParentTree pairs exceed the node limit"));
                }
                let key = pair[0]
                    .as_i64()
                    .map_err(|_| malformed("invalid ParentTree key"))?;
                if key < 0 || previous.is_some_and(|previous| key <= previous) {
                    return Err(malformed("ParentTree keys are not strictly ordered"));
                }
                previous = Some(key);
                range = Some((range.map_or(key, |range| range.0), key));
                let values = resolve(document, &pair[1])?
                    .1
                    .as_array()
                    .map_err(|_| unsupported("non-page ParentTree entries are unsupported"))?;
                *nodes = nodes.saturating_add(values.len());
                if *nodes > MAX_STRUCTURE_NODES {
                    return Err(limit("ParentTree entries exceed the node limit"));
                }
                let values = values
                    .iter()
                    .map(|value| match value {
                        Object::Null => Ok(None),
                        Object::Reference(id) => {
                            dictionary(document, value)?;
                            Ok(Some(*id))
                        }
                        _ => Err(malformed("invalid ParentTree owner")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if entries.insert(key, values).is_some() {
                    return Err(malformed("duplicate ParentTree key"));
                }
            }
        }
        (None, Some(kids)) => {
            let kids = resolve(document, kids)?
                .1
                .as_array()
                .map_err(|_| malformed("invalid ParentTree Kids"))?;
            for kid in kids {
                if let Some(child_range) =
                    read_number_tree(document, kid, depth + 1, visited, nodes, entries)?
                {
                    if range.is_some_and(|range| child_range.0 <= range.1) {
                        return Err(malformed("ParentTree child ranges are not ordered"));
                    }
                    range = Some((range.map_or(child_range.0, |range| range.0), child_range.1));
                }
            }
        }
        (None, None) => {}
        _ => return Err(malformed("ParentTree has both Kids and Nums")),
    }
    if let Some(limits) = optional(tree, b"Limits") {
        let limits = resolve(document, limits)?
            .1
            .as_array()
            .map_err(|_| malformed("invalid ParentTree Limits"))?;
        let [lower, upper] = limits.as_slice() else {
            return Err(malformed("invalid ParentTree Limits length"));
        };
        let lower = lower
            .as_i64()
            .map_err(|_| malformed("invalid ParentTree lower limit"))?;
        let upper = upper
            .as_i64()
            .map_err(|_| malformed("invalid ParentTree upper limit"))?;
        if range != Some((lower, upper)) {
            return Err(malformed("ParentTree Limits disagree with entries"));
        }
    }
    Ok(range)
}

fn supported_role(role: &[u8]) -> bool {
    matches!(
        role,
        b"Document"
            | b"Part"
            | b"Sect"
            | b"Div"
            | b"P"
            | b"H"
            | b"H1"
            | b"H2"
            | b"H3"
            | b"H4"
            | b"H5"
            | b"H6"
            | b"Span"
            | b"Figure"
    )
}

fn resolve<'a>(
    document: &'a Document,
    mut object: &'a Object,
) -> Result<(Option<ObjectId>, &'a Object), WorkerFailure> {
    let mut seen = BTreeSet::new();
    let mut last_id = None;
    for _ in 0..MAX_PDF_OBJECT_DEPTH {
        match object {
            Object::Reference(id) => {
                if !seen.insert(*id) {
                    return Err(malformed("cyclic structure reference"));
                }
                last_id = Some(*id);
                object = document
                    .objects
                    .get(id)
                    .ok_or_else(|| malformed("unresolved structure reference"))?;
            }
            _ => return Ok((last_id, object)),
        }
    }
    Err(limit("structure reference exceeds the depth limit"))
}

fn dictionary<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Result<&'a Dictionary, WorkerFailure> {
    resolve(document, object)?
        .1
        .as_dict()
        .map_err(|_| malformed("structure value is not a dictionary"))
}

fn optional<'a>(dictionary: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dictionary.get(key).ok()
}

fn required<'a>(dictionary: &'a Dictionary, key: &[u8]) -> Result<&'a Object, WorkerFailure> {
    optional(dictionary, key).ok_or_else(|| malformed("required structure field is missing"))
}

fn reference(object: &Object) -> Result<ObjectId, WorkerFailure> {
    object
        .as_reference()
        .map_err(|_| malformed("structure page or parent is not indirect"))
}

fn page_reference(
    document: &Document,
    object: Option<&Object>,
) -> Result<Option<ObjectId>, WorkerFailure> {
    object
        .map(|object| {
            let id = reference(object)?;
            let page = dictionary(document, object)?;
            check_type(page, b"Page")?;
            Ok(id)
        })
        .transpose()
}

fn check_type(dictionary: &Dictionary, expected: &[u8]) -> Result<(), WorkerFailure> {
    if optional(dictionary, b"Type").and_then(|object| object.as_name().ok()) != Some(expected) {
        return Err(malformed("invalid structure dictionary type"));
    }
    Ok(())
}

fn check_keys(dictionary: &Dictionary, allowed: &[&[u8]]) -> Result<(), WorkerFailure> {
    if dictionary
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        return Err(unsupported(
            "unknown structure or marked-content field is unsupported",
        ));
    }
    Ok(())
}

fn text_string(object: &Object) -> Result<String, WorkerFailure> {
    let bytes = object
        .as_str()
        .map_err(|_| malformed("structure text is not a string"))?;
    if bytes.len() > MAX_STRUCTURE_TEXT_BYTES {
        return Err(limit("structure string exceeds the byte limit"));
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err(malformed("invalid UTF-16 structure text"));
        }
        let code_units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&code_units).map_err(|_| malformed("invalid UTF-16 structure text"))
    } else if bytes.is_ascii() {
        String::from_utf8(bytes.to_vec()).map_err(|_| malformed("invalid structure text"))
    } else {
        Err(unsupported(
            "non-Unicode structure text encoding is unsupported",
        ))
    }
}

fn malformed(message: &'static str) -> WorkerFailure {
    failure(WorkerFailureCode::ParserDisagreement, message)
}

fn unsupported(message: &'static str) -> WorkerFailure {
    failure(WorkerFailureCode::UnsupportedSemanticConstruct, message)
}

fn limit(message: &'static str) -> WorkerFailure {
    failure(WorkerFailureCode::InspectionResourceLimitExceeded, message)
}

struct FontDecoder<'a> {
    encoding: lopdf::Encoding<'a>,
    code_bytes: usize,
}

impl FontDecoder<'_> {
    fn preflight(&self, bytes: &[u8], remaining: usize) -> Result<(), WorkerFailure> {
        if bytes.len() % self.code_bytes != 0 {
            return Err(malformed("tagged text ends in a partial character code"));
        }
        // Every accepted mapping has exactly one non-surrogate UTF-16 code unit.
        // Its UTF-8 output is at most three bytes, before any decoder allocation.
        let maximum_output = (bytes.len() / self.code_bytes)
            .checked_mul(3)
            .ok_or_else(|| limit("tagged text output size overflow"))?;
        if maximum_output > remaining {
            return Err(limit(
                "tagged text exceeds the output budget before decoding",
            ));
        }
        match &self.encoding {
            lopdf::Encoding::UnicodeMapEncoding(map) => {
                for code in bytes.chunks_exact(self.code_bytes) {
                    let code = code
                        .iter()
                        .fold(0u32, |value, byte| value * 256 + u32::from(*byte));
                    if map.get(code, self.code_bytes as u8).is_none() {
                        return Err(unsupported("tagged text has an unmapped character code"));
                    }
                }
            }
            lopdf::Encoding::OneByteEncoding(map) => {
                if bytes.iter().any(|byte| {
                    map.get(usize::from(*byte))
                        .is_none_or(|glyph| glyph.is_none())
                }) {
                    return Err(unsupported(
                        "tagged text has an unmapped single-byte character",
                    ));
                }
            }
            _ => {
                return Err(unsupported(
                    "tagged text decoder is outside the bounded subset",
                ));
            }
        }
        Ok(())
    }
}

/// Validate the complete small CMap grammar before lopdf expands bfrange into
/// its reverse map. In particular, a short four-byte range cannot create 2^32
/// entries, and a short source glyph cannot expand into an unbounded string.
fn validate_cmap(bytes: &[u8]) -> Result<(usize, usize), WorkerFailure> {
    if bytes.len() > MAX_CMAP_BYTES {
        return Err(limit("tagged CMap exceeds the source byte limit"));
    }
    check_cmap_delimiters(bytes)?;
    let content =
        Content::decode_strict(bytes).map_err(|_| malformed("tagged CMap syntax is invalid"))?;
    let operations = &content.operations;
    if operations.len() > MAX_CMAP_MAPPINGS {
        return Err(limit("tagged CMap exceeds the operation limit"));
    }
    if operations.len() < 14 {
        return Err(unsupported("tagged CMap envelope is unsupported"));
    }
    if operations[0].operator != "findresource"
        || !matches!(operations[0].operands.as_slice(), [Object::Name(first), Object::Name(second)] if first == b"CIDInit" && second == b"ProcSet")
        || !empty_operation(&operations[1], "begin")
        || operations[2].operator != "dict"
        || !matches!(operations[2].operands.as_slice(), [Object::Integer(size)] if (1..=MAX_CMAP_MAPPINGS as i64).contains(size))
        || !empty_operation(&operations[3], "begin")
        || !empty_operation(&operations[4], "begincmap")
    {
        return Err(unsupported("tagged CMap envelope is unsupported"));
    }
    let mut at = 5usize;
    let mut definitions = BTreeSet::new();
    while operations
        .get(at)
        .is_some_and(|operation| operation.operator == "def")
    {
        let [Object::Name(name), value] = operations[at].operands.as_slice() else {
            return Err(malformed("invalid tagged CMap definition"));
        };
        if !definitions.insert(name.as_slice()) {
            return Err(malformed("duplicate tagged CMap definition"));
        }
        match name.as_slice() {
            b"CIDSystemInfo" => {
                let info = value
                    .as_dict()
                    .map_err(|_| malformed("invalid CMap system information"))?;
                check_keys(info, &[b"Registry", b"Ordering", b"Supplement"])?;
                if required(info, b"Registry")?.as_str().is_err()
                    || required(info, b"Ordering")?.as_str().is_err()
                    || required(info, b"Supplement")?.as_i64().is_err()
                {
                    return Err(malformed("invalid CMap system information fields"));
                }
            }
            b"CMapName" if value.as_name().is_ok() => {}
            b"CMapType" if value.as_i64().ok() == Some(2) => {}
            b"WMode" if value.as_i64().ok() == Some(0) => {}
            _ => return Err(unsupported("tagged CMap definition is unsupported")),
        }
        at += 1;
    }
    if ![
        b"CIDSystemInfo".as_slice(),
        b"CMapName".as_slice(),
        b"CMapType".as_slice(),
    ]
    .iter()
    .all(|name| definitions.contains(name))
    {
        return Err(malformed("tagged CMap lacks required definitions"));
    }
    let start = operations
        .get(at)
        .ok_or_else(|| malformed("missing CMap codespace"))?;
    if start.operator != "begincodespacerange"
        || !matches!(start.operands.as_slice(), [Object::Integer(1)])
    {
        return Err(unsupported(
            "tagged CMap must use one fixed-width codespace",
        ));
    }
    let end = operations
        .get(at + 1)
        .ok_or_else(|| malformed("missing CMap codespace end"))?;
    let [lower, upper] = end.operands.as_slice() else {
        return Err(malformed("invalid tagged CMap codespace"));
    };
    if end.operator != "endcodespacerange" {
        return Err(malformed("unbalanced tagged CMap codespace"));
    }
    let lower_bytes = cmap_string(lower)?;
    if !matches!(lower_bytes.len(), 1 | 2) {
        return Err(unsupported("tagged CMap source codes exceed two bytes"));
    }
    let width = lower_bytes.len();
    let lower = cmap_source(lower, width)?;
    let upper = cmap_source(upper, width)?;
    if lower > upper {
        return Err(malformed("reversed tagged CMap codespace"));
    }
    at += 2;
    let mut mapped = BTreeSet::new();
    while let Some(begin) = operations.get(at) {
        if begin.operator == "endcmap" {
            break;
        }
        if !matches!(begin.operator.as_str(), "beginbfchar" | "beginbfrange") {
            return Err(unsupported("tagged CMap operator is unsupported"));
        }
        let [Object::Integer(count)] = begin.operands.as_slice() else {
            return Err(malformed("invalid tagged CMap section count"));
        };
        if *count < 1 || *count as u64 > MAX_CMAP_MAPPINGS as u64 {
            return Err(limit("tagged CMap section exceeds the mapping limit"));
        }
        let count = *count as usize;
        let end = operations
            .get(at + 1)
            .ok_or_else(|| malformed("unclosed tagged CMap section"))?;
        match (begin.operator.as_str(), end.operator.as_str()) {
            ("beginbfchar", "endbfchar") => {
                if end.operands.len() != count * 2 {
                    return Err(malformed("tagged bfchar count disagrees with operands"));
                }
                for pair in end.operands.chunks_exact(2) {
                    let code = cmap_source(&pair[0], width)?;
                    cmap_destination(&pair[1])?;
                    insert_cmap_code(&mut mapped, code, lower, upper)?;
                }
            }
            ("beginbfrange", "endbfrange") => {
                if end.operands.len() != count * 3 {
                    return Err(malformed("tagged bfrange count disagrees with operands"));
                }
                for triple in end.operands.chunks_exact(3) {
                    let start = cmap_source(&triple[0], width)?;
                    let end = cmap_source(&triple[1], width)?;
                    if end < start {
                        return Err(malformed("reversed tagged CMap source range"));
                    }
                    let count = (end - start) as usize + 1;
                    if mapped.len().saturating_add(count) > MAX_CMAP_MAPPINGS {
                        return Err(limit(
                            "tagged CMap expanded range exceeds the mapping limit",
                        ));
                    }
                    match &triple[2] {
                        Object::Array(destinations) => {
                            if destinations.len() != count {
                                return Err(malformed(
                                    "tagged CMap destination array has the wrong length",
                                ));
                            }
                            for destination in destinations {
                                cmap_destination(destination)?;
                            }
                        }
                        destination => {
                            let first = u32::from(cmap_destination(destination)?);
                            let last = first + count as u32 - 1;
                            if last > u16::MAX as u32
                                || first <= 0xdfff && last >= 0xd800
                                || first <= 0xfffd && last >= 0xfffd
                            {
                                return Err(unsupported(
                                    "tagged CMap destination range is unsupported",
                                ));
                            }
                        }
                    }
                    for code in start..=end {
                        insert_cmap_code(&mut mapped, code, lower, upper)?;
                    }
                }
            }
            _ => return Err(unsupported("tagged CMap operator is unsupported")),
        }
        at += 2;
    }
    let tail = &operations[at..];
    if tail.len() != 7
        || !empty_operation(&tail[0], "endcmap")
        || !empty_operation(&tail[1], "CMapName")
        || !empty_operation(&tail[2], "currentdict")
        || tail[3].operator != "defineresource"
        || !matches!(tail[3].operands.as_slice(), [Object::Name(name)] if name == b"CMap")
        || !empty_operation(&tail[4], "pop")
        || !empty_operation(&tail[5], "end")
        || !empty_operation(&tail[6], "end")
        || mapped.is_empty()
    {
        return Err(unsupported("tagged CMap trailer is unsupported"));
    }
    Ok((width, mapped.len()))
}

fn empty_operation(operation: &Operation, operator: &str) -> bool {
    operation.operator == operator && operation.operands.is_empty()
}

fn cmap_string(object: &Object) -> Result<&[u8], WorkerFailure> {
    match object {
        Object::String(bytes, lopdf::StringFormat::Hexadecimal) => Ok(bytes),
        _ => Err(unsupported("tagged CMap codes must be hexadecimal strings")),
    }
}

fn cmap_source(object: &Object, width: usize) -> Result<u32, WorkerFailure> {
    let bytes = cmap_string(object)?;
    if bytes.len() != width {
        return Err(unsupported("tagged CMap mixes source code widths"));
    }
    Ok(bytes
        .iter()
        .fold(0u32, |value, byte| value * 256 + u32::from(*byte)))
}

fn cmap_destination(object: &Object) -> Result<u16, WorkerFailure> {
    let bytes = cmap_string(object)?;
    let [high, low] = bytes else {
        return Err(unsupported(
            "tagged CMap destination has multiple Unicode units",
        ));
    };
    let value = u16::from_be_bytes([*high, *low]);
    if value == 0 || value == 0xfffd || (0xd800..=0xdfff).contains(&value) {
        return Err(unsupported(
            "tagged CMap destination is not a supported scalar",
        ));
    }
    Ok(value)
}

fn insert_cmap_code(
    mapped: &mut BTreeSet<u32>,
    code: u32,
    lower: u32,
    upper: u32,
) -> Result<(), WorkerFailure> {
    if code < lower || code > upper || !mapped.insert(code) {
        return Err(malformed(
            "tagged CMap has overlapping or out-of-codespace mappings",
        ));
    }
    if mapped.len() > MAX_CMAP_MAPPINGS {
        return Err(limit("tagged CMap exceeds the mapping limit"));
    }
    Ok(())
}

fn check_cmap_delimiters(bytes: &[u8]) -> Result<(), WorkerFailure> {
    let mut depth = 0usize;
    let mut index = 0usize;
    let mut comment = false;
    let mut literal = false;
    let mut hex = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if comment {
            comment = !matches!(byte, b'\r' | b'\n');
        } else if literal {
            match byte {
                b')' => literal = false,
                b'(' | b'\\' => {
                    return Err(unsupported("complex CMap literal strings are unsupported"));
                }
                _ if !byte.is_ascii() => {
                    return Err(unsupported("non-ASCII CMap metadata is unsupported"));
                }
                _ => {}
            }
        } else if hex {
            if byte == b'>' {
                hex = false;
            } else if !byte.is_ascii_hexdigit() && !byte.is_ascii_whitespace() {
                return Err(malformed("invalid CMap hexadecimal string"));
            }
        } else {
            match byte {
                b'%' => comment = true,
                b'(' => literal = true,
                b'<' if bytes.get(index + 1) == Some(&b'<') => {
                    depth += 1;
                    index += 1;
                }
                b'<' => hex = true,
                b'>' if bytes.get(index + 1) == Some(&b'>') => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| malformed("unbalanced CMap dictionary"))?;
                    index += 1;
                }
                b'[' => depth += 1,
                b']' => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| malformed("unbalanced CMap array"))?
                }
                b'{' | b'}' => return Err(unsupported("CMap procedures are unsupported")),
                _ if !byte.is_ascii() => {
                    return Err(unsupported("non-ASCII CMap syntax is unsupported"));
                }
                _ => {}
            }
            if depth > 4 {
                return Err(limit("tagged CMap nesting exceeds the depth limit"));
            }
        }
        index += 1;
    }
    if literal || hex || depth != 0 {
        return Err(malformed("unbalanced tagged CMap delimiters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn synthetic_cmap(codespace: &str, mappings: &str) -> Vec<u8> {
        format!(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
             /CIDSystemInfo << /Registry (Synthetic) /Ordering (UCS) /Supplement 0 >> def\n\
             /CMapName /Synthetic-Identity-UCS def\n/CMapType 2 def\n\
             1 begincodespacerange\n{codespace}\nendcodespacerange\n\
             {mappings}\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n"
        )
        .into_bytes()
    }

    #[test]
    fn accepts_small_one_and_two_byte_scalar_cmaps() {
        let one = synthetic_cmap("<00> <ff>", "1 beginbfchar\n<41> <0041>\nendbfchar");
        assert_eq!(validate_cmap(&one).unwrap(), (1, 1));
        let two = synthetic_cmap("<0000> <ffff>", "1 beginbfchar\n<0001> <03a9>\nendbfchar");
        assert_eq!(validate_cmap(&two).unwrap(), (2, 1));
    }

    #[test]
    fn accepts_bounded_scalar_and_array_ranges() {
        let bytes = synthetic_cmap(
            "<0000> <ffff>",
            "2 beginbfrange\n<0001> <0002> <0041>\n<0003> <0004> [<0043> <0044>]\nendbfrange",
        );
        assert_eq!(validate_cmap(&bytes).unwrap(), (2, 4));
    }

    #[test]
    fn expanded_mapping_count_is_checked_at_the_boundary() {
        let exact = synthetic_cmap(
            "<0000> <ffff>",
            "1 beginbfrange\n<0001> <4000> <0001>\nendbfrange",
        );
        assert_eq!(validate_cmap(&exact).unwrap(), (2, MAX_CMAP_MAPPINGS));
        let over = synthetic_cmap(
            "<0000> <ffff>",
            "1 beginbfrange\n<0001> <4001> <0001>\nendbfrange",
        );
        assert_eq!(
            validate_cmap(&over).unwrap_err().code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }

    #[test]
    fn rejects_four_byte_full_range_before_lopdf_expansion() {
        let bytes = synthetic_cmap(
            "<00000000> <ffffffff>",
            "1 beginbfrange\n<00000000> <ffffffff> <0041>\nendbfrange",
        );
        assert_eq!(
            validate_cmap(&bytes).unwrap_err().code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
        );
    }

    #[test]
    fn rejects_multi_unit_destinations_before_lopdf_expansion() {
        let bytes = synthetic_cmap(
            "<0000> <ffff>",
            "1 beginbfchar\n<0001> <00410042>\nendbfchar",
        );
        assert_eq!(
            validate_cmap(&bytes).unwrap_err().code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
        );
    }

    #[test]
    fn rejects_unknown_cmap_grammar_and_overlapping_codes() {
        let unknown = synthetic_cmap("<0000> <ffff>", "/OtherMap usecmap");
        assert_eq!(
            validate_cmap(&unknown).unwrap_err().code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
        );
        let overlap = synthetic_cmap(
            "<0000> <ffff>",
            "2 beginbfchar\n<0001> <0041>\n<0001> <0042>\nendbfchar",
        );
        assert_eq!(
            validate_cmap(&overlap).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
    }

    #[test]
    fn rejects_wrong_section_counts_and_destination_lengths() {
        let count = synthetic_cmap("<0000> <ffff>", "2 beginbfchar\n<0001> <0041>\nendbfchar");
        assert_eq!(
            validate_cmap(&count).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
        let array = synthetic_cmap(
            "<0000> <ffff>",
            "1 beginbfrange\n<0001> <0002> [<0041>]\nendbfrange",
        );
        assert_eq!(
            validate_cmap(&array).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
    }

    #[test]
    fn source_and_nesting_limits_precede_cmap_parsing() {
        assert_eq!(
            validate_cmap(&vec![b' '; MAX_CMAP_BYTES + 1])
                .unwrap_err()
                .code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
        assert_eq!(
            check_cmap_delimiters(b"[[[[[0]]]]]").unwrap_err().code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }

    #[test]
    fn text_output_preflight_checks_before_decoding() {
        let document = Document::new();
        let font =
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
        let decoder =
            bounded_font_encoding(&document, &font, &mut PdfDecodeBudget::default()).unwrap();
        assert!(decoder.preflight(b"AAAA", 12).is_ok());
        assert_eq!(
            decoder.preflight(b"AAAA", 11).unwrap_err().code(),
            WorkerFailureCode::InspectionResourceLimitExceeded
        );
    }

    #[test]
    fn validates_pagination_artifact_properties() {
        let header = dictionary! { "Type" => "Pagination", "Subtype" => "Header", "Attached" => vec![Object::Name(b"Top".to_vec())] };
        assert!(validate_artifact_properties(&header).is_ok());
        let footer = dictionary! { "Type" => "Pagination", "Subtype" => "Footer", "Attached" => vec![Object::Name(b"Bottom".to_vec())] };
        assert!(validate_artifact_properties(&footer).is_ok());
        assert!(validate_artifact_properties(&Dictionary::new()).is_ok());
        let unknown = dictionary! { "Type" => "Unknown" };
        assert_eq!(
            validate_artifact_properties(&unknown).unwrap_err().code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
        );
        let wrong_type = dictionary! { "Type" => 1 };
        assert_eq!(
            validate_artifact_properties(&wrong_type)
                .unwrap_err()
                .code(),
            WorkerFailureCode::ParserDisagreement
        );
        let wrong_edge = dictionary! { "Type" => "Pagination", "Subtype" => "Header", "Attached" => vec![Object::Name(b"Bottom".to_vec())] };
        assert_eq!(
            validate_artifact_properties(&wrong_edge)
                .unwrap_err()
                .code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
        );
    }

    #[test]
    fn rejects_unqualified_or_nonfinite_artifact_bounds() {
        let nonfinite = dictionary! { "BBox" => vec![Object::Integer(0), Object::Integer(0), Object::Real(f32::INFINITY), Object::Integer(1)] };
        assert_eq!(
            validate_artifact_properties(&nonfinite).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
        let finite = dictionary! { "BBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(1), Object::Integer(1)] };
        assert_eq!(
            validate_artifact_properties(&finite).unwrap_err().code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
        );
    }

    #[test]
    fn delimiter_prepass_distinguishes_malformed_from_balanced_nested_marks() {
        let operations = |names: &[&str]| {
            names
                .iter()
                .map(|name| Operation {
                    operator: (*name).into(),
                    operands: Vec::new(),
                })
                .collect::<Vec<_>>()
        };
        let missing_end = operations(&["BDC", "BDC", "EMC"]);
        assert_eq!(
            validate_marked_delimiters(&missing_end).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
        let extra_end = operations(&["EMC", "BMC", "EMC"]);
        assert_eq!(
            validate_marked_delimiters(&extra_end).unwrap_err().code(),
            WorkerFailureCode::ParserDisagreement
        );
        // Balanced nesting proceeds to the separate semantic ownership check.
        let balanced = operations(&["BDC", "BDC", "EMC", "EMC"]);
        assert!(validate_marked_delimiters(&balanced).is_ok());
    }
}
