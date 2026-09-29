use std::collections::{BTreeMap, BTreeSet};

use document_diff_core::{
    ChangeOperation, ComparisonBudget, SourceLocator, UnverifiedReason, WorkerChange,
};
use document_semantic_inspection_worker::{WorkerFailureCode, inspect_vba_project};
use serde_json::Value;

pub(super) fn compare_vba(
    base: &[u8],
    target: &[u8],
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    let old = inspect_vba_project(base).map_err(|e| reason(e.code()))?;
    let new = inspect_vba_project(target).map_err(|e| reason(e.code()))?;
    compare_projections(&old.projection, &new.projection, budget, changes)
}

fn compare_projections(
    old: &Value,
    new: &Value,
    budget: &mut ComparisonBudget,
    changes: &mut Vec<WorkerChange>,
) -> Result<(), UnverifiedReason> {
    if old["references"] != new["references"] {
        push_project_change(
            changes,
            budget,
            "xlsm_vba_reference",
            "vba_reference_changed",
        )?;
    }
    if old["system_kind"] != new["system_kind"] || old["code_page"] != new["code_page"] {
        push_project_change(
            changes,
            budget,
            "xlsm_vba_project",
            "vba_project_setting_changed",
        )?;
    }
    let old_modules = modules(old)?;
    let new_modules = modules(new)?;
    let names: BTreeSet<_> = old_modules
        .keys()
        .chain(new_modules.keys())
        .copied()
        .collect();
    for name in names {
        budget
            .consume_candidates(1)
            .map_err(|_| UnverifiedReason::ResourceLimit)?;
        let before = old_modules.get(name).copied();
        let after = new_modules.get(name).copied();
        if before == after {
            continue;
        }
        let (operation, facet, base_procedure, target_procedure) = match (before, after) {
            (None, Some(_)) => (ChangeOperation::Added, "xlsm_vba_module", None, None),
            (Some(_), None) => (ChangeOperation::Removed, "xlsm_vba_module", None, None),
            (Some(before), Some(after)) => {
                let before_procedure = unique_procedure(before);
                let after_procedure = unique_procedure(after);
                if before["syntax"] != after["syntax"]
                    && before["stream_name"] == after["stream_name"]
                    && before["module_type"] == after["module_type"]
                    && before["read_only"] == after["read_only"]
                    && before["private"] == after["private"]
                    && before_procedure.is_some()
                    && before_procedure == after_procedure
                {
                    (
                        ChangeOperation::Modified,
                        "xlsm_vba_procedure",
                        before_procedure,
                        after_procedure,
                    )
                } else {
                    (ChangeOperation::Modified, "xlsm_vba_module", None, None)
                }
            }
            (None, None) => unreachable!(),
        };
        budget
            .consume_changes(1)
            .map_err(|_| UnverifiedReason::ResourceLimit)?;
        changes.push(WorkerChange {
            operation: Some(operation),
            relocation: None,
            facet: facet.to_owned(),
            base: before.map(|_| SourceLocator::VbaModule {
                module: name.to_owned(),
                procedure: base_procedure,
            }),
            target: after.map(|_| SourceLocator::VbaModule {
                module: name.to_owned(),
                procedure: target_procedure,
            }),
            reason_code: "vba_semantics_changed".to_owned(),
        });
    }
    Ok(())
}

fn modules(projection: &Value) -> Result<BTreeMap<&str, &Value>, UnverifiedReason> {
    let values = projection["modules"]
        .as_array()
        .ok_or(UnverifiedReason::CorruptedSource)?;
    let mut result = BTreeMap::new();
    for module in values {
        let name = module["name"]
            .as_str()
            .ok_or(UnverifiedReason::CorruptedSource)?;
        if result.insert(name, module).is_some() {
            return Err(UnverifiedReason::AmbiguousAlignment);
        }
    }
    Ok(result)
}

fn unique_procedure(module: &Value) -> Option<String> {
    let tokens = module["syntax"].as_array()?;
    let values: Vec<_> = tokens
        .iter()
        .map(|token| token.get(1)?.as_str())
        .collect::<Option<Vec<_>>>()?;
    let mut names = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if matches!(*value, "sub" | "function" | "property")
            && (index == 0 || values[index - 1] != "end")
        {
            let name = values.get(index + 1)?;
            if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                names.push((*name).to_owned());
            }
        }
    }
    (names.len() == 1).then(|| names.remove(0))
}

fn push_project_change(
    changes: &mut Vec<WorkerChange>,
    budget: &mut ComparisonBudget,
    facet: &str,
    reason_code: &str,
) -> Result<(), UnverifiedReason> {
    budget
        .consume_changes(1)
        .map_err(|_| UnverifiedReason::ResourceLimit)?;
    let path = if facet == "xlsm_vba_reference" {
        "xl/vbaProject.bin#references"
    } else {
        "xl/vbaProject.bin"
    };
    let locator = SourceLocator::OfficePath {
        path: path.to_owned(),
    };
    changes.push(WorkerChange {
        operation: Some(ChangeOperation::Modified),
        relocation: None,
        facet: facet.to_owned(),
        base: Some(locator.clone()),
        target: Some(locator),
        reason_code: reason_code.to_owned(),
    });
    Ok(())
}

fn reason(code: WorkerFailureCode) -> UnverifiedReason {
    match code {
        WorkerFailureCode::InspectionResourceLimitExceeded
        | WorkerFailureCode::InspectionTimeout => UnverifiedReason::ResourceLimit,
        WorkerFailureCode::SemanticExtractionFailed | WorkerFailureCode::ParserDisagreement => {
            UnverifiedReason::CorruptedSource
        }
        _ => UnverifiedReason::UnsupportedSemanticConstruct,
    }
}

#[cfg(test)]
mod tests {
    use document_diff_core::{ComparisonBudget, SourceLocator};
    use serde_json::json;

    use super::compare_projections;

    #[test]
    fn vba_reference_and_module_presence_have_separate_facets() {
        let base =
            json!({"references": [], "system_kind": "Win32", "code_page": 1252, "modules": []});
        let target = json!({"references": ["SyntheticReference"], "system_kind": "Win32", "code_page": 1252, "modules": [{
            "name": "Module1", "stream_name": "Module1", "module_type": "Standard", "read_only": false, "private": false, "syntax": []
        }]});
        let mut budget = ComparisonBudget::new(10, 10);
        let mut changes = vec![];
        compare_projections(&base, &target, &mut budget, &mut changes).unwrap();
        assert!(
            changes
                .iter()
                .any(|change| change.facet == "xlsm_vba_reference")
        );
        assert!(changes.iter().any(|change| {
            change.facet == "xlsm_vba_module"
                && matches!(change.target, Some(SourceLocator::VbaModule { ref module, procedure: None }) if module == "Module1")
        }));
    }
}
