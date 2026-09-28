use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fusion::{FusionCase, FusionStrategy, evaluate_fusion};
use crate::lexical::{AnalyzerKind, LexicalCase, LexicalResource, evaluate_lexical};

pub const POSTGRES_IMAGE_TAG: &str = "18.6-bookworm";

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
    pub advisory_exception: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualificationReport {
    pub fixture_set: String,
    pub fixture_sha256: BTreeMap<String, String>,
    pub candidate_versions: BTreeMap<String, String>,
    pub lexical_metrics: Vec<serde_json::Value>,
    pub graph_correctness_metrics: Vec<serde_json::Value>,
    pub graph_traversal_measurements: Vec<serde_json::Value>,
    pub fusion_metrics: Vec<serde_json::Value>,
    pub dependency_license_gate: DependencyLicenseGate,
}

impl QualificationReport {
    pub fn harness() -> Self {
        Self {
            fixture_set: "harness-v0".into(),
            fixture_sha256: BTreeMap::new(),
            candidate_versions: BTreeMap::new(),
            lexical_metrics: Vec::new(),
            graph_correctness_metrics: Vec::new(),
            graph_traversal_measurements: Vec::new(),
            fusion_metrics: Vec::new(),
            dependency_license_gate: DependencyLicenseGate {
                dependency: GateVerdict::Pending,
                license: GateVerdict::Pending,
                advisory_exception: None,
            },
        }
    }

    pub fn evidence() -> Result<Self, serde_json::Error> {
        serde_json::from_str(include_str!("../qualification-report.json"))
    }

    /// Rechecks deterministic quality and source identity. Timings remain captured measurements.
    pub fn verify_evidence(&self) -> Result<(), Box<dyn Error>> {
        if self.fixture_set != "search-discovery-synthetic-v0"
            || self.fixture_sha256.len() != 5
            || self.candidate_versions.len() != 7
            || self.lexical_metrics.len() != 2
            || self.graph_correctness_metrics.len() != 1
            || self.graph_traversal_measurements.len() != 5
            || self.fusion_metrics.len() != 6
            || self.dependency_license_gate.dependency != GateVerdict::Pass
            || self.dependency_license_gate.license == GateVerdict::Fail
            || self.dependency_license_gate.advisory_exception.as_deref()
                != Some("RUSTSEC-2026-0253")
            || !include_str!("../osv-scanner.toml").contains("id = \"RUSTSEC-2026-0253\"")
        {
            return Err("qualification report is incomplete or has a failed gate".into());
        }
        let graph = &self.graph_correctness_metrics[0];
        if graph["fixture_relation_count"].as_u64() != Some(9)
            || [
                "false_composite",
                "high_degree_budget",
                "namespace",
                "postgres_path_parity",
                "role_swap",
                "two_hop_relation_identity",
            ]
            .iter()
            .any(|key| graph[*key] != "pass")
        {
            return Err("graph correctness receipt is incomplete or failed".into());
        }
        self.verify_source_identity()?;
        self.verify_stable_quality()?;
        Ok(())
    }

    fn verify_source_identity(&self) -> Result<(), Box<dyn Error>> {
        for (name, content) in [
            (
                "lexical/resources.json",
                include_bytes!("../fixtures/lexical/resources.json").as_slice(),
            ),
            (
                "lexical/queries.json",
                include_bytes!("../fixtures/lexical/queries.json").as_slice(),
            ),
            (
                "graph/relations.json",
                include_bytes!("../fixtures/graph/relations.json").as_slice(),
            ),
            (
                "graph/generate.rs",
                include_bytes!("../fixtures/graph/generate.rs").as_slice(),
            ),
            (
                "fusion/cases.json",
                include_bytes!("../fixtures/fusion/cases.json").as_slice(),
            ),
        ] {
            let observed = Sha256::digest(content)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            if self.fixture_sha256.get(name) != Some(&observed) {
                return Err(format!("fixture hash mismatch: {name}").into());
            }
        }
        let lock = include_str!("../Cargo.lock");
        if self
            .candidate_versions
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != [
                "lindera",
                "lindera-analysis",
                "lindera-ipadic",
                "postgres",
                "tantivy",
                "testcontainers",
                "tokio-postgres",
            ]
        {
            return Err("candidate version set mismatch".into());
        }
        for (name, recorded) in &self.candidate_versions {
            if name == "postgres" {
                if recorded != POSTGRES_IMAGE_TAG {
                    return Err("PostgreSQL image tag mismatch".into());
                }
            } else if !lock.contains(&format!("name = \"{name}\"\nversion = \"{recorded}\"")) {
                return Err(format!("locked candidate version mismatch: {name}").into());
            }
        }
        Ok(())
    }

    fn verify_stable_quality(&self) -> Result<(), Box<dyn Error>> {
        let resources: Vec<LexicalResource> =
            serde_json::from_str(include_str!("../fixtures/lexical/resources.json"))?;
        let cases: Vec<LexicalCase> =
            serde_json::from_str(include_str!("../fixtures/lexical/queries.json"))?;
        for analyzer in [AnalyzerKind::TantivyDefault, AnalyzerKind::LinderaIpadic] {
            let observed = evaluate_lexical(analyzer, &resources, &cases)?;
            let record = self
                .lexical_metrics
                .iter()
                .find(|record| record["analyzer_id"] == observed.measurement.analyzer_id)
                .ok_or("missing lexical analyzer measurement")?;
            compare_quality(
                record,
                observed.measurement.recall_at_10,
                observed.measurement.mrr,
                observed.measurement.ndcg_at_10,
            )?;
            if record["index_bytes"].as_u64() != Some(observed.measurement.index_bytes)
                || record["runs"].as_u64() != Some(5)
            {
                return Err("lexical index size or run count mismatch".into());
            }
            let missing = observed
                .cases
                .iter()
                .zip(&cases)
                .filter(|(result, case)| {
                    case.expected_resource_ids
                        .iter()
                        .any(|expected| !result.retrieved_ids.contains(expected))
                })
                .map(|(_, case)| case.id.clone())
                .collect::<Vec<_>>();
            if record["missing_cases"] != serde_json::to_value(missing)? {
                return Err("lexical missing-case list mismatch".into());
            }
        }
        let cases: Vec<FusionCase> =
            serde_json::from_str(include_str!("../fixtures/fusion/cases.json"))?;
        let lexical_queries: Vec<LexicalCase> =
            serde_json::from_str(include_str!("../fixtures/lexical/queries.json"))?;
        if cases.iter().any(|case| case.retrievers.len() != 3) {
            return Err("fusion fixture requires three diagnostic retrievers".into());
        }
        for case in &cases {
            let query = lexical_queries
                .iter()
                .find(|query| query.id == case.lexical_query_id)
                .ok_or("fusion case has no lexical hard-discriminator contract")?;
            let candidates = case
                .retrievers
                .iter()
                .flat_map(|retriever| &retriever.candidates)
                .map(|candidate| candidate.resource_id.as_str())
                .collect::<BTreeSet<_>>();
            let expected = resources
                .iter()
                .filter(|resource| candidates.contains(resource.id.as_str()))
                .filter(|resource| {
                    query
                        .required_kind
                        .as_deref()
                        .is_none_or(|kind| resource.kind == kind)
                        && query.required_audience.as_deref().is_none_or(|audience| {
                            resource.audience == audience || resource.audience == "any"
                        })
                })
                .map(|resource| resource.id.as_str())
                .collect::<BTreeSet<_>>();
            let declared = case
                .eligible_resource_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            if expected != declared || !declared.contains(case.expected_resource_id.as_str()) {
                return Err(format!("fusion hard eligibility mismatch: {}", case.id).into());
            }
        }
        for (name, selected, strategy) in [
            ("first", vec![0, 1, 2], FusionStrategy::FirstRetrieverOnly),
            ("graph_only", vec![1], FusionStrategy::FirstRetrieverOnly),
            (
                "vector_like_only",
                vec![2],
                FusionStrategy::FirstRetrieverOnly,
            ),
            ("priority", vec![0, 1, 2], FusionStrategy::PriorityConcat),
            (
                "rrf_lexical_graph_k20",
                vec![0, 1],
                FusionStrategy::Rrf { k: 20 },
            ),
            ("rrf_k20", vec![0, 1, 2], FusionStrategy::Rrf { k: 20 }),
        ] {
            let selected_cases = cases
                .iter()
                .cloned()
                .map(|mut case| {
                    case.retrievers = selected
                        .iter()
                        .map(|index| case.retrievers[*index].clone())
                        .collect();
                    case
                })
                .collect::<Vec<_>>();
            let observed = evaluate_fusion(&selected_cases, strategy);
            let record = self
                .fusion_metrics
                .iter()
                .find(|record| record["strategy"] == name)
                .ok_or("missing fusion strategy measurement")?;
            compare_quality(
                &record["metrics"],
                observed.recall_at_10,
                observed.mrr,
                observed.ndcg_at_10,
            )?;
            if record["metrics"]["case_count"].as_u64() != Some(observed.case_count as u64) {
                return Err("fusion case count mismatch".into());
            }
        }
        Ok(())
    }

    pub fn verify_harness(&self) -> Result<(), &'static str> {
        if self.fixture_set.is_empty() {
            return Err("fixture set must be named");
        }
        serde_json::to_vec(self).map_err(|_| "report must serialize")?;
        Ok(())
    }
}

fn compare_quality(
    record: &serde_json::Value,
    recall: f64,
    mrr: f64,
    ndcg: f64,
) -> Result<(), Box<dyn Error>> {
    for (name, observed) in [("recall_at_10", recall), ("mrr", mrr), ("ndcg_at_10", ndcg)] {
        let recorded = record[name].as_f64().ok_or("quality metric missing")?;
        if (recorded - observed).abs() > 1e-9 {
            return Err(format!("recorded quality mismatch: {name}").into());
        }
    }
    Ok(())
}
