use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Criticality {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Requirement {
    pub id: String,
    pub kind: String,
    pub criticality: Criticality,
    pub domain: String,
    pub source: PathBuf,
    pub section: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    id: String,
    kind: String,
    criticality: Criticality,
    domain: String,
}

#[derive(Debug, Error)]
pub enum RequirementError {
    #[error("requirement I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid requirement metadata: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("duplicate requirement id: {0}")]
    Duplicate(String),
}

pub fn extract(
    markdown: &str,
    source: impl AsRef<Path>,
) -> Result<Vec<Requirement>, RequirementError> {
    let source = source.as_ref().to_path_buf();
    let mut output = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut heading: Option<String> = None;
    let mut requirement: Option<String> = None;

    for event in Parser::new_ext(markdown, Options::all()) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(value) = heading.take() {
                    let value = value.trim().to_string();
                    if !value.is_empty() {
                        current_heading = Some(value);
                    }
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.as_ref().trim() == "requirement" =>
            {
                requirement = Some(String::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(body) = requirement.take() {
                    let metadata: Metadata = toml::from_str(&body)?;
                    output.push(Requirement {
                        id: metadata.id,
                        kind: metadata.kind,
                        criticality: metadata.criticality,
                        domain: metadata.domain,
                        source: source.clone(),
                        section: current_heading.clone(),
                    });
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(buffer) = requirement.as_mut() {
                    buffer.push_str(&text);
                } else if let Some(buffer) = heading.as_mut() {
                    buffer.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(buffer) = requirement.as_mut() {
                    buffer.push('\n');
                } else if let Some(buffer) = heading.as_mut() {
                    buffer.push(' ');
                }
            }
            _ => {}
        }
    }

    Ok(output)
}

pub fn scan(root: &Path) -> Result<Vec<Requirement>, RequirementError> {
    let mut files = Vec::new();
    collect_markdown(&root.join("spec"), &mut files)?;
    files.sort();

    let mut ids = HashSet::new();
    let mut output = Vec::new();
    for path in files {
        let markdown = fs::read_to_string(&path)?;
        let relative = path.strip_prefix(root).unwrap_or(&path);
        for item in extract(&markdown, relative)? {
            if !ids.insert(item.id.clone()) {
                return Err(RequirementError::Duplicate(item.id));
            }
            output.push(item);
        }
    }
    Ok(output)
}

fn collect_markdown(dir: &Path, output: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_markdown(&path, output)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
            output.push(path);
        }
    }
    Ok(())
}
