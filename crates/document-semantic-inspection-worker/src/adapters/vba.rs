use std::io::{Cursor, Read};

use serde_json::{Value, json};
use tree_sitter::Parser;
use zip::ZipArchive;

use crate::vba_language::VBA_LANGUAGE;
use crate::{WorkerFailure, WorkerFailureCode};

use super::vba_guard::preflight_vba_project;

const VBA_PROJECT_PATH: &str = "xl/vbaProject.bin";
const MAX_VBA_PROJECT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VBA_MODULES: usize = 1_024;
const MAX_VBA_SOURCE_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct VbaInspection {
    pub projection: Value,
    pub module_count: usize,
}

pub(super) fn inspect_xlsm(input: &[u8]) -> Result<VbaInspection, WorkerFailure> {
    let mut archive = ZipArchive::new(Cursor::new(input)).map_err(|_| semantic_failure())?;
    let vba_bytes = {
        let entry = archive
            .by_name(VBA_PROJECT_PATH)
            .map_err(|_| semantic_failure())?;
        let declared_size = entry.size();
        if declared_size > MAX_VBA_PROJECT_BYTES {
            return Err(resource_limit());
        }

        let capacity = usize::try_from(declared_size).map_err(|_| resource_limit())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| resource_limit())?;
        entry
            .take(MAX_VBA_PROJECT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| semantic_failure())?;
        let actual_size = u64::try_from(bytes.len()).map_err(|_| resource_limit())?;
        if actual_size > MAX_VBA_PROJECT_BYTES {
            return Err(resource_limit());
        }
        if actual_size != declared_size {
            return Err(semantic_failure());
        }
        bytes
    };

    preflight_vba_project(&vba_bytes)?;
    let project = ovba::open_project(vba_bytes).map_err(map_ovba_error)?;
    if project.modules.is_empty() {
        return Err(semantic_failure());
    }
    if project.modules.len() > MAX_VBA_MODULES {
        return Err(resource_limit());
    }

    let mut source_bytes = 0usize;
    let mut modules = Vec::with_capacity(project.modules.len());
    for module in &project.modules {
        let remaining_source_bytes = MAX_VBA_SOURCE_BYTES
            .checked_sub(source_bytes)
            .ok_or_else(resource_limit)?;
        let source = project
            .module_source_bounded(&module.name, remaining_source_bytes)
            .map_err(map_ovba_error)?;
        source_bytes = source_bytes
            .checked_add(source.len())
            .filter(|total| *total <= MAX_VBA_SOURCE_BYTES)
            .ok_or_else(resource_limit)?;
        let syntax = canonicalize_source(&source)?;

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
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
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

fn canonicalize_source(source: &str) -> Result<Value, WorkerFailure> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = VBA_LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|_| semantic_failure())?;
    let tree = parser.parse(source, None).ok_or_else(semantic_failure)?;

    let mut pending = vec![tree.root_node()];
    let mut tokens = Vec::<(String, String)>::new();
    while let Some(node) = pending.pop() {
        if node.is_error() || node.is_missing() {
            return Err(semantic_failure());
        }
        if node.kind() == "comment" {
            continue;
        }
        if node.child_count() != 0 {
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    pending.push(child);
                }
            }
            continue;
        }

        let kind = node.kind();
        if kind.contains("newline") {
            continue;
        }
        let raw = source
            .as_bytes()
            .get(node.start_byte()..node.end_byte())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(semantic_failure)?;
        if raw.trim().is_empty() {
            continue;
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
        tokens.push((kind.to_owned(), value));
    }

    serde_json::to_value(tokens).map_err(|_| {
        WorkerFailure::new(
            WorkerFailureCode::InvalidWorkerResult,
            "VBA semantic projection could not be serialized",
        )
    })
}

fn semantic_failure() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::SemanticExtractionFailed,
        "VBA project could not be inspected safely",
    )
}

fn map_ovba_error(error: ovba::Error) -> WorkerFailure {
    match error {
        ovba::Error::ResourceLimit => resource_limit(),
        _ => semantic_failure(),
    }
}

fn resource_limit() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "VBA project exceeds a configured inspection limit",
    )
}
