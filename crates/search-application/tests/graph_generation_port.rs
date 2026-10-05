//! P3-G01: SQLx-free Graph generation records and ports type-check from an
//! external crate with a pure adapter; public references are identities only.

use search_application::SearchError;
use search_application::graph_generation::{
    BuildGuardHandle, ClosureBasis, DurableGraphGenerationPort, GenerationScopedGraphAccessPort,
    GraphBatchCursor, GraphBatchPhase, GraphBuildRef, GraphGenerationReceipt,
    GraphIncrementalDelta, GraphLeaseVerifierPort, GraphReadLease, GraphRelationClosureProof,
    GraphResourceRecord, GraphSourceMapping, GraphSourceMappingReceipt,
    GraphSourceMappingValidatorPort, GraphStage, GraphStageReport, PinnedGraphRetrievalPort,
    RegisteredFullBuildHandle,
};
use search_application::ports::{AccessDecision, BoxFuture, GraphRetrievalResult};
use search_application::scoped::{AuthorizedSourceScope, TrustedDiscoveryBinding};
use search_core::graph::GraphTraversalPlan;
use search_core::id::{
    DiscoveryEvaluationId, ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest, TemporalProjection,
};
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use search_core::resource::ResourceKind;
use search_core::temporal::TemporalDiscoveryProfile;
use uuid::Uuid;

fn key(n: u128) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: SourceId::from_uuid(Uuid::from_u128(1)),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(n)),
    }
}

/// A pure adapter: no storage, only shape. Its errors prove nothing about
/// database admission.
struct PureAdapter;

impl DurableGraphGenerationPort for PureAdapter {
    fn stage_full_registered<'a>(
        &'a self,
        target: &'a RegisteredFullBuildHandle,
        _: &'a [GraphResourceRecord],
        _: &'a [TypedRelationInstance],
    ) -> BoxFuture<'a, GraphStage> {
        Box::pin(async move { Ok(GraphStage { key: target.key() }) })
    }
    fn copy_batch<'a>(
        &'a self,
        _: &'a BuildGuardHandle,
        expected: &'a GraphBatchCursor,
        _: u32,
    ) -> BoxFuture<'a, GraphBatchCursor> {
        Box::pin(async move { Ok(*expected) })
    }
    fn verify_copy<'a>(&'a self, _: &'a BuildGuardHandle) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::FenceLost) })
    }
    fn apply_delta_batch<'a>(
        &'a self,
        _: &'a BuildGuardHandle,
        _: &'a GraphIncrementalDelta,
        expected: &'a GraphBatchCursor,
        _: u32,
    ) -> BoxFuture<'a, GraphBatchCursor> {
        Box::pin(async move { Ok(*expected) })
    }
    fn validate_staged<'a>(&'a self, target: &'a GraphBuildRef) -> BoxFuture<'a, GraphStageReport> {
        Box::pin(async move {
            Ok(GraphStageReport {
                key: target.target_key(),
                source_snapshot: "snapshot".into(),
                projection_manifest_digest: "sha256:projection".into(),
                source_mapping_digest: "sha256:mapping".into(),
                graph_content_digest: "sha256:content".into(),
                resource_count: 0,
                relation_count: 0,
                graph_schema_version: "graph-v1".into(),
            })
        })
    }
    fn recover_ready<'a>(
        &'a self,
        _: &'a ProjectionGenerationKey,
        _: &'a str,
    ) -> BoxFuture<'a, GraphGenerationReceipt> {
        Box::pin(async { Err(SearchError::OperationFailed("not READY".into())) })
    }
}

impl GraphSourceMappingValidatorPort for PureAdapter {
    fn validate_authoritative<'a>(
        &'a self,
        manifest: &'a ProjectionGenerationManifest,
        _: &'a [GraphResourceRecord],
    ) -> BoxFuture<'a, GraphSourceMappingReceipt> {
        Box::pin(async move {
            Ok(GraphSourceMappingReceipt {
                key: manifest.key(),
                source_snapshot: manifest.source_snapshot.clone(),
                mapping_digest: "sha256:mapping".into(),
            })
        })
    }
}

impl GraphLeaseVerifierPort for PureAdapter {
    fn verify<'a>(
        &'a self,
        _: &'a GraphReadLease,
        _: &'a TrustedDiscoveryBinding,
        _: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Err(SearchError::FenceLost) })
    }
}

impl GenerationScopedGraphAccessPort for PureAdapter {
    fn evaluate<'a>(
        &'a self,
        _: &'a ProjectionGenerationKey,
        _: ResourceId,
        _: &'a TrustedDiscoveryBinding,
        _: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Ok(AccessDecision::Unknown) })
    }
}

impl PinnedGraphRetrievalPort for PureAdapter {
    fn retrieve_pinned<'a>(
        &'a self,
        _: &'a GraphReadLease,
        _: &'a GraphTraversalPlan,
        _: &'a TrustedDiscoveryBinding,
        _: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, GraphRetrievalResult> {
        Box::pin(async { Err(SearchError::FenceLost) })
    }
}

/// Type-checks the read-side signatures against the existing P4 scope types;
/// never called, because constructing a trusted scope is not this test's job.
#[allow(dead_code)]
async fn read_side_signatures(
    lease: &GraphReadLease,
    plan: &GraphTraversalPlan,
    binding: &TrustedDiscoveryBinding,
    scope: &AuthorizedSourceScope,
) {
    let verifier: &dyn GraphLeaseVerifierPort = &PureAdapter;
    let access: &dyn GenerationScopedGraphAccessPort = &PureAdapter;
    let reader: &dyn PinnedGraphRetrievalPort = &PureAdapter;
    let _ = verifier.verify(lease, binding, scope).await;
    let _ = access
        .evaluate(
            &lease.key(),
            ResourceId::from_uuid(Uuid::nil()),
            binding,
            scope,
        )
        .await;
    let _ = reader.retrieve_pinned(lease, plan, binding, scope).await;
}

#[tokio::test]
async fn all_graph_ports_compile_without_sqlx() {
    let port: &dyn DurableGraphGenerationPort = &PureAdapter;
    let full = RegisteredFullBuildHandle::from_identifiers(key(2), Uuid::from_u128(3), 4);
    let guard = BuildGuardHandle::from_identifiers(key(2), key(5), Uuid::from_u128(6), 7);
    let cursor = GraphBatchCursor {
        target_key: key(5),
        phase: GraphBatchPhase::Copy,
        committed_sequence: 0,
    };
    assert_eq!(
        port.stage_full_registered(&full, &[], &[])
            .await
            .unwrap()
            .key,
        key(2)
    );
    assert_eq!(port.copy_batch(&guard, &cursor, 10).await.unwrap(), cursor);
    assert!(matches!(
        port.verify_copy(&guard).await,
        Err(SearchError::FenceLost)
    ));
    let report = port
        .validate_staged(&GraphBuildRef::Incremental(guard))
        .await
        .unwrap();
    assert_eq!(report.key, key(5));
    assert!(
        port.recover_ready(&key(2), "sha256:projection")
            .await
            .is_err()
    );
}

#[test]
fn public_reference_is_constructible_but_not_a_grant() {
    let token = Uuid::from_u128(0xfeed);
    let full = RegisteredFullBuildHandle::from_identifiers(key(9), token, 11);
    assert_eq!(
        (full.key(), full.guard_token(), full.build_fence()),
        (key(9), token, 11)
    );
    let guard = BuildGuardHandle::from_identifiers(key(8), key(9), token, 12);
    assert_eq!(
        (
            guard.base_key(),
            guard.target_key(),
            guard.guard_token(),
            guard.build_fence()
        ),
        (key(8), key(9), token, 12)
    );
    assert_eq!(GraphBuildRef::Full(full).target_key(), key(9));
    let evaluation = DiscoveryEvaluationId::from_uuid(Uuid::from_u128(13));
    let lease = GraphReadLease::from_identifiers(key(9), evaluation, token);
    assert_eq!(
        (lease.key(), lease.evaluation_id(), lease.lease_id()),
        (key(9), evaluation, token)
    );
}

#[test]
fn graph_record_shape_keeps_frozen_meaning() {
    let resource = ResourceId::from_uuid(Uuid::from_u128(20));
    let relation = TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(21)),
        RelationNamespace::Discovery,
        "document_current_placement",
        vec![RelationParticipant::new("document", resource)],
    );
    let record = GraphResourceRecord {
        resource_ref: resource,
        kind: ResourceKind::Document,
        resource_version_ref: None,
        temporal: TemporalProjection {
            resource_ref: resource,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        mapping: GraphSourceMapping::Document {
            document_id: Uuid::from_u128(22),
        },
        attached_relations: vec![relation.clone()],
    };
    let delta = GraphIncrementalDelta {
        changed_resources: vec![record.clone()],
        retired_resources: vec![],
        changed_relation_ids: vec![relation.relation_id],
        retired_relation_ids: vec![],
        replacement_relations: vec![relation],
        target_source_mapping_digest: "sha256:mapping".into(),
        proof: GraphRelationClosureProof {
            base_snapshot: "s1".into(),
            target_snapshot: "s2".into(),
            affected_resources: vec![resource],
            old_relation_ids: vec![],
            new_relation_ids: vec![RelationId::from_uuid(Uuid::from_u128(21))],
            basis: ClosureBasis::CompleteEnumeration,
        },
    };
    assert_eq!(delta.changed_resources[0], record);
    let mapping = GraphSourceMappingReceipt {
        key: key(30),
        source_snapshot: "s2".into(),
        mapping_digest: "sha256:mapping".into(),
    };
    let receipt = GraphGenerationReceipt {
        key: key(30),
        projection_manifest_digest: "sha256:projection".into(),
        source_snapshot: "s2".into(),
        source_mapping_digest: mapping.mapping_digest.clone(),
        graph_content_digest: "sha256:content".into(),
        resource_count: 1,
        relation_count: 1,
        graph_schema_version: "graph-v1".into(),
    };
    // The mapping digest is its own field, never the schema version.
    assert_ne!(receipt.source_mapping_digest, receipt.graph_schema_version);
}
