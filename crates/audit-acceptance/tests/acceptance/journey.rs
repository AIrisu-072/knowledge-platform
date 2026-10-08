//! T1 (design §14.4): every Document producer of the audit catalog (all 23
//! relay-origin types), called through the production application
//! services, reaches the Audit Store through the running relay
//! (`audit-relay run`) exactly once, with the source identity, the
//! scheduler's attribution (`service/scheduler` beside the requester) and
//! only a summary of every free-text reason. Nothing the Store holds
//! contains a reason text, file content, a storage locator or an ACL list.
//! produced / delivered / stored / verified are reported apart; the Store
//! chain verifies, a checkpoint is taken, and the offline assessment of an
//! export against it is `authentic`.

use std::collections::BTreeSet;

use audit_store_postgres::admin::AccessOperation;
use audit_store_postgres::assess::{AssessClass, AssessInputs, assess_dir};
use audit_store_postgres::files::{ExportRequest, export_to_dir};
use document_application::{ApplicationError, DueExecutionOutcome, ReadStateMutationKind};
use document_domain::FolderId;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::document::{ACL_ONLY_GROUP, Platform, READER, READERS_GROUP, editor_ctx, reader_ctx};
use crate::support::*;

const V1: &[u8] = b"A synthetic acceptance original CONTENTMARK-7a1f\n";
const V2: &[u8] = b"B synthetic acceptance revision CONTENTMARK-9b2e\n";
const V2_UPDATED: &[u8] = b"C synthetic acceptance update CONTENTMARK-c3d0\n";
const V3: &[u8] = b"D synthetic acceptance draft CONTENTMARK-d4f1\n";
const V_REVOKED: &[u8] = b"E synthetic revoked schedule CONTENTMARK-e5a2\n";

/// Free-text reasons: unique tokens that must never reach the Store.
const METADATA_REASON: &str = "SYNTHREASON-metadata-41c7 監査用の理由";
const FOLDER_CREATE_REASON: &str = "SYNTHREASON-folder-create-0d3a";
const FOLDER_RENAME_REASON: &str = "SYNTHREASON-folder-rename-5b19";
const FOLDER_MOVE_REASON: &str = "SYNTHREASON-folder-move-77e2";
const DOCUMENT_MOVE_REASON: &str = "SYNTHREASON-document-move-a90c";
const POLICY_REASON: &str = "SYNTHREASON-policy-3f61";
const WITHDRAW_REASON: &str = "SYNTHREASON-withdraw-c2d8";
const END_REASON: &str = "SYNTHREASON-end-e5b4";
const REVOKE_REASON: &str = "SYNTHREASON-revoke-publish-8a3c";

const REASONS: [&str; 9] = [
    METADATA_REASON,
    FOLDER_CREATE_REASON,
    FOLDER_RENAME_REASON,
    FOLDER_MOVE_REASON,
    DOCUMENT_MOVE_REASON,
    POLICY_REASON,
    WITHDRAW_REASON,
    END_REASON,
    REVOKE_REASON,
];

#[test]
fn every_document_producer_reaches_the_store_once_and_verifies() {
    run_scenario(journey);
}

async fn journey() {
    let env = Env::start().await;
    assert_relay_posture_clean(&env).await;
    let platform = Platform::new(env.document.clone());
    // The relay runs while the Document platform works (audit-relay run).
    let relay = RunningRelay::start(&env);

    // Bootstrap: the one-time root policy (access_policy.changed, bootstrap).
    platform.bootstrap().await;

    // Create (document.created + document.version.created), publish.
    let (document, v1) = platform
        .create_document(Platform::root(), "Synthetic acceptance document", V1)
        .await
        .expect("create");
    platform.publish(document, v1).await.expect("publish v1");

    // First read confirmation (editor), then the reader's VIEW, RESET and
    // VIEW again (detail_viewed first_record, marked_unread, detail_viewed).
    assert!(
        platform
            .mark_read(&editor_ctx(), document, v1)
            .await
            .expect("read")
    );
    for (expected, kind) in [
        (0, ReadStateMutationKind::View),
        (1, ReadStateMutationKind::Reset),
        (2, ReadStateMutationKind::View),
    ] {
        assert!(
            platform
                .read_state(&reader_ctx(), document, v1, expected, kind)
                .await
                .expect("read state")
        );
    }

    // The original file (file.access_granted, with a correlation id).
    let correlation = Uuid::now_v7();
    assert_eq!(platform.open_original(document, v1, correlation).await, V1);

    // Metadata change with a reason; revision 1.0 → 1.1; a revision
    // comparison of the same version (no diff executed).
    platform
        .update_metadata(document, "synthetic-category", METADATA_REASON)
        .await
        .expect("metadata");
    let r10 = platform.revision_id(document, 1, 0).await;
    let r11 = platform.revision_id(document, 1, 1).await;
    assert!(
        !platform.compare_revisions(document, r10, r11).await,
        "same authoritative version: no content comparison"
    );

    // Folders: create A and B, rename A, move A under B; move the document
    // into A; an explicit policy on B (normal ACL change).
    let folder_a = FolderId::from_uuid(Uuid::now_v7());
    let folder_b = FolderId::from_uuid(Uuid::now_v7());
    for (folder, name) in [(folder_a, "Synthetic A"), (folder_b, "Synthetic B")] {
        platform
            .create_folder(
                &editor_ctx(),
                folder,
                Platform::root(),
                name,
                FOLDER_CREATE_REASON,
            )
            .await
            .expect("create folder");
    }
    platform
        .rename_folder(folder_a, "Synthetic A renamed", FOLDER_RENAME_REASON)
        .await
        .expect("rename");
    platform
        .move_folder(folder_a, Platform::root(), folder_b, FOLDER_MOVE_REASON)
        .await
        .expect("move folder");
    platform
        .move_document(document, Platform::root(), folder_a, DOCUMENT_MOVE_REASON)
        .await
        .expect("move document");
    platform
        .set_folder_policy(folder_b, 0, POLICY_REASON)
        .await
        .expect("folder policy");

    // Refusals. Management path: the reader may not create a folder.
    let refused = platform
        .create_folder(
            &reader_ctx(),
            FolderId::from_uuid(Uuid::now_v7()),
            Platform::root(),
            "Refused",
            "SYNTHREASON-refused-1c0b",
        )
        .await;
    assert!(
        matches!(refused, Err(ApplicationError::Forbidden)),
        "{refused:?}"
    );

    // A new WORKING version v2; read-state refusals on it (the reader holds
    // Read but not ReadHistory, which a non-current version needs).
    let v2 = platform
        .create_version(document, "Synthetic acceptance v2", V2)
        .await
        .expect("create version");
    let refused = platform.mark_read(&reader_ctx(), document, v2).await;
    assert!(
        matches!(refused, Err(ApplicationError::Forbidden)),
        "{refused:?}"
    );
    let refused = platform
        .read_state(&reader_ctx(), document, v2, 0, ReadStateMutationKind::View)
        .await;
    assert!(
        matches!(refused, Err(ApplicationError::Forbidden)),
        "{refused:?}"
    );

    // The WORKING v2 is replaced (document.version.updated); a schedule is
    // reserved and cancelled (publication.scheduled, .cancelled).
    platform
        .update_working(document, v2, "Synthetic acceptance v2 updated", V2_UPDATED)
        .await
        .expect("update working");
    let far = OffsetDateTime::now_utc() + time::Duration::hours(1);
    let cancelled = platform
        .schedule_publish(document, v2, far)
        .await
        .expect("schedule");
    platform
        .cancel_schedule(document, v2, cancelled)
        .await
        .expect("cancel schedule");

    // Scheduled publications executed by the scheduler identity: v2, and a
    // second document whose requester loses Publish before the due time
    // (publication.terminal, authorization_revoked).
    let due = OffsetDateTime::now_utc() + time::Duration::seconds(2);
    let schedule = platform
        .schedule_publish(document, v2, due)
        .await
        .expect("schedule");
    let (revoked_document, revoked_version) = platform
        .create_document(Platform::root(), "Synthetic revoked schedule", V_REVOKED)
        .await
        .expect("create the second document");
    let revoked = platform
        .schedule_publish(revoked_document, revoked_version, due)
        .await
        .expect("schedule the second document");
    platform
        .revoke_publish(revoked_document, REVOKE_REASON)
        .await
        .expect("revoke Publish");
    let outcomes = platform.run_scheduler_until(&[schedule, revoked]).await;
    assert!(
        matches!(
            outcomes[&schedule.as_uuid()],
            DueExecutionOutcome::Published(_)
        ),
        "{outcomes:?}"
    );
    assert!(
        matches!(
            &outcomes[&revoked.as_uuid()],
            DueExecutionOutcome::Terminal(reason) if reason == "authorization_revoked"
        ),
        "{outcomes:?}"
    );

    // Diff v1 → v2 (both originals opened: two file.access_granted, then
    // diff.result_access_granted); a revision comparison across versions
    // (cache hit: diff + revision comparison grants).
    assert!(!platform.compare_versions(document, v1, v2).await, "miss");
    let r20 = platform.revision_id(document, 2, 0).await;
    assert!(
        platform.compare_revisions(document, r11, r20).await,
        "different versions: content compared"
    );

    // A WORKING v3 on top of v2; withdraw v2 (v1 is restored), rebase v3
    // onto v1 (document.version.rebased), then end the publication.
    let v3 = platform
        .create_version(document, "Synthetic acceptance v3", V3)
        .await
        .expect("create v3");
    let restored = platform
        .withdraw(document, v2, WITHDRAW_REASON)
        .await
        .expect("withdraw");
    assert_eq!(restored, Some(v1));
    platform.rebase(document, v3).await.expect("rebase v3");
    platform
        .end_publication(document, v1, END_REASON)
        .await
        .expect("end publication");

    // ------------------------------------------------------------------
    // Drain, then stop the relay as an operator would.
    // ------------------------------------------------------------------
    wait_until("the relay drains", CONVERGE, || drained(&env)).await;
    // The running relay reports its closed circuit for health (sampled
    // every second).
    health_when(&env, |report| {
        report["circuit"]["running"] == json!(1) && report["circuit"]["state"] == json!("closed")
    })
    .await;
    let summary = relay.stop().await;
    if let Some(summary) = summary {
        assert!(summary.claimed >= summary.settled);
    }

    // ------------------------------------------------------------------
    // What the producers staged.
    // ------------------------------------------------------------------
    let staged = staged_rows(&env).await;
    let types: Vec<&str> = staged.iter().map(|row| row.event_type.as_str()).collect();
    let count = |event_type: &str| types.iter().filter(|t| **t == event_type).count();
    for (event_type, expected) in [
        ("access_policy.changed", 3),
        ("document.created", 2),
        ("document.version.created", 4),
        ("document.version.updated", 1),
        ("document.version.rebased", 1),
        ("document.version.published", 2),
        ("document.version.publication.scheduled", 3),
        ("document.version.publication.cancelled", 1),
        ("document.version.publication.terminal", 1),
        ("document.version.read_confirmed", 1),
        ("document.version.detail_viewed", 2),
        ("document.version.marked_unread", 1),
        ("document.file.access_granted", 3),
        ("document.metadata.changed", 1),
        ("document.revision_comparison.result_access_granted", 2),
        ("folder.created", 2),
        ("folder.renamed", 1),
        ("folder.moved", 1),
        ("document.moved", 1),
        ("authorization.denied", 3),
        ("document.diff.result_access_granted", 2),
        ("document.version.withdrawn", 1),
        ("document.publication.ended", 1),
    ] {
        assert_eq!(count(event_type), expected, "{event_type}: {types:?}");
    }
    // Every Document type of the audit catalog (relay origin).
    let distinct: BTreeSet<&str> = types.iter().copied().collect();
    assert_eq!(distinct.len(), 23, "{distinct:?}");
    let denials: BTreeSet<String> = staged
        .iter()
        .filter(|row| row.event_type == "authorization.denied")
        .map(|row| {
            assert_eq!(row.actor_pid, READER);
            row.data["action_code"].as_str().expect("code").to_owned()
        })
        .collect();
    assert_eq!(
        denials,
        BTreeSet::from([
            "create_folder".to_owned(),
            "mark_version_read".to_owned(),
            "mutate_read_state".to_owned()
        ])
    );
    assert!(
        staged
            .iter()
            .any(|row| row.event_type == "access_policy.changed"
                && row.data["bootstrap"] == json!(true)),
        "the bootstrap policy change is staged"
    );
    // The scheduler's executions: the requester stays the actor, the
    // executing service is recorded beside it.
    let executed: Vec<_> = staged
        .iter()
        .filter(|row| row.data.get("serviceExecutor").is_some())
        .collect();
    let executed_types: BTreeSet<&str> =
        executed.iter().map(|row| row.event_type.as_str()).collect();
    assert_eq!(
        executed_types,
        BTreeSet::from([
            "document.version.published",
            "document.version.publication.terminal"
        ]),
        "{executed:?}"
    );
    for row in &executed {
        assert_eq!(
            (row.actor_idp.as_str(), row.actor_pid.as_str()),
            ("test-idp", "editor"),
            "the requester stays the actor"
        );
        assert_eq!(
            row.data["serviceExecutor"],
            json!({"identityProvider": "service", "principalId": "scheduler"})
        );
    }
    let original = staged
        .iter()
        .find(|row| row.trace_id == Some(correlation.to_string()))
        .expect("the original file access carries its correlation id");
    assert_eq!(original.event_type, "document.file.access_granted");

    // ------------------------------------------------------------------
    // Exactly once in the Store, as acknowledged, with the source identity.
    // ------------------------------------------------------------------
    let store = env.store_client().await;
    let stored = assert_delivered_exactly_once(&env, &env.store_admin, &store).await;
    let ledger = deliveries(&env).await;
    for delivery in ledger.values() {
        assert_eq!(delivery.store_outcome.as_deref(), Some("stored"));
        assert_eq!(delivery.attempt_count, 1, "no retries were needed");
        assert_eq!(delivery.registration_kind, "trigger");
    }
    for row in &executed {
        let envelope = &stored[&row.event_id].envelope;
        assert_eq!(
            envelope["data"]["actor"],
            json!({"issuer": "test-idp", "principal_id": "editor"})
        );
        assert_eq!(
            envelope["data"]["service_executor"],
            json!({"issuer": "service", "principal_id": "scheduler"})
        );
    }
    let reasons_staged = staged
        .iter()
        .filter(|row| row.data.get("reason").is_some())
        .count();
    assert_eq!(
        reasons_staged, 8,
        "metadata, folders ×4, document move, withdraw, end keep their reason at the source \
         (the ACL changes record none)"
    );
    let policy = stored
        .values()
        .filter(|event| event.event_type == "access_policy.changed")
        .collect::<Vec<_>>();
    for event in policy {
        let details = &event.envelope["data"]["details"];
        assert!(details.get("grants").is_none() && details.get("mode").is_none());
    }
    assert_eq!(
        assert_store_chain(&env.store_admin).await,
        staged.len(),
        "the chain verifies with audit-core and every body fits the catalog"
    );

    // Nothing the Store holds (any table) contains a reason text, file
    // content, a storage locator, the storage root or an ACL-only subject.
    let locators: Vec<String> = sqlx::query_scalar("SELECT storage_locator FROM file_objects")
        .fetch_all(&env.doc_admin)
        .await
        .expect("locators");
    assert!(locators.len() >= 2);
    let mut needles: Vec<String> = REASONS.iter().map(|r| (*r).to_owned()).collect();
    needles.extend(
        REASONS
            .iter()
            .map(|r| r[..r.find(' ').unwrap_or(r.len())].to_owned()),
    );
    needles.push("SYNTHREASON".to_owned());
    needles.push("CONTENTMARK".to_owned());
    for body in [V1, V2, V2_UPDATED, V3, V_REVOKED] {
        needles.push(String::from_utf8_lossy(body).trim().to_owned());
    }
    needles.push(ACL_ONLY_GROUP.to_owned());
    needles.push(READERS_GROUP.to_owned());
    needles.push(
        platform
            .storage_root
            .path()
            .to_str()
            .expect("utf8 path")
            .to_owned(),
    );
    needles.extend(locators);
    let dump = env.store_dump_text(STORE_DB).await;
    assert!(
        dump.contains(&document.as_uuid().to_string()),
        "the dump is the Store"
    );
    assert_absent(&dump, "the Store database", &needles);

    // ------------------------------------------------------------------
    // produced / delivered / stored / verified stay distinct.
    // ------------------------------------------------------------------
    let produced = json!(staged.len());
    let report = env.health(true).await;
    assert_eq!(report["produced"]["staged"], produced, "{report}");
    assert_eq!(report["produced"]["registered"], produced);
    assert_eq!(report["produced"]["unregistered"], json!(0));
    assert_eq!(report["delivered"]["delivered"], produced);
    assert_eq!(report["delivered"]["pending"], json!(0));
    assert_eq!(report["circuit"]["running"], json!(0), "stopped cleanly");
    let head: i64 = report["stored"]["head_seq"].as_i64().expect("head");
    assert!(
        head > staged.len() as i64,
        "stored counts the Store's own control events too"
    );
    assert_eq!(report["stored"]["gate"], json!("ok"));
    assert_eq!(report["stored"]["missing_types"], json!([]));
    assert_eq!(report["verified"]["last_verified_seq"], Value::Null);
    assert_eq!(report["verified"]["unverified_events"], json!(head));
    assert_eq!(report["reconcile"]["counts"]["ok"], produced);
    assert_eq!(report["alarms"], json!([]), "{report}");

    let verifier = env.verifier_admin().await;
    let status = verifier.store_status().await.expect("store status");
    assert_eq!(status.head_seq, head);
    let verified = verifier.verify(Some(1), Some(head)).await.expect("verify");
    assert_eq!(verified.outcome, "ok", "{verified:?}");
    assert_eq!((verified.from_seq, verified.to_seq), (1, head));
    let report = env.health(false).await;
    assert_eq!(report["verified"]["last_verified_seq"], json!(head));
    assert_eq!(report["verified"]["outcome"], json!("ok"));
    assert_eq!(
        report["verified"]["unverified_events"],
        json!(1),
        "only the verification record itself"
    );
    assert_eq!(report["delivered"]["delivered"], produced);

    // Checkpoint, export, offline assessment: authentic.
    let checkpoint = verifier.checkpoint().await.expect("checkpoint");
    assert_eq!(checkpoint.outcome, "ok");
    let checkpoint = checkpoint.checkpoint().expect("checkpoint value");
    let dir = scratch_dir("journey");
    let exported = export_to_dir(
        &verifier,
        &ExportRequest {
            operation: AccessOperation::Verify,
            filter: json!({}),
            page_size: 1000,
            max_pages: 100,
            checkpoint: None,
        },
        &dir,
    )
    .await
    .expect("export");
    assert!(exported.manifest.complete, "{:?}", exported.manifest);
    assert_eq!(exported.manifest.watermark, Some(checkpoint.seq));
    let export_text = std::fs::read_to_string(&exported.export_path).expect("export file");
    assert_absent(&export_text, "the export", &needles);
    let assessed = assess_dir(
        &dir,
        &AssessInputs {
            checkpoint,
            anchor: None,
            records: Vec::new(),
        },
    )
    .expect("assess");
    assert_eq!(assessed.class(), AssessClass::Authentic, "{assessed:?}");
    eprintln!(
        "journey: produced={} types={} delivered={} stored_head={head} verified_through={} \
         checkpoint_seq={} export_rows={} verdict={}",
        staged.len(),
        distinct.len(),
        ledger.len(),
        verified.to_seq,
        checkpoint.seq,
        exported.manifest.rows,
        assessed.verdict
    );
    let _ = std::fs::remove_dir_all(&dir);
}
