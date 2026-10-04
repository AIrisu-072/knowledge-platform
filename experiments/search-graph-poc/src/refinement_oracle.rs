//! Expected results for the named refinement corpus come from the real memory oracle.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use search_application::SearchError;
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, HyperGraphRetrieverPort,
};
use search_core::graph::GraphTraversalPlan;
use search_core::id::ResourceId;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_graph_memory::MemoryGraphRetriever;
use serde_json::{Value, json};

use super::{Generation, manifest, projection};

struct CurrentFixtureAccess {
    decisions: BTreeMap<ResourceId, String>,
}

impl CurrentAccessEvaluatorPort for CurrentFixtureAccess {
    fn evaluate<'a>(&'a self, id: ResourceId, context: &'a str) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if context != "p3-refinement:trusted" {
                return Ok(AccessDecision::Unknown);
            }
            match self
                .decisions
                .get(&id)
                .map(String::as_str)
                .unwrap_or("Unknown")
            {
                "Allowed" => Ok(AccessDecision::Allowed),
                "Denied" => Ok(AccessDecision::Denied),
                "Error" => Err(SearchError::SourceUnavailable(
                    "synthetic Source error".into(),
                )),
                _ => Ok(AccessDecision::Unknown),
            }
        })
    }
}

pub(super) async fn write_expectations(path: &Path) {
    let mut fixture: Value = serde_json::from_slice(&fs::read(path).expect("refinement fixture"))
        .expect("valid refinement JSON");
    let generations = fixture["generations"]
        .as_array()
        .expect("generations")
        .clone();
    let authority = fixture["authority"].clone();
    for case in fixture["scenarios"].as_array_mut().expect("scenarios") {
        let source = case["source_id"].as_str().expect("source");
        let generation_id = case["generation_id"].as_str().expect("generation");
        let revision = case["revision"].as_str().expect("revision");
        let raw = generations
            .iter()
            .find(|g| {
                g["source_id"].as_str() == Some(source)
                    && g["generation_id"].as_str() == Some(generation_id)
            })
            .expect("matching generation");
        let generation: Generation =
            serde_json::from_value(raw.clone()).expect("generation schema");
        let plan: GraphTraversalPlan =
            serde_json::from_value(case["plan"].clone()).expect("plan schema");
        let decisions = generation
            .resources
            .iter()
            .map(|row| {
                let value = authority[source][row.resource_id.as_uuid().to_string()]["revision"]
                    [revision]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_owned();
                (row.resource_id, value)
            })
            .collect();
        let graph = MemoryGraphRetriever::new(Arc::new(CurrentFixtureAccess { decisions }));
        let source_record = DiscoverableSource::new(
            generation.source_id,
            "synthetic-refinement",
            EnumerationSemantics::Complete,
            RetentionMode::PersistentDiscoveryMetadata,
        );
        let m = manifest(&generation);
        let projections = generation
            .resources
            .iter()
            .map(|r| projection(&m, r, &generation.relations))
            .collect();
        graph
            .build_generation(m.clone(), &source_record, projections)
            .expect("oracle generation");
        match graph.retrieve(m.key(), &plan).await {
            Ok(result) => {
                let hits: Vec<Value> = result
                    .hits
                    .into_iter()
                    .map(|hit| {
                        json!({"target": hit.candidate.resource_ref.expect("resource target"),
                           "paths": hit.paths})
                    })
                    .collect();
                case["expected"] = Value::Array(hits);
                case["expected_status"] = json!("ok");
            }
            Err(error) => {
                case["expected"] = Value::Null;
                case["expected_status"] = json!(format!("error:{error}"));
            }
        }
    }
    fs::write(
        path,
        serde_json::to_vec(&fixture).expect("serialize fixture"),
    )
    .expect("save oracle fixture");
}
