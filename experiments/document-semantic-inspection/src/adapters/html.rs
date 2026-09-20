use crate::{
    canonical_json_bytes, AdapterOutput, FormatId, InspectionAdapter, InspectionProfile, PocError,
};
use encoding_rs::UTF_8;
use scraper::{Html, Selector};
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
        let document = Html::parse_document(decoded.as_ref());
        let selector = Selector::parse(
            "title,h1,h2,h3,h4,h5,h6,p,ul,ol,li,table,tr,th,td,a,img",
        )
        .map_err(|error| {
            PocError::InvalidWorkerResult(format!("static semantic selector invalid: {error}"))
        })?;

        let mut nodes = Vec::new();
        for element in document.select(&selector) {
            let tag = element.value().name().to_ascii_lowercase();
            let text = normalize_text(element.text());
            let href = if tag == "a" {
                element.value().attr("href").map(normalize_uri)
            } else {
                None
            };
            let (src, alt) = if tag == "img" {
                (
                    element.value().attr("src").map(normalize_uri),
                    element.value().attr("alt").map(normalize_text_value),
                )
            } else {
                (None, None)
            };

            nodes.push(HtmlSemanticNode {
                tag,
                text,
                href,
                src,
                alt,
            });
        }

        let projection = canonical_json_bytes(&nodes)
            .map_err(|error| PocError::InvalidWorkerResult(error.to_string()))?;
        Ok(AdapterOutput::projection_only(projection))
    }
}

fn normalize_text<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    normalize_text_value(&parts.collect::<Vec<_>>().join(" "))
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
