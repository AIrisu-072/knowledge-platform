//! Every control event type the SQL builds conforms to the audit-core
//! catalog on its own origin path (design §4.5). This store produces every
//! store and relay_control type of the catalog, including
//! `audit.retention.expire_refused` and `audit.recovery.epoch_started`
//! (declare_recovery_pending / report_regression, then begin_recovery_epoch),
//! along every emitting path: every value of the catalog's discriminating
//! enums (denial codes, access changes, retention refusals, verification
//! triggers, epoch and checkpoint classifications) is produced and validated.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use audit_core::{
    AuditEnvelope, AuditStore, BoundedCode, Catalog, Checkpoint, Origin, ReceiptIdentity,
    ReconcileCounts, ReconcileMode, RelayControl, RelayControlKind, SourceMismatchCode,
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
    // invalid_input: an event type that is neither registered nor control.
    let _ = reader
        .open_access(
            AccessOperation::Investigate,
            &json!({"event_types": ["not.registered"]}),
            10,
            1,
        )
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
    // self_grant
    let _ = admin
        .change_access(ISSUER, "admin-1", "verify", AccessChange::Grant)
        .await;
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
    // held: v1 has no function to place a hold; a reserved row blocks expiry.
    db.exec("INSERT INTO audit_store.legal_holds (hold_id) VALUES (gen_random_uuid())")
        .await;
    let held = maintainer
        .expire("all_documents", policy.revision, far, 1)
        .await
        .expect("held is recorded");
    assert_eq!(held.status, "held");
    // integrity.verified (verify and checkpoint)
    let verifier = cast.verifier.admin().await;
    assert_eq!(
        verifier.verify(None, None).await.expect("verify").outcome,
        "ok"
    );
    let first_checkpoint = verifier
        .checkpoint()
        .await
        .expect("checkpoint")
        .checkpoint()
        .expect("value");
    // relay control events (replay and repair: the operator's own login)
    let operator = relay_operator(&db, &cast).await.store().await;
    operator
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
    operator
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
    start_recovery_epoch(&maintainer, Some(&checkpoint), None)
        .await
        .expect("epoch");
    // A regression epoch past an older checkpoint (ahead), then declared
    // restores against checkpoints whose epoch, chain or seq disagree.
    store
        .report_regression(&ReceiptIdentity {
            seq: 1,
            event_id: Uuid::now_v7(),
            envelope_digest: [7; 32],
        })
        .await
        .expect("reported");
    let (regression, _) = start_recovery_epoch(&maintainer, Some(&first_checkpoint), None)
        .await
        .expect("regression epoch");
    assert_eq!(
        (
            regression.classification.as_str(),
            regression.checkpoint_classification.as_deref()
        ),
        ("regression", Some("ahead"))
    );
    let (head_seq, _, head_epoch) = head(&db.admin).await;
    for (n, (claimed, class)) in [
        (
            Checkpoint {
                epoch: 9,
                ..first_checkpoint
            },
            "epoch_mismatch",
        ),
        (
            Checkpoint {
                chain: [7; 32],
                ..first_checkpoint
            },
            "mismatch",
        ),
        (
            Checkpoint {
                epoch: head_epoch,
                seq: head_seq + 10,
                chain: [1; 32],
            },
            "store_behind",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        maintainer
            .declare_recovery_pending(&format!("incident_{}", n + 2))
            .await
            .expect("declared");
        let (started, _) = start_recovery_epoch(&maintainer, Some(&claimed), None)
            .await
            .expect("epoch");
        assert_eq!(
            (
                started.classification.as_str(),
                started.checkpoint_classification.as_deref()
            ),
            ("restore", Some(class))
        );
    }
    // reapplied: retention re-run (held, at the current revision), then the
    // maintainer's and the administrator's records.
    let rerun = maintainer
        .expire("all_documents", policy.revision, far, 1)
        .await
        .expect("re-run");
    assert_eq!(rerun.status, "held");
    maintainer
        .confirm_retention_reapplied()
        .await
        .expect("retention re-applied");
    admin
        .record_access_reapplied()
        .await
        .expect("access re-applied");
    // not_source_service (last: an ingest login that is not a registered
    // source service also breaks the posture).
    let intruder = db.login("intruder", &["audit_store_ingest"]).await;
    cast.dba
        .owner()
        .await
        .bind_principal(&intruder.role, ISSUER, "intruder-1")
        .await
        .expect("bound");
    let _ = intruder
        .store()
        .await
        .ingest(&document_created(
            Uuid::now_v7(),
            Uuid::now_v7(),
            OCCURRED,
            9,
        ))
        .await;

    let rows = sqlx::query(
        "SELECT e.origin, e.event_type, b.envelope::text AS body FROM audit_store.events AS e \
         JOIN audit_store.event_bodies AS b ON b.seq = e.seq WHERE e.origin <> 'relay' \
         ORDER BY e.seq",
    )
    .fetch_all(&db.admin)
    .await
    .expect("control events");
    let mut seen = BTreeSet::new();
    let mut discriminators: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
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
            assert_eq!(
                envelope.as_value()["data"]["details"]["session_role"],
                actor["principal_id"]
            );
        }
        seen.insert(envelope.event_type().to_owned());
        for (field, value) in envelope.as_value()["data"]["details"]
            .as_object()
            .expect("details")
        {
            if let Some(value) = value.as_str() {
                discriminators
                    .entry((envelope.event_type().to_owned(), field.clone()))
                    .or_default()
                    .insert(value.to_owned());
            }
        }
    }
    // Every value of the discriminating enums was emitted (and validated).
    for (event_type, field) in [
        ("audit.access.denied", "denial_code"),
        ("audit.access_policy.changed", "change"),
        ("audit.retention.expire_refused", "refusal"),
        ("audit.integrity.verified", "trigger"),
        ("audit.integrity.verified", "outcome"),
        ("audit.recovery.epoch_started", "classification"),
        ("audit.recovery.epoch_started", "checkpoint_classification"),
        ("audit.reconciliation.completed", "mode"),
    ] {
        let spec = Catalog::embedded().get(event_type).expect("type");
        let values: BTreeSet<String> = spec.fields[field].values.iter().cloned().collect();
        let emitted = discriminators
            .get(&(event_type.to_owned(), field.to_owned()))
            .cloned()
            .unwrap_or_default();
        let missing: Vec<&String> = values.difference(&emitted).collect();
        // outcome "violations" needs a tampered chain (store_integrity) and
        // mode "read_only" a read-only reconcile run (relay tests).
        let allowed: &[&str] = match field {
            "outcome" => &["violations"],
            "mode" => &["read_only"],
            _ => &[],
        };
        assert!(
            missing.iter().all(|m| allowed.contains(&m.as_str())),
            "{event_type}.{field} never emitted: {missing:?}"
        );
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
