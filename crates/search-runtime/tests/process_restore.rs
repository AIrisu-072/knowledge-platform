//! P7-12 with P3-G08/C04: a restarted process re-verifies the current key
//! from every stored artifact, collects only expired builds, keeps a broken
//! current unusable, and a backup restored into a different database serves
//! the same key and digest only once its lexical directory is restored too.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/graph.rs"]
mod graph_support;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use std::path::Path;

use graph_support::*;
use search_application::graph_generation::GraphReadLease;
use search_application::scoped::SyntheticAuthorityAdapter;
use search_graph::PostgresGraphReader;
use search_runtime::recovery::{CurrentState, PgStartupRecovery, RecoveryError};
use sqlx::postgres::PgPoolOptions;

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

async fn state(admin: &PgPool, key: ProjectionGenerationKey) -> Option<String> {
    sqlx::query_scalar(
        "SELECT state FROM search_generation WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_optional(admin)
    .await
    .unwrap()
}

fn verified(state: &CurrentState) -> ProjectionGenerationKey {
    match state {
        CurrentState::Verified(bundle) => bundle.key(),
        other => panic!("expected a verified current: {other:?}"),
    }
}

#[tokio::test]
async fn restart_reverifies_current_and_cleans_only_expired_builds() {
    let fixture = fixture().await;
    let current = fixture.publish_current(9_501).await;
    let live = fixture.build(9_502, "document-platform").await;
    let stale = fixture.build(9_503, "document-platform").await;
    sqlx::query(
        "UPDATE search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(stale.key.source_id.as_uuid())
    .bind(stale.key.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();

    // A new process: its own pool and recovery instance, nothing from RAM.
    let restarted = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(fixture.options.clone())
        .await
        .unwrap();
    let report = PgStartupRecovery::new(restarted, &fixture.root, source())
        .startup(10)
        .await
        .unwrap();
    assert_eq!(report.cleanup.generations, 1);
    assert_eq!(verified(&report.current), current);
    assert_eq!(state(&fixture.admin, stale.key).await, None);
    assert_eq!(
        state(&fixture.admin, live.key).await.as_deref(),
        Some("BUILDING")
    );
}

#[tokio::test]
async fn missing_bytes_payload_drift_or_index_keeps_the_runtime_from_serving() {
    let fixture = fixture().await;
    let current = fixture.publish_current(9_601).await;
    let recovery = PgStartupRecovery::new(fixture.admin.clone(), &fixture.root, source());
    assert_eq!(verified(&recovery.verify_current().await.unwrap()), current);

    // The lexical bytes outside the database disappear.
    let dir = LexicalArtifactStore::new(&fixture.root, fixture.admin.clone()).final_dir(current);
    let moved = dir.with_extension("moved");
    std::fs::rename(&dir, &moved).unwrap();
    assert!(matches!(
        recovery.verify_current().await.unwrap(),
        CurrentState::Unusable(key, ReadyError::Lexical(_)) if key == current
    ));
    std::fs::rename(&moved, &dir).unwrap();
    assert_eq!(verified(&recovery.verify_current().await.unwrap()), current);

    // A stored payload changes below the triggers.
    let mut tx = fixture.admin.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE search_unit_segment \
         SET payload = jsonb_set(payload, '{body,units,0,text}', '\"大阪の本文\"') \
         WHERE segment_digest = (SELECT segment_digest FROM search_generation_segment \
         WHERE source_id=$1 AND generation_id=$2 AND ordinal=0)",
    )
    .bind(current.source_id.as_uuid())
    .bind(current.generation_id.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // Segments are verified once per process; a restarted process re-reads it.
    search_runtime::payload::forget_verified_segments();
    assert!(matches!(
        recovery.verify_current().await.unwrap(),
        CurrentState::Unusable(key, ReadyError::Payload(_)) if key == current
    ));

    // A missing required index keeps the whole runtime unready.
    sqlx::query("DROP INDEX search_graph.graph_participant_incidence")
        .execute(&fixture.admin)
        .await
        .unwrap();
    assert_eq!(
        recovery.startup(10).await,
        Err(RecoveryError::Schema(
            "search_graph.graph_participant_incidence"
        ))
    );
}

#[tokio::test]
async fn backup_restore_to_new_db_preserves_key_digest_and_query() {
    let fixture = fixture().await;
    let current = fixture.publish_current(9_701).await;
    let original = PgStartupRecovery::new(fixture.admin.clone(), &fixture.root, source())
        .verify_current()
        .await
        .unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&fixture.admin)
        .await
        .unwrap();
    fixture
        ._guard
        .exec_sh(&format!(
            "pg_dump -U postgres -Fc -d {database} -f /tmp/search.dump \
             && createdb -U postgres -T template0 restored_search \
             && pg_restore -U postgres --exit-on-error -d restored_search /tmp/search.dump"
        ))
        .await
        .expect("pg_dump and pg_restore inside the disposable server");
    let restored = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(fixture.options.clone().database("restored_search"))
        .await
        .unwrap();

    // Only the database is restored: the key is not READY to serve.
    let empty = std::env::temp_dir().join(format!("search-restore-empty-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        PgStartupRecovery::new(restored.clone(), &empty, source())
            .verify_current()
            .await
            .unwrap(),
        CurrentState::Unusable(key, ReadyError::Lexical(_)) if key == current
    ));

    // The lexical tree restored separately: same key, same digests.
    let restored_root = std::env::temp_dir().join(format!("search-restore-{}", Uuid::now_v7()));
    copy_tree(&fixture.root, &restored_root);
    let report = PgStartupRecovery::new(restored.clone(), &restored_root, source())
        .startup(10)
        .await
        .unwrap();
    assert_eq!(report.current, original);

    // A real Graph query on the restored database.
    let catalog = fixture.catalog().await;
    let authority = SyntheticAuthorityAdapter::new();
    let (binding, scope) = actor(&catalog, &authority).await;
    let access = Access {
        denied: Default::default(),
        unknown: Default::default(),
    };
    let verifier = Verifier::new(usize::MAX);
    let reader = PostgresGraphReader::new(restored.clone(), &access, &verifier);
    let mut placement = plan(
        &[10],
        "document_current_placement",
        ("document", "folder"),
        1,
    );
    placement.expansion_budget.max_hops = 1;
    let lease = GraphReadLease::from_identifiers(current, binding.evaluation(), Uuid::nil());
    let result = reader
        .retrieve(&lease, &placement, &binding, &scope)
        .await
        .unwrap();
    assert_eq!(
        result
            .hits
            .iter()
            .map(|hit| hit.candidate.resource_ref)
            .collect::<Vec<_>>(),
        vec![Some(rid(11))]
    );
    let _ = std::fs::remove_dir_all(&empty);
    let _ = std::fs::remove_dir_all(&restored_root);
}
