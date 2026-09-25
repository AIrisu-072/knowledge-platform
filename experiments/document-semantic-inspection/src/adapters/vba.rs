use crate::vba_language::VBA_LANGUAGE;
use crate::PocError;
use serde_json::{json, Value};
use std::io::{Cursor, Read};
use tree_sitter::{Node, Parser};
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, Default)]
pub struct VbaAdapter;

#[derive(Debug)]
pub(crate) struct VbaInspection {
    pub projection: Value,
    pub module_count: usize,
}

impl VbaAdapter {
    pub fn canonicalize_source(source: &str) -> Result<Vec<u8>, PocError> {
        let mut parser = Parser::new();
        let language: tree_sitter::Language = VBA_LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("VBA grammar ABI mismatch: {error}")))?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| PocError::SemanticExtractionFailed("VBA parser returned no tree".into()))?;
        let root = tree.root_node();
        if root.has_error() || contains_recovery_node(root) {
            return Err(PocError::SemanticExtractionFailed(
                "VBA syntax recovery node".into(),
            ));
        }

        let mut tokens = Vec::<(String, String)>::new();
        collect_tokens(root, source.as_bytes(), &mut tokens)?;
        serde_json::to_vec(&tokens)
            .map_err(|error| PocError::InvalidWorkerResult(format!("VBA canonical JSON: {error}")))
    }

    pub(crate) fn inspect_xlsm(input: &[u8]) -> Result<VbaInspection, PocError> {
        let mut archive = ZipArchive::new(Cursor::new(input))
            .map_err(|error| PocError::SemanticExtractionFailed(format!("XLSM ZIP: {error}")))?;
        let mut vba_bytes = Vec::new();
        archive
            .by_name("xl/vbaProject.bin")
            .map_err(|_| PocError::SemanticExtractionFailed("XLSM missing xl/vbaProject.bin".into()))?
            .read_to_end(&mut vba_bytes)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("read vbaProject.bin: {error}")))?;

        let project = ovba::open_project(vba_bytes)
            .map_err(|error| PocError::SemanticExtractionFailed(format!("MS-OVBA parse: {error}")))?;
        if project.modules.is_empty() {
            return Err(PocError::SemanticExtractionFailed(
                "VBA project exposes no modules".into(),
            ));
        }

        let mut modules = Vec::with_capacity(project.modules.len());
        for module in &project.modules {
            let source = project
                .module_source(&module.name)
                .map_err(|error| PocError::SemanticExtractionFailed(format!(
                    "VBA module {} extraction: {error}",
                    module.name
                )))?;
            let canonical = Self::canonicalize_source(&source)?;
            let syntax: Value = serde_json::from_slice(&canonical).map_err(|error| {
                PocError::InvalidWorkerResult(format!("VBA canonical projection decode: {error}"))
            })?;
            modules.push(json!({
                "name": module.name,
                "stream_name": module.stream_name,
                "module_type": format!("{:?}", module.module_type),
                "read_only": module.read_only,
                "private": module.private,
                "syntax": syntax,
            }));
        }
        modules.sort_by(|left, right| {
            left["name"].as_str().unwrap_or_default().cmp(right["name"].as_str().unwrap_or_default())
        });

        let mut references: Vec<String> = project
            .references
            .iter()
            .map(|reference| format!("{reference:?}"))
            .collect();
        references.sort();

        Ok(VbaInspection {
            module_count: modules.len(),
            projection: json!({
                "system_kind": format!("{:?}", project.information.sys_kind),
                "code_page": project.information.code_page,
                "references": references,
                "modules": modules,
            }),
        })
    }
}

fn contains_recovery_node(node: Node<'_>) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    (0..node.child_count()).any(|index| {
        node.child(index)
            .is_some_and(contains_recovery_node)
    })
}

fn collect_tokens(
    node: Node<'_>,
    source: &[u8],
    output: &mut Vec<(String, String)>,
) -> Result<(), PocError> {
    if node.kind() == "comment" {
        return Ok(());
    }
    if node.child_count() != 0 {
        for index in 0..node.child_count() {
            if let Some(child) = node.child(index) {
                collect_tokens(child, source, output)?;
            }
        }
        return Ok(());
    }

    let kind = node.kind();
    if kind.contains("newline") {
        return Ok(());
    }
    let raw = std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .map_err(|_| PocError::SemanticExtractionFailed("VBA parser produced non-UTF8 span".into()))?;
    if raw.trim().is_empty() {
        return Ok(());
    }

    let normalized_line_endings = raw.replace("\r\n", "\n").replace('\r', "\n");
    let value = match kind {
        "string_literal" | "date_literal" | "integer_literal" | "float_literal" => {
            normalized_line_endings
        }
        "class_header" => normalized_line_endings
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
        _ => normalized_line_endings.to_lowercase(),
    };
    output.push((kind.to_owned(), value));
    Ok(())
}
