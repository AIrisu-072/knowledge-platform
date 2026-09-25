use document_semantic_inspection_core::FormatId;
use encoding_rs::UTF_8;
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::{WorkerFailure, WorkerFailureCode};

use super::{AdapterProfile, SemanticAdapter, SemanticAdapterOutput, canonical_json_bytes};

const PARSER_LIBRARIES: [(&str, &str); 4] = [
    ("encoding_rs", "0.8.41"),
    ("html5ever", "0.39.0"),
    ("markup5ever_rcdom", "0.39.0"),
    ("unicode-normalization", "0.1.25"),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct HtmlAdapter;

#[derive(Debug, Serialize)]
struct HtmlSemanticNode {
    tag: String,
    text: String,
    href: Option<String>,
    src: Option<String>,
    alt: Option<String>,
}

impl SemanticAdapter for HtmlAdapter {
    fn format(&self) -> FormatId {
        FormatId::Html
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &AdapterProfile,
    ) -> Result<SemanticAdapterOutput, WorkerFailure> {
        if profile.html_script_required() {
            return Err(WorkerFailure::new(
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "HTML semantics require script execution",
            ));
        }

        let decoded = UTF_8
            .decode_without_bom_handling_and_without_replacement(input)
            .ok_or_else(|| {
                WorkerFailure::new(
                    WorkerFailureCode::SemanticExtractionFailed,
                    "invalid UTF-8 HTML",
                )
            })?;

        let dom = parse_document(RcDom::default(), Default::default()).one(decoded.as_ref());
        let mut nodes = Vec::new();
        walk_semantic_nodes(&dom.document, &mut nodes);
        let semantic_projection = canonical_json_bytes(&nodes).map_err(|error| {
            WorkerFailure::new(
                WorkerFailureCode::InvalidWorkerResult,
                format!("HTML semantic projection could not be serialized: {error}"),
            )
        })?;

        Ok(SemanticAdapterOutput::from_projection(
            &semantic_projection,
            &["reader_content"],
            "html",
            &PARSER_LIBRARIES,
        ))
    }
}

fn walk_semantic_nodes(handle: &Handle, output: &mut Vec<HtmlSemanticNode>) {
    if let NodeData::Element { name, attrs, .. } = &handle.data {
        let tag = name.local.as_ref();
        if matches!(tag, "script" | "style" | "noscript") {
            return;
        }

        if is_semantic_tag(tag) {
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

            output.push(HtmlSemanticNode {
                tag: tag.to_owned(),
                text: normalized_descendant_text(handle),
                href,
                src,
                alt,
            });
        }
    }

    for child in handle.children.borrow().iter() {
        walk_semantic_nodes(child, output);
    }
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

fn normalized_descendant_text(handle: &Handle) -> String {
    let mut raw = String::new();
    collect_text(handle, &mut raw);
    normalize_text_value(&raw)
}

fn collect_text(handle: &Handle, output: &mut String) {
    match &handle.data {
        NodeData::Text { contents } => output.push_str(contents.borrow().as_ref()),
        NodeData::Element { name, .. }
            if matches!(name.local.as_ref(), "script" | "style" | "noscript") =>
        {
            return;
        }
        _ => {}
    }

    for child in handle.children.borrow().iter() {
        collect_text(child, output);
    }
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
