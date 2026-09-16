use crate::capability::Capability;
use crate::graph::Graph;
use crate::requirement::{Criticality, Requirement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub kind: String,
    pub subject: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub requirements: Vec<String>,
    pub controls: Vec<String>,
    pub capabilities: Vec<String>,
    pub gaps: Vec<Gap>,
}

pub fn plan(
    _graph: &Graph,
    requirements: &[Requirement],
    capabilities: &[Capability],
    changed_paths: &[PathBuf],
) -> Plan {
    let affected = requirements
        .iter()
        .filter(|requirement| changed_paths.iter().any(|path| affects(requirement, path)))
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    plan_ids(requirements, capabilities, affected)
}

pub fn plan_all(requirements: &[Requirement], capabilities: &[Capability]) -> Plan {
    let ids = requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    plan_ids(requirements, capabilities, ids)
}

fn plan_ids(
    requirements: &[Requirement],
    capabilities: &[Capability],
    ids: BTreeSet<String>,
) -> Plan {
    let mut controls = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let mut gaps = Vec::new();

    for id in &ids {
        let control = id.replacen("REQ-", "CTRL-", 1);
        controls.insert(control.clone());
        let matching = capabilities
            .iter()
            .filter(|capability| capability.controls.iter().any(|item| item == &control))
            .collect::<Vec<_>>();
        for capability in &matching {
            selected.insert(capability.id.clone());
        }

        let criticality = requirements
            .iter()
            .find(|requirement| &requirement.id == id)
            .map(|requirement| requirement.criticality);
        if matching.is_empty()
            && matches!(criticality, Some(Criticality::Medium | Criticality::High))
        {
            gaps.push(Gap {
                kind: "NO_CAPABILITY".to_string(),
                subject: id.clone(),
            });
        }
    }

    Plan {
        requirements: ids.into_iter().collect(),
        controls: controls.into_iter().collect(),
        capabilities: selected.into_iter().collect(),
        gaps,
    }
}

fn affects(requirement: &Requirement, changed: &Path) -> bool {
    let changed = changed.to_string_lossy().replace('\\', "/");
    match requirement.id.as_str() {
        "REQ-DEV-GITHUB-HOSTED-CI" => changed.starts_with(".github/"),
        "REQ-DEV-MISE-TASK-SSOT" => changed == "mise.toml",
        "REQ-DEV-PRODUCTION-LINUX-AMD64" => changed == "Dockerfile",
        _ => {
            changed == requirement.source.to_string_lossy().replace('\\', "/")
                || changed.starts_with("spec/architecture/")
        }
    }
}
