//! P7-08: one READY commit from stored payloads, the sealed lexical directory
//! and the PostgreSQL Graph rows; any drift leaves nothing READY.

#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

#[path = "support/bundle.rs"]
mod bundle;

use bundle::*;

#[tokio::test]
async fn real_payload_lexical_and_graph_become_ready_in_one_commit() {
    let fixture = fixture().await;
    let built = fixture.build(7_810, "document-platform").await;
    let verified = fixture
        .coordinator()
        .ready_manual(&built.handle)
        .await
        .unwrap();
    assert_eq!(verified.key(), built.key);
    assert_eq!(
        verified.receipt().composite_digest,
        built.expected_composite
    );
    assert_eq!(verified.graph().relation_count, 1);
    assert_eq!(
        fixture.states(built.key).await,
        ("READY".into(), "READY".into(), 1)
    );
    let (composite, backend): (String, String) = sqlx::query_as(
        "SELECT composite_digest, graph_backend FROM search_generation_receipt \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(built.key.source_id.as_uuid())
    .bind(built.key.generation_id.as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    let hex: String = built
        .expected_composite
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        (composite, backend),
        (format!("sha256:{hex}"), "postgresql".into())
    );
    // READY never moves the current pointer.
    let current: Option<Uuid> = sqlx::query_scalar(
        "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id().as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(current, None);
    // A second READY attempt is fenced.
    assert_eq!(
        fixture.coordinator().ready_manual(&built.handle).await,
        Err(ReadyError::Fence)
    );
}

#[tokio::test]
async fn graph_lexical_guard_or_payload_drift_leaves_nothing_ready() {
    let fixture = fixture().await;

    // Graph rows that differ from the projection's typed relations.
    let diverged = fixture.build(7_820, "another-authority").await;
    assert_eq!(
        fixture.coordinator().ready_manual(&diverged.handle).await,
        Err(ReadyError::Mapping)
    );
    assert_eq!(
        fixture.states(diverged.key).await,
        ("BUILDING".into(), "BUILDING".into(), 0)
    );

    // The finalized lexical directory disappeared.
    let missing = fixture.build(7_821, "document-platform").await;
    std::fs::remove_dir_all(
        LexicalArtifactStore::new(&fixture.root, fixture.admin.clone()).final_dir(missing.key),
    )
    .unwrap();
    assert!(matches!(
        fixture.coordinator().ready_manual(&missing.handle).await,
        Err(ReadyError::Lexical(_))
    ));
    assert_eq!(fixture.states(missing.key).await.0, "BUILDING");

    // The full guard expired before READY.
    let expired = fixture.build(7_822, "document-platform").await;
    sqlx::query(
        "UPDATE public.search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(expired.key.source_id.as_uuid())
    .bind(expired.key.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        fixture.coordinator().ready_manual(&expired.handle).await,
        Err(ReadyError::Fence)
    );
    assert_eq!(fixture.states(expired.key).await.0, "BUILDING");

    // A stored Unit text changed behind its digest column.
    let tampered = fixture.build(7_823, "document-platform").await;
    sqlx::query(
        "UPDATE search_generation_payload \
         SET payload = jsonb_set(payload, '{body,entries,0,units,0,text}', '\"大阪の本文\"') \
         WHERE source_id=$1 AND generation_id=$2 AND kind='unit_manifest'",
    )
    .bind(tampered.key.source_id.as_uuid())
    .bind(tampered.key.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert!(matches!(
        fixture.coordinator().ready_manual(&tampered.handle).await,
        Err(ReadyError::Payload(_))
    ));
    assert_eq!(fixture.states(tampered.key).await.0, "BUILDING");
}
