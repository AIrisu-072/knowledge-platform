use std::collections::{BTreeMap, BTreeSet};

use search_application::ports::{
    AssertionStorePort, ConceptRegistryPort, DirectoryRetrieverPort, ProjectionGenerationStore,
    SemanticRegistrySnapshot, StructuredFacetFilter, StructuredFacetOutcome,
    StructuredRetrieverPort,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::authority::{AuthorityConflict, AuthorityResolution};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::observation::Coverage;
use search_core::predicate::{TruthValue, TypedValue};
use search_core::profile::FacetState;
use search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_core::temporal::{TemporalDiscoveryProfile, TemporalEvaluationContext};
use search_projection_memory::{MemoryProjectionStore, generation_digest};
use time::OffsetDateTime;
use uuid::Uuid;

fn source(n: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(n))
}
fn generation(n: u128) -> ProjectionGenerationId {
    ProjectionGenerationId::from_uuid(Uuid::from_u128(n))
}
fn resource(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}
fn relation(n: u128) -> RelationId {
    RelationId::from_uuid(Uuid::from_u128(n))
}
fn at(n: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(n).unwrap()
}

fn manifest(
    source_id: SourceId,
    generation_id: ProjectionGenerationId,
    snapshot: &str,
    count: u64,
) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id,
        generation_id,
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: snapshot.into(),
        resource_count: count,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: String::new(),
        built_at: at(100),
    }
}

fn registry(version: &str, synonym: bool) -> SemanticRegistrySnapshot {
    let mut result = SemanticRegistrySnapshot::new(version);
    result
        .concepts
        .extend(["purchase".into(), "buy".into(), "operation".into()]);
    if synonym {
        result.synonyms.insert(("purchase".into(), "buy".into()));
    }
    result.is_a.insert(("purchase".into(), "operation".into()));
    result
}

fn projection(
    manifest: &ProjectionGenerationManifest,
    id: ResourceId,
    name: &str,
) -> CompiledResourceProjection {
    CompiledResourceProjection {
        manifest: manifest.clone(),
        retention_mode: RetentionMode::PersistentDiscoveryMetadata,
        directory: DirectoryProjection {
            resource_ref: id,
            resource_version: None,
            kind: ResourceKind::Knowledge,
            canonical_name: name.into(),
            title: None,
            aliases: Vec::new(),
        },
        structured: StructuredProjection {
            resource_ref: id,
            concept_refs: vec!["purchase".into()],
            high_signal_facets: BTreeMap::new(),
            typed_facets: BTreeMap::new(),
            assertions: Vec::new(),
            authority_resolutions: BTreeMap::new(),
        },
        temporal: TemporalProjection {
            resource_ref: id,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        access: AccessProjection {
            resource_ref: id,
            access_scope: Some("team".into()),
            source_access_model: Some("policy".into()),
        },
        relations: Vec::new(),
    }
}

fn finalized(
    mut m: ProjectionGenerationManifest,
    resources: &[CompiledResourceProjection],
    semantic: &SemanticRegistrySnapshot,
) -> ProjectionGenerationManifest {
    m.digest = generation_digest(m.source_id, resources, semantic).unwrap();
    m
}

fn rebound(
    mut p: CompiledResourceProjection,
    m: &ProjectionGenerationManifest,
) -> CompiledResourceProjection {
    p.manifest = m.clone();
    p
}

fn persist_manifest(m: ProjectionGenerationManifest) -> PersistableGenerationManifest {
    let src = DiscoverableSource::new(
        m.source_id,
        "fixture",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    PersistableGenerationManifest::try_from((m, &src)).unwrap()
}

fn persist_resource(p: CompiledResourceProjection) -> PersistableResourceProjection {
    PersistableResourceProjection::try_from(p).unwrap()
}

fn request() -> DiscoveryRequest {
    let purpose = IntentFact::new("find".into(), IntentFactOrigin::Explicit);
    let now = at(1000);
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(1)),
            intent_signature: IntentSignature::new(purpose),
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
        access_context: "team".into(),
    }
}

#[tokio::test]
async fn validation_and_publication_reject_missing_duplicate_wrong_count_and_wrong_digest() {
    let store = MemoryProjectionStore::new();
    let s = source(1);
    let semantic = registry("registry-1", false);
    let m1 = manifest(s, generation(1), "snapshot-1", 1);
    let p1 = projection(&m1, resource(10), "first");
    let m1 = finalized(m1, std::slice::from_ref(&p1), &semantic);
    let key1 = m1.key();
    store
        .begin_generation(persist_manifest(m1.clone()))
        .await
        .unwrap();
    assert!(store.publish_generation(key1).await.is_err());
    assert!(store.pin_current(s).await.unwrap().is_none());
    store
        .stage_concept_registry(key1, semantic.clone())
        .await
        .unwrap();
    let mut wrong_manifest = projection(&m1, resource(11), "wrong snapshot");
    wrong_manifest.manifest.source_snapshot = "different snapshot".into();
    assert!(
        store
            .stage_resource(persist_resource(wrong_manifest))
            .await
            .is_err()
    );
    let p1 = rebound(p1, &m1);
    store
        .stage_resource(persist_resource(p1.clone()))
        .await
        .unwrap();
    assert!(store.stage_resource(persist_resource(p1)).await.is_err());
    store.validate_generation(key1).await.unwrap();
    assert!(
        store
            .stage_concept_registry(key1, semantic.clone())
            .await
            .is_err()
    );
    assert!(
        store
            .stage_resource(persist_resource(projection(&m1, resource(11), "late")))
            .await
            .is_err()
    );
    store.publish_generation(key1).await.unwrap();

    let m2 = manifest(s, generation(2), "snapshot-2", 2);
    let p2 = projection(&m2, resource(10), "first");
    let m2 = finalized(m2, std::slice::from_ref(&p2), &semantic);
    store
        .begin_generation(persist_manifest(m2.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m2.key(), semantic.clone())
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(p2, &m2)))
        .await
        .unwrap();
    assert!(store.validate_generation(m2.key()).await.is_err());
    assert!(store.publish_generation(m2.key()).await.is_err());
    assert_eq!(store.pin_current(s).await.unwrap(), Some(m1.clone()));

    let m3 = manifest(s, generation(3), "snapshot-3", 1);
    let p3 = projection(&m3, resource(10), "first");
    let mut m3 = finalized(m3, std::slice::from_ref(&p3), &semantic);
    m3.digest = "sha256:wrong".into();
    store
        .begin_generation(persist_manifest(m3.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m3.key(), semantic)
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(p3, &m3)))
        .await
        .unwrap();
    assert!(store.validate_generation(m3.key()).await.is_err());
    assert!(store.publish_generation(m3.key()).await.is_err());
    assert_eq!(store.pin_current(s).await.unwrap(), Some(m1));

    let m4 = manifest(s, generation(4), "snapshot-4", 0);
    let m4 = finalized(m4, &[], &registry("registry-1", false));
    store
        .begin_generation(persist_manifest(m4.clone()))
        .await
        .unwrap();
    store.fail_generation(m4.key()).await.unwrap();
    assert!(store.publish_generation(m4.key()).await.is_err());
}

#[tokio::test]
async fn incremental_and_full_same_snapshot_are_equal_and_old_pin_survives() {
    let store = MemoryProjectionStore::new();
    let s = source(1);
    let semantic = registry("registry-1", true);
    let base = manifest(s, generation(1), "snapshot-1", 3);
    let base_resources = vec![
        projection(&base, resource(1), "unchanged"),
        projection(&base, resource(2), "old"),
        projection(&base, resource(3), "retired"),
    ];
    let base = finalized(base, &base_resources, &semantic);
    let mut reversed = base_resources.clone();
    reversed.reverse();
    assert_eq!(
        generation_digest(s, &base_resources, &semantic).unwrap(),
        generation_digest(s, &reversed, &semantic).unwrap()
    );
    store
        .begin_generation(persist_manifest(base.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(base.key(), semantic.clone())
        .await
        .unwrap();
    for p in base_resources {
        store
            .stage_resource(persist_resource(rebound(p, &base)))
            .await
            .unwrap();
    }
    store.validate_generation(base.key()).await.unwrap();
    store.publish_generation(base.key()).await.unwrap();
    let old_pin = store.pin_current(s).await.unwrap().unwrap().key();

    let target = manifest(s, generation(2), "snapshot-2", 2);
    let changed = projection(&target, resource(2), "changed");
    let unchanged = projection(&target, resource(1), "unchanged");
    let target = finalized(target, &[unchanged, changed.clone()], &semantic);
    let retired = BTreeSet::from([resource(3)]);
    store
        .begin_incremental_generation(persist_manifest(target.clone()), old_pin, retired)
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(changed, &target)))
        .await
        .unwrap();
    store.validate_generation(target.key()).await.unwrap();
    store.publish_generation(target.key()).await.unwrap();

    let full = manifest(s, generation(4), "snapshot-2", 2);
    let full_resources = vec![
        projection(&full, resource(1), "unchanged"),
        projection(&full, resource(2), "changed"),
    ];
    let full = finalized(full, &full_resources, &semantic);
    assert_eq!(target.digest, full.digest);
    store
        .begin_generation(persist_manifest(full.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(full.key(), semantic.clone())
        .await
        .unwrap();
    for p in full_resources {
        store
            .stage_resource(persist_resource(rebound(p, &full)))
            .await
            .unwrap();
    }
    store.validate_generation(full.key()).await.unwrap();
    assert!(
        store.resource_at(full.key(), resource(1)).await.is_err(),
        "validated but unpublished data must not be readable"
    );
    store.publish_generation(full.key()).await.unwrap();
    for id in [resource(1), resource(2)] {
        let mut incremental = store.resource_at(target.key(), id).await.unwrap().unwrap();
        let rebuilt = store.resource_at(full.key(), id).await.unwrap().unwrap();
        assert_eq!(incremental.manifest, target);
        incremental.manifest = full.clone();
        assert_eq!(
            incremental, rebuilt,
            "full and incremental payloads differ for {id:?}"
        );
    }
    assert_eq!(
        store
            .resource_at(old_pin, resource(2))
            .await
            .unwrap()
            .unwrap()
            .directory
            .canonical_name,
        "old"
    );
    assert_eq!(
        store
            .resource_at(target.key(), resource(2))
            .await
            .unwrap()
            .unwrap()
            .directory
            .canonical_name,
        "changed"
    );
    assert!(
        store
            .resource_at(target.key(), resource(3))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.pin_current(s).await.unwrap(), Some(full));
}

#[tokio::test]
async fn source_collision_and_incompatible_base_are_isolated() {
    let store = MemoryProjectionStore::new();
    let semantic = registry("registry-1", false);
    for (s, name) in [(source(1), "a"), (source(2), "b")] {
        let m = manifest(s, generation(1), "snapshot", 1);
        let p = projection(&m, resource(1), name);
        let m = finalized(m, std::slice::from_ref(&p), &semantic);
        store
            .begin_generation(persist_manifest(m.clone()))
            .await
            .unwrap();
        store
            .stage_concept_registry(m.key(), semantic.clone())
            .await
            .unwrap();
        store
            .stage_resource(persist_resource(rebound(p, &m)))
            .await
            .unwrap();
        store.validate_generation(m.key()).await.unwrap();
        store.publish_generation(m.key()).await.unwrap();
    }
    assert_eq!(
        store
            .resource_at(
                ProjectionGenerationKey {
                    source_id: source(1),
                    generation_id: generation(1)
                },
                resource(1)
            )
            .await
            .unwrap()
            .unwrap()
            .directory
            .canonical_name,
        "a"
    );
    assert_eq!(
        store
            .resource_at(
                ProjectionGenerationKey {
                    source_id: source(2),
                    generation_id: generation(1)
                },
                resource(1)
            )
            .await
            .unwrap()
            .unwrap()
            .directory
            .canonical_name,
        "b"
    );
    let mut incompatible = manifest(source(1), generation(2), "next", 1);
    incompatible.lens_version = 2;
    assert!(
        store
            .begin_incremental_generation(
                persist_manifest(incompatible),
                ProjectionGenerationKey {
                    source_id: source(1),
                    generation_id: generation(1)
                },
                BTreeSet::new()
            )
            .await
            .is_err()
    );
    let mut incompatible_coverage = manifest(source(1), generation(4), "next", 1);
    incompatible_coverage.coverage = Coverage::PartialEnumeration;
    assert!(
        store
            .begin_incremental_generation(
                persist_manifest(incompatible_coverage),
                ProjectionGenerationKey {
                    source_id: source(1),
                    generation_id: generation(1),
                },
                BTreeSet::new(),
            )
            .await
            .is_err()
    );
    let other = manifest(source(1), generation(3), "next", 1);
    assert!(
        store
            .begin_incremental_generation(
                persist_manifest(other),
                ProjectionGenerationKey {
                    source_id: source(2),
                    generation_id: generation(1)
                },
                BTreeSet::new()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn structured_outcomes_and_assertions_preserve_unknown_na_conflict_and_evidence() {
    let store = MemoryProjectionStore::new();
    let s = source(1);
    let semantic = registry("registry-1", false);
    let m = manifest(s, generation(1), "snapshot", 1);
    let mut p = projection(&m, resource(1), "item");
    p.structured
        .typed_facets
        .insert("known".into(), FacetState::Known(TypedValue::Integer(7)));
    p.structured
        .typed_facets
        .insert("unknown".into(), FacetState::Unknown);
    p.structured
        .typed_facets
        .insert("na".into(), FacetState::NotApplicable);
    p.structured
        .typed_facets
        .insert("conflict".into(), FacetState::Conflict);
    for value in ["a", "b"] {
        let mut assertion = Assertion::new(
            "item",
            "conflict",
            TypedValue::String(value.into()),
            "source",
            AssertionOrigin::Authoritative,
            "scope",
            at(10),
        );
        assertion.evidence_refs.push(format!("evidence-{value}"));
        p.structured.assertions.push(assertion);
    }
    p.structured.authority_resolutions.insert(
        "conflict".into(),
        AuthorityResolution::Conflict(AuthorityConflict {
            subject_ref: "item".into(),
            predicate: "conflict".into(),
            authority_scope: "scope".into(),
            rank: 10,
            values: vec![
                TypedValue::String("a".into()),
                TypedValue::String("b".into()),
            ],
        }),
    );
    let m = finalized(m, std::slice::from_ref(&p), &semantic);
    store
        .begin_generation(persist_manifest(m.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m.key(), semantic)
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(p, &m)))
        .await
        .unwrap();
    store.validate_generation(m.key()).await.unwrap();
    store.publish_generation(m.key()).await.unwrap();
    let filters = [
        StructuredFacetFilter::eq("known", TypedValue::Integer(7)),
        StructuredFacetFilter::eq("known", TypedValue::Integer(8)),
        StructuredFacetFilter::eq("unknown", TypedValue::Integer(7)),
        StructuredFacetFilter::eq("na", TypedValue::Integer(7)),
        StructuredFacetFilter::eq("conflict", TypedValue::Integer(7)),
        StructuredFacetFilter::eq("absent", TypedValue::Integer(7)),
    ];
    let hits = StructuredRetrieverPort::retrieve(&store, m.key(), &request(), &filters)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].outcomes,
        vec![
            StructuredFacetOutcome::Match,
            StructuredFacetOutcome::Mismatch,
            StructuredFacetOutcome::Unknown,
            StructuredFacetOutcome::NotApplicable,
            StructuredFacetOutcome::Conflict,
            StructuredFacetOutcome::Unknown,
        ]
    );
    let assertions = store
        .assertions_for(m.key(), resource(1), "conflict")
        .await
        .unwrap();
    assert_eq!(assertions.len(), 2);
    assert_eq!(assertions[0].evidence_refs, vec!["evidence-a"]);
    assert_eq!(assertions[1].evidence_refs, vec!["evidence-b"]);
    assert!(matches!(
        store
            .resource_at(m.key(), resource(1))
            .await
            .unwrap()
            .unwrap()
            .structured
            .authority_resolutions["conflict"],
        AuthorityResolution::Conflict(_)
    ));
    let directory = DirectoryRetrieverPort::retrieve(&store, m.key(), &request())
        .await
        .unwrap();
    assert_eq!(directory.len(), 1);
    assert_eq!(directory[0].resource_ref, Some(resource(1)));
}

#[tokio::test]
async fn synonym_and_hierarchy_use_pinned_registry_generation() {
    let store = MemoryProjectionStore::new();
    let s = source(1);
    let registry1 = registry("registry-1", false);
    let m1 = manifest(s, generation(1), "snapshot", 0);
    let m1 = finalized(m1, &[], &registry1);
    store
        .begin_generation(persist_manifest(m1.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m1.key(), registry1)
        .await
        .unwrap();
    store.validate_generation(m1.key()).await.unwrap();
    store.publish_generation(m1.key()).await.unwrap();
    assert_eq!(
        store
            .same_concept(m1.key(), "buy", "purchase")
            .await
            .unwrap(),
        TruthValue::False
    );
    assert_eq!(
        store.is_a(m1.key(), "buy", "operation").await.unwrap(),
        TruthValue::False
    );
    let old_view = store.pin_view(m1.key()).await.unwrap();

    let mut m2 = manifest(s, generation(2), "snapshot", 0);
    m2.semantic_registry_version = "registry-2".into();
    let registry2 = registry("registry-2", true);
    let m2 = finalized(m2, &[], &registry2);
    store
        .begin_generation(persist_manifest(m2.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m2.key(), registry2)
        .await
        .unwrap();
    store.validate_generation(m2.key()).await.unwrap();
    store.publish_generation(m2.key()).await.unwrap();
    assert_eq!(
        store
            .same_concept(m1.key(), "buy", "purchase")
            .await
            .unwrap(),
        TruthValue::False
    );
    assert_eq!(
        store.is_a(m1.key(), "buy", "operation").await.unwrap(),
        TruthValue::False
    );
    assert_eq!(
        store
            .same_concept(m2.key(), "buy", "purchase")
            .await
            .unwrap(),
        TruthValue::True
    );
    assert_eq!(
        store.is_a(m2.key(), "buy", "operation").await.unwrap(),
        TruthValue::True
    );
    assert_eq!(
        store
            .same_concept(m2.key(), "missing", "purchase")
            .await
            .unwrap(),
        TruthValue::Unknown
    );
    let new_view = store.pin_view(m2.key()).await.unwrap();
    assert_eq!(old_view.same_concept("buy", "purchase"), TruthValue::False);
    assert_eq!(
        old_view.descendant_of("buy", "operation"),
        TruthValue::False
    );
    assert_eq!(new_view.same_concept("buy", "purchase"), TruthValue::True);
    assert_eq!(new_view.descendant_of("buy", "operation"), TruthValue::True);
}

#[tokio::test]
async fn relation_count_and_no_retention_are_enforced() {
    let store = MemoryProjectionStore::new();
    let s = source(1);
    let semantic = registry("registry-1", false);
    let mut m = manifest(s, generation(1), "snapshot", 1);
    m.relation_count = Some(1);
    let p = projection(&m, resource(1), "item");
    let m = finalized(m, std::slice::from_ref(&p), &semantic);
    store
        .begin_generation(persist_manifest(m.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m.key(), semantic.clone())
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(p, &m)))
        .await
        .unwrap();
    assert!(store.validate_generation(m.key()).await.is_err());

    let mut with_relation = manifest(s, generation(4), "snapshot-rel", 2);
    with_relation.relation_count = Some(1);
    let edge = TypedRelationInstance::new(
        relation(1),
        RelationNamespace::Discovery,
        "member-of",
        vec![
            RelationParticipant::new("member", resource(1)),
            RelationParticipant::new("group", resource(2)),
        ],
    );
    let mut first = projection(&with_relation, resource(1), "member");
    first.relations.push(edge.clone());
    let mut second = projection(&with_relation, resource(2), "group");
    let mut reversed_edge = edge;
    reversed_edge.participants.reverse();
    second.relations.push(reversed_edge);
    let mut same_order_second = second.clone();
    same_order_second.relations[0].participants.reverse();
    assert_eq!(
        generation_digest(s, &[first.clone(), second.clone()], &semantic).unwrap(),
        generation_digest(s, &[first.clone(), same_order_second], &semantic).unwrap(),
        "participant ordering does not change an n-ary relation",
    );
    let with_relation = finalized(with_relation, &[first.clone(), second.clone()], &semantic);
    store
        .begin_generation(persist_manifest(with_relation.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(with_relation.key(), semantic)
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(first, &with_relation)))
        .await
        .unwrap();
    store
        .stage_resource(persist_resource(rebound(second, &with_relation)))
        .await
        .unwrap();
    store
        .validate_generation(with_relation.key())
        .await
        .unwrap();
    store.publish_generation(with_relation.key()).await.unwrap();
    assert_eq!(store.pin_current(s).await.unwrap(), Some(with_relation));

    let mut no_retention = projection(
        &manifest(s, generation(2), "snapshot", 1),
        resource(1),
        "forbidden",
    );
    no_retention.retention_mode = RetentionMode::NoRetention;
    assert!(PersistableResourceProjection::try_from(no_retention).is_err());
    let source = DiscoverableSource::new(
        s,
        "remote",
        EnumerationSemantics::QueryOnly,
        RetentionMode::NoRetention,
    );
    assert!(
        PersistableGenerationManifest::try_from((
            manifest(s, generation(3), "snapshot", 0),
            &source
        ))
        .is_err()
    );
}

#[tokio::test]
async fn retiring_an_n_ary_participant_requires_restaging_remaining_relation_owners() {
    let store = MemoryProjectionStore::new();
    let s = source(31);
    let semantic = registry("registry-1", false);
    let mut base = manifest(s, generation(1), "before", 3);
    base.relation_count = Some(1);
    let edge = TypedRelationInstance::new(
        relation(1),
        RelationNamespace::Discovery,
        "membership",
        vec![
            RelationParticipant::new("member", resource(1)),
            RelationParticipant::new("group", resource(2)),
            RelationParticipant::new("context", resource(3)),
        ],
    );
    let mut a = projection(&base, resource(1), "a");
    a.relations.push(edge.clone());
    let b = projection(&base, resource(2), "b");
    let mut c = projection(&base, resource(3), "c");
    c.relations.push(edge);
    let base = finalized(base, &[a.clone(), b.clone(), c.clone()], &semantic);
    store
        .begin_generation(persist_manifest(base.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(base.key(), semantic.clone())
        .await
        .unwrap();
    for p in [a.clone(), b, c.clone()] {
        store
            .stage_resource(persist_resource(rebound(p, &base)))
            .await
            .unwrap();
    }
    store.validate_generation(base.key()).await.unwrap();
    store.publish_generation(base.key()).await.unwrap();

    let retired = BTreeSet::from([resource(2)]);
    let mut stale = manifest(s, generation(2), "after", 2);
    stale.relation_count = Some(1);
    let stale = finalized(stale, &[a.clone(), c.clone()], &semantic);
    store
        .begin_incremental_generation(persist_manifest(stale.clone()), base.key(), retired.clone())
        .await
        .unwrap();
    assert!(
        store.validate_generation(stale.key()).await.is_err(),
        "a carried relation still names the explicitly retired participant"
    );
    assert_eq!(store.pin_current(s).await.unwrap(), Some(base.clone()));

    a.relations.clear();
    c.relations.clear();
    let mut repaired = manifest(s, generation(3), "after", 2);
    repaired.relation_count = Some(0);
    let repaired = finalized(repaired, &[a.clone(), c.clone()], &semantic);
    store
        .begin_incremental_generation(persist_manifest(repaired.clone()), base.key(), retired)
        .await
        .unwrap();
    for p in [a.clone(), c.clone()] {
        store
            .stage_resource(persist_resource(rebound(p, &repaired)))
            .await
            .unwrap();
    }
    store.validate_generation(repaired.key()).await.unwrap();
    store.publish_generation(repaired.key()).await.unwrap();

    let mut full = manifest(s, generation(4), "after", 2);
    full.relation_count = Some(0);
    let full = finalized(full, &[a.clone(), c.clone()], &semantic);
    assert_eq!(repaired.digest, full.digest);
    store
        .begin_generation(persist_manifest(full.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(full.key(), semantic)
        .await
        .unwrap();
    for p in [a, c] {
        store
            .stage_resource(persist_resource(rebound(p, &full)))
            .await
            .unwrap();
    }
    store.validate_generation(full.key()).await.unwrap();
    store.publish_generation(full.key()).await.unwrap();
    for id in [resource(1), resource(3)] {
        let mut increment = store
            .resource_at(repaired.key(), id)
            .await
            .unwrap()
            .unwrap();
        increment.manifest = full.clone();
        assert_eq!(
            increment,
            store.resource_at(full.key(), id).await.unwrap().unwrap()
        );
    }
}

#[tokio::test]
async fn digest_and_published_payload_ignore_unordered_permutations_but_keep_list_order() {
    let store = MemoryProjectionStore::new();
    let s = source(32);
    let semantic = registry("registry-1", false);
    let mut m1 = manifest(s, generation(1), "same", 1);
    m1.relation_count = Some(1);
    let mut p1 = projection(&m1, resource(1), "item");
    p1.directory.aliases = vec!["zeta".into(), "alpha".into()];
    p1.structured.concept_refs = vec!["purchase".into(), "buy".into()];
    p1.structured.typed_facets.insert(
        "set".into(),
        FacetState::Known(TypedValue::Set(vec![
            TypedValue::String("z".into()),
            TypedValue::String("a".into()),
        ])),
    );
    for label in ["b", "a"] {
        let mut assertion = Assertion::new(
            "item",
            "tag",
            TypedValue::List(vec![
                TypedValue::String(label.into()),
                TypedValue::Set(vec![TypedValue::Integer(2), TypedValue::Integer(1)]),
            ]),
            "source",
            AssertionOrigin::Authoritative,
            "scope",
            at(10),
        );
        assertion.evidence_refs = vec!["e2".into(), "e1".into()];
        p1.structured.assertions.push(assertion);
    }
    p1.structured.authority_resolutions.insert(
        "tag".into(),
        AuthorityResolution::Conflict(AuthorityConflict {
            subject_ref: "item".into(),
            predicate: "tag".into(),
            authority_scope: "scope".into(),
            rank: 1,
            values: vec![
                TypedValue::String("b".into()),
                TypedValue::String("a".into()),
            ],
        }),
    );
    let mut edge = TypedRelationInstance::new(
        relation(1),
        RelationNamespace::Discovery,
        "self-context",
        vec![
            RelationParticipant::new("context", resource(1)),
            RelationParticipant::new("item", resource(1)),
        ],
    );
    edge.evidence_refs = vec!["r2".into(), "r1".into()];
    p1.relations.push(edge);

    let mut p2 = p1.clone();
    p2.directory.aliases.reverse();
    p2.structured.concept_refs.reverse();
    p2.structured.assertions.reverse();
    for assertion in &mut p2.structured.assertions {
        assertion.evidence_refs.reverse();
        if let TypedValue::List(values) = &mut assertion.value
            && let TypedValue::Set(values) = &mut values[1]
        {
            values.reverse();
        }
    }
    if let FacetState::Known(TypedValue::Set(values)) =
        &mut p2.structured.typed_facets.get_mut("set").unwrap()
    {
        values.reverse();
    }
    if let AuthorityResolution::Conflict(conflict) =
        p2.structured.authority_resolutions.get_mut("tag").unwrap()
    {
        conflict.values.reverse();
    }
    p2.relations[0].participants.reverse();
    p2.relations[0].evidence_refs.reverse();
    assert_eq!(
        generation_digest(s, std::slice::from_ref(&p1), &semantic).unwrap(),
        generation_digest(s, std::slice::from_ref(&p2), &semantic).unwrap()
    );

    let m1 = finalized(m1, std::slice::from_ref(&p1), &semantic);
    let mut m2 = manifest(s, generation(2), "same", 1);
    m2.relation_count = Some(1);
    let m2 = finalized(m2, std::slice::from_ref(&p2), &semantic);
    for (m, p) in [(m1.clone(), p1.clone()), (m2.clone(), p2)] {
        store
            .begin_generation(persist_manifest(m.clone()))
            .await
            .unwrap();
        store
            .stage_concept_registry(m.key(), semantic.clone())
            .await
            .unwrap();
        store
            .stage_resource(persist_resource(rebound(p, &m)))
            .await
            .unwrap();
        store.validate_generation(m.key()).await.unwrap();
        store.publish_generation(m.key()).await.unwrap();
    }
    let mut first = store
        .resource_at(m1.key(), resource(1))
        .await
        .unwrap()
        .unwrap();
    first.manifest = m2.clone();
    assert_eq!(
        first,
        store
            .resource_at(m2.key(), resource(1))
            .await
            .unwrap()
            .unwrap()
    );

    p1.structured.assertions[0].value = TypedValue::List(vec![
        TypedValue::Set(vec![TypedValue::Integer(2), TypedValue::Integer(1)]),
        TypedValue::String("b".into()),
    ]);
    assert_ne!(
        generation_digest(s, std::slice::from_ref(&p1), &semantic).unwrap(),
        m1.digest,
        "List order remains meaningful"
    );
}

#[tokio::test]
async fn registry_version_content_is_immutable_across_builders_and_publications() {
    let store = MemoryProjectionStore::new();
    let s = source(33);
    let first = registry("registry-1", false);
    let drift = registry("registry-1", true);
    let m1 = finalized(manifest(s, generation(1), "first", 0), &[], &first);
    let m2 = finalized(manifest(s, generation(2), "parallel", 0), &[], &drift);
    store
        .begin_generation(persist_manifest(m1.clone()))
        .await
        .unwrap();
    store
        .begin_generation(persist_manifest(m2.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(m1.key(), first.clone())
        .await
        .unwrap();
    assert!(
        store
            .stage_concept_registry(m2.key(), drift.clone())
            .await
            .is_err(),
        "parallel building generations cannot claim different contents for one version"
    );
    store.validate_generation(m1.key()).await.unwrap();
    assert!(
        store
            .stage_concept_registry(m2.key(), drift.clone())
            .await
            .is_err(),
        "validation cannot release the version/content binding"
    );
    store.publish_generation(m1.key()).await.unwrap();

    let m3 = finalized(manifest(s, generation(3), "later", 0), &[], &drift);
    store
        .begin_generation(persist_manifest(m3.clone()))
        .await
        .unwrap();
    assert!(
        store
            .stage_concept_registry(m3.key(), drift.clone())
            .await
            .is_err(),
        "a published generation also fixes that registry version"
    );
    assert_eq!(store.pin_current(s).await.unwrap(), Some(m1));

    let mut bumped = manifest(s, generation(4), "bumped", 0);
    bumped.semantic_registry_version = "registry-2".into();
    let bumped_registry = registry("registry-2", true);
    let bumped = finalized(bumped, &[], &bumped_registry);
    store
        .begin_generation(persist_manifest(bumped.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(bumped.key(), bumped_registry)
        .await
        .unwrap();
    store.validate_generation(bumped.key()).await.unwrap();
    store.publish_generation(bumped.key()).await.unwrap();
    assert_eq!(store.pin_current(s).await.unwrap(), Some(bumped));
}

#[tokio::test]
async fn failed_validation_releases_registry_version_for_correct_retry() {
    let store = MemoryProjectionStore::new();
    let s = source(34);
    let abandoned_registry = registry("registry-1", false);
    let correct_registry = registry("registry-1", true);
    let abandoned = finalized(
        manifest(s, generation(1), "incomplete", 1),
        &[],
        &abandoned_registry,
    );
    store
        .begin_generation(persist_manifest(abandoned.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(abandoned.key(), abandoned_registry)
        .await
        .unwrap();
    assert!(store.validate_generation(abandoned.key()).await.is_err());
    assert_eq!(store.pin_current(s).await.unwrap(), None);

    let retry = finalized(
        manifest(s, generation(2), "correct", 0),
        &[],
        &correct_registry,
    );
    store
        .begin_generation(persist_manifest(retry.clone()))
        .await
        .unwrap();
    store
        .stage_concept_registry(retry.key(), correct_registry)
        .await
        .unwrap();
    store.validate_generation(retry.key()).await.unwrap();
    store.publish_generation(retry.key()).await.unwrap();
    assert_eq!(store.pin_current(s).await.unwrap(), Some(retry));
}

#[tokio::test]
async fn explicit_failure_releases_registry_version_from_building_and_validated() {
    for validate_first in [false, true] {
        let store = MemoryProjectionStore::new();
        let s = source(35);
        let abandoned_registry = registry("registry-1", false);
        let correct_registry = registry("registry-1", true);
        let abandoned = finalized(
            manifest(s, generation(1), "abandoned", 0),
            &[],
            &abandoned_registry,
        );
        store
            .begin_generation(persist_manifest(abandoned.clone()))
            .await
            .unwrap();
        store
            .stage_concept_registry(abandoned.key(), abandoned_registry)
            .await
            .unwrap();
        if validate_first {
            store.validate_generation(abandoned.key()).await.unwrap();
        }
        store.fail_generation(abandoned.key()).await.unwrap();

        let retry = finalized(
            manifest(s, generation(2), "correct", 0),
            &[],
            &correct_registry,
        );
        store
            .begin_generation(persist_manifest(retry.clone()))
            .await
            .unwrap();
        store
            .stage_concept_registry(retry.key(), correct_registry)
            .await
            .unwrap();
        store.validate_generation(retry.key()).await.unwrap();
        store.publish_generation(retry.key()).await.unwrap();
        assert_eq!(store.pin_current(s).await.unwrap(), Some(retry));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_builders_cannot_stage_conflicting_registry_contents() {
    let store = MemoryProjectionStore::new();
    let s = source(36);
    let first_registry = registry("registry-1", false);
    let second_registry = registry("registry-1", true);
    let first = finalized(manifest(s, generation(1), "first", 0), &[], &first_registry);
    let second = finalized(
        manifest(s, generation(2), "second", 0),
        &[],
        &second_registry,
    );
    store
        .begin_generation(persist_manifest(first.clone()))
        .await
        .unwrap();
    store
        .begin_generation(persist_manifest(second.clone()))
        .await
        .unwrap();

    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let first_store = store.clone();
    let first_barrier = barrier.clone();
    let first_task = tokio::spawn(async move {
        first_barrier.wait().await;
        first_store
            .stage_concept_registry(first.key(), first_registry)
            .await
    });
    let second_store = store.clone();
    let second_barrier = barrier.clone();
    let second_task = tokio::spawn(async move {
        second_barrier.wait().await;
        second_store
            .stage_concept_registry(second.key(), second_registry)
            .await
    });
    barrier.wait().await;
    let first_result = first_task.await.unwrap();
    let second_result = second_task.await.unwrap();
    assert_ne!(
        first_result.is_ok(),
        second_result.is_ok(),
        "only one active builder may claim a registry version"
    );
}
