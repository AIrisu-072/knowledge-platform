use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    Pending,
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyLicenseGate {
    pub dependency: GateVerdict,
    pub license: GateVerdict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualificationReport {
    pub fixture_set: String,
    pub candidate_versions: BTreeMap<String, String>,
    pub lexical_metrics: Vec<serde_json::Value>,
    pub graph_correctness_metrics: Vec<serde_json::Value>,
    pub graph_traversal_measurements: Vec<serde_json::Value>,
    pub dependency_license_gate: DependencyLicenseGate,
}

impl QualificationReport {
    pub fn harness() -> Self {
        Self {
            fixture_set: "harness-v0".into(),
            candidate_versions: BTreeMap::new(),
            lexical_metrics: Vec::new(),
            graph_correctness_metrics: Vec::new(),
            graph_traversal_measurements: Vec::new(),
            dependency_license_gate: DependencyLicenseGate {
                dependency: GateVerdict::Pending,
                license: GateVerdict::Pending,
            },
        }
    }

    pub fn verify_harness(&self) -> Result<(), &'static str> {
        if self.fixture_set.is_empty() {
            return Err("fixture set must be named");
        }
        serde_json::to_vec(self).map_err(|_| "report must serialize")?;
        Ok(())
    }
}
