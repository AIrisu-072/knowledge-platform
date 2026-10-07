//! Ingest (design §7.2), idempotency, genesis/chain, append-only guards,
//! head serialization and the AuditStore adapter's error mapping.

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::{
    AuditEnvelope, AuditStore, Catalog, GENESIS, IngestOutcome, Origin, OutageCode, StoreError,
};
use audit_store_postgres::{PostgresAuditStore, SessionError};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, Row};
use support::*;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn relay_store(db: &TestDb) -> (Login, PostgresAuditStore) {
    let relay = db
        .login(
            "relay",
            &["audit_store_ingest", "audit_store_relay_control"],
        )
        .await;
    let store = PostgresAuditStore::new(relay.pool.clone(), TIMEOUT)
        .await
        .expect("relay session is not privileged");
    (relay, store)
}

fn with(envelope: &AuditEnvelope, edit: impl FnOnce(&mut Value)) -> Value {
    let mut value = envelope.as_value().clone();
    edit(&mut value);
    value
}

#[tokio::test]
async fn registered_types_equal_the_catalog_relay_types() {
    let db = TestDb::start().await;
    let rows =
        sqlx::query("SELECT source, event_type, adapter_version FROM audit_store.registered_types")
            .fetch_all(&db.admin)
            .await
            .expect("registered types");
    let stored: BTreeSet<(String, String, i32)> = rows
        .iter()
        .map(|r| {
            (
                r.get("source"),
                r.get("event_type"),
                r.get("adapter_version"),
            )
        })
        .collect();
    let catalog: BTreeSet<(String, String, i32)> = Catalog::embedded()
        .events()
        .iter()
        .filter(|spec| spec.origin == Origin::Relay)
        .map(|spec| {
            (
                spec.source.clone(),
                spec.event_type.clone(),
                audit_core::legacy::LEGACY_ADAPTER_VERSION,
            )
        })
        .collect();
    assert_eq!(stored, catalog);
    assert_eq!(stored.len(), 21);
}

#[tokio::test]
async fn genesis_head_and_chain_match_audit_core() {
    let db = TestDb::start().await;
    let (last_seq, last_chain, epoch) = head(&db.admin).await;
    assert_eq!((last_seq, epoch), (0, 1));
    assert_eq!(last_chain, GENESIS.to_vec());
    // The SQL chain step equals the documented audit-core vector.
    let row = sqlx::query(
        "SELECT audit_store.chain_step(audit_store.genesis(), 1, \
            '0199a1b2-0000-7000-8000-000000000001'::uuid, \
            decode('c147ac99fc21ba6cc69a47812b1bb2415e353995e22b65d6a2b58a5222dd5f6f', 'hex')) AS c",
    )
    .fetch_one(&db.admin)
    .await
    .expect("chain step");
    let chain: Vec<u8> = row.get("c");
    assert_eq!(
        hex(&chain),
        "babbd336ec2c8556082424a253afb6d8e88ef5a63c9cbcab6cea8f441ad528e1"
    );

    let (_relay, store) = relay_store(&db).await;
    let mut receipts = Vec::new();
    for n in 0..3 {
        let envelope = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, n);
        let receipt = store.ingest(&envelope).await.expect("stored");
        assert_eq!(receipt.outcome, IngestOutcome::Stored);
        assert_eq!(receipt.adapter_version, 1);
        receipts.push(receipt);
    }
    assert_eq!(
        receipts.iter().map(|r| r.seq).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let first: Vec<u8> = sqlx::query("SELECT prev_chain FROM audit_store.events WHERE seq = 1")
        .fetch_one(&db.admin)
        .await
        .expect("seq 1")
        .get("prev_chain");
    assert_eq!(first, GENESIS.to_vec(), "seq 1 links to GENESIS");
    let status = store.probe().await.expect("probe");
    assert_eq!((status.head_seq, status.recovery_epoch), (3, 1));
    assert!(status.writable);
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn idempotency_outcomes_and_conflicts() {
    let db = TestDb::start().await;
    let (relay, store) = relay_store(&db).await;
    let id = Uuid::now_v7();
    let doc = Uuid::now_v7();
    let original = document_created(id, doc, OCCURRED, 7);
    let stored = store.ingest(&original).await.expect("stored");
    assert_eq!(stored.outcome, IngestOutcome::Stored);

    let again = store.ingest(&original).await.expect("duplicate");
    assert_eq!(again.outcome, IngestOutcome::Duplicate);
    assert_eq!(
        (again.seq, again.envelope_digest),
        (stored.seq, stored.envelope_digest)
    );

    // Same commitment and adapter version, different projection: conflict.
    let reprojected = document_created(id, doc, "2026-10-01T00:00:01.000000Z", 7);
    assert_eq!(store.ingest(&reprojected).await, Err(StoreError::Conflict));
    // Different commitment: conflict.
    let other_commitment = document_created(id, doc, OCCURRED, 8);
    assert_eq!(
        store.ingest(&other_commitment).await,
        Err(StoreError::Conflict)
    );

    // A newer adapter version re-projecting the same source row converges.
    db.exec(
        "SET audit_store.write_context = 'migration'; \
         INSERT INTO audit_store.registered_types VALUES \
             ('urn:knowledge-platform:document-platform', 'document.created', 2); \
         RESET audit_store.write_context;",
    )
    .await;
    let v2 = AuditEnvelope::from_value(
        with(&reprojected, |v| {
            v["data"]["provenance"]["adapter_version"] = json!(2);
        }),
        Origin::Relay,
    )
    .expect("v2 envelope");
    let receipt = store.ingest(&v2).await.expect("reprojected");
    assert_eq!(receipt.outcome, IngestOutcome::DuplicateReprojected);
    assert_eq!(
        (
            receipt.seq,
            receipt.envelope_digest,
            receipt.adapter_version
        ),
        (stored.seq, stored.envelope_digest, 1),
        "the original receipt is returned"
    );

    // Conflicts on identity: origin, source and type mismatches.
    let control_id: Uuid = sqlx::query(
        "SELECT event_id FROM audit_store.events WHERE origin = 'store' ORDER BY seq LIMIT 1",
    )
    .fetch_one(&db.admin)
    .await
    .expect("conflict control event")
    .get("event_id");
    let reused = with(&original, |v| v["id"] = json!(control_id.to_string()));
    assert_eq!(
        sql_ingest(&relay.pool, &reused).await,
        (
            "conflict".to_owned(),
            Some(2),
            Some("identity_mismatch".to_owned())
        )
    );
    let typed = read_confirmed(id, doc, OCCURRED, 7);
    let (status, _, code) = sql_ingest(&relay.pool, typed.as_value()).await;
    assert_eq!(
        (status.as_str(), code.as_deref()),
        ("conflict", Some("identity_mismatch"))
    );
    db.exec(
        "SET audit_store.write_context = 'migration'; \
         INSERT INTO audit_store.registered_types VALUES \
             ('urn:knowledge-platform:search-platform', 'document.created', 1); \
         RESET audit_store.write_context;",
    )
    .await;
    let other_source = with(&original, |v| {
        v["source"] = json!("urn:knowledge-platform:search-platform");
        v["data"]["provenance"]
            .as_object_mut()
            .expect("provenance")
            .remove("source_commitment");
    });
    let (status, _, code) = sql_ingest(&relay.pool, &other_source).await;
    assert_eq!(
        (status.as_str(), code.as_deref()),
        ("conflict", Some("identity_mismatch"))
    );

    // A commitment-less adapter: duplicates need byte-identical envelopes.
    let mut free = other_source.clone();
    free["id"] = json!(Uuid::now_v7().to_string());
    assert_eq!(sql_ingest(&relay.pool, &free).await.0, "stored");
    assert_eq!(sql_ingest(&relay.pool, &free).await.0, "duplicate");
    let mut changed_free = free.clone();
    changed_free["time"] = json!("2026-10-01T00:00:02.000000Z");
    let (status, _, code) = sql_ingest(&relay.pool, &changed_free).await;
    assert_eq!(
        (status.as_str(), code.as_deref()),
        ("conflict", Some("digest_mismatch"))
    );

    // Every conflict is recorded with its kind.
    let kinds: Vec<String> = control_events(&db.admin, "audit.integrity.conflict_detected")
        .await
        .into_iter()
        .map(|(_, d)| d["conflict_kind"].as_str().expect("kind").to_owned())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "projection_mismatch",
            "commitment_mismatch",
            "identity_mismatch",
            "identity_mismatch",
            "identity_mismatch",
            "digest_mismatch"
        ]
    );
    let first = &control_events(&db.admin, "audit.integrity.conflict_detected").await[0].1;
    assert_eq!(first["commitment_match"], json!(true));
    assert_eq!(first["existing_seq"], json!(stored.seq));
    assert_eq!(first["session_role"], json!(relay.role));
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn structural_rejections_are_verdict_rows_and_version_skew_is_an_outage() {
    let db = TestDb::start().await;
    let (relay, store) = relay_store(&db).await;
    let base = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 1);
    let cases: Vec<(&str, Value, &str, &str)> = vec![
        (
            "control source",
            with(&base, |v| {
                v["source"] = json!("urn:knowledge-platform:audit-store")
            }),
            "rejected",
            "control_type_forbidden",
        ),
        (
            "control type",
            with(&base, |v| v["type"] = json!("audit.access.denied")),
            "rejected",
            "control_type_forbidden",
        ),
        (
            "control resource",
            with(&base, |v| {
                v["data"]["resource"]["type"] = json!("AuditStore")
            }),
            "rejected",
            "control_type_forbidden",
        ),
        (
            "extension attribute",
            with(&base, |v| v["traceparent"] = json!("00-x")),
            "rejected",
            "invalid_envelope",
        ),
        (
            "bad id",
            with(&base, |v| v["id"] = json!("not-a-uuid")),
            "rejected",
            "invalid_envelope",
        ),
        (
            "bad time",
            with(&base, |v| v["time"] = json!("2026-02-30T00:00:00.000000Z")),
            "rejected",
            "invalid_envelope",
        ),
        (
            "missing commitment",
            with(&base, |v| {
                v["data"]["provenance"]
                    .as_object_mut()
                    .expect("provenance")
                    .remove("source_commitment");
            }),
            "rejected",
            "invalid_provenance",
        ),
        (
            "oversize",
            with(&base, |v| {
                v["data"]["details"]["pad"] = json!("x".repeat(33 * 1024))
            }),
            "rejected",
            "envelope_too_large",
        ),
        (
            "unregistered type",
            with(&base, |v| {
                v["type"] = json!("document.deleted");
                v["data"]["action"] = json!("document.deleted");
            }),
            "outage",
            "unregistered_type",
        ),
    ];
    for (name, envelope, status, code) in cases {
        let (got_status, seq, got_code) = sql_ingest(&relay.pool, &envelope).await;
        assert_eq!(
            (got_status.as_str(), got_code.as_deref(), seq),
            (status, Some(code), None),
            "{name}"
        );
    }
    assert_eq!(head(&db.admin).await.0, 0, "nothing was stored");

    // Through the adapter: Rust validation rejects control envelopes first,
    // and an unregistered adapter version is version skew (an outage).
    let skew = AuditEnvelope::from_value(
        with(&base, |v| {
            v["data"]["provenance"]["adapter_version"] = json!(3)
        }),
        Origin::Relay,
    )
    .expect("valid in Rust");
    assert_eq!(
        store.ingest(&skew).await,
        Err(StoreError::Outage {
            code: OutageCode::VersionSkew
        })
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn append_only_for_every_role_including_the_owner_path() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), TIMEOUT)
        .await
        .expect("relay");
    let envelope = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 1);
    store.ingest(&envelope).await.expect("stored");
    let statements = [
        "INSERT INTO audit_store.events SELECT * FROM audit_store.events WHERE seq = 1",
        "UPDATE audit_store.events SET subject = 'x' WHERE seq = 1",
        "UPDATE audit_store.events SET expired_at = now(), expired_by_seq = 2 WHERE seq = 1",
        "DELETE FROM audit_store.events WHERE seq = 1",
        "TRUNCATE audit_store.events CASCADE",
        "INSERT INTO audit_store.event_bodies VALUES (99, '{}')",
        "UPDATE audit_store.event_bodies SET envelope = '{}' WHERE seq = 1",
        "DELETE FROM audit_store.event_bodies WHERE seq = 1",
        "TRUNCATE audit_store.event_bodies",
        "UPDATE audit_store.publication_head SET last_seq = 0",
        "DELETE FROM audit_store.publication_head",
        "UPDATE audit_store.access_grants SET capability = 'maintain'",
        "DELETE FROM audit_store.access_grants",
        "UPDATE audit_store.principal_bindings SET principal_id = 'someone'",
        "DELETE FROM audit_store.principal_bindings",
        "TRUNCATE audit_store.access_intents",
        "TRUNCATE audit_store.retention_policies",
        "TRUNCATE audit_store.principal_bindings CASCADE",
        "DELETE FROM audit_store.registered_types",
        "TRUNCATE audit_store.registered_types",
    ];
    // Capability logins have no table privileges at all.
    for login in [
        &cast.relay,
        &cast.reader,
        &cast.verifier,
        &cast.admin,
        &cast.maintainer,
    ] {
        for statement in statements {
            let error = sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                .execute(&login.pool)
                .await
                .expect_err(statement);
            assert_eq!(sqlstate(&error), "42501", "{} {statement}", login.role);
        }
    }
    // The owner path (owner member, and the superuser) hits the guards.
    for pool in [&cast.dba.pool, &db.admin] {
        for statement in statements {
            let error = sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                .execute(pool)
                .await
                .expect_err(statement);
            assert!(
                matches!(sqlstate(&error).as_str(), "55000" | "23503"),
                "{statement}: {error}"
            );
        }
    }
    // A direct INSERT of a fresh row by the owner path is refused as well.
    let error = sqlx::raw_sql(sqlx::AssertSqlSafe(
        "INSERT INTO audit_store.events (seq, event_id, origin, source, event_type, \
            event_class, subject, occurred_at, stored_at, actor_issuer, actor_principal_id, \
            resource_type, resource_id, result, envelope_digest, digest_algorithm, \
            source_commitment, adapter_version, prev_chain, chain, recovery_epoch) \
         SELECT last_seq + 1, gen_random_uuid(), 'relay', \
            'urn:knowledge-platform:document-platform', 'document.created', \
            'CONTENT_LIFECYCLE', 'document/x', now(), now(), 'a', 'b', 'Document', 'x', \
            'success', sha256(''), 'kp-audit-jsonb-sha256-v1', sha256(''), 1, last_chain, \
            sha256(''), 1 FROM audit_store.publication_head",
    ))
    .execute(&cast.dba.pool)
    .await
    .expect_err("owner insert");
    assert_eq!(sqlstate(&error), "55000");
    assert_eq!(head(&db.admin).await.0, 1 + 1 + 5 + 5);
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn head_lock_serializes_publication_in_commit_order() {
    let db = TestDb::start().await;
    let (relay, _store) = relay_store(&db).await;
    let first = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 1);
    let second = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 2);

    let mut holder = PgConnection::connect(&relay.url).await.expect("connect");
    sqlx::query("BEGIN")
        .execute(&mut holder)
        .await
        .expect("begin");
    let held = sqlx::query("SELECT status, seq FROM audit_store.ingest($1::text::jsonb)")
        .bind(first.to_json_string())
        .fetch_one(&mut holder)
        .await
        .expect("held ingest");
    assert_eq!(held.get::<i64, _>("seq"), 1);

    let pool = relay.pool.clone();
    let text = second.to_json_string();
    let waiter = tokio::spawn(async move {
        sqlx::query("SELECT status, seq FROM audit_store.ingest($1::text::jsonb)")
            .bind(text)
            .fetch_one(&pool)
            .await
            .expect("waiting ingest")
            .get::<i64, _>("seq")
    });
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        !waiter.is_finished(),
        "the second ingest waits on the head lock"
    );
    assert_eq!(head(&db.admin).await.0, 0, "uncommitted seq is not visible");
    sqlx::query("COMMIT")
        .execute(&mut holder)
        .await
        .expect("commit");
    assert_eq!(waiter.await.expect("join"), 2);
    let rows = sqlx::query("SELECT seq, prev_chain, chain FROM audit_store.events ORDER BY seq")
        .fetch_all(&db.admin)
        .await
        .expect("events");
    let chain_1: Vec<u8> = rows[0].get("chain");
    let prev_2: Vec<u8> = rows[1].get("prev_chain");
    assert_eq!(chain_1, prev_2);
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn adapter_maps_every_non_verdict_failure_to_an_outage() {
    let db = TestDb::start().await;
    let (relay, store) = relay_store(&db).await;
    let envelope = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 1);

    // Posture violation: an outage, never a verdict.
    db.exec("GRANT EXECUTE ON FUNCTION audit_store.store_status() TO PUBLIC")
        .await;
    let posture = StoreError::Outage {
        code: OutageCode::PostureInvalid,
    };
    assert_eq!(store.ingest(&envelope).await, Err(posture.clone()));
    assert_eq!(store.probe().await, Err(posture));
    db.exec("REVOKE EXECUTE ON FUNCTION audit_store.store_status() FROM PUBLIC")
        .await;
    store.ingest(&envelope).await.expect("stored after repair");

    // Read-only sessions.
    let options: PgConnectOptions = relay.url.parse().expect("url");
    let read_only = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.options([("default_transaction_read_only", "on")]))
        .await
        .expect("read-only pool");
    let read_only = PostgresAuditStore::new(read_only, TIMEOUT)
        .await
        .expect("session");
    let other = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 2);
    let outage = StoreError::Outage {
        code: OutageCode::ReadOnly,
    };
    assert_eq!(read_only.ingest(&other).await, Err(outage.clone()));
    assert_eq!(read_only.probe().await, Err(outage));

    // Statement timeout (role setting) while another session holds the head.
    let mut holder = PgConnection::connect(&relay.url).await.expect("connect");
    sqlx::query("BEGIN")
        .execute(&mut holder)
        .await
        .expect("begin");
    sqlx::query("SELECT * FROM audit_store.probe()")
        .fetch_one(&mut holder)
        .await
        .expect("hold head");
    let error = store.ingest(&other).await.expect_err("lock wait");
    assert!(
        matches!(
            error,
            StoreError::Outage {
                code: OutageCode::LockUnavailable | OutageCode::Timeout
            }
        ),
        "{error:?}"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut holder)
        .await
        .expect("rollback");

    // Transport: a closed pool.
    relay.pool.close().await;
    assert_eq!(
        store.ingest(&other).await,
        Err(StoreError::Outage {
            code: OutageCode::Transport
        })
    );
    // Rust-side validation rejects a control envelope before any SQL.
    let control = with(&envelope, |v| v["type"] = json!("audit.access.denied"));
    assert!(AuditEnvelope::from_value(control, Origin::Relay).is_err());
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn privileged_sessions_are_refused() {
    let db = TestDb::start().await;
    let dba = db.owner_login("dba").await;
    let reader = db.login("reader", &["audit_store_reader"]).await;
    assert!(matches!(
        PostgresAuditStore::new(db.admin.clone(), TIMEOUT).await,
        Err(SessionError::Privileged)
    ));
    assert!(matches!(
        PostgresAuditStore::new(dba.pool.clone(), TIMEOUT).await,
        Err(SessionError::Privileged)
    ));
    assert!(matches!(
        audit_store_postgres::admin::AuditAdmin::connect(dba.pool.clone()).await,
        Err(SessionError::Privileged)
    ));
    assert!(matches!(
        audit_store_postgres::admin::AuditAdmin::connect_owner(reader.pool.clone()).await,
        Err(SessionError::NotOwnerMember)
    ));
    assert!(
        audit_store_postgres::admin::AuditAdmin::connect(reader.pool.clone())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn relay_receipts_and_relay_control_events() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), TIMEOUT)
        .await
        .expect("relay");
    let admin = cast.relay.admin().await;
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
    let mut receipts = Vec::new();
    for (n, id) in ids.iter().enumerate() {
        let n = u8::try_from(n).expect("small");
        receipts.push(
            store
                .ingest(&document_created(*id, Uuid::now_v7(), OCCURRED, n))
                .await
                .expect("stored"),
        );
    }
    let before = head(&db.admin).await.0;
    let found = admin.lookup_receipts(&ids).await.expect("lookup");
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].envelope_digest, receipts[0].envelope_digest);
    assert_eq!(found[0].source_commitment, Some([0; 32]));
    assert!(!found[0].expired);
    let listed = admin
        .list_source_receipts(DOCUMENT_SOURCE, found[0].seq, 10)
        .await
        .expect("list");
    assert_eq!(
        listed.iter().map(|r| r.event_id).collect::<Vec<_>>(),
        ids[1..]
    );
    assert_eq!(head(&db.admin).await.0, before, "lookups record nothing");

    let replay = admin
        .record_relay_control(
            "audit.delivery.replay_requested",
            ids[0],
            "delivery_unknown_at_limit",
            None,
        )
        .await
        .expect("replay recorded");
    let mismatch = admin
        .record_relay_control(
            "audit.integrity.source_mismatch_detected",
            ids[1],
            "source_digest_mismatch",
            None,
        )
        .await
        .expect("mismatch recorded");
    let counts = json!({
        "watermark": 3, "id_set_digest": "ab".repeat(32), "ok": 3, "delivered_missing": 0,
        "digest_mismatch": 0, "quarantined_stored": 0, "quarantined_conflict": 0,
        "pending": 0, "quarantined": 0, "unregistered": 0, "source_tampered": 0,
        "store_only": 0, "unaudited_replay": 0, "repaired_delivered_missing": 0,
        "repaired_quarantined_stored": 0, "repaired_unregistered": 0
    });
    let run = Uuid::now_v7();
    let reconciliation = admin
        .record_relay_control(
            "audit.reconciliation.completed",
            run,
            "read_only",
            Some(&counts),
        )
        .await
        .expect("reconciliation recorded");
    let controls = admin
        .lookup_control_receipts(&[replay, mismatch, reconciliation])
        .await
        .expect("control receipts");
    assert_eq!(
        controls
            .iter()
            .map(|c| (c.origin.as_str(), c.target_event_id))
            .collect::<Vec<_>>(),
        vec![
            ("relay_control", Some(ids[0])),
            ("relay_control", Some(ids[1])),
            ("relay_control", None),
        ]
    );
    // Invalid input and an unbound session are refused and recorded.
    let mut extra = counts.clone();
    extra["free_text"] = json!(1);
    assert_eq!(
        admin
            .record_relay_control(
                "audit.reconciliation.completed",
                run,
                "read_only",
                Some(&extra)
            )
            .await,
        Err(audit_store_postgres::AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    assert_eq!(
        admin
            .record_relay_control("audit.delivery.replay_requested", ids[0], "Bad Code", None)
            .await,
        Err(audit_store_postgres::AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    let unbound = db.login("operator", &["audit_store_relay_control"]).await;
    assert_eq!(
        unbound
            .admin()
            .await
            .record_relay_control(
                "audit.delivery.replay_requested",
                ids[0],
                "delivery_unknown_at_limit",
                None
            )
            .await,
        Err(audit_store_postgres::AdminError::Denied {
            code: "unbound".into()
        })
    );
    let denials = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(
        denials
            .iter()
            .map(|(_, d)| d["denial_code"].as_str().expect("code"))
            .collect::<Vec<_>>(),
        vec!["invalid_input", "invalid_input", "unbound"]
    );
    assert!(cast.relay.pool.acquire().await.is_ok());
    db.assert_store_conforms().await;
}
