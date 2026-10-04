use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use redb::{Database, ReadableDatabase, TableDefinition};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, HyperGraphRetrieverPort,
};
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{
    DiscoveryEvaluationId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::observation::Coverage;
use search_core::predicate::TypedValue;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_core::temporal::{TemporalDiscoveryProfile, TemporalEvaluationContext};
use search_graph_memory::MemoryGraphRetriever;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;

mod refinement_oracle;

const RELATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("relations");
const INCIDENCE: TableDefinition<&str, &str> = TableDefinition::new("incidence");
const RESOURCES: TableDefinition<&str, &[u8]> = TableDefinition::new("resources");

#[derive(Clone, Serialize, Deserialize)]
struct ResourceRow {
    source_id: SourceId,
    generation_id: ProjectionGenerationId,
    resource_id: ResourceId,
    owner_id: ResourceId,
    temporal: TemporalProjection,
}

#[derive(Clone, Serialize, Deserialize)]
struct Query {
    name: String,
    seed: ResourceId,
    required_collateral: ResourceId,
    denied: Option<ResourceId>,
    expected: Vec<PathSignature>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
struct PathSignature {
    target: ResourceId,
    relation_id: RelationId,
    participants: Vec<RelationParticipant>,
    evidence_refs: Vec<String>,
    provenance: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Generation {
    source_id: SourceId,
    generation_id: ProjectionGenerationId,
    resources: Vec<ResourceRow>,
    relations: Vec<TypedRelationInstance>,
    queries: Vec<Query>,
}

#[derive(Serialize, Deserialize)]
struct Fixture {
    seed: u64,
    relation_groups: usize,
    baseline: Generation,
    updated: Generation,
    #[serde(default)]
    cross_source: Option<Generation>,
}

#[derive(Default)]
struct Access;

impl CurrentAccessEvaluatorPort for Access {
    fn evaluate<'a>(&'a self, id: ResourceId, context: &'a str) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if context == format!("deny:{}", id.as_uuid()) {
                Ok(AccessDecision::Denied)
            } else {
                Ok(AccessDecision::Allowed)
            }
        })
    }
}

fn id(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}
fn relation_id(n: u128) -> RelationId {
    RelationId::from_uuid(Uuid::from_u128(n))
}
fn at(nanos: i128) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(nanos).expect("fixture timestamp")
}
fn temporal(id: ResourceId, group: usize, position: usize) -> TemporalProjection {
    let mut projection = TemporalProjection {
        resource_ref: id,
        valid_from: None,
        valid_to: None,
        profile: TemporalDiscoveryProfile::default(),
    };
    if group == 2 {
        let offset = UtcOffset::from_hms(5, 30, 0).unwrap();
        projection.valid_from = Some(at(100_000_000_000).to_offset(offset));
        projection.profile.freshness_anchor_at = Some(at(99_123_456_789).to_offset(offset));
        projection.profile.freshness_basis = Some("synthetic-source-revision".into());
        projection.profile.effective_from = Some(at(100_000_000_000).to_offset(offset));
        projection.profile.effective_to = Some(at(101_000_000_000).to_offset(offset));
    }
    if group == 3 && position == 1 {
        projection.valid_to = Some(at(100_000_000_001));
    }
    projection
}
fn group_resource(group: usize, position: usize) -> ResourceId {
    id(10_000 + (group * 4 + position) as u128)
}
fn make_relation(group: usize) -> TypedRelationInstance {
    let mut relation = TypedRelationInstance::new(
        relation_id(1_000_000 + group as u128),
        RelationNamespace::Discovery,
        "loan",
        vec![
            RelationParticipant::new("borrower", group_resource(group, 0)),
            RelationParticipant::new("product", group_resource(group, 1)),
            RelationParticipant::new("product", group_resource(group, 2)),
            RelationParticipant::new("collateral", group_resource(group, 3)),
        ],
    );
    relation.authority = Some("finance:authoritative".into());
    relation.provenance = Some(format!("synthetic-{group}"));
    relation.evidence_refs = vec![format!("synthetic-evidence-{group}")];
    relation.qualifiers.insert(
        "ordered_terms".into(),
        TypedValue::List(vec![
            TypedValue::String("first".into()),
            TypedValue::String("second".into()),
        ]),
    );
    relation
}
fn manifest(generation: &Generation) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: generation.source_id,
        generation_id: generation.generation_id,
        projection_schema_version: "synthetic-poc-v1".into(),
        lens_version: 1,
        semantic_registry_version: "synthetic-registry".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: Some("typed-nary-v1".into()),
        source_snapshot: "deterministic-seed-314159".into(),
        resource_count: generation.resources.len() as u64,
        relation_count: Some(generation.relations.len() as u64),
        coverage: Coverage::CompleteEnumeration,
        digest: "poc-projection-digest".into(),
        built_at: at(100_000_000_000),
    }
}
fn projection(
    manifest: &ProjectionGenerationManifest,
    resource: &ResourceRow,
    relations: &[TypedRelationInstance],
) -> CompiledResourceProjection {
    let id = resource.resource_id;
    CompiledResourceProjection {
        manifest: manifest.clone(),
        retention_mode: RetentionMode::PersistentDiscoveryMetadata,
        directory: DirectoryProjection {
            resource_ref: id,
            resource_version: None,
            kind: ResourceKind::Knowledge,
            canonical_name: id.as_uuid().to_string(),
            title: None,
            aliases: vec![],
        },
        structured: StructuredProjection {
            resource_ref: id,
            concept_refs: vec![],
            high_signal_facets: BTreeMap::new(),
            typed_facets: BTreeMap::new(),
            assertions: vec![],
            authority_resolutions: BTreeMap::new(),
        },
        temporal: resource.temporal.clone(),
        access: AccessProjection {
            resource_ref: id,
            access_scope: None,
            source_access_model: None,
        },
        relations: relations
            .iter()
            .filter(|r| r.participants[0].resource_ref == id)
            .cloned()
            .collect(),
    }
}
fn plan(query: &Query) -> GraphTraversalPlan {
    let mut pattern =
        RelationPathPattern::new(RelationNamespace::Discovery, "loan", "borrower", "product");
    pattern.required_participants.push(RelationParticipant::new(
        "collateral",
        query.required_collateral,
    ));
    GraphTraversalPlan {
        seed_nodes: vec![query.seed],
        path_patterns: vec![pattern],
        allowed_relation_types: vec!["loan".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some("finance:authoritative".into()),
        temporal_context: Some(TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(9)),
            at(100_000_000_001),
            at(100_000_000_001),
            "UTC",
        )),
        access_context: query
            .denied
            .map_or_else(|| "allow".into(), |id| format!("deny:{}", id.as_uuid())),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 16,
            max_branching_per_node: 4,
            max_seed_nodes: 1,
            max_paths: 8,
        },
        stop_conditions: vec![],
    }
}
async fn oracle(generation: &mut Generation) {
    let m = manifest(generation);
    let source = DiscoverableSource::new(
        generation.source_id,
        "synthetic",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    let graph = MemoryGraphRetriever::new(Arc::new(Access));
    let projections = generation
        .resources
        .iter()
        .map(|resource| projection(&m, resource, &generation.relations))
        .collect();
    graph
        .build_generation(m.clone(), &source, projections)
        .unwrap();
    for query in &mut generation.queries {
        let result = graph.retrieve(m.key(), &plan(query)).await.unwrap();
        let mut signatures = Vec::new();
        for hit in result.hits {
            for path in hit.paths {
                let step = &path.steps[0];
                signatures.push(PathSignature {
                    target: hit.candidate.resource_ref.unwrap(),
                    relation_id: step.relation_id,
                    participants: step.participants.clone(),
                    evidence_refs: step.evidence_refs.clone(),
                    provenance: step.provenance.clone(),
                });
            }
        }
        signatures.sort();
        query.expected = signatures;
    }
}
fn make_generation(groups: usize, updated: bool) -> Generation {
    let source_id = SourceId::from_uuid(Uuid::from_u128(1));
    let generation_id =
        ProjectionGenerationId::from_uuid(Uuid::from_u128(if updated { 101 } else { 100 }));
    let resources = (0..groups)
        .flat_map(|group| {
            (0..4).map(move |position| {
                let resource_id = group_resource(group, position);
                ResourceRow {
                    source_id,
                    generation_id,
                    resource_id,
                    owner_id: group_resource(group, 0),
                    temporal: temporal(resource_id, group, position),
                }
            })
        })
        .collect();
    let mut relations: Vec<_> = (0..groups).map(make_relation).collect();
    if updated {
        relations.remove(1);
        relations[0].qualifiers.insert(
            "ordered_terms".into(),
            TypedValue::List(vec![
                TypedValue::String("second".into()),
                TypedValue::String("first".into()),
            ]),
        );
        let mut added = make_relation(2);
        added.relation_id = relation_id(2_000_000);
        added.participants[0].resource_ref = group_resource(0, 0);
        added.provenance = Some("synthetic-relation-only-add".into());
        relations.push(added);
    }
    let cases = [
        ("base", 0, 0, None),
        ("false-composite", 0, 1, None),
        ("relation-only-add", 0, 2, None),
        ("relation-only-delete", 1, 1, None),
        ("current-access-denied", 0, 0, Some(group_resource(0, 3))),
        ("half-open-resource", 3, 3, None),
        ("missing-seed", groups + 5, 0, None),
    ];
    let queries = cases
        .into_iter()
        .map(|(name, group, collateral_group, denied)| Query {
            name: name.into(),
            seed: group_resource(group, 0),
            required_collateral: group_resource(collateral_group, 3),
            denied,
            expected: vec![],
        })
        .collect();
    Generation {
        source_id,
        generation_id,
        resources,
        relations,
        queries,
    }
}
async fn make(groups: usize, output: &Path) {
    assert!(groups >= 4);
    let mut fixture = Fixture {
        seed: 314159,
        relation_groups: groups,
        baseline: make_generation(groups, false),
        updated: make_generation(groups, true),
        cross_source: None,
    };
    let mut cross_source = make_generation(4, false);
    cross_source.source_id = SourceId::from_uuid(Uuid::from_u128(2));
    cross_source.resources.truncate(4);
    for resource in &mut cross_source.resources {
        resource.source_id = cross_source.source_id;
    }
    cross_source.relations.truncate(1);
    cross_source.relations[0].provenance = Some("different-source-same-bare-ids".into());
    cross_source.queries.truncate(2);
    fixture.cross_source = Some(cross_source);
    oracle(&mut fixture.baseline).await;
    oracle(&mut fixture.updated).await;
    oracle(fixture.cross_source.as_mut().unwrap()).await;
    fs::write(output, serde_json::to_vec(&fixture).unwrap()).unwrap();
    println!(
        "fixture={} groups={} resources={} baseline_relations={} updated_relations={}",
        output.display(),
        groups,
        fixture.baseline.resources.len(),
        fixture.baseline.relations.len(),
        fixture.updated.relations.len()
    );
}
fn key(source: SourceId, generation: ProjectionGenerationId, suffix: &str) -> String {
    format!("{}|{}|{suffix}", source.as_uuid(), generation.as_uuid())
}
fn stage(db: &Database, generation: &Generation) {
    let txn = db.begin_write().unwrap();
    {
        let mut resources = txn.open_table(RESOURCES).unwrap();
        let mut relations = txn.open_table(RELATIONS).unwrap();
        let mut incidence = txn.open_table(INCIDENCE).unwrap();
        for resource in &generation.resources {
            let k = key(
                generation.source_id,
                generation.generation_id,
                &resource.resource_id.as_uuid().to_string(),
            );
            let bytes = serde_json::to_vec(resource).unwrap();
            resources.insert(k.as_str(), bytes.as_slice()).unwrap();
        }
        for relation in &generation.relations {
            let id = relation.relation_id.as_uuid().to_string();
            let k = key(generation.source_id, generation.generation_id, &id);
            let bytes = serde_json::to_vec(relation).unwrap();
            relations.insert(k.as_str(), bytes.as_slice()).unwrap();
            for participant in &relation.participants {
                let suffix = format!(
                    "{}|{}|{id}",
                    participant.resource_ref.as_uuid(),
                    participant.role
                );
                let ik = key(generation.source_id, generation.generation_id, &suffix);
                incidence.insert(ik.as_str(), "1").unwrap();
            }
        }
    }
    txn.commit().unwrap();
}
fn load(db: &Database, template: &Generation) -> Generation {
    let txn = db.begin_read().unwrap();
    let resources = txn.open_table(RESOURCES).unwrap();
    let relations = txn.open_table(RELATIONS).unwrap();
    let prefix = key(template.source_id, template.generation_id, "");
    let end = format!("{prefix}\u{10ffff}");
    let resources: Vec<ResourceRow> = resources
        .range(prefix.as_str()..end.as_str())
        .unwrap()
        .map(|entry| {
            let (_, bytes) = entry.unwrap();
            serde_json::from_slice(bytes.value()).unwrap()
        })
        .collect();
    let relations: Vec<TypedRelationInstance> = relations
        .range(prefix.as_str()..end.as_str())
        .unwrap()
        .map(|entry| {
            let (_, bytes) = entry.unwrap();
            serde_json::from_slice(bytes.value()).unwrap()
        })
        .collect();
    Generation {
        source_id: template.source_id,
        generation_id: template.generation_id,
        resources,
        relations,
        queries: template.queries.clone(),
    }
}
fn stage_incremental(db: &Database, baseline: &Generation, updated: &Generation) {
    let mut copied = load(db, baseline);
    copied.generation_id = updated.generation_id;
    for resource in &mut copied.resources {
        resource.generation_id = updated.generation_id;
    }
    stage(db, &copied);
    let old: BTreeMap<_, _> = copied
        .relations
        .iter()
        .map(|r| (r.relation_id, r))
        .collect();
    let new: BTreeMap<_, _> = updated
        .relations
        .iter()
        .map(|r| (r.relation_id, r))
        .collect();
    let txn = db.begin_write().unwrap();
    {
        let mut relations = txn.open_table(RELATIONS).unwrap();
        let mut incidence = txn.open_table(INCIDENCE).unwrap();
        for (&id, relation) in &old {
            if new
                .get(&id)
                .is_some_and(|replacement| *replacement == *relation)
            {
                continue;
            }
            let relation_id = id.as_uuid().to_string();
            let rk = key(updated.source_id, updated.generation_id, &relation_id);
            relations.remove(rk.as_str()).unwrap();
            for participant in &relation.participants {
                let suffix = format!(
                    "{}|{}|{relation_id}",
                    participant.resource_ref.as_uuid(),
                    participant.role
                );
                let ik = key(updated.source_id, updated.generation_id, &suffix);
                incidence.remove(ik.as_str()).unwrap();
            }
        }
        for (&id, relation) in &new {
            if old.get(&id).is_some_and(|prior| *prior == *relation) {
                continue;
            }
            let relation_id = id.as_uuid().to_string();
            let rk = key(updated.source_id, updated.generation_id, &relation_id);
            let bytes = serde_json::to_vec(relation).unwrap();
            relations.insert(rk.as_str(), bytes.as_slice()).unwrap();
            for participant in &relation.participants {
                let suffix = format!(
                    "{}|{}|{relation_id}",
                    participant.resource_ref.as_uuid(),
                    participant.role
                );
                let ik = key(updated.source_id, updated.generation_id, &suffix);
                incidence.insert(ik.as_str(), "1").unwrap();
            }
        }
    }
    txn.commit().unwrap();
}
fn query(db: &Database, generation: &Generation, seed: ResourceId) -> Vec<TypedRelationInstance> {
    let txn = db.begin_read().unwrap();
    let incidence = txn.open_table(INCIDENCE).unwrap();
    let relations = txn.open_table(RELATIONS).unwrap();
    let prefix = key(
        generation.source_id,
        generation.generation_id,
        &format!("{}|borrower|", seed.as_uuid()),
    );
    let end = format!("{prefix}\u{10ffff}");
    let mut found = Vec::new();
    for item in incidence.range(prefix.as_str()..end.as_str()).unwrap() {
        let (k, _) = item.unwrap();
        let relation_id = k.value().rsplit('|').next().unwrap();
        let rk = key(generation.source_id, generation.generation_id, relation_id);
        let bytes = relations.get(rk.as_str()).unwrap().unwrap();
        found.push(serde_json::from_slice(bytes.value()).unwrap());
    }
    found
}
fn percentile(values: &mut [u128], percent: usize) -> u128 {
    values.sort_unstable();
    values[((values.len() - 1) * percent).div_ceil(100)]
}
async fn verify_redb(db: &Database, generation: &Generation) {
    let mut loaded = load(db, generation);
    assert_eq!(
        serde_json::to_value(&loaded.resources).unwrap(),
        serde_json::to_value(&generation.resources).unwrap(),
        "resource roundtrip"
    );
    let mut expected_relations = generation.relations.clone();
    expected_relations.sort_by_key(|r| r.relation_id);
    loaded.relations.sort_by_key(|r| r.relation_id);
    assert_eq!(
        loaded.relations, expected_relations,
        "typed relation roundtrip"
    );
    oracle(&mut loaded).await;
    for (actual, expected) in loaded.queries.iter().zip(&generation.queries) {
        assert_eq!(
            actual.expected, expected.expected,
            "{} oracle parity",
            expected.name
        );
    }
    let wrong = Generation {
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(999)),
        ..generation.clone()
    };
    assert!(query(db, &wrong, generation.queries[0].seed).is_empty());
}
async fn bench_redb(fixture_path: &Path, db_path: &Path) {
    let fixture: Fixture = serde_json::from_slice(&fs::read(fixture_path).unwrap()).unwrap();
    let _ = fs::remove_file(db_path);
    let db = Database::create(db_path).unwrap();
    let started = Instant::now();
    stage(&db, &fixture.baseline);
    let ingest_ms = started.elapsed().as_millis();
    verify_redb(&db, &fixture.baseline).await;
    let seed = fixture.baseline.queries[0].seed;
    let mut samples: Vec<u128> = (0..100)
        .map(|_| {
            let now = Instant::now();
            let result = query(&db, &fixture.baseline, seed);
            assert_eq!(result.len(), 1);
            now.elapsed().as_micros()
        })
        .collect();
    let (p50, p95, p99) = (
        percentile(&mut samples, 50),
        percentile(&mut samples, 95),
        percentile(&mut samples, 99),
    );
    let started = Instant::now();
    stage_incremental(&db, &fixture.baseline, &fixture.updated);
    let incremental_ms = started.elapsed().as_millis();
    verify_redb(&db, &fixture.updated).await;
    let mut full = fixture.updated.clone();
    full.generation_id = ProjectionGenerationId::from_uuid(Uuid::from_u128(102));
    for resource in &mut full.resources {
        resource.generation_id = full.generation_id;
    }
    let started = Instant::now();
    stage(&db, &full);
    let updated_full_ms = started.elapsed().as_millis();
    let mut full_relations = load(&db, &full).relations;
    let mut incremental_relations = load(&db, &fixture.updated).relations;
    full_relations.sort_by_key(|r| r.relation_id);
    incremental_relations.sort_by_key(|r| r.relation_id);
    assert_eq!(
        full_relations, incremental_relations,
        "full-incremental parity"
    );
    if let Some(cross_source) = &fixture.cross_source {
        stage(&db, cross_source);
        verify_redb(&db, cross_source).await;
        assert_eq!(query(&db, &fixture.baseline, seed).len(), 1);
        assert_eq!(
            query(&db, cross_source, seed)[0].provenance.as_deref(),
            Some("different-source-same-bare-ids")
        );
    }
    drop(db);
    let restart = Instant::now();
    let db = Database::open(db_path).unwrap();
    let recovered = query(&db, &fixture.updated, seed);
    assert_eq!(recovered.len(), 2);
    let restart_ms = restart.elapsed().as_millis();
    let size = fs::metadata(db_path).unwrap().len();
    println!(
        "{{\"backend\":\"redb-4.3.0\",\"groups\":{},\"resources\":{},\"ingest_ms\":{},\"lookup_us_p50\":{},\"lookup_us_p95\":{},\"lookup_us_p99\":{},\"incremental_ms\":{},\"updated_full_ms\":{},\"restart_ms\":{},\"file_bytes\":{},\"recovered_relations\":{},\"oracle_queries_per_generation\":7,\"oracle_parity\":true,\"full_incremental_parity\":true}}",
        fixture.relation_groups,
        fixture.baseline.resources.len(),
        ingest_ms,
        p50,
        p95,
        p99,
        incremental_ms,
        updated_full_ms,
        restart_ms,
        size,
        recovered.len()
    );
}
async fn check_redb(fixture_path: &Path, db_path: &Path) {
    let fixture: Fixture = serde_json::from_slice(&fs::read(fixture_path).unwrap()).unwrap();
    let db = Database::open(db_path).unwrap();
    verify_redb(&db, &fixture.baseline).await;
    verify_redb(&db, &fixture.updated).await;
    if let Some(cross_source) = &fixture.cross_source {
        verify_redb(&db, cross_source).await;
    }
    assert_eq!(
        query(&db, &fixture.updated, fixture.updated.queries[0].seed).len(),
        2
    );
    println!(
        "redb restored oracle parity=true file_bytes={}",
        fs::metadata(db_path).unwrap().len()
    );
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, command, n, output] if command == "make" => {
            make(n.parse().unwrap(), Path::new(output)).await
        }
        [_, command, fixture, db] if command == "redb" => {
            bench_redb(Path::new(fixture), Path::new(db)).await
        }
        [_, command, fixture, db] if command == "check" => {
            check_redb(Path::new(fixture), Path::new(db)).await
        }
        [_, command, fixture] if command == "refine-oracle" => {
            refinement_oracle::write_expectations(Path::new(fixture)).await
        }
        _ => panic!(
            "usage: search-graph-backend-poc make GROUPS OUT.json | redb FIXTURE.json DB.redb"
        ),
    }
}
