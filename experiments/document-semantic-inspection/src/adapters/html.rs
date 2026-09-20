use crate::{
    canonical_json_bytes, AdapterOutput, FormatId, InspectionAdapter, InspectionProfile, PocError,
};
use encoding_rs::UTF_8;
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use url::Url;

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

impl InspectionAdapter for HtmlAdapter {
    fn format(&self) -> FormatId {
        FormatId::Html
    }

    fn inspect(
        &self,
        input: &[u8],
        profile: &InspectionProfile,
    ) -> Result<AdapterOutput, PocError> {
        if profile.html_script_required {
            return Err(PocError::UnsupportedSemanticConstruct(
                "HTML semantics require script execution".into(),
            ));
        }

        let decoded = UTF_8
            .decode_without_bom_handling_and_without_replacement(input)
            .ok_or_else(|| PocError::SemanticExtractionFailed("invalid UTF-8 HTML".into()))?;

        let dom = parse_document(RcDom::default(), Default::default()).one(decoded.as_ref());
        let mut nodes = Vec::new();
        walk_semantic_nodes(&dom.document, &mut nodes);

        let projection = canonical_json_bytes(&nodes)
            .map_err(|error| PocError::InvalidWorkerResult(error.to_string()))?;
        Ok(AdapterOutput::projection_only(projection))
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
    Url::parse(trimmed)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| trimmed.to_owned())
}
