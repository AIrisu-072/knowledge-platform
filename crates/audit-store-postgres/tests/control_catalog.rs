//! Every control event type the SQL builds conforms to the audit-core
//! catalog on its own origin path (design §4.5). This store produces every
//! store and relay_control type of the catalog, including
//! `audit.retention.expire_refused` and `audit.recovery.epoch_started`
//! (declare_recovery_pending, then begin_recovery_epoch).

mod support;

use std::collections::BTreeSet;

use audit_core::{
    AuditEnvelope, AuditStore, BoundedCode, Catalog, Origin, ReconcileCounts, ReconcileMode,
    RelayControl, RelayControlKind, SourceMismatchCode,
};
use audit_store_postgres::admin::{AccessChange, AccessOperation, parse_utc_text};
use serde_json::json;
use sqlx::Row;
use support::*;
use uuid::Uuid;

#[tokio::test]
async fn every_sql_built_control_event_conforms_to_the_catalog() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    let id = Uuid::now_v7();
    let document = Uuid::now_v7();
    store
        .ingest(&document_created(
            id,
            document,
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
    let (through, _, _) = head(&db.admin).await;
    let token = reader
        .open_access(
            AccessOperation::Investigate,
            &json!({
                "event_types": ["document.created"],
                "source": "urn:knowledge-platform:document-platform",
                "actor": {"issuer": "poc", "principal_id": "synthetic-human"},
                "resource": {"type": "Document", "id": document.to_string()},
                "occurred_from": "2019-01-01T00:00:00.000000Z",
                "occurred_to": "2030-01-01T00:00:00.000000Z",
                "event_ids": [id.to_string()],
                "seq_after": 0,
                "seq_through": through
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
    admin
        .set_retention_policy("kept", &json!({"event_classes": ["SECURITY"]}), None)
        .await
        .expect("retain_days is nullable");
    let maintainer = cast.maintainer.admin().await;
    let far = parse_utc_text("2100-01-01T00:00:00.000000Z").expect("far");
    let expired = maintainer
        .expire("all_documents", policy.revision, far, 1)
        .await
        .expect("expired");
    assert_eq!(expired.expired_count, 1);
    // expire_refused: stale revision and a policy without retain_days
    let stale = maintainer
        .expire("all_documents", policy.revision + 1, far, 1)
        .await
        .expect("stale attempt is recorded");
    assert_eq!(stale.status, "stale_revision");
    let kept = maintainer
        .expire("kept", 1, far, 1)
        .await
        .expect("not expirable is recorded");
    assert_eq!(kept.status, "not_expirable");
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
    store
        .record_relay_control(&RelayControl::from(RelayControlKind::ReplayRequested {
            event_id: id,
            previous_code: BoundedCode::new("delivery_unknown_at_limit").expect("code"),
        }))
        .await
        .expect("replay");
    store
        .record_relay_control(&RelayControl::from(
            RelayControlKind::SourceMismatchDetected {
                event_id: id,
                code: SourceMismatchCode::ActorMismatch,
            },
        ))
        .await
        .expect("mismatch");
    store
        .record_relay_control(&RelayControl::from(
            RelayControlKind::ReconciliationCompleted {
                run_id: Uuid::now_v7(),
                mode: ReconcileMode::Repair,
                watermark: 2,
                id_set_digest: [0xcd; 32],
                counts: ReconcileCounts {
                    ok: 2,
                    ..ReconcileCounts::default()
                },
            },
        ))
        .await
        .expect("reconciliation");
    // unbound actor: the owner member's own events and the unbound reader's
    // denial carry issuer db_role
    cast.dba
        .owner()
        .await
        .unbind_principal(&cast.reader.role)
        .await
        .expect("unbound");
    let _ = reader
        .open_access(AccessOperation::Investigate, &json!({}), 10, 1)
        .await;
    // recovery.epoch_started: declared, then a new epoch (planned move at
    // the checkpoint)
    let checkpoint = verifier
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("c");
    maintainer
        .declare_recovery_pending("incident_1")
        .await
        .expect("declared");
    maintainer
        .begin_recovery_epoch(Some(&checkpoint), None)
        .await
        .expect("epoch");

    let rows = sqlx::query(
        "SELECT e.origin, e.event_type, b.envelope::text AS body FROM audit_store.events AS e \
         JOIN audit_store.event_bodies AS b ON b.seq = e.seq WHERE e.origin <> 'relay' \
         ORDER BY e.seq",
    )
    .fetch_all(&db.admin)
    .await
    .expect("control events");
    let mut seen = BTreeSet::new();
    let mut db_role_actors = 0;
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
        let actor = &envelope.as_value()["data"]["actor"];
        if actor["issuer"] == "db_role" {
            db_role_actors += 1;
            let role = actor["principal_id"].as_str().expect("principal_id");
            assert!(
                role == cast.dba.role || role == cast.reader.role,
                "db_role actor is the session role: {role}"
            );
        }
        seen.insert(envelope.event_type().to_owned());
    }
    assert!(db_role_actors >= 2, "{db_role_actors}");
    let expected: BTreeSet<String> = Catalog::embedded()
        .events()
        .iter()
        .filter(|spec| spec.origin != Origin::Relay)
        .map(|spec| spec.event_type.clone())
        .collect();
    assert_eq!(seen, expected);
    assert_eq!(db.assert_store_conforms_count().await, rows.len());
}
