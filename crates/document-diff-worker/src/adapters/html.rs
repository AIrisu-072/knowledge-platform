use std::collections::HashMap;

use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, FormatId, SourceLocator, UnverifiedReason,
    WorkerChange, WorkerDiffRequest, WorkerDiffResponse, WorkerUnverifiedRegion,
};
use encoding_rs::UTF_8;
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::WorkerError;

const MAX_HTML_COMBINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 256;
const MAX_NODES: usize = 100_000;
const MAX_VISITS: usize = 1_000_000;
const PARSER_PROVENANCE: &str = "document-diff-html-v0;encoding_rs=0.8.41;html5ever=0.39.0;markup5ever_rcdom=0.39.0;unicode-normalization=0.1.25";

#[derive(Debug, Clone, Copy, Default)]
pub struct HtmlComparator;

#[derive(Debug, PartialEq, Eq)]
struct SemanticNode {
    tag: String,
    text: String,
    href: Option<String>,
    src: Option<String>,
    alt: Option<String>,
    path: String,
}

impl SemanticNode {
    fn same_meaning(&self, other: &Self) -> bool {
        self.tag == other.tag
            && self.text == other.text
            && self.href == other.href
            && self.src == other.src
            && self.alt == other.alt
    }

    fn locator(&self) -> SourceLocator {
        SourceLocator::HtmlNode {
            path: self.path.clone(),
        }
    }
}

struct ParsedHtml {
    nodes: Vec<SemanticNode>,
}

impl HtmlComparator {
    pub fn compare(
        request: &WorkerDiffRequest,
        base: &[u8],
        target: &[u8],
        budget: &mut ComparisonBudget,
    ) -> Result<WorkerDiffResponse, WorkerError> {
        if request.format != FormatId::Html {
            return Err(WorkerError::InvalidRequest);
        }
        request
            .validate()
            .map_err(|_| WorkerError::ResourceLimit("source bytes"))?;
        if base.len() as u64 != request.base_size_bytes
            || target.len() as u64 != request.target_size_bytes
            || <[u8; 32]>::from(Sha256::digest(base)) != request.base_raw_sha256
            || <[u8; 32]>::from(Sha256::digest(target)) != request.target_raw_sha256
        {
            return Err(WorkerError::RawBindingMismatch);
        }
        if base.len().saturating_add(target.len()) > MAX_HTML_COMBINED_BYTES {
            return Ok(unverified(request, UnverifiedReason::ResourceLimit));
        }
        let base = match parse_semantic_nodes(base) {
            Ok(parsed) => parsed,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        let target = match parse_semantic_nodes(target) {
            Ok(parsed) => parsed,
            Err(reason) => return Ok(unverified(request, reason)),
        };
        if base.nodes.is_empty() || target.nodes.is_empty() {
            return Ok(unverified(
                request,
                UnverifiedReason::UnsupportedSemanticConstruct,
            ));
        }
        match compare_nodes(&base.nodes, &target.nodes, budget) {
            Ok(changes) => Ok(response(request, DiffCoverage::Full, changes, Vec::new())),
            Err(reason) => Ok(unverified(request, reason)),
        }
    }
}

fn parse_semantic_nodes(input: &[u8]) -> Result<ParsedHtml, UnverifiedReason> {
    let decoded = UTF_8
        .decode_without_bom_handling_and_without_replacement(input)
        .ok_or(UnverifiedReason::CorruptedSource)?;
    let dom = parse_document(RcDom::default(), Default::default()).one(decoded.as_ref());
    let mut nodes = Vec::new();
    let mut visits = 0;
    walk(&dom.document, "", 0, &mut visits, &mut nodes)?;
    Ok(ParsedHtml { nodes })
}

fn walk(
    handle: &Handle,
    path: &str,
    depth: usize,
    visits: &mut usize,
    output: &mut Vec<SemanticNode>,
) -> Result<(), UnverifiedReason> {
    charge_visit(visits, depth)?;
    if let NodeData::Element { name, attrs, .. } = &handle.data {
        let tag = name.local.as_ref();
        if matches!(tag, "script" | "style" | "noscript") {
            return Ok(());
        }
        if is_semantic_tag(tag) {
            if output.len() >= MAX_NODES {
                return Err(UnverifiedReason::ResourceLimit);
            }
            let attrs = attrs.borrow();
            let href = if tag == "a" {
                attrs
                    .iter()
                    .find(|attr| attr.name.local.as_ref() == "href")
                    .map(|attr| normalize_uri(attr.value.as_ref()))
            } else {
                None
            };
            let (src, alt) = if tag == "img" {
                (
                    attrs
                        .iter()
                        .find(|attr| attr.name.local.as_ref() == "src")
                        .map(|attr| normalize_uri(attr.value.as_ref())),
                    attrs
                        .iter()
                        .find(|attr| attr.name.local.as_ref() == "alt")
                        .map(|attr| normalize_text_value(attr.value.as_ref())),
                )
            } else {
                (None, None)
            };
            drop(attrs);
            let mut text = String::new();
            collect_own_text(handle, true, depth, visits, &mut text)?;
            output.push(SemanticNode {
                tag: tag.to_owned(),
                text: normalize_text_value(&text),
                href,
                src,
                alt,
                path: path.to_owned(),
            });
        }
    }
    let mut per_tag_index = HashMap::<String, usize>::new();
    for child in handle.children.borrow().iter() {
        let child_path = if let NodeData::Element { name, .. } = &child.data {
            let tag = name.local.as_ref();
            let index = per_tag_index.entry(tag.to_owned()).or_default();
            *index += 1;
            format!("{path}/{tag}[{index}]")
        } else {
            path.to_owned()
        };
        walk(child, &child_path, depth + 1, visits, output)?;
    }
    Ok(())
}

fn collect_own_text(
    handle: &Handle,
    is_root: bool,
    depth: usize,
    visits: &mut usize,
    output: &mut String,
) -> Result<(), UnverifiedReason> {
    charge_visit(visits, depth)?;
    match &handle.data {
        NodeData::Text { contents } => output.push_str(contents.borrow().as_ref()),
        NodeData::Element { name, .. }
            if matches!(name.local.as_ref(), "script" | "style" | "noscript")
                || (!is_root && is_semantic_tag(name.local.as_ref())) =>
        {
            return Ok(());
        }
        _ => {}
    }
    for child in handle.children.borrow().iter() {
        collect_own_text(child, false, depth + 1, visits, output)?;
    }
    Ok(())
}

fn charge_visit(visits: &mut usize, depth: usize) -> Result<(), UnverifiedReason> {
    *visits += 1;
    if depth > MAX_DEPTH || *visits > MAX_VISITS {
        return Err(UnverifiedReason::ResourceLimit);
    }
    Ok(())
}

fn is_semantic_tag(tag: &str) -> bool {
    matches!(
        tag,
        "title"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "tr"
            | "th"
            | "td"
            | "a"
            | "img"
    )
}

fn normalize_text_value(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfc()
        .collect()
}

fn normalize_uri(value: &str) -> String {
    let trimmed = value.trim();
    let Some((scheme, remainder)) = trimmed.split_once("://") else {
        return trimmed.to_owned();
    };
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() {
        return trimmed.to_owned();
    }
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(userinfo, host_port)| {
            (Some(userinfo), host_port)
        });
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (host, Some(port))
        }
        _ => (host_port, None),
    };
    if host.is_empty() {
        return trimmed.to_owned();
    }
    let normalized_scheme = scheme.to_ascii_lowercase();
    let normalized_host = host.to_ascii_lowercase();
    let default_port = match normalized_scheme.as_str() {
        "http" => Some("80"),
        "https" => Some("443"),
        _ => None,
    };
    let port = port
        .filter(|port| Some(*port) != default_port)
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let userinfo = userinfo
        .map(|value| format!("{value}@"))
        .unwrap_or_default();
    let suffix = &remainder[authority_end..];
    format!("{normalized_scheme}://{userinfo}{normalized_host}{port}{suffix}")
}

fn compare_nodes(
    base: &[SemanticNode],
    target: &[SemanticNode],
    budget: &mut ComparisonBudget,
) -> Result<Vec<WorkerChange>, UnverifiedReason> {
    budget
        .consume_candidates((base.len() + target.len()) as u64)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    if base.len() == target.len() && base.iter().zip(target).all(|(old, new)| old.tag == new.tag) {
        let mut changes = Vec::new();
        for (old, new) in base.iter().zip(target) {
            for (different, facet, reason) in [
                (old.text != new.text, "html_text", "visible_text_changed"),
                (old.href != new.href, "html_link", "link_target_changed"),
                (
                    old.src != new.src || old.alt != new.alt,
                    "html_image",
                    "image_source_or_alt_changed",
                ),
            ] {
                if different {
                    add_change(
                        &mut changes,
                        budget,
                        ChangeOperation::Modified,
                        facet,
                        Some(old),
                        Some(new),
                        reason,
                    )?;
                }
            }
        }
        return Ok(changes);
    }

    let mut prefix = 0;
    while prefix < base.len() && prefix < target.len() && base[prefix].same_meaning(&target[prefix])
    {
        prefix += 1;
    }
    let mut base_end = base.len();
    let mut target_end = target.len();
    while base_end > prefix
        && target_end > prefix
        && base[base_end - 1].same_meaning(&target[target_end - 1])
    {
        base_end -= 1;
        target_end -= 1;
    }
    let old = &base[prefix..base_end];
    let new = &target[prefix..target_end];
    let mut changes = Vec::new();
    if old.len() == 1 && new.len() == 1 {
        add_change(
            &mut changes,
            budget,
            ChangeOperation::Modified,
            "html_structure",
            Some(&old[0]),
            Some(&new[0]),
            "semantic_node_changed",
        )?;
    } else if old.is_empty() {
        for node in new {
            add_change(
                &mut changes,
                budget,
                ChangeOperation::Added,
                "html_structure",
                None,
                Some(node),
                "semantic_node_added",
            )?;
        }
    } else if new.is_empty() {
        for node in old {
            add_change(
                &mut changes,
                budget,
                ChangeOperation::Removed,
                "html_structure",
                Some(node),
                None,
                "semantic_node_removed",
            )?;
        }
    } else {
        return Err(UnverifiedReason::AmbiguousAlignment);
    }
    Ok(changes)
}

fn add_change(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    operation: ChangeOperation,
    facet: &str,
    base: Option<&SemanticNode>,
    target: Option<&SemanticNode>,
    reason: &str,
) -> Result<(), UnverifiedReason> {
    budget
        .consume_changes(1)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    changes.push(WorkerChange {
        operation: Some(operation),
        relocation: None,
        facet: facet.to_owned(),
        base: base.map(SemanticNode::locator),
        target: target.map(SemanticNode::locator),
        reason_code: reason.to_owned(),
    });
    Ok(())
}

fn unverified(request: &WorkerDiffRequest, reason: UnverifiedReason) -> WorkerDiffResponse {
    response(
        request,
        DiffCoverage::None,
        Vec::new(),
        vec![WorkerUnverifiedRegion {
            base: Some(SourceLocator::ContentItem),
            target: Some(SourceLocator::ContentItem),
            reason,
            navigation_hint: Some("原本の両側を確認してください".to_owned()),
        }],
    )
}

fn response(
    request: &WorkerDiffRequest,
    coverage: DiffCoverage,
    changes: Vec<WorkerChange>,
    unverified_regions: Vec<WorkerUnverifiedRegion>,
) -> WorkerDiffResponse {
    WorkerDiffResponse {
        protocol_version: request.protocol_version,
        diff_profile_version: request.diff_profile_version,
        resource_profile_version: request.resource_profile_version,
        base_raw_sha256: request.base_raw_sha256,
        base_size_bytes: request.base_size_bytes,
        target_raw_sha256: request.target_raw_sha256,
        target_size_bytes: request.target_size_bytes,
        format: request.format,
        coverage,
        changes,
        unverified_regions,
        ancillary_changes: vec![],
        parser_provenance: PARSER_PROVENANCE.to_owned(),
    }
}
