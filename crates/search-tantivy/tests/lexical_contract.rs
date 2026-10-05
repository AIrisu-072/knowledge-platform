use search_application::ports::{LexicalQuery, LexicalRetrieverPort};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::observation::Coverage;
use search_core::projection::ProjectionGenerationManifest;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use search_tantivy::{
    LexicalBuildInput, LexicalDocument, LexicalIndexError, SourceSuppliedBody, TantivyLexicalIndex,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id(n: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(n))
}

fn resource_id(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

fn manifest(
    source: SourceId,
    generation: u128,
    resource_count: u64,
) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source,
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: format!("snapshot-{generation}"),
        // Total Source snapshot count; the lexical input can be a subset.
        resource_count,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("digest-{generation}"),
        built_at: OffsetDateTime::from_unix_timestamp(100).unwrap(),
    }
}

fn source(id: SourceId, retention: RetentionMode, content_search: bool) -> DiscoverableSource {
    let mut source =
        DiscoverableSource::new(id, "local", EnumerationSemantics::Complete, retention);
    source.discovery_modes.push(DiscoveryMode::LocalDirectory);
    if content_search {
        source
            .discovery_modes
            .push(DiscoveryMode::LocalContentSearch);
    }
    source
}

fn card(id: u128, name: &str) -> LexicalDocument {
    LexicalDocument {
        resource_ref: resource_id(id),
        kind: ResourceKind::Knowledge,
        canonical_name: name.into(),
        title: None,
        aliases: Vec::new(),
        high_signal_text: None,
        body: None,
        locator: None,
    }
}

fn input(source: SourceId, generation: u128, documents: Vec<LexicalDocument>) -> LexicalBuildInput {
    LexicalBuildInput::new(
        source,
        format!("snapshot-{generation}"),
        "schema-1",
        1,
        documents,
    )
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(1)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: Vec::new(),
            authority_requirements: Vec::new(),
            freshness_requirements: Vec::new(),
            constraints: Vec::new(),
            completion_requirement: EvidenceRequirement::new(Vec::new()),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(2)),
            now,
            now,
            "UTC",
        ),
        access_context: "test-principal".into(),
    }
}

async fn find(
    index: &TantivyLexicalIndex,
    manifest: &ProjectionGenerationManifest,
    text: &str,
) -> Vec<search_core::discovery::FederatedCandidate> {
    index
        .retrieve(manifest.key(), &request(), &LexicalQuery::new(text, 10))
        .await
        .unwrap()
}

#[tokio::test]
async fn field_priority_recovers_exact_alias_and_preserves_locator_and_generation_trace() {
    let id = source_id(10);
    let source = source(id, RetentionMode::PersistentResource, true);
    let manifest = manifest(id, 20, 5);
    let mut alias = card(2, "別の名前");
    alias.aliases = vec!["融資".into()];
    alias.locator = Some("source://loan/2".into());
    let mut title = card(3, "別の名前");
    title.title = Some("融資".into());
    let mut signal = card(4, "別の名前");
    signal.high_signal_text = Some("融資".into());
    let mut body = card(5, "別の名前");
    body.body = Some(SourceSuppliedBody::new(id, "融資"));
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(
            manifest.clone(),
            &source,
            input(id, 20, vec![body, signal, title, alias, card(1, "融資")]),
        )
        .unwrap();

    let hits = find(&index, &manifest, "融資").await;
    assert_eq!(
        hits.iter()
            .map(|hit| hit.resource_ref.unwrap())
            .collect::<Vec<_>>(),
        (1..=5).map(resource_id).collect::<Vec<_>>()
    );
    assert_eq!(hits[1].locator.as_deref(), Some("source://loan/2"));
    assert_eq!(hits[1].matched_signals, vec!["aliases"]);
    assert_eq!(hits[1].source_ref, id);
    assert_eq!(
        hits[1].retrieval_trace_ref.as_deref(),
        Some(format!("{}:{}", id.as_uuid(), manifest.generation_id.as_uuid()).as_str())
    );
}

#[tokio::test]
async fn exact_alias_beats_title_and_repeated_queries_have_stable_order() {
    let id = source_id(11);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let manifest = manifest(id, 21, 2);
    let mut alias = card(7, "商品案内");
    alias.aliases = vec!["法人ローン".into()];
    let mut title = card(8, "商品案内");
    title.title = Some("法人ローン".into());
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(manifest.clone(), &source, input(id, 21, vec![title, alias]))
        .unwrap();

    let first = find(&index, &manifest, "法人ローン").await;
    let second = find(&index, &manifest, "法人ローン").await;
    assert_eq!(first, second);
    assert_eq!(first[0].resource_ref, Some(resource_id(7)));
    assert_eq!(first[1].resource_ref, Some(resource_id(8)));
}

#[test]
fn body_requires_source_marker_capability_and_resource_retention() {
    let id = source_id(12);
    let mut body = card(1, "案内");
    body.body = Some(SourceSuppliedBody::new(id, "秘密語"));
    let index = TantivyLexicalIndex::new();
    let metadata_only = source(id, RetentionMode::PersistentDiscoveryMetadata, true);
    assert!(matches!(
        index.build_generation(
            manifest(id, 22, 1),
            &metadata_only,
            input(id, 22, vec![body.clone()])
        ),
        Err(LexicalIndexError::BodyNotPermitted)
    ));
    let no_capability = source(id, RetentionMode::PersistentResource, false);
    assert!(matches!(
        index.build_generation(
            manifest(id, 22, 1),
            &no_capability,
            input(id, 22, vec![body.clone()])
        ),
        Err(LexicalIndexError::BodyNotPermitted)
    ));
    let allowed = source(id, RetentionMode::PersistentResource, true);
    let mut wrong_source = body.clone();
    wrong_source.body = Some(SourceSuppliedBody::new(source_id(99), "秘密語"));
    assert!(matches!(
        index.build_generation(
            manifest(id, 22, 1),
            &allowed,
            input(id, 22, vec![wrong_source])
        ),
        Err(LexicalIndexError::BodySourceMismatch)
    ));
    assert!(matches!(
        index.build_generation(
            manifest(id, 22, 1),
            &source(id, RetentionMode::NoRetention, false),
            input(id, 22, vec![card(1, "案内")]),
        ),
        Err(LexicalIndexError::PersistenceDenied)
    ));
    index
        .build_generation(manifest(id, 22, 1), &allowed, input(id, 22, vec![body]))
        .unwrap();
}

#[tokio::test]
async fn generation_is_immutable_and_old_pin_remains_readable_after_new_build() {
    let id = source_id(13);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let old = manifest(id, 23, 1);
    let next = manifest(id, 24, 1);
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(old.clone(), &source, input(id, 23, vec![card(1, "旧規程")]))
        .unwrap();
    index
        .build_generation(
            next.clone(),
            &source,
            input(id, 24, vec![card(2, "新規程")]),
        )
        .unwrap();
    assert_eq!(
        find(&index, &old, "旧規程").await[0].resource_ref,
        Some(resource_id(1))
    );
    assert!(find(&index, &next, "旧規程").await.is_empty());
    assert!(matches!(
        index.build_generation(old.clone(), &source, input(id, 23, vec![card(3, "旧規程")])),
        Err(LexicalIndexError::DuplicateGeneration)
    ));
    assert!(
        index
            .retrieve(
                manifest(id, 25, 1).key(),
                &request(),
                &LexicalQuery::new("規程", 10)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn source_key_and_required_kind_limit_visible_hits() {
    let left_id = source_id(14);
    let right_id = source_id(15);
    let left = source(left_id, RetentionMode::PersistentDiscoveryMetadata, false);
    let right = source(right_id, RetentionMode::PersistentDiscoveryMetadata, false);
    let left_manifest = manifest(left_id, 26, 2);
    let right_manifest = manifest(right_id, 26, 1);
    let mut policy = card(2, "共通語");
    policy.kind = ResourceKind::Policy;
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(
            left_manifest.clone(),
            &left,
            input(left_id, 26, vec![policy, card(1, "共通語")]),
        )
        .unwrap();
    index
        .build_generation(
            right_manifest.clone(),
            &right,
            input(right_id, 26, vec![card(3, "共通語")]),
        )
        .unwrap();
    let left_hits = find(&index, &left_manifest, "共通語").await;
    let right_hits = find(&index, &right_manifest, "共通語").await;
    assert_eq!(left_hits.len(), 1);
    assert_eq!(left_hits[0].resource_ref, Some(resource_id(1)));
    assert_eq!(right_hits.len(), 1);
    assert_eq!(right_hits[0].resource_ref, Some(resource_id(3)));
}

#[tokio::test]
async fn kind_filter_and_query_window_return_an_eligible_hit_after_many_decoys() {
    let id = source_id(18);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let manifest = manifest(id, 29, 51);
    let mut documents = Vec::new();
    for n in 1..=50 {
        let mut decoy = card(n, "共通語");
        decoy.kind = ResourceKind::Policy;
        documents.push(decoy);
    }
    documents.push(card(51, "共通語"));
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(manifest.clone(), &source, input(id, 29, documents))
        .unwrap();
    let hits = index
        .retrieve(manifest.key(), &request(), &LexicalQuery::new("共通語", 1))
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].resource_ref, Some(resource_id(51)));
    assert!(
        index
            .retrieve(
                manifest.key(),
                &request(),
                &LexicalQuery::new("共通語", 257)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn qualified_poc_mandatory_exact_and_alias_queries_remain_recoverable() {
    let resources: Value = serde_json::from_str(include_str!(
        "../../../experiments/search-discovery-poc/fixtures/lexical/resources.json"
    ))
    .unwrap();
    let queries: Value = serde_json::from_str(include_str!(
        "../../../experiments/search-discovery-poc/fixtures/lexical/queries.json"
    ))
    .unwrap();
    let id = source_id(19);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let manifest = manifest(id, 30, resources.as_array().unwrap().len() as u64);
    let mut names = std::collections::BTreeMap::new();
    let documents = resources
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(position, row)| {
            let key = row["id"].as_str().unwrap();
            let resource_ref = resource_id(position as u128 + 1);
            names.insert(key.to_owned(), resource_ref);
            LexicalDocument {
                resource_ref,
                kind: match row["kind"].as_str().unwrap() {
                    "workflow" => ResourceKind::Workflow,
                    "policy" => ResourceKind::Policy,
                    "knowledge" => ResourceKind::Knowledge,
                    other => panic!("unexpected fixture kind: {other}"),
                },
                canonical_name: row["title"].as_str().unwrap().into(),
                title: Some(row["title"].as_str().unwrap().into()),
                aliases: row["aliases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|alias| alias.as_str().unwrap().into())
                    .collect(),
                high_signal_text: Some(row["summary"].as_str().unwrap().into()),
                body: None,
                locator: None,
            }
        })
        .collect();
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(manifest.clone(), &source, input(id, 30, documents))
        .unwrap();
    let mandatory = queries
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["mandatory"].as_bool() == Some(true));
    let mut checked = 0;
    for case in mandatory {
        checked += 1;
        let mut request = request();
        request.need.required_resource_types =
            vec![match case["required_kind"].as_str().unwrap() {
                "workflow" => ResourceKind::Workflow,
                "policy" => ResourceKind::Policy,
                "knowledge" => ResourceKind::Knowledge,
                other => panic!("unexpected fixture kind: {other}"),
            }];
        let hits = index
            .retrieve(
                manifest.key(),
                &request,
                &LexicalQuery::new(case["query"].as_str().unwrap(), 10),
            )
            .await
            .unwrap();
        for expected in case["expected_resource_ids"].as_array().unwrap() {
            assert!(
                hits.iter()
                    .any(|hit| hit.resource_ref == Some(names[expected.as_str().unwrap()])),
                "{} missing {}",
                case["id"].as_str().unwrap(),
                expected.as_str().unwrap()
            );
        }
    }
    assert_eq!(checked, 11);
}

#[tokio::test]
async fn exact_alias_beats_repeated_phrase_in_a_longer_alias() {
    let id = source_id(16);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let manifest = manifest(id, 27, 2);
    let mut exact = card(1, "別名一");
    exact.aliases = vec!["法人ローン".into()];
    let mut repeated = card(2, "別名二");
    repeated.aliases = vec!["法人ローン 法人ローン 法人ローン".into()];
    let index = TantivyLexicalIndex::new();
    index
        .build_generation(
            manifest.clone(),
            &source,
            input(id, 27, vec![repeated, exact]),
        )
        .unwrap();
    let hits = find(&index, &manifest, "法人ローン").await;
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].resource_ref, Some(resource_id(1)));
}

#[test]
fn source_resource_count_and_analyzer_must_match_index_input() {
    let id = source_id(17);
    let valid_source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let index = TantivyLexicalIndex::new();
    assert!(matches!(
        index.build_generation(
            manifest(id, 28, 0),
            &valid_source,
            input(id, 28, vec![card(1, "案内")])
        ),
        Err(LexicalIndexError::ResourceCountExceeded)
    ));
    let mut unqualified = manifest(id, 28, 1);
    unqualified.analyzer_version = Some("other-analyzer".into());
    assert!(matches!(
        index.build_generation(
            unqualified,
            &valid_source,
            input(id, 28, vec![card(1, "案内")])
        ),
        Err(LexicalIndexError::AnalyzerMismatch)
    ));
    assert!(matches!(
        index.build_generation(
            manifest(id, 28, 1),
            &source(
                source_id(99),
                RetentionMode::PersistentDiscoveryMetadata,
                false
            ),
            input(id, 28, vec![card(1, "案内")]),
        ),
        Err(LexicalIndexError::SourceMismatch)
    ));
}

#[tokio::test]
async fn old_source_snapshot_cannot_be_published_under_new_generation() {
    let id = source_id(20);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let next = manifest(id, 32, 1);
    let index = TantivyLexicalIndex::new();
    let old_input =
        LexicalBuildInput::new(id, "snapshot-31", "schema-1", 1, vec![card(1, "旧規程")]);

    assert!(matches!(
        index.build_generation(next.clone(), &source, old_input),
        Err(LexicalIndexError::SourceSnapshotMismatch)
    ));
    assert!(
        index
            .retrieve(next.key(), &request(), &LexicalQuery::new("旧規程", 10))
            .await
            .is_err()
    );
}

#[test]
fn lexical_input_source_must_match_manifest_source() {
    let id = source_id(23);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let input = LexicalBuildInput::new(
        source_id(99),
        "snapshot-36",
        "schema-1",
        1,
        vec![card(1, "案内")],
    );

    assert!(matches!(
        TantivyLexicalIndex::new().build_generation(manifest(id, 36, 1), &source, input),
        Err(LexicalIndexError::SourceMismatch)
    ));
}

#[test]
fn lexical_input_schema_and_adapter_schema_must_match_manifest() {
    let id = source_id(21);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let index = TantivyLexicalIndex::new();
    let valid = manifest(id, 33, 1);
    let wrong_input =
        LexicalBuildInput::new(id, "snapshot-33", "schema-2", 1, vec![card(1, "案内")]);
    assert!(matches!(
        index.build_generation(valid, &source, wrong_input),
        Err(LexicalIndexError::SchemaVersionMismatch)
    ));

    let mut unsupported = manifest(id, 34, 1);
    unsupported.projection_schema_version = "schema-2".into();
    let unsupported_input =
        LexicalBuildInput::new(id, "snapshot-34", "schema-2", 1, vec![card(2, "案内")]);
    assert!(matches!(
        index.build_generation(unsupported, &source, unsupported_input),
        Err(LexicalIndexError::UnsupportedSchemaVersion)
    ));
}

#[test]
fn lexical_input_lens_version_must_match_manifest() {
    let id = source_id(22);
    let source = source(id, RetentionMode::PersistentDiscoveryMetadata, false);
    let input = LexicalBuildInput::new(id, "snapshot-35", "schema-1", 2, vec![card(1, "案内")]);

    assert!(matches!(
        TantivyLexicalIndex::new().build_generation(manifest(id, 35, 1), &source, input),
        Err(LexicalIndexError::LensVersionMismatch)
    ));
}
