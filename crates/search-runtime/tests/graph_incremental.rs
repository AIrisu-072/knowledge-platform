//! P3-G06 and P3-C01: guarded incremental Graph build on the one Source row.
//! Copy batches follow the committed cursor, the whole copy is verified
//! before a closure-checked delta, the result equals a full build, and an
//! expired guard stops every later step until GC removes the target.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/graph.rs"]
mod graph_support;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use graph_support::*;
use search_application::graph_generation::{
    BuildGuardHandle, ClosureBasis, GraphBatchCursor, GraphBatchPhase, GraphBuildRef,
    GraphGenerationReceipt, GraphIncrementalDelta, GraphReadLease, GraphRelationClosureProof,
};
use search_application::scoped::SyntheticAuthorityAdapter;
use search_graph::{GRAPH_SCHEMA_VERSION, GraphError, PostgresGraphReader, canonical_graph_digest};
use search_runtime::gc::{GcOutcome, PgGenerationGc, Protection};
use search_runtime::generation_registration::GenerationError;

fn ttl() -> FullGuardTtl {
    FullGuardTtl::new(Duration::from_secs(60)).unwrap()
}

fn relation_id(n: u128) -> RelationId {
    RelationId::from_uuid(Uuid::from_u128(n))
}

async fn base(fixture: &Fixture, generation: u128) -> GraphGenerationReceipt {
    let key = graph_ready(fixture, generation, false).await;
    PostgresGraphStore::new(fixture.admin.clone())
        .recover(key, &manifest(generation).digest)
        .await
        .unwrap()
}

async fn register(
    fixture: &Fixture,
    base: &GraphGenerationReceipt,
    generation: u128,
) -> Result<BuildGuardHandle, GenerationError> {
    fixture
        .registrar
        .register_incremental(
            base,
            &FullBuildRequest {
                manifest: manifest(generation),
                expected_snapshot: SNAPSHOT.into(),
            },
            &base.source_mapping_digest,
            ttl(),
        )
        .await
}

fn cursor(handle: &BuildGuardHandle, phase: GraphBatchPhase, sequence: u64) -> GraphBatchCursor {
    GraphBatchCursor {
        target_key: handle.target_key(),
        phase,
        committed_sequence: sequence,
    }
}

/// Copies the whole base in batches of `limit` and verifies the copy.
async fn copy_all(store: &PostgresGraphStore, handle: &BuildGuardHandle, limit: u32) {
    let mut position = cursor(handle, GraphBatchPhase::Copy, 0);
    loop {
        let next = store.copy_batch(handle, &position, limit).await.unwrap();
        if next == position {
            break;
        }
        position = next;
        if store.verify_copy(handle).await.is_ok() {
            return;
        }
    }
    store.verify_copy(handle).await.unwrap();
}

fn delta(
    base: &GraphGenerationReceipt,
    changed_resources: Vec<GraphResourceRecord>,
    replacements: Vec<TypedRelationInstance>,
    old: &[u128],
) -> GraphIncrementalDelta {
    let ids: Vec<RelationId> = replacements.iter().map(|r| r.relation_id).collect();
    GraphIncrementalDelta {
        proof: GraphRelationClosureProof {
            base_snapshot: SNAPSHOT.into(),
            target_snapshot: SNAPSHOT.into(),
            affected_resources: changed_resources.iter().map(|r| r.resource_ref).collect(),
            old_relation_ids: old.iter().copied().map(relation_id).collect(),
            new_relation_ids: ids.clone(),
            basis: ClosureBasis::CompleteEnumeration,
        },
        changed_resources,
        retired_resources: vec![],
        changed_relation_ids: ids,
        retired_relation_ids: vec![],
        replacement_relations: replacements,
        target_source_mapping_digest: base.source_mapping_digest.clone(),
    }
}

#[tokio::test]
async fn relation_only_replacement_matches_full_build_digest_and_paths() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let base = base(&fixture, 9_001).await;
    let handle = register(&fixture, &base, 9_002).await.unwrap();
    copy_all(&store, &handle, 5).await;

    // Relation 302 (R2→R4) is replaced in place by R2→R3.
    let replaced = cites(302, &[("source", 102), ("target", 103)]);
    let change = delta(&base, vec![], vec![replaced.clone()], &[302]);
    let applied = store
        .apply_delta_batch(
            &handle,
            &change,
            &cursor(&handle, GraphBatchPhase::Delta, 0),
            16,
        )
        .await
        .unwrap();
    assert_eq!(applied, cursor(&handle, GraphBatchPhase::Delta, 1));
    let old_incidence: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM search_graph.participant WHERE source_id=$1 AND generation_id=$2 \
         AND relation_id=$3 AND resource_id=$4",
    )
    .bind(source_id().as_uuid())
    .bind(handle.target_key().generation_id.as_uuid())
    .bind(relation_id(302).as_uuid())
    .bind(rid(104).as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(old_incidence, 0);
    let report = store
        .validate(&GraphBuildRef::Incremental(handle))
        .await
        .unwrap();
    let mut connection = fixture.admin.acquire().await.unwrap();
    search_graph::store::settle_ready_on(
        &mut connection,
        &GraphBuildRef::Incremental(handle),
        &report,
    )
    .await
    .unwrap();

    // The same target built in full has the same content digest and paths.
    let (resources, mut relations) = graph(false);
    relations.retain(|relation| relation.relation_id != relation_id(302));
    relations.push(replaced);
    let full = graph_ready_with(&fixture, 9_003, &resources, &relations).await;
    let full_receipt = store.recover(full, &manifest(9_003).digest).await.unwrap();
    assert_eq!(
        report.graph_content_digest,
        full_receipt.graph_content_digest
    );
    assert_eq!(
        report.graph_content_digest,
        canonical_graph_digest(
            source_id(),
            GRAPH_SCHEMA_VERSION,
            &records(&resources, &relations),
            &relations
        )
        .unwrap()
    );
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let (binding, scope) = actor(&catalog, &authority).await;
    let access = Access {
        denied: denied(false),
        unknown: Default::default(),
    };
    let verifier = Verifier::new(usize::MAX);
    let reader = PostgresGraphReader::new(fixture.admin.clone(), &access, &verifier);
    let two_hops = plan(&[101], "cites", ("source", "target"), 2);
    let read = |key| {
        let lease = GraphReadLease::from_identifiers(key, binding.evaluation(), Uuid::nil());
        let reader = &reader;
        let two_hops = &two_hops;
        let (binding, scope) = (&binding, &scope);
        async move {
            reader
                .retrieve(&lease, two_hops, binding, scope)
                .await
                .unwrap()
        }
    };
    let incremental = read(handle.target_key()).await;
    let rebuilt = read(full).await;
    assert_eq!(public(&incremental), public(&rebuilt));
    let reached: Vec<_> = incremental
        .hits
        .iter()
        .map(|hit| hit.candidate.resource_ref)
        .collect();
    assert_eq!(reached, vec![Some(rid(103)), Some(rid(106))]);
}

#[tokio::test]
async fn resource_change_requires_complete_new_closure() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let base = base(&fixture, 9_101).await;
    let handle = register(&fixture, &base, 9_102).await.unwrap();
    copy_all(&store, &handle, 64).await;
    let (resources, relations) = graph(false);
    let mut changed = records(&resources, &relations)
        .into_iter()
        .find(|record| record.resource_ref == rid(105))
        .unwrap();
    changed.temporal.valid_from = Some(OffsetDateTime::UNIX_EPOCH);

    // R5 takes part in 303 and 304; a proof that omits them is not a closure.
    let partial = delta(&base, vec![changed.clone()], vec![], &[303]);
    assert_eq!(
        store
            .apply_delta_batch(
                &handle,
                &partial,
                &cursor(&handle, GraphBatchPhase::Delta, 0),
                16
            )
            .await,
        Err(GraphError::RequiresFullRebuild)
    );
    // Nothing changed; the complete closure applies at the same cursor.
    let incident: Vec<TypedRelationInstance> = relations
        .iter()
        .filter(|relation| [303, 304].map(relation_id).contains(&relation.relation_id))
        .cloned()
        .collect();
    let complete = delta(&base, vec![changed], incident, &[303, 304]);
    store
        .apply_delta_batch(
            &handle,
            &complete,
            &cursor(&handle, GraphBatchPhase::Delta, 0),
            16,
        )
        .await
        .unwrap();
    let report = store
        .validate(&GraphBuildRef::Incremental(handle))
        .await
        .unwrap();
    assert_eq!((report.resource_count, report.relation_count), (7, 5));
}

#[tokio::test]
async fn copy_rejects_partial_cursor_and_expired_guard_until_gc_cleans_up() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let gc = PgGenerationGc::new(fixture.admin.clone(), &fixture.root);
    let base = base(&fixture, 9_201).await;
    let base_fence: i64 = sqlx::query_scalar(
        "SELECT build_fence_seq FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id().as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    let handle = register(&fixture, &base, 9_202).await.unwrap();
    // The guard and target exist before the first copy, on the shared fence.
    assert_eq!(handle.build_fence(), base_fence + 1);
    let (state, kind): (String, String) = sqlx::query_as(
        "SELECT g.state, p.build_kind FROM search_graph.generation g \
         JOIN search_generation p USING (source_id, generation_id) \
         WHERE g.source_id=$1 AND g.generation_id=$2",
    )
    .bind(source_id().as_uuid())
    .bind(handle.target_key().generation_id.as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!((state.as_str(), kind.as_str()), ("BUILDING", "INCREMENTAL"));

    let first = store
        .copy_batch(&handle, &cursor(&handle, GraphBatchPhase::Copy, 0), 5)
        .await
        .unwrap();
    assert_eq!(first.committed_sequence, 5);
    // A stale cursor, a premature verification and a wrong phase are refused.
    assert_eq!(
        store
            .copy_batch(&handle, &cursor(&handle, GraphBatchPhase::Copy, 0), 5)
            .await,
        Err(GraphError::FenceLost)
    );
    assert_eq!(store.verify_copy(&handle).await, Err(GraphError::FenceLost));
    assert!(matches!(
        store
            .copy_batch(&handle, &cursor(&handle, GraphBatchPhase::Delta, 5), 5)
            .await,
        Err(GraphError::Invalid(_))
    ));
    // The pointer moving on does not break a valid copy.
    fixture.publish_current(9_203).await;
    let second = store.copy_batch(&handle, &first, 5).await.unwrap();
    assert_eq!(second.committed_sequence, 10);
    fixture
        .registrar
        .renew_build_guard(&handle, ttl())
        .await
        .unwrap();

    // Expiry: no batch, no renewal; the base stays protected until cleanup.
    sqlx::query(
        "UPDATE search_graph.build_guard SET expires_at = clock_timestamp() - interval '1 second' \
         WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(source_id().as_uuid())
    .bind(handle.target_key().generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        store.copy_batch(&handle, &second, 5).await,
        Err(GraphError::FenceLost)
    );
    assert_eq!(
        fixture.registrar.renew_build_guard(&handle, ttl()).await,
        Err(GenerationError::Lost)
    );
    sqlx::query(
        "UPDATE search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(source_id().as_uuid())
    .bind(base.key.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        gc.discard_unpublished(base.key).await,
        Ok(GcOutcome::Protected(Protection::BaseOfBuild))
    );
    assert_eq!(
        gc.discard_unpublished(handle.target_key()).await,
        Ok(GcOutcome::Deleted)
    );
    assert_eq!(
        gc.discard_unpublished(base.key).await,
        Ok(GcOutcome::Deleted)
    );
}

#[tokio::test]
async fn abort_fence_overflow_and_missing_base() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let gc = PgGenerationGc::new(fixture.admin.clone(), &fixture.root);
    let base = base(&fixture, 9_301).await;
    let handle = register(&fixture, &base, 9_302).await.unwrap();
    store
        .copy_batch(&handle, &cursor(&handle, GraphBatchPhase::Copy, 0), 4)
        .await
        .unwrap();
    // Only the live guard's own holder can abort its target.
    let forged = BuildGuardHandle::from_identifiers(
        handle.base_key(),
        handle.target_key(),
        Uuid::new_v4(),
        handle.build_fence(),
    );
    assert_eq!(
        gc.abort_incremental(&forged).await,
        Ok(GcOutcome::Protected(Protection::Guarded))
    );
    assert_eq!(gc.abort_incremental(&handle).await, Ok(GcOutcome::Deleted));
    assert_eq!(
        store
            .copy_batch(&handle, &cursor(&handle, GraphBatchPhase::Copy, 4), 4)
            .await,
        Err(GraphError::FenceLost)
    );

    // A base that no longer exists cannot seed an incremental build.
    let missing = GraphGenerationReceipt {
        key: ProjectionGenerationKey {
            source_id: source_id(),
            generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(9_399)),
        },
        ..base.clone()
    };
    assert_eq!(
        register(&fixture, &missing, 9_303).await,
        Err(GenerationError::Lost)
    );
    // The shared fence refuses to overflow.
    sqlx::query(
        "UPDATE search_source_coordination SET build_fence_seq = 9223372036854775807 \
         WHERE source_id=$1",
    )
    .bind(source_id().as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        register(&fixture, &base, 9_304).await,
        Err(GenerationError::FenceOverflow)
    );
}

#[tokio::test]
async fn database_guards_hold_without_the_rust_gates() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let base = base(&fixture, 9_401).await;
    let handle = register(&fixture, &base, 9_402).await.unwrap();
    copy_all(&store, &handle, 100).await;
    let target = handle.target_key();

    // A verified copy without the applied delta is never READY.
    let early = sqlx::query(
        "UPDATE search_graph.generation SET state='READY', graph_content_digest=$3, \
         resource_count=$4, relation_count=$5, ready_at=clock_timestamp() \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(source_id().as_uuid())
    .bind(target.generation_id.as_uuid())
    .bind(&base.graph_content_digest)
    .bind(base.resource_count as i64)
    .bind(base.relation_count as i64)
    .execute(&fixture.admin)
    .await
    .unwrap_err();
    assert!(early.to_string().contains("applied delta"), "{early}");

    // A guard whose base receipt is not the READY base is refused.
    let forged = sqlx::query(
        "INSERT INTO search_graph.build_guard (source_id,base_generation_id, \
         target_generation_id,guard_token,fence,base_manifest_digest, \
         base_graph_content_digest,base_source_snapshot,base_source_mapping_digest, \
         base_resource_count,base_relation_count,target_manifest_digest,expires_at) \
         SELECT source_id,base_generation_id,target_generation_id,guard_token,fence, \
         base_manifest_digest,'sha256:' || repeat('0',64),base_source_snapshot, \
         base_source_mapping_digest,base_resource_count,base_relation_count, \
         target_manifest_digest,clock_timestamp() + interval '1 minute' \
         FROM search_graph.build_guard WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(source_id().as_uuid())
    .bind(target.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap_err();
    assert!(
        forged.to_string().contains("READY base receipt"),
        "{forged}"
    );

    // TRUNCATE bypasses row guards, so it is refused outright.
    for statement in [
        "TRUNCATE search_graph.participant CASCADE",
        "TRUNCATE search_graph.relation CASCADE",
        "TRUNCATE search_graph.resource CASCADE",
        "TRUNCATE search_graph.build_guard CASCADE",
        "TRUNCATE search_graph.generation CASCADE",
    ] {
        let truncated = sqlx::query(statement)
            .execute(&fixture.admin)
            .await
            .unwrap_err();
        assert!(
            truncated.to_string().contains("never truncated"),
            "{truncated}"
        );
    }
}
