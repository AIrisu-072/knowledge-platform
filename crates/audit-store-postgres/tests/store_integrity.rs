//! In-database verification and tamper detection (design §8), offline
//! export and checkpoint verification (§8, §10.4), and retention (§9).

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use audit_core::{
    AuditStore, ChainVerdict, IngestOutcome, assess_recovery, verify_export, verify_identity_chain,
};
use audit_store_postgres::admin::{AccessOperation, parse_utc_text};
use audit_store_postgres::files::{ExportRequest, export_to_dir, write_checkpoint};
use audit_store_postgres::{AdminError, PostgresAuditStore};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};
use support::*;
use uuid::Uuid;

async fn relay_store(cast: &Cast) -> PostgresAuditStore {
    cast.relay.store().await
}

async fn populated(count: u8) -> (TestDb, Cast, Vec<i64>) {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = relay_store(&cast).await;
    let mut seqs = Vec::new();
    for n in 0..count {
        let receipt = store
            .ingest(&document_created(
                Uuid::now_v7(),
                Uuid::now_v7(),
                OCCURRED,
                n,
            ))
            .await
            .expect("stored");
        seqs.push(receipt.seq);
    }
    (db, cast, seqs)
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("audit-store-{name}-{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

fn count(violations: &Value, code: &str) -> i64 {
    violations[code]
        .as_i64()
        .unwrap_or_else(|| panic!("{code} missing"))
}

#[tokio::test]
async fn verify_records_once_and_never_recurses() {
    let (db, cast, _) = populated(4).await;
    let verifier = cast.verifier.admin().await;
    let first = verifier.verify(None, None).await.expect("verify");
    assert_eq!(first.outcome, "ok");
    assert_eq!(first.seq, first.to_seq + 1, "recorded after the watermark");
    assert_eq!(first.checked, first.to_seq);
    let second = verifier.verify(None, None).await.expect("verify");
    assert_eq!(
        second.to_seq, first.seq,
        "the previous record is verified next time"
    );
    assert_eq!(second.seq, second.to_seq + 1);
    let partial = verifier.verify(Some(3), Some(5)).await.expect("partial");
    assert_eq!((partial.checked, partial.outcome.as_str()), (3, "ok"));
    let recorded = control_events(&db.admin, "audit.integrity.verified").await;
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].1["trigger"], json!("verify"));
    assert_eq!(recorded[0].1["violations_total"], json!(0));
    // The status is the verification coverage (contiguous from genesis),
    // not the newest record: the partial range does not extend it.
    let status = verifier.store_status().await.expect("status");
    assert_eq!(status.last_verified_seq, Some(second.to_seq));
    assert_eq!(status.last_verified_outcome.as_deref(), Some("ok"));
    assert!(!status.recovery_mode && status.posture_ok);

    // The checkpoint is the chain position of its own verified record.
    let checkpoint = verifier.checkpoint().await.expect("checkpoint");
    assert_eq!(checkpoint.outcome, "ok");
    assert_eq!(checkpoint.verified_through + 1, checkpoint.seq);
    assert_eq!(checkpoint.epoch, 1);
    let (last_seq, last_chain, _) = head(&db.admin).await;
    assert_eq!(
        (last_seq, hex(&last_chain)),
        (checkpoint.seq, checkpoint.chain.clone()),
        "nothing follows the checkpoint record"
    );
    let recorded = control_events(&db.admin, "audit.integrity.verified").await;
    let (seq, details) = recorded.last().expect("checkpoint record");
    assert_eq!(*seq, checkpoint.seq);
    assert_eq!(details["trigger"], json!("checkpoint"));
    assert_eq!(details["to_seq"], json!(checkpoint.verified_through));
    assert_eq!(details["watermark"], json!(checkpoint.verified_through));
    // A span over the bound is refused.
    assert_eq!(
        verifier.verify(Some(1), Some(10_000_001)).await,
        Err(AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn verify_refuses_ranges_beyond_the_head() {
    let (db, cast, _) = populated(3).await;
    let verifier = cast.verifier.admin().await;
    // Each refusal is recorded (audit.access.denied), so the head moves:
    // every range is taken relative to the head before that call.
    type Range = fn(i64) -> (Option<i64>, Option<i64>);
    let cases: [(&str, Range); 4] = [
        ("to past the head", |w| (Some(1), Some(w + 5))),
        ("to just past the head", |w| (None, Some(w + 1))),
        ("empty tail", |w| (Some(w + 1), None)),
        ("from past the head", |w| (Some(w + 2), Some(w + 3))),
    ];
    for (name, range) in cases {
        let (w, _, _) = head(&db.admin).await;
        let (from, to) = range(w);
        assert_eq!(
            verifier.verify(from, to).await,
            Err(AdminError::Denied {
                code: "invalid_input".into()
            }),
            "{name}"
        );
    }
    assert!(
        control_events(&db.admin, "audit.integrity.verified")
            .await
            .is_empty(),
        "no verification result was recorded for a range past the head"
    );
    let status = verifier.store_status().await.expect("status");
    assert_eq!(
        (status.last_verified_seq, status.last_verified_outcome),
        (None, None)
    );
    // A range ending exactly at the head is verified and recorded.
    let (w, _, _) = head(&db.admin).await;
    let report = verifier.verify(Some(1), Some(w)).await.expect("verify");
    assert_eq!(
        (report.outcome.as_str(), report.to_seq, report.checked),
        ("ok", w, w)
    );
    let recorded = control_events(&db.admin, "audit.integrity.verified").await;
    assert_eq!(recorded[0].1["head_seq"], json!(w));
    assert_eq!(recorded[0].1["to_seq"], json!(w));
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn verification_status_is_coverage_and_a_violation_stays_until_covered() {
    let (db, cast, seqs) = populated(6).await;
    let verifier = cast.verifier.admin().await;
    let relay = relay_store(&cast).await;
    let full = verifier.verify(None, None).await.expect("verify");
    let status = verifier.store_status().await.expect("status");
    assert_eq!(
        (
            status.last_verified_seq,
            status.last_verified_outcome.as_deref()
        ),
        (Some(full.to_seq), Some("ok"))
    );
    let probed = relay.probe(&expectation(None)).await.expect("probe");
    assert_eq!(probed.last_verified_seq, Some(full.to_seq));

    // A forged body is found by a full verification.
    let target = seqs[3];
    let original: String =
        sqlx::query_scalar("SELECT envelope::text FROM audit_store.event_bodies WHERE seq = $1")
            .bind(target)
            .fetch_one(&db.admin)
            .await
            .expect("body");
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.event_bodies \
            SET envelope = jsonb_set(envelope, '{{subject}}', '\"document/forged\"') \
            WHERE seq = {target}; \
         RESET session_replication_role;"
    ))
    .await;
    let failed = verifier.verify(None, None).await.expect("verify");
    assert_eq!(failed.outcome, "violations");

    // Neither a partial range before the forged row nor one after it
    // clears the alarm or claims coverage past what was verified since.
    let early = verifier.verify(Some(1), Some(2)).await.expect("early");
    assert_eq!(early.outcome, "ok");
    let late = verifier.verify(Some(target + 1), None).await.expect("late");
    assert_eq!(late.outcome, "ok");
    let status = verifier.store_status().await.expect("status");
    assert_eq!(status.last_verified_outcome.as_deref(), Some("violations"));
    assert_eq!(
        status.last_verified_seq,
        Some(2),
        "covered from genesis since the violation"
    );
    let probed = relay.probe(&expectation(None)).await.expect("probe");
    assert_eq!(probed.last_verified_seq, Some(2));

    // Repaired, a full verification covers the violated range again.
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.event_bodies SET envelope = '{}'::jsonb WHERE seq = {target}; \
         RESET session_replication_role;",
        original.replace('\'', "''")
    ))
    .await;
    // Re-verifying the violated range alone (short of the head) does not
    // clear it: only one verification from genesis to its head does, even
    // when partial ranges chain up to the head between them.
    let short = verifier
        .verify(Some(1), Some(failed.to_seq))
        .await
        .expect("short");
    assert_eq!(short.outcome, "ok");
    let status = verifier.store_status().await.expect("status");
    assert_eq!(
        (
            status.last_verified_seq,
            status.last_verified_outcome.as_deref()
        ),
        (Some(late.to_seq), Some("violations"))
    );
    let repaired = verifier.verify(None, None).await.expect("verify");
    assert_eq!(repaired.outcome, "ok");
    let status = verifier.store_status().await.expect("status");
    assert_eq!(
        (
            status.last_verified_seq,
            status.last_verified_outcome.as_deref()
        ),
        (Some(repaired.to_seq), Some("ok"))
    );
    assert_eq!(
        status.head_seq - repaired.to_seq,
        1,
        "only the verification record itself is unverified"
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn verify_reads_the_watermark_without_the_head_lock() {
    let (db, cast, _) = populated(3).await;
    let (w, _, _) = head(&db.admin).await;
    // A relay ingest holds the head lock with seq w+1 uncommitted.
    let mut holder = PgConnection::connect(&cast.relay.url)
        .await
        .expect("connect");
    sqlx::query("BEGIN")
        .execute(&mut holder)
        .await
        .expect("begin");
    let held: i64 = sqlx::query_scalar("SELECT seq FROM audit_store.ingest($1::text::jsonb)")
        .bind(document_created(Uuid::now_v7(), Uuid::now_v7(), OCCURRED, 9).to_json_string())
        .fetch_one(&mut holder)
        .await
        .expect("held ingest");
    assert_eq!(held, w + 1);
    // verify reads W and scans without the lock; only its append waits.
    let verifier = cast.verifier.admin().await;
    let running = tokio::spawn(async move { verifier.verify(None, None).await });
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!running.is_finished(), "the append waits for the head lock");
    let waiting: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity \
         WHERE wait_event_type = 'Lock' AND query LIKE '%audit_store.verify%'",
    )
    .fetch_one(&db.admin)
    .await
    .expect("activity");
    assert_eq!(waiting, 1);
    sqlx::query("COMMIT")
        .execute(&mut holder)
        .await
        .expect("commit");
    let report = running.await.expect("join").expect("verify");
    assert_eq!(
        (report.watermark, report.to_seq, report.checked),
        (w, w, w),
        "W was read before the held ingest committed"
    );
    assert_eq!(report.seq, w + 2, "the result follows the ingest");
    assert_eq!(report.outcome, "ok");
    db.assert_store_conforms().await;
}

/// Applies `sql` as the superuser with triggers and FKs bypassed and
/// returns the violations reported by verify.
async fn tampered(sql: &str) -> Value {
    let (db, cast, seqs) = populated(4).await;
    let sql = sql
        .replace("$A", &seqs[1].to_string())
        .replace("$B", &seqs[2].to_string());
    db.exec(&format!(
        "SET session_replication_role = replica; {sql}; RESET session_replication_role;"
    ))
    .await;
    let report = cast
        .verifier
        .admin()
        .await
        .verify(None, None)
        .await
        .expect("verify");
    assert_eq!(report.outcome, "violations", "{sql}");
    let recorded = control_events(&db.admin, "audit.integrity.verified").await;
    assert_eq!(
        recorded.last().expect("recorded").1["outcome"],
        json!("violations")
    );
    report.violations
}

#[tokio::test]
async fn modified_body_is_detected() {
    let v = tampered(
        "UPDATE audit_store.event_bodies \
         SET envelope = jsonb_set(envelope, '{subject}', '\"document/forged\"') WHERE seq = $A",
    )
    .await;
    assert_eq!(count(&v, "body_digest_mismatch"), 1);
    assert_eq!(count(&v, "identity_mismatch"), 1);
}

#[tokio::test]
async fn deleted_row_is_detected() {
    let v = tampered(
        "DELETE FROM audit_store.event_bodies WHERE seq = $A; \
         DELETE FROM audit_store.events WHERE seq = $A",
    )
    .await;
    assert_eq!(count(&v, "seq_gap"), 1);
    assert_eq!(count(&v, "prev_chain_mismatch"), 1);
}

/// A range whose last row is missing is a gap; the recorded head is the
/// last row that exists, never a chain the range does not have.
#[tokio::test]
async fn a_missing_range_end_reports_the_last_existing_row() {
    let (db, cast, seqs) = populated(4).await;
    let end = seqs[2];
    let kept: Vec<u8> = sqlx::query_scalar("SELECT chain FROM audit_store.events WHERE seq = $1")
        .bind(end - 1)
        .fetch_one(&db.admin)
        .await
        .expect("chain");
    db.exec(&format!(
        "SET session_replication_role = replica; \
         DELETE FROM audit_store.event_bodies WHERE seq = {end}; \
         DELETE FROM audit_store.events WHERE seq = {end}; \
         RESET session_replication_role;"
    ))
    .await;
    let report = cast
        .verifier
        .admin()
        .await
        .verify(Some(1), Some(end))
        .await
        .expect("verify");
    assert_eq!(report.outcome, "violations");
    assert_eq!(count(&report.violations, "seq_gap"), 1);
    assert_eq!(report.head_chain, hex(&kept));
    let recorded = control_events(&db.admin, "audit.integrity.verified").await;
    let details = &recorded.last().expect("recorded").1;
    assert_eq!(
        (
            &details["to_seq"],
            &details["head_seq"],
            &details["head_chain"]
        ),
        (&json!(end), &json!(end - 1), &json!(hex(&kept)))
    );
}

#[tokio::test]
async fn reordered_rows_are_detected() {
    let v = tampered(
        "UPDATE audit_store.event_bodies SET seq = 1000000 WHERE seq = $A; \
         UPDATE audit_store.events SET seq = 1000000 WHERE seq = $A; \
         UPDATE audit_store.event_bodies SET seq = $A WHERE seq = $B; \
         UPDATE audit_store.events SET seq = $A WHERE seq = $B; \
         UPDATE audit_store.event_bodies SET seq = $B WHERE seq = 1000000; \
         UPDATE audit_store.events SET seq = $B WHERE seq = 1000000",
    )
    .await;
    assert!(count(&v, "prev_chain_mismatch") >= 2, "{v}");
    assert!(count(&v, "chain_mismatch") >= 2, "{v}");
}

#[tokio::test]
async fn head_and_expiry_state_tampering_are_detected() {
    let v = tampered(
        "UPDATE audit_store.publication_head SET last_chain = sha256('x'::bytea); \
         DELETE FROM audit_store.event_bodies WHERE seq = $B",
    )
    .await;
    assert_eq!(count(&v, "head_mismatch"), 1);
    assert_eq!(count(&v, "expiry_state_mismatch"), 1);
}

#[tokio::test]
async fn full_rewrite_passes_in_database_but_fails_offline() {
    let (db, cast, seqs) = populated(4).await;
    let verifier = cast.verifier.admin().await;
    let dir = scratch_dir("rewrite");
    let record = verifier.checkpoint().await.expect("checkpoint");
    let file = write_checkpoint(&record, &dir.join("checkpoint.json")).expect("written");
    assert_eq!(mode(&dir.join("checkpoint.json")), 0o600);
    let checkpoint = file.checkpoint().expect("checkpoint");

    let request = ExportRequest {
        operation: AccessOperation::IdentityChain,
        filter: json!({}),
        page_size: 1000,
        max_pages: 1,
        checkpoint: Some(checkpoint),
    };
    let honest_dir = dir.join("honest");
    std::fs::create_dir(&honest_dir).expect("dir");
    let honest = export_to_dir(&verifier, &request, &honest_dir)
        .await
        .expect("export");
    assert!(honest.manifest.anchored && honest.manifest.complete);
    let text = std::fs::read_to_string(&honest.export_path).expect("identity chain");
    let offline = verify_identity_chain(&text, audit_core::Anchor::Genesis).expect("offline");
    assert_eq!(offline.head, honest.report.head);
    assert_eq!(offline.bodies, 0, "the identity chain carries no bodies");
    // The export reaches the first intent's watermark: the checkpoint head.
    assert_eq!(
        honest
            .manifest
            .checkpoint
            .as_ref()
            .expect("compared")
            .comparison,
        "match"
    );
    assert_eq!(
        assess_recovery(&honest.report, &[checkpoint], &[]).verdict,
        ChainVerdict::Authentic
    );

    // The owner rewrites a body and recomputes every digest and chain.
    let target = seqs[1];
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.event_bodies \
            SET envelope = jsonb_set(envelope, '{{subject}}', '\"document/rewritten\"') \
            WHERE seq = {target}; \
         UPDATE audit_store.events AS e SET subject = 'document/rewritten', \
            envelope_digest = audit_store.jsonb_digest(b.envelope) \
            FROM audit_store.event_bodies AS b WHERE b.seq = e.seq AND e.seq = {target}; \
         DO $$ DECLARE r record; prev bytea; BEGIN \
            SELECT chain INTO prev FROM audit_store.events WHERE seq = {target} - 1; \
            FOR r IN SELECT seq, event_id, envelope_digest FROM audit_store.events \
                     WHERE seq >= {target} ORDER BY seq LOOP \
                UPDATE audit_store.events SET prev_chain = prev, \
                    chain = audit_store.chain_step(prev, r.seq, r.event_id, r.envelope_digest) \
                    WHERE seq = r.seq RETURNING chain INTO prev; \
            END LOOP; \
            UPDATE audit_store.publication_head SET last_chain = prev; \
         END $$; \
         RESET session_replication_role;"
    ))
    .await;
    // In-database verification cannot see a consistent rewrite (design §8).
    assert_eq!(
        verifier.verify(None, None).await.expect("verify").outcome,
        "ok"
    );
    let rewritten_dir = dir.join("rewritten");
    std::fs::create_dir(&rewritten_dir).expect("dir");
    let rewritten = export_to_dir(&verifier, &request, &rewritten_dir)
        .await
        .expect("export is internally consistent");
    assert_eq!(
        rewritten
            .manifest
            .checkpoint
            .as_ref()
            .expect("compared")
            .comparison,
        "mismatch"
    );
    assert_eq!(
        assess_recovery(&rewritten.report, &[checkpoint], &[]).verdict,
        ChainVerdict::Tampered
    );
}

#[tokio::test]
async fn full_export_verifies_offline_and_filtered_exports_are_unanchored() {
    let (db, cast, _) = populated(5).await;
    let verifier = cast.verifier.admin().await;
    let dir = scratch_dir("export");
    let record = verifier.checkpoint().await.expect("checkpoint");
    let checkpoint = record.checkpoint().expect("checkpoint");
    let request = ExportRequest {
        operation: AccessOperation::Verify,
        filter: json!({}),
        page_size: 7,
        max_pages: 10,
        checkpoint: Some(checkpoint),
    };
    let full = export_to_dir(&verifier, &request, &dir)
        .await
        .expect("full export");
    assert!(full.report.anchored);
    assert_eq!(full.report.rows, full.report.bodies);
    let manifest = &full.manifest;
    assert!(manifest.complete && manifest.epochs_authenticated);
    assert_eq!(manifest.first_seq, Some(1));
    assert_eq!(manifest.last_seq, manifest.watermark);
    assert_eq!(
        manifest.rows,
        u64::try_from(manifest.watermark.expect("w")).expect("u64")
    );
    assert_eq!(manifest.intents.len(), 1);
    assert_eq!(
        manifest.intents[0].page_digests.len(),
        manifest.rows.div_ceil(7) as usize
    );
    assert_eq!(manifest.genesis, hex(&audit_core::GENESIS));
    assert_eq!(
        manifest.checkpoint.as_ref().expect("compared").comparison,
        "match"
    );
    assert_eq!(mode(&full.export_path), 0o600);
    assert_eq!(mode(&full.manifest_path), 0o600);
    let text = std::fs::read_to_string(&full.export_path).expect("export");
    assert!(audit_core::verify_export(&text, audit_core::Anchor::Genesis).is_ok());
    // The same files are never overwritten.
    assert!(export_to_dir(&verifier, &request, &dir).await.is_err());

    // The reader sees no control events: an unanchored subset.
    let reader_dir = scratch_dir("subset");
    let subset = export_to_dir(
        &cast.reader.admin().await,
        &ExportRequest {
            operation: AccessOperation::Export,
            filter: json!({}),
            page_size: 100,
            max_pages: 1,
            checkpoint: None,
        },
        &reader_dir,
    )
    .await
    .expect("subset");
    assert!(!subset.manifest.anchored);
    assert_eq!(subset.report.rows, 5);
    // Three closes: the full export, the refused rewrite (closed before the
    // file write failed) and the reader's subset.
    let closed = control_events(&db.admin, "audit.access.closed").await;
    assert_eq!(closed.len(), 3);
    assert_eq!(closed[2].1["returned_count"], json!(5));
    db.assert_store_conforms().await;
}

async fn now_minus(db: &TestDb, interval: &str) -> String {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT to_char((now() - interval '{interval}') AT TIME ZONE 'UTC', \
                'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS t"
    )))
    .fetch_one(&db.admin)
    .await
    .expect("time")
    .get("t")
}

fn set_digest(seqs: &[i64]) -> String {
    hex(&audit_core::expired_set_digest(seqs))
}

#[tokio::test]
async fn retention_follows_policy_revisions_cutoffs_holds_and_keeps_tombstones() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = relay_store(&cast).await;
    let recent = now_minus(&db, "1 day").await;
    let mut ids = Vec::new();
    for (occurred, kind) in [
        ("2020-01-01T00:00:00.000000Z", "created"),
        ("2020-01-02T00:00:00.000000Z", "read"),
        ("2025-01-01T00:00:00.000000Z", "created"),
        (recent.as_str(), "created"),
        (recent.as_str(), "read"),
        (recent.as_str(), "created"),
        (recent.as_str(), "created"),
        (recent.as_str(), "created"),
    ] {
        let id = Uuid::now_v7();
        let envelope = if kind == "created" {
            document_created(id, Uuid::now_v7(), occurred, 9)
        } else {
            read_confirmed(id, Uuid::now_v7(), occurred, 9)
        };
        let receipt = store.ingest(&envelope).await.expect("stored");
        ids.push((id, receipt.seq, envelope));
    }
    let admin = cast.admin.admin().await;
    let maintainer = cast.maintainer.admin().await;
    let selector = json!({"event_types": ["document.version.read_confirmed", "document.created"]});
    let far = parse_utc_text("2100-01-01T00:00:00.000000Z").expect("far");

    // retain_days NULL: never expirable.
    let rev1 = admin
        .set_retention_policy("documents", &selector, None)
        .await
        .expect("policy");
    assert_eq!(rev1.revision, 1);
    let outcome = maintainer
        .expire("documents", 1, far, 100)
        .await
        .expect("attempt");
    assert_eq!(
        (outcome.status.as_str(), outcome.expired_count),
        ("not_expirable", 0)
    );

    let rev2 = admin
        .set_retention_policy("documents", &selector, Some(30))
        .await
        .expect("policy");
    assert_eq!(rev2.revision, 2);
    // A stale expected revision deletes nothing.
    let outcome = maintainer
        .expire("documents", 1, far, 100)
        .await
        .expect("attempt");
    assert_eq!(
        (outcome.status.as_str(), outcome.expired_count),
        ("stale_revision", 0)
    );
    // An active legal hold deletes nothing.
    db.exec("INSERT INTO audit_store.legal_holds (hold_id) VALUES (gen_random_uuid())")
        .await;
    let outcome = maintainer
        .expire("documents", 2, far, 100)
        .await
        .expect("attempt");
    assert_eq!(
        (outcome.status.as_str(), outcome.expired_count),
        ("held", 0)
    );
    db.exec(
        "SET session_replication_role = replica; DELETE FROM audit_store.legal_holds; \
         RESET session_replication_role;",
    )
    .await;
    // Each refusal is recorded as audit.retention.expire_refused.
    let refusals = control_events(&db.admin, "audit.retention.expire_refused").await;
    assert_eq!(
        refusals
            .iter()
            .map(|(_, d)| (
                d["refusal"].as_str().expect("refusal").to_owned(),
                d["expected_revision"].clone(),
                d["current_revision"].clone(),
                d["retain_days"].clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("not_expirable".to_owned(), json!(1), json!(1), Value::Null),
            ("stale_revision".to_owned(), json!(1), json!(2), json!(30)),
            ("held".to_owned(), json!(2), json!(2), json!(30)),
        ]
    );
    assert!(
        control_events(&db.admin, "audit.retention.expired")
            .await
            .is_empty(),
        "nothing expired yet"
    );
    // Cutoffs outside what the control timestamp kind can represent
    // (years 0001–9999 AD) are refused before anything is recorded.
    for cutoff in ["10000-01-01 00:00:00+00", "0044-03-15 00:00:00+00 BC"] {
        let row = sqlx::query(
            "SELECT status, code FROM audit_store.expire('documents', 2, $1::text::timestamptz, 10)",
        )
        .bind(cutoff)
        .fetch_one(maintainer.pool())
        .await
        .expect("expire");
        assert_eq!(
            (
                row.get::<String, _>("status"),
                row.get::<Option<String>, _>("code")
            ),
            ("denied".to_owned(), Some("invalid_input".to_owned())),
            "{cutoff}"
        );
    }
    assert!(
        control_events(&db.admin, "audit.retention.expire_refused")
            .await
            .len()
            == 3
            && control_events(&db.admin, "audit.retention.expired")
                .await
                .is_empty()
    );

    // The effective cutoff is the earlier of the request and now - retain_days.
    let cutoff = parse_utc_text("2024-01-01T00:00:00.000000Z").expect("cutoff");
    let first = maintainer
        .expire("documents", 2, cutoff, 100)
        .await
        .expect("expire");
    assert_eq!(first.status, "expired");
    assert_eq!(first.expired_count, 2);
    assert_eq!(
        first.effective_cutoff.as_deref(),
        Some("2024-01-01T00:00:00.000000Z")
    );
    let second = maintainer
        .expire("documents", 2, far, 100)
        .await
        .expect("expire");
    assert_eq!(
        second.expired_count, 1,
        "only the 2025 event is older than 30 days"
    );
    let expired_details = control_events(&db.admin, "audit.retention.expired").await;
    let evidence = &expired_details
        .iter()
        .find(|(s, _)| *s == first.seq)
        .expect("evidence")
        .1;
    assert_eq!(evidence["count"], json!(2));
    assert_eq!(
        evidence["expired_set_digest"],
        json!(set_digest(&[ids[0].1, ids[1].1]))
    );
    assert_eq!(evidence["retain_days"], json!(30));
    assert_eq!(evidence["revision"], json!(2));
    assert_eq!(evidence["cutoff"], json!("2024-01-01T00:00:00.000000Z"));
    assert_eq!(
        evidence["effective_cutoff"],
        json!("2024-01-01T00:00:00.000000Z")
    );
    assert_eq!(evidence["first_seq"], json!(ids[0].1));
    assert_eq!(evidence["last_seq"], json!(ids[1].1));
    assert_eq!(
        evidence["selector_event_types"],
        json!(["document.created", "document.version.read_confirmed"])
    );
    // An empty run records count 0, null bounds and the prefix-only digest.
    let empty = maintainer
        .expire("documents", 2, cutoff, 100)
        .await
        .expect("expire");
    assert_eq!(empty.expired_count, 0);
    let empty_details = &control_events(&db.admin, "audit.retention.expired")
        .await
        .into_iter()
        .find(|(s, _)| *s == empty.seq)
        .expect("evidence")
        .1;
    assert_eq!(
        (
            empty_details["first_seq"].clone(),
            empty_details["last_seq"].clone()
        ),
        (Value::Null, Value::Null)
    );
    assert_eq!(empty_details["expired_set_digest"], json!(set_digest(&[])));

    // Control events never expire, even when a selector matches their class.
    let classes = admin
        .set_retention_policy(
            "data_access",
            &json!({"event_classes": ["DATA_ACCESS"]}),
            Some(1),
        )
        .await
        .expect("policy");
    let outcome = maintainer
        .expire("data_access", classes.revision, far, 100)
        .await
        .expect("expire");
    assert_eq!(outcome.expired_count, 1, "only the recent relay read event");
    let expired_controls: i64 = sqlx::query(
        "SELECT count(*) AS n FROM audit_store.events \
         WHERE origin <> 'relay' AND expired_at IS NOT NULL",
    )
    .fetch_one(&db.admin)
    .await
    .expect("count")
    .get("n");
    assert_eq!(expired_controls, 0);
    let error = sqlx::raw_sql(
        "SET audit_store.maintenance_context = 'retention'; \
         UPDATE audit_store.events SET expired_at = now(), \
             expired_by_seq = (SELECT last_seq FROM audit_store.publication_head) \
         WHERE seq = 1",
    )
    .execute(&db.admin)
    .await
    .expect_err("control events are not expirable");
    assert_eq!(sqlstate(&error), "23514");
    db.exec("RESET audit_store.maintenance_context").await;

    // Tombstones keep the identity columns; the body is gone.
    let columns: Vec<String> = sqlx::query(
        "SELECT column_name::text AS c FROM information_schema.columns \
         WHERE table_schema = 'audit_store' AND table_name = 'events' ORDER BY ordinal_position",
    )
    .fetch_all(&db.admin)
    .await
    .expect("columns")
    .iter()
    .map(|r| r.get("c"))
    .collect();
    assert_eq!(
        columns,
        [
            "seq",
            "event_id",
            "origin",
            "source",
            "event_type",
            "event_class",
            "subject",
            "occurred_at",
            "stored_at",
            "actor_issuer",
            "actor_principal_id",
            "resource_type",
            "resource_id",
            "resource_version_id",
            "result",
            "envelope_digest",
            "digest_algorithm",
            "source_commitment",
            "adapter_version",
            "prev_chain",
            "chain",
            "recovery_epoch",
            "expired_at",
            "expired_by_seq",
            "ingested_by_db_role"
        ]
    );
    let tombstone = sqlx::query(
        "SELECT to_jsonb(e) AS row, b.seq IS NULL AS bodyless FROM audit_store.events AS e \
         LEFT JOIN audit_store.event_bodies AS b ON b.seq = e.seq WHERE e.seq = $1",
    )
    .bind(ids[0].1)
    .fetch_one(&db.admin)
    .await
    .expect("tombstone");
    assert!(tombstone.get::<bool, _>("bodyless"));
    let row: Value = tombstone.get("row");
    let null_columns: Vec<&str> = row
        .as_object()
        .expect("row")
        .iter()
        .filter(|(_, v)| v.is_null())
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(null_columns, vec!["resource_version_id"]);
    assert_eq!(row["expired_by_seq"], json!(first.seq));

    // Re-delivery of an expired event does not revive the body.
    let again = store.ingest(&ids[0].2).await.expect("duplicate");
    assert_eq!(again.outcome, IngestOutcome::DuplicateExpired);
    assert_eq!(again.seq, ids[0].1);

    // Purge of one body for a minimization failure.
    let purged = maintainer
        .purge_body(ids[3].0, "minimization_failure")
        .await
        .expect("purged");
    assert_eq!(
        maintainer
            .purge_body(ids[3].0, "minimization_failure")
            .await,
        Err(AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    let control_id: Uuid = sqlx::query("SELECT event_id FROM audit_store.events WHERE seq = 1")
        .fetch_one(&db.admin)
        .await
        .expect("control")
        .get("event_id");
    assert_eq!(
        maintainer.purge_body(control_id, "adapter_defect").await,
        Err(AdminError::Denied {
            code: "invalid_input".into()
        })
    );
    let verifier = cast.verifier.admin().await;
    let report = verifier.verify(None, None).await.expect("verify");
    assert_eq!(report.outcome, "ok", "{}", report.violations);

    // Offline: every expiry is attested by the chained evidence in the
    // export (10-key lines with expired_by_seq), nothing unverified.
    let dir = scratch_dir("expiry");
    let export = export_to_dir(
        &verifier,
        &ExportRequest {
            operation: AccessOperation::Verify,
            filter: json!({}),
            page_size: 1000,
            max_pages: 1,
            checkpoint: None,
        },
        &dir,
    )
    .await
    .expect("export with expiry evidence");
    assert!(export.manifest.complete);
    assert_eq!(export.report.expired, 5);
    assert_eq!(export.report.unverified_expiry_evidence, 0);
    let text = std::fs::read_to_string(&export.export_path).expect("export");
    let offline = verify_export(&text, audit_core::Anchor::Genesis).expect("offline");
    assert_eq!(offline.expired, 5);
    let tombstone_line = text
        .lines()
        .find(|l| l.starts_with(&format!("{{\"seq\":{},", ids[0].1)))
        .expect("tombstone line");
    assert!(
        tombstone_line.contains(&format!(
            "\"expired\":true,\"expired_by_seq\":{},\"envelope\":null}}",
            first.seq
        )),
        "{tombstone_line}"
    );

    // Forged marks: an extra row under a retention record, an extra row
    // under a purge record, and a mark pointing at a non-retention event.
    let intent_seq = cast
        .reader
        .admin()
        .await
        .open_access(AccessOperation::Export, &json!({}), 1, 1)
        .await
        .expect("intent")
        .intent_seq;
    db.exec(&format!(
        "SET session_replication_role = replica; \
         UPDATE audit_store.events SET expired_at = now(), expired_by_seq = {} WHERE seq = {}; \
         DELETE FROM audit_store.event_bodies WHERE seq = {}; \
         UPDATE audit_store.events SET expired_at = now(), expired_by_seq = {} WHERE seq = {}; \
         DELETE FROM audit_store.event_bodies WHERE seq = {}; \
         UPDATE audit_store.events SET expired_at = now(), expired_by_seq = {} WHERE seq = {}; \
         DELETE FROM audit_store.event_bodies WHERE seq = {}; \
         RESET session_replication_role;",
        first.seq,
        ids[5].1,
        ids[5].1,
        purged.seq.expect("purge seq"),
        ids[6].1,
        ids[6].1,
        intent_seq,
        ids[7].1,
        ids[7].1,
    ))
    .await;
    let forged = verifier.verify(None, None).await.expect("verify");
    assert_eq!(forged.outcome, "violations");
    assert_eq!(count(&forged.violations, "retention_evidence_mismatch"), 1);
    assert_eq!(count(&forged.violations, "purge_evidence_mismatch"), 1);
    assert_eq!(count(&forged.violations, "retention_evidence_missing"), 1);
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn chain_exports_loop_over_intents_up_to_the_first_watermark() {
    let (db, cast, _) = populated(6).await;
    let verifier = cast.verifier.admin().await;
    let record = verifier.checkpoint().await.expect("checkpoint");
    let checkpoint = record.checkpoint().expect("checkpoint");
    let target = checkpoint.seq;
    for (operation, name) in [
        (AccessOperation::IdentityChain, "identity"),
        (AccessOperation::Verify, "bodies"),
    ] {
        let dir = scratch_dir(&format!("intents-{name}"));
        let export = export_to_dir(
            &verifier,
            &ExportRequest {
                operation,
                filter: json!({}),
                page_size: 2,
                max_pages: 2,
                checkpoint: Some(checkpoint),
            },
            &dir,
        )
        .await
        .expect("export");
        let manifest = &export.manifest;
        assert!(manifest.anchored && manifest.complete, "{name}");
        // The first intent fixes the watermark; later intents reach it in
        // contiguous (seq_after, seq_through] ranges of at most 4 rows.
        let first = &manifest.intents[0];
        assert_eq!(first.seq_after, 0);
        assert_eq!(first.seq_through, None);
        let watermark = manifest.watermark.expect("watermark");
        assert_eq!(watermark, first.watermark);
        assert!(manifest.intents.len() >= 4, "{}", manifest.intents.len());
        let mut after = 0;
        for intent in &manifest.intents {
            assert_eq!(intent.seq_after, after);
            assert!(intent.rows <= 4);
            if intent.intent_seq != first.intent_seq {
                assert_eq!(intent.seq_through, Some(watermark));
            }
            after += i64::try_from(intent.rows).expect("rows");
        }
        assert_eq!(after, watermark);
        assert_eq!(manifest.rows, u64::try_from(watermark).expect("rows"));
        assert_eq!(export.report.head.seq, watermark);
        // Every intent was recorded and closed in the Store.
        let opened: Vec<i64> = control_events(&db.admin, "audit.access.intent_opened")
            .await
            .into_iter()
            .map(|(seq, _)| seq)
            .collect();
        for intent in &manifest.intents {
            assert!(opened.contains(&intent.intent_seq));
        }
        if operation == AccessOperation::IdentityChain {
            assert_eq!(
                watermark, target,
                "the first export starts at the checkpoint head"
            );
            assert_eq!(
                manifest.checkpoint.as_ref().expect("checkpoint").comparison,
                "match"
            );
        }
    }
    // A complete export must reach its watermark: a truncated text fails.
    let dir = scratch_dir("truncated");
    let export = export_to_dir(
        &verifier,
        &ExportRequest {
            operation: AccessOperation::IdentityChain,
            filter: json!({"seq_through": 5}),
            page_size: 1000,
            max_pages: 1,
            checkpoint: None,
        },
        &dir,
    )
    .await
    .expect("bounded range");
    assert_eq!(export.manifest.watermark, Some(5));
    let text = std::fs::read_to_string(&export.export_path).expect("text");
    let truncated: String = text.lines().take(3).map(|l| format!("{l}\n")).collect();
    assert!(matches!(
        audit_store_postgres::files::verify_chain_export(
            &truncated,
            audit_core::Anchor::Genesis,
            5,
            false
        ),
        Err(audit_store_postgres::files::FileError::Incomplete {
            head: 3,
            watermark: 5
        })
    ));
    db.assert_store_conforms().await;
}

/// Authenticity needs an exact checkpoint at the export head; a checkpoint
/// behind the head authenticates only through itself; and an expiry that
/// commits after an intent fixed its watermark is never presented as
/// covered (deterministic interleaving: intent, then expire, then read).
#[tokio::test]
async fn exports_are_authentic_only_at_an_exact_checkpoint_and_never_cover_later_expiry() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = relay_store(&cast).await;
    let old = "2020-01-01T00:00:00.000000Z";
    for n in 0..3 {
        store
            .ingest(&document_created(Uuid::now_v7(), Uuid::now_v7(), old, n))
            .await
            .expect("stored");
    }
    let verifier = cast.verifier.admin().await;
    let export = |checkpoint: audit_core::Checkpoint, name: &'static str| {
        let verifier = verifier.clone();
        async move {
            export_to_dir(
                &verifier,
                &ExportRequest {
                    operation: AccessOperation::Verify,
                    filter: json!({}),
                    page_size: 1000,
                    max_pages: 1,
                    checkpoint: Some(checkpoint),
                },
                &scratch_dir(name),
            )
            .await
            .expect("export")
        }
    };
    let checkpoint = |admin: &audit_store_postgres::admin::AuditAdmin| {
        let admin = admin.clone();
        async move {
            admin
                .checkpoint()
                .await
                .expect("checkpoint")
                .checkpoint()
                .expect("value")
        }
    };

    // 1. The head equals the checkpoint: authentic.
    let c1 = checkpoint(&verifier).await;
    let exact = export(c1, "exact").await;
    assert_eq!(
        exact.manifest.checkpoint.as_ref().expect("c1").comparison,
        "match"
    );
    assert!(exact.manifest.complete);
    assert_eq!(exact.manifest.chain_integrity, "intact");
    assert_eq!(
        assess_recovery(&exact.report, &[c1], &[]).verdict,
        ChainVerdict::Authentic
    );

    // 2. Rows after the checkpoint: authentic only through it.
    for n in 3..5 {
        store
            .ingest(&document_created(Uuid::now_v7(), Uuid::now_v7(), old, n))
            .await
            .expect("stored");
    }
    let ahead = export(c1, "ahead").await;
    assert_eq!(
        ahead.manifest.checkpoint.as_ref().expect("c1").comparison,
        "ahead"
    );
    let assessment = assess_recovery(&ahead.report, &[c1], &[]);
    assert_eq!(
        assessment.verdict,
        ChainVerdict::AuthenticThrough { seq: c1.seq }
    );
    assert!(ahead.report.head.seq > c1.seq);

    // 3. The race: the intent fixes W, then an expiry commits (evidence
    //    past W) before the pages are read.
    let admin = cast.admin.admin().await;
    let policy = admin
        .set_retention_policy(
            "documents",
            &json!({"event_types": ["document.created"]}),
            Some(1),
        )
        .await
        .expect("policy");
    let c2 = checkpoint(&verifier).await;
    let token = verifier
        .open_access(AccessOperation::Verify, &json!({}), 1000, 1)
        .await
        .expect("intent");
    let watermark = token.watermark;
    assert_eq!(
        watermark, c2.seq,
        "the checkpoint is the head the intent fixes"
    );
    let far = parse_utc_text("2100-01-01T00:00:00.000000Z").expect("far");
    let expired = cast
        .maintainer
        .admin()
        .await
        .expire("documents", policy.revision, far, 1000)
        .await
        .expect("expired");
    assert_eq!(expired.expired_count, 5);
    assert!(expired.seq > watermark);
    let pages = verifier
        .read_all(&token)
        .await
        .expect("read after the expiry");
    let rows: Vec<_> = pages.into_iter().flatten().collect();
    verifier
        .close_access(token.secret(), i64::try_from(rows.len()).expect("n"), &[])
        .await
        .expect("closed");
    let text: String = rows.iter().map(|r| format!("{}\n", r.line)).collect();
    assert!(
        matches!(
            audit_core::verify_export_complete(&text, audit_core::Anchor::Genesis, watermark),
            Err(audit_core::ExportError::ExpiryEvidenceMissing { .. })
        ),
        "core refuses to call it complete"
    );
    let check = audit_store_postgres::files::verify_chain_export(
        &text,
        audit_core::Anchor::Genesis,
        watermark,
        true,
    )
    .expect("verified as an ordinary anchored export");
    assert!(!check.complete);
    assert_eq!(check.expired_after_watermark, 5);
    assert_eq!(check.report.head.seq, watermark);
    assert_eq!(check.report.unverified_expiry_evidence, 5);
    let assessment = assess_recovery(&check.report, &[c2], &[]);
    assert_eq!(assessment.authenticated_through, Some(c2.seq));
    assert_eq!(assessment.unconfirmed_expiries, 5);
    assert_eq!(
        assessment.verdict,
        ChainVerdict::UnverifiedExpiry,
        "the matching checkpoint does not make the later expiry covered"
    );
    // A missing evidence row at or below W is never excused as a race.
    let forged: String = rows
        .iter()
        .map(|r| {
            let mut line: Value = serde_json::from_str(&r.line).expect("line");
            if line["expired"] == json!(true) {
                line["expired_by_seq"] = json!(watermark);
            }
            format!("{line}\n")
        })
        .collect();
    assert!(
        audit_store_postgres::files::verify_chain_export(
            &forged,
            audit_core::Anchor::Genesis,
            watermark,
            true
        )
        .is_err()
    );

    // 4. Exported again after the expiry: the evidence is inside, the export
    //    is complete and the exact checkpoint makes it authentic.
    let c3 = checkpoint(&verifier).await;
    let after = export(c3, "after-expiry").await;
    assert!(after.manifest.complete);
    assert_eq!(after.manifest.expired_after_watermark, 0);
    assert_eq!(after.report.unverified_expiry_evidence, 0);
    assert_eq!(after.report.expired, 5);
    assert_eq!(
        assess_recovery(&after.report, &[c3], &[]).verdict,
        ChainVerdict::Authentic
    );
    db.assert_store_conforms().await;
}
