//! Every control event type the SQL builds conforms to the audit-core
//! catalog on its own origin path (design §4.5). This store produces all
//! store and relay_control types except `audit.recovery.fingerprint_rebound`,
//! which needs a restored database and is validated in store_recovery.rs.

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use audit_core::{AuditEnvelope, AuditStore, Catalog, Origin};
use audit_store_postgres::PostgresAuditStore;
use audit_store_postgres::admin::{AccessChange, AccessOperation, parse_utc_text};
use serde_json::json;
use sqlx::Row;
use support::*;
use uuid::Uuid;

#[tokio::test]
async fn every_sql_built_control_event_conforms_to_the_catalog() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), Duration::from_secs(10))
        .await
        .expect("relay");
    let id = Uuid::now_v7();
    store
        .ingest(&document_created(
            id,
            Uuid::now_v7(),
            "2020-01-01T00:00:00.000000Z",
            1,
        ))
        .await
        .expect("stored");
    let other = Uuid::now_v7();
    store
        .ingest(&document_created(
            other,
            Uuid::now_v7(),
            "2020-01-01T00:00:00.000000Z",
            2,
        ))
        .await
        .expect("stored");
    // conflict_detected
    let _ = store
        .ingest(&document_created(id, Uuid::now_v7(), OCCURRED, 3))
        .await;
    // intent_opened (with every filter member), closed, denied
    let reader = cast.reader.admin().await;
    let token = reader
        .open_access(
            AccessOperation::Investigate,
            &json!({
                "event_types": ["document.created"],
                "source": "urn:knowledge-platform:document-platform",
                "actor": {"issuer": "poc", "principal_id": "synthetic-human"},
                "resource": {"type": "Document"},
                "occurred_from": "2019-01-01T00:00:00.000000Z",
                "occurred_to": "2030-01-01T00:00:00.000000Z",
                "event_ids": [id.to_string()],
                "seq_after": 0
            }),
            10,
            1,
        )
        .await
        .expect("opened");
    let page = reader.read_page(token.secret(), 0).await.expect("page");
    assert_eq!(page.len(), 1);
    reader
        .close_access(
            token.secret(),
            1,
            &[audit_store_postgres::files::page_digest(&page)],
        )
        .await
        .expect("closed");
    let _ = reader
        .open_access(AccessOperation::Verify, &json!({}), 10, 1)
        .await;
    // access_policy.changed (granted/revoked) and retention events
    let admin = cast.admin.admin().await;
    admin
        .change_access(ISSUER, "reader-1", "verify", AccessChange::Grant)
        .await
        .expect("granted");
    admin
        .change_access(ISSUER, "reader-1", "verify", AccessChange::Revoke)
        .await
        .expect("revoked");
    let policy = admin
        .set_retention_policy(
            "all_documents",
            &json!({
                "event_types": ["document.created"],
                "event_classes": ["CONTENT_LIFECYCLE"],
                "sources": ["urn:knowledge-platform:document-platform"]
            }),
            Some(30),
        )
        .await
        .expect("policy");
    let maintainer = cast.maintainer.admin().await;
    let far = parse_utc_text("2100-01-01T00:00:00.000000Z").expect("far");
    let expired = maintainer
        .expire("all_documents", policy.revision, far, 1)
        .await
        .expect("expired");
    assert_eq!(expired.expired_count, 1);
    maintainer
        .expire("all_documents", policy.revision + 1, far, 1)
        .await
        .expect("stale attempt is recorded");
    maintainer
        .purge_body(other, "adapter_defect")
        .await
        .expect("purged");
    // integrity.verified (verify and checkpoint)
    let verifier = cast.verifier.admin().await;
    assert_eq!(
        verifier.verify(None, None).await.expect("verify").outcome,
        "ok"
    );
    verifier.checkpoint().await.expect("checkpoint");
    // relay control events
    let relay = cast.relay.admin().await;
    relay
        .record_relay_control(
            "audit.delivery.replay_requested",
            id,
            "delivery_unknown_at_limit",
            None,
        )
        .await
        .expect("replay");
    relay
        .record_relay_control(
            "audit.integrity.source_mismatch_detected",
            id,
            "actor_mismatch",
            None,
        )
        .await
        .expect("mismatch");
    let counts = json!({
        "watermark": 2, "id_set_digest": "cd".repeat(32), "ok": 2, "delivered_missing": 0,
        "digest_mismatch": 0, "quarantined_stored": 0, "quarantined_conflict": 0,
        "pending": 0, "quarantined": 0, "unregistered": 0, "source_tampered": 0,
        "store_only": 0, "unaudited_replay": 0, "repaired_delivered_missing": 0,
        "repaired_quarantined_stored": 0, "repaired_unregistered": 0
    });
    relay
        .record_relay_control(
            "audit.reconciliation.completed",
            Uuid::now_v7(),
            "repair",
            Some(&counts),
        )
        .await
        .expect("reconciliation");
    // recovery.epoch_started via a regression claim
    let checkpoint = verifier.checkpoint().await.expect("checkpoint");
    let (head_seq, _, _) = head(&db.admin).await;
    maintainer
        .begin_recovery_epoch(&checkpoint.checkpoint().expect("c"), head_seq + 1)
        .await
        .expect("epoch");
    // unbound actor
    cast.dba
        .owner()
        .await
        .unbind_principal(&cast.reader.role)
        .await
        .expect("unbound");

    let rows = sqlx::query(
        "SELECT e.origin, e.event_type, b.envelope::text AS body FROM audit_store.events AS e \
         JOIN audit_store.event_bodies AS b ON b.seq = e.seq WHERE e.origin <> 'relay' \
         ORDER BY e.seq",
    )
    .fetch_all(&db.admin)
    .await
    .expect("control events");
    let mut seen = BTreeSet::new();
    for row in &rows {
        let origin = Origin::parse(row.get::<&str, _>("origin")).expect("origin");
        let body: String = row.get("body");
        let envelope = AuditEnvelope::from_json(&body, origin)
            .unwrap_or_else(|r| panic!("{}: {r}", row.get::<&str, _>("event_type")));
        assert_eq!(envelope.origin(), origin);
        assert!(
            audit_core::validate_envelope(envelope.as_value(), Origin::Relay).is_err(),
            "control events never pass the relay path"
        );
        seen.insert(envelope.event_type().to_owned());
    }
    let expected: BTreeSet<String> = Catalog::embedded()
        .events()
        .iter()
        .filter(|spec| spec.origin != Origin::Relay)
        .map(|spec| spec.event_type.clone())
        .filter(|t| t != "audit.recovery.fingerprint_rebound")
        .collect();
    assert_eq!(seen, expected);
    assert_eq!(db.assert_store_conforms_count().await, rows.len());
}
