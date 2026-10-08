//! Ingest (design §7.2), idempotency, genesis/chain, append-only guards,
//! head serialization, the source-service gate (denial and coalescing), the
//! admission probe and the AuditStore adapter's error mapping.

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use audit_core::catalog::DOCUMENT_SOURCE;
use audit_core::{
    AuditEnvelope, AuditStore, Catalog, GENESIS, IngestOutcome, Origin, OutageCode,
    ReceiptIdentity, RelayControl, RelayControlKind, SourceMismatchCode, StoreError, StoreState,
};
use audit_store_postgres::{PostgresAuditStore, SessionError};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, Row};
use support::*;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(10);

async fn relay_store(db: &TestDb) -> (Login, PostgresAuditStore) {
    let relay = db.service_relay().await;
    let store = relay.store().await;
    (relay, store)
}

fn with(envelope: &AuditEnvelope, edit: impl FnOnce(&mut Value)) -> Value {
    let mut value = envelope.as_value().clone();
    edit(&mut value);
    value
}

fn outage(code: OutageCode) -> StoreError {
    StoreError::outage(code)
}

/// The Store's closed set of control types (event type filters accept
/// registered relay types and these) is the catalog's control entries.
#[tokio::test]
async fn control_types_equal_the_catalog_control_types() {
    let db = TestDb::start().await;
    let source: String = sqlx::query_scalar(
        "SELECT p.prosrc FROM pg_proc AS p JOIN pg_namespace AS n ON n.oid = p.pronamespace \
         WHERE n.nspname = 'audit_store' AND p.proname = 'is_control_type'",
    )
    .fetch_one(&db.admin)
    .await
    .expect("is_control_type");
    let listed: BTreeSet<String> = source
        .split('\'')
        .filter(|part| part.starts_with("audit."))
        .map(str::to_owned)
        .collect();
    let catalog = Catalog::embedded();
    let control: BTreeSet<String> = catalog
        .events()
        .iter()
        .filter(|spec| spec.origin != Origin::Relay)
        .map(|spec| spec.event_type.clone())
        .collect();
    assert_eq!(listed, control);
    assert_eq!(control.len(), 14);
    for spec in catalog.events() {
        let is_control: bool = sqlx::query_scalar("SELECT audit_store.is_control_type($1)")
            .bind(&spec.event_type)
            .fetch_one(&db.admin)
            .await
            .expect("call");
        let is_registered: bool = sqlx::query_scalar("SELECT audit_store.is_registered_type($1)")
            .bind(&spec.event_type)
            .fetch_one(&db.admin)
            .await
            .expect("call");
        assert_eq!(
            is_control,
            spec.origin != Origin::Relay,
            "{}",
            spec.event_type
        );
        assert_eq!(
            is_registered,
            spec.origin == Origin::Relay,
            "{}",
            spec.event_type
        );
    }
}

#[tokio::test]
async fn registered_types_equal_the_catalog_registered_types() {
    let db = TestDb::start().await;
    let rows = sqlx::query(
        "SELECT source, event_type, adapter_version, source_format FROM audit_store.registered_types",
    )
    .fetch_all(&db.admin)
    .await
    .expect("registered types");
    let stored: BTreeSet<(String, String, i32, String)> = rows
        .iter()
        .map(|r| {
            (
                r.get("source"),
                r.get("event_type"),
                r.get("adapter_version"),
                r.get("source_format"),
            )
        })
        .collect();
    let catalog = Catalog::embedded();
    let expected: BTreeSet<(String, String, i32, String)> = catalog
        .registered_types()
        .into_iter()
        .map(|(source, event_type, version)| {
            (
                source.to_owned(),
                event_type.to_owned(),
                version,
                catalog
                    .adapter(source)
                    .expect("adapter")
                    .source_format
                    .clone(),
            )
        })
        .collect();
    assert_eq!(stored, expected);
    assert_eq!(stored.len(), 21);
    // The seeded source service is the v1 relay principal for that source.
    let services: Vec<(String, String, String)> =
        sqlx::query_as("SELECT issuer, principal_id, source FROM audit_store.source_services")
            .fetch_all(&db.admin)
            .await
            .expect("source services");
    assert_eq!(
        services,
        vec![(
            "service".to_owned(),
            "audit-relay".to_owned(),
            DOCUMENT_SOURCE.to_owned()
        )]
    );
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
    // The SQL expired-set digest equals audit_core::expired_set_digest.
    for seqs in [vec![], vec![3_i64, 5, 9], vec![9, 3, 5], vec![1]] {
        let digest: Vec<u8> = sqlx::query_scalar("SELECT audit_store.expired_set_digest($1)")
            .bind(&seqs)
            .fetch_one(&db.admin)
            .await
            .expect("expired set digest");
        assert_eq!(digest, audit_core::expired_set_digest(&seqs).to_vec());
    }

    let (relay, store) = relay_store(&db).await;
    let before = head(&db.admin).await.0;
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
        vec![before + 1, before + 2, before + 3]
    );
    let first: Vec<u8> = sqlx::query("SELECT prev_chain FROM audit_store.events WHERE seq = 1")
        .fetch_one(&db.admin)
        .await
        .expect("seq 1")
        .get("prev_chain");
    assert_eq!(first, GENESIS.to_vec(), "seq 1 links to GENESIS");
    let status = store.probe(&expectation(None)).await.expect("probe");
    assert_eq!((status.head_seq, status.recovery_epoch), (before + 3, 1));
    assert_eq!(status.state, StoreState::Operational);
    assert_eq!(status.admission(), Ok(()));
    let by: String =
        sqlx::query_scalar("SELECT ingested_by_db_role FROM audit_store.events WHERE seq = $1")
            .bind(receipts[0].seq)
            .fetch_one(&db.admin)
            .await
            .expect("ingested_by_db_role");
    assert_eq!(by, relay.role);
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn idempotency_outcomes_and_conflicts() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let relay = &cast.relay;
    let store = relay.store().await;
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

    // Same commitment and adapter version, different projection: conflict
    // (a forgotten adapter_version bump).
    let reprojected = document_created(id, doc, "2026-10-01T00:00:01.000000Z", 7);
    assert!(matches!(
        store.ingest(&reprojected).await,
        Err(StoreError::Conflict { .. })
    ));
    // Different commitment: conflict.
    let other_commitment = document_created(id, doc, OCCURRED, 8);
    assert!(matches!(
        store.ingest(&other_commitment).await,
        Err(StoreError::Conflict { .. })
    ));

    // A newer adapter version re-projecting the same source row converges.
    // AuditEnvelope only carries the catalog's adapter version, so the v2
    // envelope goes to the SQL function directly.
    db.exec(
        "SET audit_store.write_context = 'migration'; \
         INSERT INTO audit_store.registered_types VALUES \
             ('urn:knowledge-platform:document-platform', 'document.created', 2, \
              'document-audit-outbox-v0'); \
         RESET audit_store.write_context;",
    )
    .await;
    let v2 = with(&reprojected, |v| {
        v["data"]["provenance"]["adapter_version"] = json!(2);
    });
    let row = sqlx::query(
        "SELECT status, seq, envelope_digest, adapter_version FROM audit_store.ingest($1::text::jsonb)",
    )
    .bind(v2.to_string())
    .fetch_one(&relay.pool)
    .await
    .expect("reprojected");
    assert_eq!(row.get::<String, _>("status"), "duplicate_reprojected");
    assert_eq!(row.get::<i64, _>("seq"), stored.seq);
    assert_eq!(
        row.get::<Vec<u8>, _>("envelope_digest"),
        stored.envelope_digest.to_vec(),
        "the original receipt is returned"
    );
    assert_eq!(row.get::<i32, _>("adapter_version"), 1);

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
            Some(1),
            Some("identity_mismatch".to_owned())
        )
    );
    let typed = read_confirmed(id, doc, OCCURRED, 7);
    let (status, _, code) = sql_ingest(&relay.pool, typed.as_value()).await;
    assert_eq!(
        (status.as_str(), code.as_deref()),
        ("conflict", Some("identity_mismatch"))
    );
    // A second (commitment-less) source the relay service is registered for.
    db.exec(
        "SET audit_store.write_context = 'migration'; \
         INSERT INTO audit_store.registered_types VALUES \
             ('urn:knowledge-platform:search-platform', 'document.created', 1, 'search-audit-v0'); \
         RESET audit_store.write_context;",
    )
    .await;
    let other_source = with(&original, |v| {
        v["source"] = json!("urn:knowledge-platform:search-platform");
        v["data"]["provenance"]["source_format"] = json!("search-audit-v0");
        v["data"]["provenance"]
            .as_object_mut()
            .expect("provenance")
            .remove("source_commitment");
    });
    // Not yet registered as a service of that source: denied, an outage.
    let (status, _, code) = sql_ingest(&relay.pool, &other_source).await;
    assert_eq!(
        (status.as_str(), code.as_deref()),
        ("denied", Some("not_source_service"))
    );
    let registered = cast
        .dba
        .owner()
        .await
        .register_source_service(
            "service",
            "audit-relay",
            "urn:knowledge-platform:search-platform",
        )
        .await
        .expect("register");
    assert_eq!(registered.status, "registered");
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
    let before = head(&db.admin).await.0;
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
            "control character in subject",
            with(&base, |v| v["subject"] = json!("document/\u{7}")),
            "rejected",
            "invalid_envelope",
        ),
        (
            "C1 control character in actor",
            with(&base, |v| {
                v["data"]["actor"]["principal_id"] = json!("synthetic\u{85}human");
            }),
            "rejected",
            "invalid_actor",
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
        (
            "unregistered adapter version",
            with(&base, |v| {
                v["data"]["provenance"]["adapter_version"] = json!(3)
            }),
            "outage",
            "unregistered_type",
        ),
        (
            "other source format",
            with(&base, |v| {
                v["data"]["provenance"]["source_format"] = json!("document-audit-outbox-v9")
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
        assert!(
            got_code
                .as_deref()
                .is_some_and(|c| audit_core::port::BoundedCode::new(c).is_some()),
            "{name}: codes are [a-z0-9_]{{1,64}}"
        );
    }
    assert_eq!(head(&db.admin).await.0, before, "nothing was stored");

    // A control envelope is refused locally by audit-core's origin precheck,
    // without any SQL call.
    let control = audit_core::AuditEnvelope::from_value(
        control_envelope_value("audit.access.denied"),
        Origin::Store,
    )
    .expect("a catalog-valid control envelope");
    let error = store.ingest(&control).await.expect_err("refused");
    assert!(matches!(
        &error,
        StoreError::Rejected { code, .. } if code.as_str() == "control_type_forbidden"
    ));
    assert!(error.is_terminal());
    assert_eq!(head(&db.admin).await.0, before);
    db.assert_store_conforms().await;
}

/// A minimal valid control envelope of `event_type` (for the local refusal).
fn control_envelope_value(event_type: &str) -> Value {
    json!({
        "specversion": "1.0",
        "id": Uuid::now_v7().to_string(),
        "source": "urn:knowledge-platform:audit-store",
        "type": event_type,
        "subject": "audit-store",
        "time": "2026-10-07T01:02:03.000000Z",
        "datacontenttype": "application/json",
        "dataschema": "urn:knowledge-platform:audit:payload:v1",
        "data": {
            "schema_version": 1,
            "event_class": "SECURITY",
            "action": event_type,
            "actor": {"issuer": "db_role", "principal_id": "someone"},
            "resource": {"type": "AuditStore", "id": "audit-store"},
            "result": "denied",
            "correlation": {},
            "details": {"session_role": "someone", "operation": "ingest",
                        "denial_code": "unbound"},
            "extensions": {},
            "provenance": {"source_format": "audit-store-control-v1", "adapter_version": 1}
        }
    })
}

#[tokio::test]
async fn append_only_for_every_role_including_the_owner_path() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
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
        "UPDATE audit_store.publication_head SET recovery_pending = FALSE",
        "DELETE FROM audit_store.publication_head",
        "UPDATE audit_store.access_grants SET capability = 'maintain'",
        "DELETE FROM audit_store.access_grants",
        "UPDATE audit_store.principal_bindings SET principal_id = 'someone'",
        "DELETE FROM audit_store.principal_bindings",
        "INSERT INTO audit_store.source_services VALUES ('a', 'b', 'c', NULL)",
        "DELETE FROM audit_store.source_services",
        "INSERT INTO audit_store.denial_streaks VALUES ('x', 'unbound', 'ingest', 1, now(), 0)",
        "TRUNCATE audit_store.access_intents",
        "TRUNCATE audit_store.retention_policies",
        "TRUNCATE audit_store.principal_bindings CASCADE",
        "TRUNCATE audit_store.source_services",
        "TRUNCATE audit_store.denial_streaks",
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
            source_commitment, adapter_version, prev_chain, chain, recovery_epoch, \
            ingested_by_db_role) \
         SELECT last_seq + 1, gen_random_uuid(), 'relay', \
            'urn:knowledge-platform:document-platform', 'document.created', \
            'CONTENT_LIFECYCLE', 'document/x', now(), now(), 'a', 'b', 'Document', 'x', \
            'success', sha256(''), 'kp-audit-jsonb-sha256-v1', sha256(''), 1, last_chain, \
            sha256(''), 1, 'postgres' FROM audit_store.publication_head",
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
    let before = head(&db.admin).await.0;
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
    assert_eq!(held.get::<i64, _>("seq"), before + 1);

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
    assert_eq!(
        head(&db.admin).await.0,
        before,
        "uncommitted seq is not visible"
    );
    sqlx::query("COMMIT")
        .execute(&mut holder)
        .await
        .expect("commit");
    assert_eq!(waiter.await.expect("join"), before + 2);
    let rows = sqlx::query(
        "SELECT seq, prev_chain, chain FROM audit_store.events WHERE seq > $1 ORDER BY seq",
    )
    .bind(before)
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

    // Posture violation: an outage (ingest) and a non-operational state
    // (probe), never a verdict.
    db.exec("GRANT EXECUTE ON FUNCTION audit_store.posture_check() TO PUBLIC")
        .await;
    assert_eq!(
        store.ingest(&envelope).await,
        Err(outage(OutageCode::PostureInvalid))
    );
    let status = store.probe(&expectation(None)).await.expect("probe runs");
    assert_eq!(status.state, StoreState::PostureInvalid);
    assert_eq!(status.admission(), Err(OutageCode::PostureInvalid));
    db.exec("REVOKE EXECUTE ON FUNCTION audit_store.posture_check() FROM PUBLIC")
        .await;
    store.ingest(&envelope).await.expect("stored after repair");

    // Read-only sessions: the probe reports the state, an ingest is an outage.
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
    assert_eq!(
        read_only.ingest(&other).await,
        Err(outage(OutageCode::ReadOnly))
    );
    let status = read_only.probe(&expectation(None)).await.expect("probe");
    assert_eq!(status.state, StoreState::ReadOnly);
    assert_eq!(status.admission(), Err(OutageCode::ReadOnly));

    // Lock timeout (5 s function lock_timeout) while another session holds the head.
    let mut holder = PgConnection::connect(&relay.url).await.expect("connect");
    sqlx::query("BEGIN")
        .execute(&mut holder)
        .await
        .expect("begin");
    sqlx::query("SELECT * FROM audit_store.probe($1, 1, ARRAY[]::text[], NULL, NULL, NULL)")
        .bind(DOCUMENT_SOURCE)
        .fetch_one(&mut holder)
        .await
        .expect("hold head");
    let error = store.ingest(&other).await.expect_err("lock wait");
    assert!(
        matches!(
            error.outage_code(),
            Some(OutageCode::LockUnavailable | OutageCode::Timeout)
        ),
        "{error:?}"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut holder)
        .await
        .expect("rollback");

    // A failed SQL call (bad probe input) is an outage too (22023 -> Other).
    let bad = audit_core::ProbeExpectation {
        source: "\u{7}".to_owned(),
        adapter_version: 1,
        types: Vec::new(),
        last_ack: None,
    };
    assert_eq!(store.probe(&bad).await, Err(outage(OutageCode::Other)));

    // Transport: a closed pool.
    relay.pool.close().await;
    assert_eq!(
        store.ingest(&other).await,
        Err(outage(OutageCode::Transport))
    );
    // Rust-side validation rejects a control envelope before any SQL.
    let control = with(&envelope, |v| v["type"] = json!("audit.access.denied"));
    assert!(AuditEnvelope::from_value(control, Origin::Relay).is_err());
    db.assert_store_conforms().await;
}

/// The adapter's sources, embedded so the check runs without a database.
const SOURCES: [(&str, &str); 8] = [
    ("admin.rs", include_str!("../src/admin.rs")),
    ("error.rs", include_str!("../src/error.rs")),
    ("files.rs", include_str!("../src/files.rs")),
    ("hex.rs", include_str!("../src/hex.rs")),
    ("lib.rs", include_str!("../src/lib.rs")),
    ("session.rs", include_str!("../src/session.rs")),
    ("store.rs", include_str!("../src/store.rs")),
    (
        "bin/audit_admin.rs",
        include_str!("../src/bin/audit_admin.rs"),
    ),
];

/// The body of the function declared by `signature` in `source`, up to its
/// closing brace (at the indentation of the declaration).
fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} exists"));
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &source[line_start..start];
    let rest = &source[start..];
    let end = rest.find(&format!("\n{indent}}}\n")).expect("function end");
    &rest[..end]
}

/// IngestRow has public fields, so audit-core's two-way failure model relies
/// on adapters building it only from `audit_store.ingest`'s result columns
/// (README "失敗の分類"). Structurally: the crate builds exactly one
/// IngestRow, in `decode_ingest_row`, each field read from the same-named
/// column of the row, and decodes it exactly once, on the row of the ingest
/// query. No error path (SQLSTATE, transport, timeout) can reach a verdict;
/// the behaviour is tested in `adapter_maps_every_non_verdict_failure_to_an_outage`.
#[test]
fn ingest_rows_are_built_only_from_the_ingest_result_columns() {
    // Every source file of the crate is scanned.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut on_disk = BTreeSet::new();
    let mut pending = vec![dir.clone()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("src") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let relative = path.strip_prefix(&dir).expect("relative");
                on_disk.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let embedded: BTreeSet<String> = SOURCES.iter().map(|(n, _)| (*n).to_owned()).collect();
    assert_eq!(embedded, on_disk, "scan every source file");

    // Occurrences in code lines (comments may name the API).
    let count = |needle: &str| -> Vec<&str> {
        SOURCES
            .iter()
            .flat_map(|(name, text)| {
                text.lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .flat_map(move |line| line.matches(needle).map(move |_| *name))
            })
            .collect()
    };
    assert_eq!(count("IngestRow {"), ["store.rs"], "one IngestRow literal");
    assert_eq!(count("IngestRow::"), Vec::<&str>::new());
    assert_eq!(count(".into_result()"), ["store.rs"], "decoded once");
    assert_eq!(count("decode_ingest_row("), ["store.rs", "store.rs"]);

    let store = SOURCES[6].1;
    let decoder = function_body(store, "fn decode_ingest_row(row: &PgRow)");
    assert!(
        decoder.contains("IngestRow {"),
        "the literal is in the decoder"
    );
    let fields: Vec<&str> = decoder
        .lines()
        .map(str::trim)
        .filter(|line| line.contains(": row.try_get("))
        .collect();
    let columns = [
        "status",
        "seq",
        "envelope_digest",
        "adapter_version",
        "code",
    ];
    assert_eq!(fields.len(), columns.len(), "{fields:?}");
    for (line, column) in fields.iter().zip(columns) {
        assert!(
            line.starts_with(&format!("{column}: row.try_get(\"{column}\")")),
            "{line}"
        );
    }
    // The only caller decodes the row of the ingest query, after the local
    // precheck, and nothing else.
    let ingest = function_body(store, "async fn call_ingest(");
    assert!(ingest.contains("precheck_ingest(envelope)?;"));
    assert!(ingest.contains(
        "\"SELECT status, seq, envelope_digest, adapter_version, code \\\n                     FROM audit_store.ingest($1::text::jsonb)\""
    ));
    assert!(
        ingest
            .trim_end()
            .ends_with("decode_ingest_row(&row)?.into_result()")
    );
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
    let store = cast.relay.store().await;
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
    let found = store.lookup_receipts(&ids).await.expect("lookup");
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].envelope_digest, receipts[0].envelope_digest);
    assert_eq!(found[0].source_commitment, Some([0; 32]));
    assert_eq!(found[0].origin, Origin::Relay);
    assert_eq!(found[0].recovery_epoch, 1);
    assert!(!found[0].expired);
    let identity = ReceiptIdentity {
        seq: receipts[2].seq,
        event_id: ids[2],
        envelope_digest: receipts[2].envelope_digest,
    };
    assert!(identity.is_confirmed_by(&found));
    let listed = store
        .list_source_receipts(DOCUMENT_SOURCE, found[0].seq, 10)
        .await
        .expect("list");
    assert_eq!(
        listed.iter().map(|r| r.event_id).collect::<Vec<_>>(),
        ids[1..]
    );
    assert_eq!(head(&db.admin).await.0, before, "lookups record nothing");
    // Bounds are refused client-side (an outage, never a verdict).
    assert_eq!(
        store.lookup_receipts(&vec![Uuid::now_v7(); 1001]).await,
        Err(outage(OutageCode::Other))
    );
    assert_eq!(
        store.list_source_receipts(DOCUMENT_SOURCE, 0, 1001).await,
        Err(outage(OutageCode::Other))
    );

    // A replay or a repair run is an operator decision: the relay service
    // (ingest-capable) login cannot record one, so the privileged control
    // event always names the operator who acted (design §6.4, §10.1).
    let replay_control = RelayControl::from(RelayControlKind::ReplayRequested {
        event_id: ids[0],
        previous_code: audit_core::port::BoundedCode::new("delivery_unknown_at_limit")
            .expect("code"),
    });
    let repair_control = RelayControl::from(RelayControlKind::ReconciliationCompleted {
        run_id: Uuid::now_v7(),
        mode: audit_core::ReconcileMode::Repair,
        watermark: 3,
        id_set_digest: [0xab; 32],
        counts: audit_core::ReconcileCounts::default(),
    });
    for control in [&replay_control, &repair_control] {
        assert_eq!(
            store.record_relay_control(control).await,
            Err(outage(OutageCode::Denied))
        );
    }
    let denied = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(
        denied
            .iter()
            .filter(|(_, d)| d["operation"] == json!("record_relay_control")
                && d["denial_code"] == json!("insufficient_capability"))
            .count(),
        2
    );
    let operator = relay_operator(&db, &cast).await.store().await;
    let replay = operator
        .record_relay_control(&replay_control)
        .await
        .expect("replay recorded");
    let mismatch_control = RelayControl::from(RelayControlKind::SourceMismatchDetected {
        event_id: ids[1],
        code: SourceMismatchCode::SourceDigestMismatch,
    });
    let mismatch = store
        .record_relay_control(&mismatch_control)
        .await
        .expect("mismatch recorded");
    // Idempotent on (event_id, code): the retry returns the original receipt.
    assert_eq!(
        store.record_relay_control(&mismatch_control).await,
        Ok(mismatch)
    );
    let run = Uuid::now_v7();
    let reconciliation = store
        .record_relay_control(&RelayControl::from(
            RelayControlKind::ReconciliationCompleted {
                run_id: run,
                mode: audit_core::ReconcileMode::ReadOnly,
                watermark: 3,
                id_set_digest: [0xab; 32],
                counts: audit_core::ReconcileCounts {
                    ok: 3,
                    replay_record_lost: 1,
                    ..Default::default()
                },
            },
        ))
        .await
        .expect("reconciliation recorded");
    assert_eq!(replay.recovery_epoch, 1);
    let controls = store
        .lookup_control_receipts(&[
            replay.seq,
            mismatch.seq,
            reconciliation.seq,
            receipts[0].seq,
        ])
        .await
        .expect("control receipts");
    assert_eq!(
        controls
            .iter()
            .map(|c| (
                c.origin,
                c.event_type.as_str(),
                c.target_event_id,
                c.code.as_ref().map(audit_core::BoundedCode::as_str)
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                Origin::RelayControl,
                "audit.delivery.replay_requested",
                Some(ids[0]),
                Some("delivery_unknown_at_limit")
            ),
            (
                Origin::RelayControl,
                "audit.integrity.source_mismatch_detected",
                Some(ids[1]),
                Some("source_digest_mismatch")
            ),
            (
                Origin::RelayControl,
                "audit.reconciliation.completed",
                None,
                Some("read_only")
            ),
        ],
        "relay events are not control receipts"
    );
    let recorded = control_events(&db.admin, "audit.reconciliation.completed").await;
    assert_eq!(recorded[0].1["count_replay_record_lost"], json!(1));
    assert_eq!(recorded[0].1["session_role"], json!(cast.relay.role));

    // Invalid details and an unbound session are refused and recorded.
    for (event_type, details) in [
        (
            "audit.delivery.replay_requested",
            json!({"event_id": ids[0].to_string(), "quarantine_code": "Bad Code"}),
        ),
        (
            "audit.delivery.replay_requested",
            json!({"event_id": ids[0].to_string(), "quarantine_code": "conflict", "extra": 1}),
        ),
        ("audit.access.denied", json!({})),
    ] {
        let row = sqlx::query(
            "SELECT status, code FROM audit_store.record_relay_control($1, $2::text::jsonb)",
        )
        .bind(event_type)
        .bind(details.to_string())
        .fetch_one(&cast.relay.pool)
        .await
        .expect("call");
        assert_eq!(
            (
                row.get::<String, _>("status"),
                row.get::<Option<String>, _>("code")
            ),
            ("denied".to_owned(), Some("invalid_input".to_owned()))
        );
    }
    let unbound = db.login("operator", &["audit_store_relay_control"]).await;
    let unbound_store = unbound.store().await;
    assert_eq!(
        unbound_store.record_relay_control(&mismatch_control).await,
        Err(outage(OutageCode::Denied))
    );
    let denials = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(
        denials
            .iter()
            .map(|(_, d)| d["denial_code"].as_str().expect("code"))
            .collect::<Vec<_>>(),
        vec![
            "insufficient_capability",
            "insufficient_capability",
            "invalid_input",
            "invalid_input",
            "invalid_input",
            "unbound"
        ]
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn non_service_ingest_is_denied_coalesced_and_breaks_the_posture() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let relay = cast.relay.store().await;
    // A login holding the ingest role but bound to an ordinary principal.
    let intruder = db.login("intruder", &RELAY_ROLES).await;
    cast.dba
        .owner()
        .await
        .bind_principal(&intruder.role, ISSUER, "intruder-1")
        .await
        .expect("bind");
    let violations = cast
        .verifier
        .admin()
        .await
        .posture()
        .await
        .expect("posture");
    assert_eq!(
        violations
            .iter()
            .map(|v| (v.violation.as_str(), v.object.as_str()))
            .collect::<Vec<_>>(),
        vec![("ingest_member_not_source_service", intruder.role.as_str())]
    );
    let store = intruder.store().await;
    let envelope = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 1);
    for _ in 0..3 {
        assert_eq!(
            store.ingest(&envelope).await,
            Err(outage(OutageCode::Denied))
        );
    }
    assert_eq!(
        store.probe(&expectation(None)).await,
        Err(outage(OutageCode::Denied))
    );
    let identity = ReceiptIdentity {
        seq: 1,
        event_id: Uuid::now_v7(),
        envelope_digest: [0; 32],
    };
    assert_eq!(
        store.report_regression(&identity).await,
        Err(outage(OutageCode::Denied))
    );
    // One denial per streak and minute (the report_regression denial has a
    // different operation but the same streak).
    let denials = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(denials.len(), 1, "{denials:?}");
    assert_eq!(denials[0].1["operation"], json!("ingest"));
    assert_eq!(denials[0].1["denial_code"], json!("not_source_service"));
    assert_eq!(denials[0].1["session_role"], json!(intruder.role));
    assert_eq!(denials[0].1.get("suppressed_since_last"), None);
    let suppressed: i64 = sqlx::query_scalar(
        "SELECT suppressed FROM audit_store.denial_streaks WHERE session_role = $1",
    )
    .bind(&intruder.role)
    .fetch_one(&db.admin)
    .await
    .expect("streak");
    assert_eq!(suppressed, 4);
    // A minute later the streak is recorded again, carrying the count of
    // the denials coalesced since the previous record: nothing is sampled.
    let a_minute_later = "SET session_replication_role = replica; \
         UPDATE audit_store.denial_streaks SET last_recorded_at = now() - interval '2 minutes'; \
         RESET session_replication_role;";
    db.exec(a_minute_later).await;
    assert_eq!(
        store.ingest(&envelope).await,
        Err(outage(OutageCode::Denied))
    );
    let denials = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(denials.len(), 2);
    assert_eq!(denials[1].1["denial_code"], json!("not_source_service"));
    assert_eq!(denials[1].1["suppressed_since_last"], json!(4));
    // A change of denial code flushes the pending count first, as its own
    // record (that record stands for one of the coalesced denials).
    for _ in 0..2 {
        assert_eq!(
            store.ingest(&envelope).await,
            Err(outage(OutageCode::Denied))
        );
    }
    cast.dba
        .owner()
        .await
        .unbind_principal(&intruder.role)
        .await
        .expect("unbind");
    assert_eq!(
        store.ingest(&envelope).await,
        Err(outage(OutageCode::Denied))
    );
    let denials = control_events(&db.admin, "audit.access.denied").await;
    let tail: Vec<(Value, Option<Value>)> = denials[2..]
        .iter()
        .map(|(_, d)| {
            (
                d["denial_code"].clone(),
                d.get("suppressed_since_last").cloned(),
            )
        })
        .collect();
    assert_eq!(
        tail,
        vec![
            (json!("not_source_service"), Some(json!(1))),
            (json!("unbound"), None),
        ]
    );
    // Every denial is accounted for: records + their suppressed counts.
    let accounted: i64 = denials
        .iter()
        .filter(|(_, d)| d["session_role"] == json!(intruder.role))
        .map(|(_, d)| 1 + d["suppressed_since_last"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(accounted, 3 + 1 + 1 + 1 + 2 + 1);
    // Meanwhile the real relay is held by the posture violation.
    assert_eq!(
        relay.ingest(&envelope).await,
        Err(outage(OutageCode::PostureInvalid))
    );
    // An unbound ingest login is denied as unbound.
    db.exec(&format!("REVOKE audit_store_ingest FROM {}", intruder.role))
        .await;
    let unbound = db.login("unbound_ingest", &["audit_store_ingest"]).await;
    assert_eq!(
        unbound.store().await.ingest(&envelope).await,
        Err(outage(OutageCode::Denied))
    );
    let last = control_events(&db.admin, "audit.access.denied").await;
    assert_eq!(
        last.last().expect("denial").1["denial_code"],
        json!("unbound")
    );
    // The streak ends with a success: its pending count is flushed as a
    // record, never dropped with the operational state.
    for _ in 0..2 {
        assert_eq!(
            unbound.store().await.ingest(&envelope).await,
            Err(outage(OutageCode::Denied))
        );
    }
    cast.dba
        .owner()
        .await
        .bind_principal(&unbound.role, "service", "audit-relay")
        .await
        .expect("bind as the source service");
    let other = document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 2);
    assert_eq!(
        unbound
            .store()
            .await
            .ingest(&other)
            .await
            .expect("stored")
            .outcome,
        IngestOutcome::Stored
    );
    let flushed = control_events(&db.admin, "audit.access.denied").await;
    let flushed = &flushed.last().expect("flush").1;
    assert_eq!(
        (
            &flushed["session_role"],
            &flushed["denial_code"],
            &flushed["suppressed_since_last"]
        ),
        (&json!(unbound.role), &json!("unbound"), &json!(1))
    );
    let streaks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_store.denial_streaks WHERE session_role = $1",
    )
    .bind(&unbound.role)
    .fetch_one(&db.admin)
    .await
    .expect("streaks");
    assert_eq!(streaks, 0);
    db.exec(&format!("REVOKE audit_store_ingest FROM {}", unbound.role))
        .await;
    // Posture clean again: the relay ingests, nothing was stored before.
    assert_eq!(
        relay.ingest(&envelope).await.expect("stored").outcome,
        IngestOutcome::Stored
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn probe_reports_missing_types_and_identity_regression() {
    let db = TestDb::start().await;
    let (_relay, store) = relay_store(&db).await;
    let id = Uuid::now_v7();
    let receipt = store
        .ingest(&document_created(id, Uuid::now_v7(), OCCURRED, 1))
        .await
        .expect("stored");
    let acked = ReceiptIdentity {
        seq: receipt.seq,
        event_id: id,
        envelope_digest: receipt.envelope_digest,
    };
    let status = store.probe(&expectation(Some(acked))).await.expect("probe");
    assert!(!status.regression_detected);
    assert!(status.missing_types.is_empty());
    assert_eq!(status.admission(), Ok(()));
    // Identity, never seq: the same seq with another digest or id regresses.
    for other in [
        ReceiptIdentity {
            envelope_digest: [1; 32],
            ..acked
        },
        ReceiptIdentity {
            event_id: Uuid::now_v7(),
            ..acked
        },
        ReceiptIdentity {
            seq: receipt.seq + 100,
            ..acked
        },
    ] {
        let status = store.probe(&expectation(Some(other))).await.expect("probe");
        assert!(status.regression_detected, "{other:?}");
        assert_eq!(status.admission(), Err(OutageCode::Regressed));
    }
    // Store-side catalog skew.
    db.exec(
        "SET audit_store.write_context = 'migration'; \
         DELETE FROM audit_store.registered_types WHERE event_type = 'folder.moved'; \
         RESET audit_store.write_context;",
    )
    .await;
    let status = store.probe(&expectation(None)).await.expect("probe");
    assert_eq!(
        status
            .missing_types
            .iter()
            .map(audit_core::EventTypeName::as_str)
            .collect::<Vec<_>>(),
        ["folder.moved"]
    );
    assert_eq!(status.admission(), Err(OutageCode::UnregisteredType));
    // The probe records nothing.
    assert_eq!(head(&db.admin).await.0, receipt.seq);
}
