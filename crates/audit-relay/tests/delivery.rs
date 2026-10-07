//! Delivery into the Audit Store (design §6, §14.3): end to end with every
//! producer shape, two-way failure classification (verdicts quarantine,
//! everything else holds without consuming attempts), source mismatch,
//! commit-unknown, concurrent duplicates, re-projection, Store outages and
//! the breaker, outage_streak, startup refusals, health and the CLI.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use audit_core::{
    AuditEnvelope, AuditStore, DocumentStagingProjection, IngestOutcome, Origin, OutageCode,
    RelayControl, RelayControlKind, SourceMismatchCode, StoreError,
};
use audit_relay::breaker::{Gate, Mode};
use audit_relay::config::RunConfig;
use audit_relay::health::{HealthOptions, health};
use audit_relay::ledger::DeliveryLedger;
use audit_relay::relay::connect_checked;
use audit_relay::session::{Side, StartupError};
use audit_relay::source::{RelayOutboxStore, RelayPolicy};
use audit_store_postgres::admin::AuditAdmin;
use outbox_delivery::{FenceResult, OutboxStore};
use serde_json::{Value, json};
use support::*;
use uuid::Uuid;

const CONVERGE: Duration = Duration::from_secs(40);

async fn quarantined(env: &Env, id: Uuid) -> Option<String> {
    env.delivery(id).await["quarantine_code"]
        .as_str()
        .map(str::to_owned)
}

async fn is_delivered(env: &Env, id: Uuid) -> bool {
    !env.delivery(id).await["delivered_at"].is_null()
}

/// FORCED_BY_TEST_SQL: make held rows claimable now instead of waiting for
/// the Store-side backoff.
async fn release_backoff(env: &Env) {
    exec(
        &env.doc_admin,
        "UPDATE audit_relay.deliveries SET available_at = clock_timestamp() \
         WHERE delivered_at IS NULL AND quarantined_at IS NULL AND lease_token IS NULL",
    )
    .await;
}

async fn store_down(env: &Env) {
    exec(
        &env.doc_admin,
        &format!(
            "ALTER DATABASE {STORE_DB} ALLOW_CONNECTIONS false; \
             SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{STORE_DB}' AND pid <> pg_backend_pid();"
        ),
    )
    .await;
}

async fn store_up(env: &Env) {
    exec(
        &env.doc_admin,
        &format!("ALTER DATABASE {STORE_DB} ALLOW_CONNECTIONS true"),
    )
    .await;
}

async fn all_delivered(env: &Env, n: i64) -> bool {
    delivered_count(env).await == n
}

#[tokio::test]
async fn every_producer_shape_reaches_the_store_without_reason_text() {
    let env = Env::start().await;
    let reasons = [
        "withdrawal reason: secret-alpha 理由",
        "metadata reason: secret-beta",
        "rename reason: secret-gamma",
    ];
    let mut traced = Staged::created();
    traced.trace_id = Some(Uuid::now_v7().to_string());
    let rows = vec![
        Staged::created(),
        traced.clone(),
        Staged::withdrawn(reasons[0]),
        Staged::metadata_changed(reasons[1]),
        Staged::folder_renamed(reasons[2]),
        Staged::access_policy_changed(),
    ];
    for row in &rows {
        env.insert(row).await;
    }
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || all_delivered(&env, rows.len() as i64)).await;
    assert_eq!(relay.breaker.mode(), Mode::Closed);
    assert_eq!(relay.breaker.gate(), Gate::Ok);
    assert!(relay.ledger.is_empty(), "every note was consumed");

    assert_eq!(assert_store_conforms(&env.store_admin).await, rows.len());
    let store = env.store_client().await;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.event_id).collect();
    let receipts = store.lookup_receipts(&ids).await.expect("receipts");
    assert_eq!(receipts.len(), rows.len());
    for row in &rows {
        let d = env.delivery(row.event_id).await;
        assert_eq!(d["store_outcome"], json!("stored"));
        assert_eq!(d["attempt_count"], json!(1));
        assert_eq!(d["store_recovery_epoch"], json!(1));
        assert!(d["lease_token"].is_null() && d["quarantined_at"].is_null());
        let receipt = receipts
            .iter()
            .find(|r| r.event_id == row.event_id)
            .expect("receipt");
        assert_eq!(d["store_seq"], json!(receipt.seq));
        assert_eq!(
            d["store_envelope_digest"],
            json!(format!(
                "\\x{}",
                audit_core::chain::to_hex(&receipt.envelope_digest)
            ))
        );
        // The Store holds the salted commitment computed on the source side.
        let commitment: Vec<u8> = sqlx::query_scalar(
            "SELECT audit_relay.commitment(commitment_salt, source_digest) \
             FROM audit_relay.deliveries WHERE event_id = $1",
        )
        .bind(row.event_id)
        .fetch_one(&env.doc_admin)
        .await
        .expect("commitment");
        assert_eq!(
            receipt.source_commitment.map(|c| c.to_vec()),
            Some(commitment)
        );
        let body = env.store_body(row.event_id).await.expect("body");
        for reason in reasons {
            assert!(
                !body.contains(reason),
                "reason text must not reach the Store"
            );
        }
        assert!(!body.contains("secret-"));
    }
    let withdrawn: Value =
        serde_json::from_str(&env.store_body(rows[2].event_id).await.expect("body")).expect("json");
    assert_eq!(
        withdrawn["data"]["reason"],
        json!({"provided": true, "utf8_bytes": reasons[0].len(), "text_retained": "source_systems"})
    );
    assert!(withdrawn["data"]["details"].get("actor").is_none());
    assert_eq!(
        withdrawn["data"]["provenance"]["registration"],
        json!("trigger")
    );
    let traced_body: Value =
        serde_json::from_str(&env.store_body(traced.event_id).await.expect("body")).expect("json");
    assert_eq!(
        traced_body["data"]["correlation"]["source_correlation_id"],
        json!(traced.trace_id)
    );
    let policy: Value =
        serde_json::from_str(&env.store_body(rows[5].event_id).await.expect("body")).expect("json");
    assert!(
        policy["data"].get("reason").is_none(),
        "no reason, not even provided:false"
    );

    // Health keeps produced / delivered / stored / verified apart.
    let report = health(
        &env.worker.pool,
        Arc::new(env.store_client().await),
        HealthOptions {
            forecast: true,
            reconcile: true,
        },
    )
    .await
    .expect("health");
    assert_eq!(report["produced"]["staged"], json!(6));
    assert_eq!(report["produced"]["registered"], json!(6));
    assert_eq!(report["produced"]["unregistered"], json!(0));
    assert_eq!(report["delivered"]["delivered"], json!(6));
    assert_eq!(report["delivered"]["pending"], json!(0));
    // The head also counts the Store's own control events (bootstrap,
    // bindings, grants): stored is not the same number as delivered.
    let head: i64 = sqlx::query_scalar("SELECT last_seq FROM audit_store.publication_head")
        .fetch_one(&env.store_admin)
        .await
        .expect("head");
    assert!(head > 6);
    assert_eq!(report["stored"]["gate"], json!("ok"));
    assert_eq!(
        report["stored"]["missing_types"],
        json!([]),
        "no catalog skew"
    );
    assert_eq!(report["stored"]["head_seq"], json!(head));
    assert_eq!(report["verified"]["last_verified_seq"], Value::Null);
    assert_eq!(report["verified"]["unverified_events"], json!(head));
    assert_eq!(report["reconcile"]["counts"]["ok"], json!(6));
    assert_eq!(report["forecast"], json!({}));
    assert_eq!(report["alarms"], json!([]), "{report}");
    let verifier = AuditAdmin::connect(env.verifier.pool.clone())
        .await
        .expect("verifier");
    let verified = verifier.verify(None, None).await.expect("verify");
    assert_eq!(verified.outcome, "ok");
    // The operator's login (no ingest role) reads the same status; it
    // cannot probe, so catalog skew is unknown rather than absent.
    let report = health(
        &env.worker.pool,
        Arc::new(env.operator_client().await),
        HealthOptions::default(),
    )
    .await
    .expect("health");
    assert_eq!(report["stored"]["gate"], json!("ok"));
    assert_eq!(report["stored"]["missing_types"], Value::Null);
    assert_eq!(report["verified"]["last_verified_seq"], json!(verified.seq));
    assert_eq!(report["verified"]["outcome"], json!("ok"));
    assert_eq!(report["verified"]["unverified_events"], json!(0));
    assert_eq!(report["stored"]["head_seq"], json!(verified.seq));
    assert_eq!(
        report["delivered"]["delivered"],
        json!(6),
        "delivered != stored"
    );
}

#[tokio::test]
async fn invalid_rows_quarantine_and_catalog_skew_is_held() {
    let env = Env::start().await;
    let good = Staged::created();
    let wrong_type = Staged::created().with_data(json!({"documentId": 123}));
    let missing = Staged::created().with_data(json!({}));
    let scalar = Staged::created().with_data(json!("text"));
    let mut oversize = Staged::created();
    oversize.subject = "s".repeat(1_500);
    let numeric_reason = Staged::withdrawn("x").with_data({
        let mut data = Staged::withdrawn("x").data;
        data["reason"] = json!(7);
        data
    });
    let future_type = Staged::created().with_type("document.future_event");
    let future_field = Staged::created().with_data({
        let mut data = Staged::created().data;
        data["futureField"] = json!(true);
        data
    });
    let rows = [
        &good,
        &wrong_type,
        &missing,
        &scalar,
        &oversize,
        &numeric_reason,
        &future_type,
        &future_field,
    ];
    for row in rows {
        env.insert(row).await;
    }
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || async {
        is_delivered(&env, good.event_id).await
            && env.status().await["quarantined_total"] == json!(5)
            && env.status().await["catalog_skew_held"] == json!(2)
    })
    .await;
    for (row, code) in [
        (&wrong_type, "invalid_field"),
        (&missing, "missing_field"),
        (&scalar, "invalid_field"),
        (&oversize, "source_row_too_large"),
        (&numeric_reason, "reason_not_string"),
    ] {
        assert_eq!(quarantined(&env, row.event_id).await.as_deref(), Some(code));
        assert!(env.store_rows(row.event_id).await.is_empty());
    }
    for row in [&future_type, &future_field] {
        let d = env.delivery(row.event_id).await;
        assert_eq!(d["last_error_code"], json!("relay_catalog_skew"));
        assert!(
            d["quarantined_at"].is_null(),
            "skew is held, not quarantined"
        );
        assert_eq!(d["attempt_count"], json!(0), "the attempt was returned");
        assert_eq!(d["outage_streak"], json!(0), "skew never counts");
    }
    assert_eq!(
        relay.breaker.mode(),
        Mode::Closed,
        "skew does not trip the breaker"
    );
    let report = health(
        &env.worker.pool,
        Arc::new(env.store_client().await),
        HealthOptions {
            forecast: true,
            reconcile: false,
        },
    )
    .await
    .expect("health");
    let alarms = report["alarms"].as_array().expect("alarms").clone();
    assert!(alarms.contains(&json!("catalog_skew_held")) && alarms.contains(&json!("quarantined")));
    assert_eq!(report["forecast"], json!({"relay_catalog_skew": 2}));
    assert_eq!(
        report["delivered"]["quarantined"]["invalid_field"],
        json!(2)
    );
    // Reconcile counts the held rows as their own class and records the
    // count in audit.reconciliation.completed (count_relay_catalog_skew).
    let run = audit_relay::reconcile::Reconciler::new(
        env.worker.pool.clone(),
        Arc::new(env.operator_client().await),
    )
    .run(false)
    .await
    .expect("reconcile");
    assert_eq!(run.counts.relay_catalog_skew, 2);
    assert_eq!(run.counts.pending, 0, "held skew is not plain pending");
    assert_eq!(run.counts.quarantined, 5);
    assert!(audit_relay::reconcile::alarms(&run.counts).contains(&"relay_catalog_skew"));
    let recorded = env.controls("audit.reconciliation.completed").await;
    let details = &recorded.last().expect("recorded").1;
    assert_eq!(details["count_relay_catalog_skew"], json!(2));
}

#[tokio::test]
async fn source_mismatch_records_the_control_event_first_and_once() {
    let env = Env::start().await;
    let tampered = Staged::created();
    let mut actor = Staged::withdrawn("why");
    actor.data["actor"] = json!({"identityProvider": "poc", "principalId": "someone-else"});
    let held = Staged::created();
    let noted = Staged::created();
    for row in [&tampered, &actor, &held, &noted] {
        env.insert(row).await;
    }
    for id in [tampered.event_id, held.event_id, noted.event_id] {
        // FORCED_BY_TEST_SQL: an owner disables the guard and edits the row.
        env.force(&format!(
            "UPDATE public.audit_outbox_events SET data = data || '{{\"documentId\": \"{}\"}}' \
             WHERE event_id = '{id}'",
            Uuid::now_v7()
        ))
        .await;
    }
    // `noted`: the control event was recorded and noted by an earlier
    // attempt that crashed before settling.
    let ledger = Arc::new(DeliveryLedger::default());
    let outbox = RelayOutboxStore::new(env.worker.pool.clone(), RelayPolicy::default(), ledger);
    let mut token = None;
    for claim in outbox
        .claim(Uuid::now_v7(), 8, Duration::from_secs(6))
        .await
        .expect("claim")
    {
        if claim.envelope.event_id == noted.event_id {
            token = Some(claim.lease_token);
        }
    }
    let token = token.expect("noted row claimed");
    let store = Arc::new(env.store_client().await);
    let earlier = store
        .record_relay_control(&RelayControl::from(
            RelayControlKind::SourceMismatchDetected {
                event_id: noted.event_id,
                code: SourceMismatchCode::SourceDigestMismatch,
            },
        ))
        .await
        .expect("earlier record")
        .seq;
    let ok: bool = sqlx::query_scalar("SELECT audit_relay.note_mismatch($1, $2, $3, $4)")
        .bind(noted.event_id)
        .bind(token)
        .bind("source_digest_mismatch")
        .bind(earlier)
        .fetch_one(&env.worker.pool)
        .await
        .expect("note");
    assert!(ok);
    exec(
        &env.doc_admin,
        "UPDATE audit_relay.deliveries SET lease_expires_at = clock_timestamp() - \
         interval '1 second' WHERE lease_token IS NOT NULL",
    )
    .await;

    // The Store cannot record: `held` stays an outage, not a quarantine.
    let wrapped = Arc::new(WrappedStore::new(store.clone()));
    wrapped.fail_control.store(true, Ordering::SeqCst);
    let relay = env
        .relay_with(RelayOverrides {
            store: Some(wrapped.clone()),
            ..RelayOverrides::default()
        })
        .await;
    drive(&relay, CONVERGE, || async {
        env.delivery(held.event_id).await["last_outage_code"] == json!("store_connection")
    })
    .await;
    let d = env.delivery(held.event_id).await;
    assert!(d["quarantined_at"].is_null());
    // 1 = the crashed manual claim above; the relay's own attempt returned.
    assert_eq!(d["attempt_count"], json!(1));
    assert!(
        env.controls("audit.integrity.source_mismatch_detected")
            .await
            .len()
            == 1
    );

    wrapped.fail_control.store(false, Ordering::SeqCst);
    release_backoff(&env).await;
    drive(&relay, CONVERGE, || async {
        env.status().await["quarantined_total"] == json!(4)
    })
    .await;
    for (row, code) in [
        (&tampered, "source_digest_mismatch"),
        (&actor, "actor_mismatch"),
        (&held, "source_digest_mismatch"),
        (&noted, "source_digest_mismatch"),
    ] {
        assert_eq!(quarantined(&env, row.event_id).await.as_deref(), Some(code));
        assert!(
            env.store_rows(row.event_id).await.is_empty(),
            "never ingested"
        );
    }
    let controls = env
        .controls("audit.integrity.source_mismatch_detected")
        .await;
    assert_eq!(controls.len(), 4, "one control event per event and code");
    for row in [&tampered, &actor, &held, &noted] {
        let matching: Vec<_> = controls
            .iter()
            .filter(|(_, details)| details["event_id"] == json!(row.event_id.to_string()))
            .collect();
        assert_eq!(matching.len(), 1, "{}", row.event_id);
        let d = env.delivery(row.event_id).await;
        assert_eq!(d["source_mismatch_seq"], json!(matching[0].0));
    }
    assert_store_conforms(&env.store_admin).await;
}

#[tokio::test]
async fn commit_unknown_retries_converge_to_a_duplicate() {
    let env = Env::start().await;
    let row = Staged::folder_renamed("unknown outcome");
    env.insert(&row).await;
    let target = row.event_id;
    let fired = Arc::new(AtomicBool::new(false));
    let mut wrapped = WrappedStore::new(Arc::new(env.store_client().await));
    let flag = fired.clone();
    wrapped.hook = Some(Box::new(move |id, result| {
        // The Store committed; the response is lost.
        (id == target && result.is_ok() && !flag.swap(true, Ordering::SeqCst)).then_some(
            StoreError::Outage {
                code: OutageCode::Transport,
            },
        )
    }));
    let wrapped = Arc::new(wrapped);
    let relay = env
        .relay_with(RelayOverrides {
            store: Some(wrapped.clone()),
            ..RelayOverrides::default()
        })
        .await;
    cycles(&relay, 2).await;
    let d = env.delivery(target).await;
    assert!(fired.load(Ordering::SeqCst));
    assert!(d["delivered_at"].is_null());
    assert_eq!(
        d["attempt_count"],
        json!(0),
        "an unknown outcome returns the attempt"
    );
    assert_eq!(d["last_outage_code"], json!("store_transport"));
    assert_eq!(
        env.store_rows(target).await.len(),
        1,
        "committed in the Store"
    );
    release_backoff(&env).await;
    drive(&relay, CONVERGE, || is_delivered(&env, target)).await;
    let d = env.delivery(target).await;
    assert_eq!(d["store_outcome"], json!("duplicate"));
    assert_eq!(d["attempt_count"], json!(1));
    assert_eq!(wrapped.calls_for(target), 2);
    assert_eq!(
        env.store_rows(target).await.len(),
        1,
        "exactly one Store event"
    );
    assert_eq!(relay.breaker.mode(), Mode::Closed);
}

async fn compete(relay: &audit_relay::relay::Relay) {
    for _ in 0..60 {
        let _ = relay.runner.run_cycle().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn concurrent_duplicates_store_once_and_stale_acks_are_lost() {
    let env = Env::start().await;
    let rows: Vec<Staged> = (0..16).map(|_| Staged::created()).collect();
    for row in &rows {
        env.insert(row).await;
    }
    // Two relay processes compete for the same rows.
    let a = env.relay().await;
    let b = env.relay().await;
    tokio::join!(compete(&a), compete(&b));
    assert_eq!(delivered_count(&env).await, 16);
    assert_eq!(assert_store_conforms(&env.store_admin).await, 16);

    // The same envelope ingested concurrently: one stored, one duplicate.
    let row = Staged::access_policy_changed();
    env.insert(&row).await;
    let ledger = Arc::new(DeliveryLedger::default());
    let outbox = RelayOutboxStore::new(env.worker.pool.clone(), RelayPolicy::default(), ledger);
    let claims = outbox
        .claim(Uuid::now_v7(), 1, Duration::from_secs(6))
        .await
        .expect("claim");
    let stale = claims[0].lease_token;
    let projection: DocumentStagingProjection =
        serde_json::from_value(claims[0].envelope.payload.clone()).expect("projection");
    let envelope = audit_core::project(&projection).expect("valid");
    let one = env.store_client().await;
    let two = env.store_client().await;
    let (x, y) = tokio::join!(one.ingest(&envelope), two.ingest(&envelope));
    let (x, y) = (x.expect("first"), y.expect("second"));
    let mut outcomes = [x.outcome, y.outcome];
    outcomes.sort_by_key(|o| o.as_str());
    assert_eq!(outcomes, [IngestOutcome::Duplicate, IngestOutcome::Stored]);
    assert_eq!((x.seq, x.envelope_digest), (y.seq, y.envelope_digest));
    assert_eq!(env.store_rows(row.event_id).await.len(), 1);
    // FORCED_BY_TEST_SQL: expire the first lease; a second relay delivers.
    exec(
        &env.doc_admin,
        &format!(
            "UPDATE audit_relay.deliveries SET lease_expires_at = clock_timestamp() - \
             interval '1 second' WHERE event_id = '{}'",
            row.event_id
        ),
    )
    .await;
    drive(&a, CONVERGE, || is_delivered(&env, row.event_id)).await;
    assert_eq!(
        env.delivery(row.event_id).await["store_outcome"],
        json!("duplicate")
    );
    // The stale holder can neither renew nor settle.
    assert_eq!(
        outbox
            .renew(row.event_id, stale, Duration::from_secs(6))
            .await,
        Ok(FenceResult::Lost)
    );
    outbox.ledger().record(
        row.event_id,
        stale,
        audit_relay::ledger::Note::Receipt {
            receipt: x,
            store_epoch: 1,
        },
    );
    assert_eq!(
        outbox.settle_success(row.event_id, stale).await,
        Ok(FenceResult::Lost)
    );
}

#[tokio::test]
async fn reprojection_acks_the_original_receipt_and_a_forgotten_bump_conflicts() {
    let env = Env::start().await;
    let row = Staged::created();
    env.insert(&row).await;
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || is_delivered(&env, row.event_id)).await;
    let original = env.delivery(row.event_id).await;

    // A newer adapter version (registered by a Store migration).
    register_type(&env.store_admin, "document.created", 2).await;
    let operator = env.operator.pool.clone();
    let row_id = row.event_id;
    let reset = |seq: i64| {
        let operator = operator.clone();
        async move {
            // Re-delivery path (operator reset; reconcile is tested elsewhere).
            let done: bool =
                sqlx::query_scalar("SELECT audit_relay.repair_reset_missing($1, $2, 1, 1)")
                    .bind(row_id)
                    .bind(seq)
                    .fetch_one(&operator)
                    .await
                    .expect("reset");
            assert!(done);
        }
    };
    reset(original["store_seq"].as_i64().expect("seq")).await;
    // The relay of the newer deploy ingests version 2 (its probe still
    // expects the version-1 catalog, which stays registered).
    let relay_v2 = env
        .relay_with(RelayOverrides {
            store: Some(Arc::new(ReprojectingStore {
                inner: Arc::new(env.store_client().await),
                adapter_version: 2,
            })),
            ..RelayOverrides::default()
        })
        .await;
    drive(&relay_v2, CONVERGE, || is_delivered(&env, row.event_id)).await;
    let reprojected = env.delivery(row.event_id).await;
    assert_eq!(reprojected["store_outcome"], json!("duplicate_reprojected"));
    assert_eq!(reprojected["store_seq"], original["store_seq"]);
    assert_eq!(
        reprojected["store_envelope_digest"],
        original["store_envelope_digest"]
    );
    assert_eq!(env.store_rows(row.event_id).await.len(), 1);

    // Same adapter version, different envelope: a conflict verdict.
    reset(original["store_seq"].as_i64().expect("seq")).await;
    let buggy: audit_relay::handler::Projector = Arc::new(|row: &DocumentStagingProjection| {
        let mut value = audit_core::project(row)?.into_value();
        value["time"] = json!("2026-10-07T09:09:09.000000Z");
        AuditEnvelope::from_value(value, Origin::Relay)
    });
    let relay_buggy = env
        .relay_with(RelayOverrides {
            projector: Some(buggy),
            ..RelayOverrides::default()
        })
        .await;
    drive(&relay_buggy, CONVERGE, || async {
        quarantined(&env, row.event_id).await.is_some()
    })
    .await;
    assert_eq!(
        quarantined(&env, row.event_id).await.as_deref(),
        Some("conflict")
    );
    assert_eq!(
        relay_buggy.breaker.mode(),
        Mode::Closed,
        "a verdict proves the Store works"
    );
    let conflicts = env.controls("audit.integrity.conflict_detected").await;
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].1["conflict_kind"],
        json!("projection_mismatch")
    );

    // The repair: fix the adapter, replay (audited), redeliver as duplicate.
    let outcome = audit_relay::replay::replay(
        &env.operator.pool,
        &env.operator_client().await,
        row.event_id,
    )
    .await
    .expect("replay");
    assert_eq!(outcome.previous_quarantine_code, "conflict");
    drive(&relay, CONVERGE, || is_delivered(&env, row.event_id)).await;
    assert_eq!(
        env.delivery(row.event_id).await["store_outcome"],
        json!("duplicate")
    );
    assert_eq!(env.store_rows(row.event_id).await.len(), 1);
    assert_store_conforms(&env.store_admin).await;
}

#[tokio::test]
async fn store_down_holds_without_consuming_attempts_then_drains() {
    let env = Env::start().await;
    let first = Staged::created();
    env.insert(&first).await;
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || is_delivered(&env, first.event_id)).await;
    assert_eq!(relay.breaker.mode(), Mode::Closed);
    let store = Arc::new(env.store_client().await);

    store_down(&env).await;
    // Business inserts continue while the Store is down.
    let rows: Vec<Staged> = (0..5).map(|_| Staged::withdrawn("held")).collect();
    for row in &rows {
        env.insert(row).await;
    }
    cycles(&relay, 8).await;
    let status = env.status().await;
    assert_eq!(status["pending"], json!(5));
    assert_eq!(status["delivered"], json!(1));
    for row in &rows {
        let d = env.delivery(row.event_id).await;
        assert_eq!(d["attempt_count"], json!(0), "no attempt consumed");
        assert!(d["quarantined_at"].is_null());
    }
    assert_ne!(relay.breaker.mode(), Mode::Closed);
    assert!(
        matches!(relay.breaker.gate(), Gate::Outage(_)),
        "{:?}",
        relay.breaker.gate()
    );
    let report = health(&env.worker.pool, store, HealthOptions::default())
        .await
        .expect("health with the Store down");
    assert_eq!(report["stored"]["available"], json!(false));
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("store_unavailable"))
    );
    assert_eq!(report["delivered"]["pending"], json!(5));

    store_up(&env).await;
    drive(&relay, CONVERGE, || all_delivered(&env, 6)).await;
    for row in &rows {
        assert_eq!(env.delivery(row.event_id).await["attempt_count"], json!(1));
    }
    assert_eq!(relay.breaker.mode(), Mode::Closed);
}

#[tokio::test]
async fn read_only_lock_and_statement_timeouts_and_version_skew_are_outages() {
    let env = Env::start().await;
    let warm = Staged::created();
    env.insert(&warm).await;
    let relay = env.relay().await;
    drive(&relay, CONVERGE, || is_delivered(&env, warm.event_id)).await;

    // Read-only Store: the probe keeps the relay from claiming at all ...
    exec(
        &env.doc_admin,
        &format!(
            "ALTER DATABASE {STORE_DB} SET default_transaction_read_only = on; \
             SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{STORE_DB}' AND pid <> pg_backend_pid();"
        ),
    )
    .await;
    let ro = Staged::created();
    env.insert(&ro).await;
    cycles(&relay, 4).await;
    assert_eq!(env.delivery(ro.event_id).await["attempt_count"], json!(0));
    assert_eq!(relay.breaker.gate(), Gate::Outage(OutageCode::ReadOnly));
    // ... and an ingest that reaches it (probe bypassed) is an outage too.
    let mut bypass = WrappedStore::new(Arc::new(env.store_client().await));
    bypass.bypass_probe = true;
    let bypassed = env
        .relay_with(RelayOverrides {
            store: Some(Arc::new(bypass)),
            ..RelayOverrides::default()
        })
        .await;
    drive(&bypassed, CONVERGE, || async {
        env.delivery(ro.event_id).await["last_outage_code"] == json!("store_read_only")
    })
    .await;
    let d = env.delivery(ro.event_id).await;
    assert_eq!(d["attempt_count"], json!(0));
    assert!(d["quarantined_at"].is_null());
    exec(
        &env.doc_admin,
        &format!(
            "ALTER DATABASE {STORE_DB} RESET default_transaction_read_only; \
             SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{STORE_DB}' AND pid <> pg_backend_pid();"
        ),
    )
    .await;
    release_backoff(&env).await;
    drive(&relay, CONVERGE, || is_delivered(&env, ro.event_id)).await;

    // Statement timeout while the head lock is held elsewhere.
    let slow = store_login(
        &env.cluster,
        &env.store_admin,
        "relay_svc_slow",
        &RELAY_SERVICE_ROLES,
    )
    .await;
    exec(
        &env.store_admin,
        &format!(
            "ALTER ROLE relay_svc_slow IN DATABASE {STORE_DB} SET statement_timeout = '300ms'"
        ),
    )
    .await;
    AuditAdmin::connect_owner(env.dba.pool.clone())
        .await
        .expect("owner")
        .bind_principal(&slow.role, "service", "audit-relay")
        .await
        .expect("bind");
    let slow_pool = connect_with_retry(&slow.url, 4).await;
    let mut slow_store = WrappedStore::new(Arc::new(
        audit_store_postgres::PostgresAuditStore::new(slow_pool, Duration::from_millis(1_500))
            .await
            .expect("slow store"),
    ));
    slow_store.bypass_probe = true;
    let slow_relay = env
        .relay_with(RelayOverrides {
            store: Some(Arc::new(slow_store)),
            ..RelayOverrides::default()
        })
        .await;
    let timed = Staged::created();
    env.insert(&timed).await;
    let mut blocker = env.store_admin.begin().await.expect("begin");
    sqlx::query("SELECT * FROM audit_store.publication_head FOR UPDATE")
        .fetch_all(&mut *blocker)
        .await
        .expect("hold head lock");
    drive(&slow_relay, CONVERGE, || async {
        env.delivery(timed.event_id).await["last_outage_code"] == json!("store_timeout")
    })
    .await;
    let d = env.delivery(timed.event_id).await;
    assert_eq!(
        (d["attempt_count"].clone(), d["outage_streak"].clone()),
        (json!(0), json!(0))
    );
    blocker.rollback().await.expect("release");
    release_backoff(&env).await;
    drive(&relay, CONVERGE, || is_delivered(&env, timed.event_id)).await;

    // Version skew: a type missing from the Store's registered_types. The
    // probe reports it (missing_types) and the gate admits nothing ...
    exec(
        &env.store_admin,
        "SET audit_store.write_context = 'migration'; \
         DELETE FROM audit_store.registered_types WHERE event_type = 'folder.renamed'; \
         RESET audit_store.write_context;",
    )
    .await;
    let skewed = Staged::folder_renamed("skew");
    let fine = Staged::created();
    env.insert(&skewed).await;
    env.insert(&fine).await;
    cycles(&relay, 4).await;
    assert_eq!(
        relay.breaker.gate(),
        Gate::Outage(OutageCode::UnregisteredType)
    );
    for row in [&skewed, &fine] {
        assert_eq!(env.delivery(row.event_id).await["attempt_count"], json!(0));
    }
    let report = health(
        &env.worker.pool,
        Arc::new(env.store_client().await),
        HealthOptions::default(),
    )
    .await
    .expect("health");
    assert_eq!(report["stored"]["missing_types"], json!(["folder.renamed"]));
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("store_catalog_skew"))
    );
    // ... and an ingest that reaches the Store anyway is held, not judged.
    let mut unprobed = WrappedStore::new(Arc::new(env.store_client().await));
    unprobed.bypass_probe = true;
    let unprobed = env
        .relay_with(RelayOverrides {
            store: Some(Arc::new(unprobed)),
            ..RelayOverrides::default()
        })
        .await;
    drive(&unprobed, CONVERGE, || async {
        is_delivered(&env, fine.event_id).await
            && env.delivery(skewed.event_id).await["last_outage_code"]
                == json!("store_unregistered_type")
    })
    .await;
    let d = env.delivery(skewed.event_id).await;
    assert!(
        d["quarantined_at"].is_null(),
        "version skew is not a verdict"
    );
    assert_eq!(d["attempt_count"], json!(0));
    register_type(&env.store_admin, "folder.renamed", 1).await;
    release_backoff(&env).await;
    drive(&relay, CONVERGE, || is_delivered(&env, skewed.event_id)).await;
    assert_eq!(relay.breaker.gate(), Gate::Ok);
    assert_store_conforms(&env.store_admin).await;
}

#[tokio::test]
async fn outage_streak_counts_residual_errors_only_after_progress() {
    let env = Env::start().await;
    // A policy revision with a small streak limit (test-only change).
    exec(
        &env.doc_admin,
        "UPDATE audit_relay.delivery_policy SET outage_streak_limit = 3, revision = 2",
    )
    .await;
    let policy = RelayPolicy {
        revision: 2,
        outage_streak_limit: 3,
        ..RelayPolicy::default()
    };
    let target = Staged::created();
    env.insert(&target).await;
    let wrapped = WrappedStore::new(Arc::new(env.store_client().await));
    wrapped.fail_before.lock().unwrap().push((
        target.event_id,
        StoreError::Outage {
            code: OutageCode::Other,
        },
    ));
    let relay = env
        .relay_with(RelayOverrides {
            store: Some(Arc::new(wrapped)),
            policy: Some(policy),
            ..RelayOverrides::default()
        })
        .await;
    // Without progress elsewhere, repeated residual errors count once.
    for _ in 0..4 {
        release_backoff(&env).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        cycles(&relay, 2).await;
    }
    let d = env.delivery(target.event_id).await;
    assert_eq!(d["outage_streak"], json!(1), "{d}");
    assert_eq!(d["attempt_count"], json!(0));
    assert!(d["quarantined_at"].is_null());
    assert_eq!(d["last_outage_code"], json!("store_other"));

    // With other deliveries succeeding in between, the streak grows.
    for _ in 0..30 {
        if quarantined(&env, target.event_id).await.is_some() {
            break;
        }
        env.insert(&Staged::created()).await;
        release_backoff(&env).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        cycles(&relay, 2).await;
    }
    assert_eq!(
        quarantined(&env, target.event_id).await.as_deref(),
        Some("outage_suspected_event_specific")
    );
    let d = env.delivery(target.event_id).await;
    assert_eq!(d["outage_streak"], json!(3));
    assert_eq!(d["attempt_count"], json!(0), "no attempt was consumed");
}

#[tokio::test]
async fn startup_refuses_privileged_same_database_options_and_bad_posture() {
    let env = Env::start().await;
    let su_doc = env.cluster.superuser_url(DOC_DB);
    let su_store = env.cluster.superuser_url(STORE_DB);
    let worker = env.worker.url.clone();
    let store = env.relay_store.url.clone();
    let timeout = Duration::from_millis(1_500);
    let refuse = |source: String, store: String, posture: bool| async move {
        connect_checked(&source, &store, timeout, posture)
            .await
            .err()
            .expect("refused")
    };
    assert_eq!(
        refuse(su_doc.clone(), store.clone(), true).await,
        StartupError::Privileged(Side::Source)
    );
    assert_eq!(
        refuse(worker.clone(), su_store.clone(), true).await,
        StartupError::Privileged(Side::Store)
    );
    assert_eq!(
        refuse(worker.clone(), su_doc.clone(), true).await,
        StartupError::SameDatabase
    );
    assert_eq!(
        refuse(
            format!("{worker}?options=-c%20synchronous_commit%3Doff"),
            store.clone(),
            true
        )
        .await,
        StartupError::UrlOptions { side: Side::Source }
    );
    assert_eq!(
        refuse(
            worker.clone(),
            format!("{store}?%6Fptions=-c%20synchronous_commit%3Doff"),
            true
        )
        .await,
        StartupError::UrlOptions { side: Side::Store }
    );
    exec(
        &env.doc_admin,
        &format!(
            "ALTER ROLE {} IN DATABASE {DOC_DB} SET synchronous_commit = off",
            env.worker.role
        ),
    )
    .await;
    assert_eq!(
        refuse(worker.clone(), store.clone(), true).await,
        StartupError::SynchronousCommitOff { side: Side::Source }
    );
    exec(
        &env.doc_admin,
        &format!(
            "ALTER ROLE {} IN DATABASE {DOC_DB} RESET synchronous_commit",
            env.worker.role
        ),
    )
    .await;
    exec(
        &env.doc_admin,
        "GRANT EXECUTE ON FUNCTION audit_relay.status() TO PUBLIC",
    )
    .await;
    assert!(matches!(
        refuse(worker.clone(), store.clone(), true).await,
        StartupError::PostureInvalid { .. }
    ));
    // health reports instead of refusing.
    let connections = connect_checked(&worker, &store, timeout, false)
        .await
        .expect("health connects");
    let report = health(
        &connections.source,
        Arc::new(connections.store),
        HealthOptions::default(),
    )
    .await
    .expect("health");
    assert!(
        report["alarms"]
            .as_array()
            .expect("alarms")
            .contains(&json!("relay_posture_invalid"))
    );
    exec(
        &env.doc_admin,
        "REVOKE EXECUTE ON FUNCTION audit_relay.status() FROM PUBLIC",
    )
    .await;

    // `run` end to end until a shutdown signal.
    let rows: Vec<Staged> = (0..3).map(|_| Staged::created()).collect();
    for row in &rows {
        env.insert(row).await;
    }
    let config = RunConfig::from_lookup(&|name: &str| match name {
        "AUDIT_SOURCE_DATABASE_URL" => Some(worker.clone()),
        "AUDIT_STORE_DATABASE_URL" => Some(store.clone()),
        "AUDIT_RELAY_LEASE_MS" => Some("6000".into()),
        "AUDIT_RELAY_RENEW_MS" => Some("1000".into()),
        "AUDIT_RELAY_POLL_MS" => Some("50".into()),
        "AUDIT_RELAY_DRAIN_MS" => Some("3000".into()),
        "AUDIT_RELAY_BREAKER_INITIAL_MS" => Some("20".into()),
        "AUDIT_RELAY_BREAKER_MAX_MS" => Some("200".into()),
        _ => None,
    })
    .expect("config");
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(async move { audit_relay::relay::run(&config, receiver).await });
    let deadline = tokio::time::Instant::now() + CONVERGE;
    while delivered_count(&env).await < 3 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "run did not deliver"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    sender.send(true).expect("signal");
    let summary = handle.await.expect("join").expect("clean shutdown");
    assert!(summary.settled >= 3, "{summary:?}");
}

#[tokio::test]
async fn cli_health_and_usage() {
    let env = Env::start().await;
    env.insert(&Staged::created()).await;
    let binary = env!("CARGO_BIN_EXE_audit-relay");
    let output = std::process::Command::new(binary)
        .output()
        .expect("run audit-relay");
    assert_eq!(output.status.code(), Some(2));
    let output = std::process::Command::new(binary)
        .args(["health", "--forecast"])
        .env("AUDIT_SOURCE_DATABASE_URL", &env.worker.url)
        .env("AUDIT_STORE_DATABASE_URL", &env.relay_store.url)
        .output()
        .expect("run audit-relay health");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("health JSON");
    assert_eq!(report["produced"]["staged"], json!(1));
    assert_eq!(report["forecast"], json!({"deliverable": 1}));
    let output = std::process::Command::new(binary)
        .args(["health"])
        .env(
            "AUDIT_SOURCE_DATABASE_URL",
            env.cluster.superuser_url(DOC_DB),
        )
        .env("AUDIT_STORE_DATABASE_URL", &env.relay_store.url)
        .output()
        .expect("run audit-relay health");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("superuser"), "{stderr}");
    assert!(
        !stderr.contains("postgres:postgres"),
        "no credentials in errors"
    );
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_relay.deliveries")
        .fetch_one(&env.doc_admin)
        .await
        .expect("count");
    assert_eq!(row.0, 1);
}
