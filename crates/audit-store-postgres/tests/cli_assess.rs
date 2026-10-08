//! `audit-admin assess` end to end: exports written by `audit-admin export`
//! are assessed offline (no database URL) against out-of-band checkpoints
//! and the out-of-band recovery records (`kp-audit-recovery-records-v1`,
//! appended from `begin-recovery-epoch`'s output), one verdict class at a
//! time. The exit status separates authentic (0), verdicts a human reviews
//! (4) and tampered, unanchored or broken exports (5).

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use audit_core::{AuditStore, chain_next, envelope_digest};
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use support::*;
use uuid::Uuid;

const ACTOR: &str = "synthetic-human";

fn audit_admin(url: Option<&str>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_audit-admin"));
    command
        .args(args)
        .env_remove("AUDIT_STORE_DATABASE_URL")
        .env_remove("AUDIT_STORE_MIGRATE_DATABASE_URL");
    if let Some(url) = url {
        command.env("AUDIT_STORE_DATABASE_URL", url);
    }
    command.output().expect("run audit-admin")
}

fn json_lines(output: &Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "audit-admin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone())
        .expect("utf8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn scratch_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("audit-assess-{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn path_text(path: &Path) -> String {
    path.to_str().expect("utf8 path").to_owned()
}

/// `audit-admin checkpoint --out <dir>/<name>`: the file path and the record.
fn checkpoint(url: &str, dir: &Path, name: &str) -> (String, Value) {
    let file = path_text(&dir.join(name));
    let written = json_lines(&audit_admin(Some(url), &["checkpoint", "--out", &file]));
    (file, written[0].clone())
}

/// `audit-admin export --dir <dir>/<name> ...`: the directory and manifest.
fn export(url: &str, dir: &Path, name: &str, args: &[&str]) -> (PathBuf, Value) {
    let target = dir.join(name);
    std::fs::create_dir(&target).expect("export dir");
    let target_text = path_text(&target);
    let mut all = vec!["export", "--dir", target_text.as_str()];
    all.extend_from_slice(args);
    let manifest = json_lines(&audit_admin(Some(url), &all));
    (target, manifest[0].clone())
}

/// `audit-admin assess` without any database URL: `(exit code, report)`.
fn assess(export_dir: &Path, checkpoint: &str, extra: &[&str]) -> (Option<i32>, Value) {
    let dir = path_text(export_dir);
    let mut args = vec!["assess", "--dir", dir.as_str(), "--checkpoint", checkpoint];
    args.extend_from_slice(extra);
    let output = audit_admin(None, &args);
    let report = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.code(), report)
}

/// A copy of an export directory whose `export.jsonl` is rewritten.
fn copy_with(source: &Path, target: &Path, rewrite: impl Fn(&str) -> String) -> PathBuf {
    std::fs::create_dir(target).expect("copy dir");
    let export = std::fs::read_to_string(source.join("export.jsonl")).expect("export");
    std::fs::write(target.join("export.jsonl"), rewrite(&export)).expect("write export");
    std::fs::copy(source.join("manifest.json"), target.join("manifest.json")).expect("manifest");
    target.to_owned()
}

#[derive(Deserialize)]
struct Line<'a> {
    seq: i64,
    event_id: Uuid,
    origin: String,
    envelope_digest: String,
    chain: String,
    recovery_epoch: i64,
    expired: bool,
    expired_by_seq: Option<i64>,
    #[serde(borrow)]
    envelope: Option<&'a RawValue>,
}

/// Rewrites the first body containing the synthetic actor and recomputes
/// its digest and every chain value after it: a consistent rewrite that
/// only an out-of-band checkpoint can expose.
fn rewrite_history(export: &str) -> String {
    let mut out = String::new();
    let mut prev = audit_core::GENESIS;
    let mut forged = false;
    for raw in export.lines() {
        let line: Line<'_> = serde_json::from_str(raw).expect("export line");
        let mut body = line.envelope.map(|e| e.get().to_owned());
        let mut digest = line.envelope_digest.clone();
        if !forged && let Some(text) = body.as_mut().filter(|b| b.contains(ACTOR)) {
            *text = text.replacen(ACTOR, "synthetic-forger", 1);
            digest = hex(&envelope_digest(text));
            forged = true;
        }
        let digest_bytes: [u8; 32] = audit_core::chain::parse_hex32(&digest).expect("digest");
        let chain = chain_next(&prev, line.seq, line.event_id, &digest_bytes);
        if !forged {
            assert_eq!(hex(&chain), line.chain, "untouched prefix");
        }
        out.push_str(&format!(
            "{{\"seq\":{},\"event_id\":\"{}\",\"origin\":\"{}\",\"envelope_digest\":\"{}\",\
             \"prev_chain\":\"{}\",\"chain\":\"{}\",\"recovery_epoch\":{},\"expired\":{},\
             \"expired_by_seq\":{},\"envelope\":{}}}\n",
            line.seq,
            line.event_id,
            line.origin,
            digest,
            hex(&prev),
            hex(&chain),
            line.recovery_epoch,
            line.expired,
            serde_json::to_string(&line.expired_by_seq).expect("seq"),
            body.as_deref().unwrap_or("null"),
        ));
        prev = chain;
    }
    assert!(forged, "a body carries the synthetic actor");
    out
}

/// Re-applies access and retention after an epoch (no retention policy).
fn reapply(cast: &Cast) {
    json_lines(&audit_admin(
        Some(&cast.admin.url),
        &["record-access-reapplied"],
    ));
    json_lines(&audit_admin(
        Some(&cast.maintainer.url),
        &["confirm-retention-reapplied"],
    ));
}

/// Declares a recovery, previews it, then starts the epoch with the
/// preview as the expectation. Returns the record line `begin-recovery-
/// epoch` printed (identical to the preview's) and the started epoch.
fn recovery_epoch(
    cast: &Cast,
    checkpoint: Option<&str>,
    relay_max_seq: Option<i64>,
) -> (String, Value) {
    let maintainer = cast.maintainer.url.as_str();
    json_lines(&audit_admin(
        Some(maintainer),
        &["declare-recovery-pending", "--incident-code", "drill_1"],
    ));
    let max = relay_max_seq.map(|seq| seq.to_string());
    let mut base = vec!["begin-recovery-epoch"];
    if let Some(checkpoint) = checkpoint {
        base.extend(["--checkpoint", checkpoint]);
    }
    if let Some(max) = &max {
        base.extend(["--relay-max-seq", max.as_str()]);
    }
    let mut preview_args = base.clone();
    preview_args.push("--preview");
    let preview = json_lines(&audit_admin(Some(maintainer), &preview_args));
    assert_eq!(preview.len(), 2, "the preview prints the record line");
    assert_eq!(preview[1]["format"], "kp-audit-recovery-records-v1");
    // The record line names the bound, or null when the Store cannot bound
    // the lost range; `--expect-lost-upper unknown` expects exactly that.
    let lost_upper = match &preview[1]["lost_upper"] {
        Value::Null => "unknown".to_owned(),
        other => text(other),
    };
    let expected = [
        ("--expect-old-epoch", text(&preview[0]["old_epoch"])),
        ("--expect-head-seq", text(&preview[0]["restored_head_seq"])),
        (
            "--expect-head-chain",
            text(&preview[0]["restored_head_chain"]),
        ),
        ("--expect-lost-upper", lost_upper),
    ];
    let mut args = base;
    for (flag, value) in &expected {
        args.extend([*flag, value.as_str()]);
    }
    let output = audit_admin(Some(maintainer), &args);
    let started = json_lines(&output);
    assert_eq!(started.len(), 2, "the epoch prints the record line");
    assert_eq!(
        started[1], preview[1],
        "the Store confirmed the previewed record"
    );
    let known = started[0]["lost_upper_known"] == Value::Bool(true);
    assert_eq!(
        started[1],
        json!({
            "format": "kp-audit-recovery-records-v1",
            "old_epoch": started[0]["old_epoch"],
            "new_epoch": started[0]["new_epoch"],
            "restored_head_seq": started[0]["restored_head_seq"],
            "restored_head_chain": started[0]["restored_head_chain"],
            "lost_upper": if known { started[0]["lost_upper_seq"].clone() } else { Value::Null },
        })
    );
    let line = String::from_utf8(output.stdout)
        .expect("utf8")
        .lines()
        .nth(1)
        .expect("record line")
        .to_owned();
    (line, started[0].clone())
}

#[tokio::test]
async fn assess_reports_each_verdict_class_offline() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    let mut events = Vec::new();
    for n in 0..3 {
        let id = Uuid::now_v7();
        store
            .ingest(&document_created(id, Uuid::now_v7(), OCCURRED, n))
            .await
            .expect("stored");
        events.push(id);
    }
    let verifier = cast.verifier.url.clone();
    let dir = scratch_dir();

    // Authentic: a body export whose head is the out-of-band checkpoint.
    let (cp1, cp1_record) = checkpoint(&verifier, &dir, "cp1.json");
    let (full, manifest) = export(&verifier, &dir, "full", &["--operation", "verify"]);
    assert_eq!(manifest["checkpoint"], Value::Null);
    let (code, report) = assess(&full, &cp1, &[]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["verdict"], "authentic");
    assert_eq!(report["chain_integrity"], "intact");
    assert_eq!(report["authenticated_through"], cp1_record["seq"]);
    assert_eq!(report["head"]["seq"], cp1_record["seq"]);
    assert_eq!(report["head"]["chain"], cp1_record["chain"]);
    assert_eq!(report["findings"], json!({"match": 1}));
    assert_eq!(report["anchor_seq"], 0);
    assert_eq!(report["complete"], Value::Bool(true));
    // Bounded: codes, seqs and counts, never envelope content.
    let rendered = report.to_string();
    for content in [ACTOR, "document/", &events[0].to_string()] {
        assert!(!rendered.contains(content), "{content} in {rendered}");
    }

    // Broken: one export line edited in place.
    let edited = copy_with(&full, &dir.join("edited"), |export| {
        export.replacen(ACTOR, "synthetic-forger", 1)
    });
    let (code, report) = assess(&edited, &cp1, &[]);
    assert_eq!(code, Some(5), "{report}");
    assert_eq!(report["verdict"], "broken");
    assert_eq!(report["chain_integrity"], "broken");
    assert!(
        text(&report["error"]).ends_with("envelope digest mismatch"),
        "{report}"
    );

    // Tampered: the same edit with every digest and chain recomputed (and
    // the manifest's head with them). The chain is intact; only the
    // out-of-band checkpoint exposes it.
    let rewritten = copy_with(&full, &dir.join("rewritten"), rewrite_history);
    let (code, report) = assess(&rewritten, &cp1, &[]);
    assert_eq!(code, Some(5), "{report}");
    assert_eq!(
        report["error"], "manifest does not match the export",
        "the manifest still names the original head"
    );
    let last: Value = serde_json::from_str(
        std::fs::read_to_string(rewritten.join("export.jsonl"))
            .expect("export")
            .lines()
            .last()
            .expect("rows"),
    )
    .expect("line");
    let mut manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(rewritten.join("manifest.json")).expect("manifest"),
    )
    .expect("manifest json");
    manifest["head"]["chain"] = last["chain"].clone();
    std::fs::write(rewritten.join("manifest.json"), manifest.to_string()).expect("manifest");
    let (code, report) = assess(&rewritten, &cp1, &[]);
    assert_eq!(code, Some(5), "{report}");
    assert_eq!(report["verdict"], "tampered");
    assert_eq!(report["chain_integrity"], "intact");
    assert_eq!(report["findings"], json!({"mismatch": 1}));
    assert_eq!(report["authenticated_through"], Value::Null);

    // AuthenticThrough: the Store grew past the checkpoint.
    for n in 3..5 {
        store
            .ingest(&document_created(
                Uuid::now_v7(),
                Uuid::now_v7(),
                OCCURRED,
                n,
            ))
            .await
            .expect("stored");
    }
    let (grown, _) = export(&verifier, &dir, "grown", &["--operation", "verify"]);
    let (code, report) = assess(&grown, &cp1, &[]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "authentic_through");
    assert_eq!(report["authenticated_through"], cp1_record["seq"]);
    assert_eq!(report["findings"], json!({"ahead": 1}));

    // A prefix export (`--seq-through`) judged against a checkpoint past its
    // last seq: the export cannot confirm or refute that checkpoint. Not
    // tampered, never authentic: the distinct usage outcome `store_behind`
    // (exit 2). With the checkpoint at its last seq the prefix is authentic.
    let (cp_grown, _) = checkpoint(&verifier, &dir, "cp-grown.json");
    let through = text(&cp1_record["seq"]);
    let (prefix, manifest) = export(
        &verifier,
        &dir,
        "prefix",
        &["--operation", "verify", "--seq-through", &through],
    );
    assert_eq!(manifest["watermark"], cp1_record["seq"]);
    let (code, report) = assess(&prefix, &cp_grown, &[]);
    assert_eq!(code, Some(2), "{report}");
    assert_eq!(report["verdict"], "store_behind");
    assert_eq!(report["authenticated_through"], Value::Null);
    assert_eq!(report["findings"], json!({"store_behind": 1}));
    assert_eq!(report["head"]["seq"], cp1_record["seq"]);
    let (code, report) = assess(&prefix, &cp1, &[]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["verdict"], "authentic");

    // NoCheckpoint: an export anchored at the checkpoint itself confirms
    // nothing new. An export after genesis needs that anchor; with it, a
    // later checkpoint at the head authenticates the tail.
    let (cp2, cp2_record) = checkpoint(&verifier, &dir, "cp2.json");
    store
        .ingest(&document_created(
            Uuid::now_v7(),
            Uuid::now_v7(),
            OCCURRED,
            6,
        ))
        .await
        .expect("stored");
    let (cp2b, cp2b_record) = checkpoint(&verifier, &dir, "cp2b.json");
    let after = text(&cp2_record["seq"]);
    let (tail, manifest) = export(
        &verifier,
        &dir,
        "tail",
        &[
            "--operation",
            "verify",
            "--seq-after",
            &after,
            "--checkpoint",
            &cp2,
        ],
    );
    assert_eq!(manifest["anchored"], Value::Bool(true));
    let (code, report) = assess(&tail, &cp2, &[]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "no_checkpoint");
    assert_eq!(report["anchor_seq"], cp2_record["seq"]);
    assert_eq!(report["findings_neutral"], 1);
    let (code, _) = assess(&tail, &cp1, &[]);
    assert_eq!(code, Some(2), "no out-of-band anchor at seq_after");
    let (code, report) = assess(&tail, &cp2b, &["--anchor", &cp2]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["verdict"], "authentic");
    assert_eq!(report["authenticated_through"], cp2b_record["seq"]);
    assert_eq!(report["anchor_seq"], cp2_record["seq"]);
    // A checkpoint before the anchor confirms nothing on this path.
    let (code, report) = assess(&tail, &cp1, &["--anchor", &cp2]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "no_checkpoint");
    assert_eq!(report["findings"], json!({"before_anchor": 1}));

    // Unanchored: a filtered subset.
    let (subset, manifest) = export(
        &cast.reader.url,
        &dir,
        "subset",
        &["--filter", r#"{"event_types":["document.created"]}"#],
    );
    assert_eq!(manifest["anchored"], Value::Bool(false));
    let (code, report) = assess(&subset, &cp1, &[]);
    assert_eq!(code, Some(5), "{report}");
    assert_eq!(report["verdict"], "unanchored");
    assert_eq!(report["chain_integrity"], "unanchored");

    // UnverifiedExpiry: a purged body seen through the identity chain (no
    // bodies, so its evidence is unverified); the body export verifies it.
    json_lines(&audit_admin(
        Some(&cast.maintainer.url),
        &[
            "purge-body",
            "--event-id",
            &events[0].to_string(),
            "--reason-code",
            "adapter_defect",
        ],
    ));
    let (cp3, _) = checkpoint(&verifier, &dir, "cp3.json");
    let (identity, _) = export(&verifier, &dir, "identity", &["--identity-chain"]);
    let (code, report) = assess(&identity, &cp3, &[]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "unverified_expiry");
    assert_eq!(report["operation"], "identity_chain");
    assert_eq!(report["unconfirmed_expiries"], 1);
    assert_eq!(report["unverified_expiry_evidence"], 1);
    let (cp3b, _) = checkpoint(&verifier, &dir, "cp3b.json");
    let (bodies, _) = export(&verifier, &dir, "bodies", &["--operation", "verify"]);
    let (code, report) = assess(&bodies, &cp3b, &[]);
    assert_eq!(code, Some(0), "the purge evidence is verified: {report}");
    assert_eq!(report["unverified_expiry_evidence"], 0);
    // A checkpoint past the export's head in the same epoch: an older (or
    // cut) export, or rows the Store lost. Never authentic; a fresh full
    // export through that checkpoint decides it.
    let (code, report) = assess(&identity, &cp3b, &[]);
    assert_eq!(code, Some(2), "{report}");
    assert_eq!(report["verdict"], "store_behind");
    assert_eq!(report["findings"], json!({"store_behind": 1}));

    // A planned move: an epoch without loss. With its record it is
    // authentic; without it, an unverified recovery.
    let records = dir.join("recovery-records.jsonl");
    let records_text = path_text(&records);
    let (cp4, cp4_record) = checkpoint(&verifier, &dir, "cp4.json");
    let (line, started) = recovery_epoch(&cast, Some(&cp4), None);
    assert_eq!(started["classification"], "planned_move");
    assert_eq!(started["restored_head_seq"], cp4_record["seq"]);
    std::fs::write(&records, format!("{line}\n")).expect("records");
    reapply(&cast);
    let (cp5, cp5_record) = checkpoint(&verifier, &dir, "cp5.json");
    let (moved, _) = export(&verifier, &dir, "moved", &["--operation", "verify"]);
    let (code, report) = assess(&moved, &cp5, &["--recovery-records", &records_text]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["verdict"], "authentic");
    assert_eq!(report["epochs_total"], 1);
    assert_eq!(report["epochs"][0]["recorded"], Value::Bool(true));
    assert_eq!(report["epochs"][0]["classification"], "planned_move");
    assert_eq!(report["epochs"][0]["new_epoch"], 2);
    let (code, report) = assess(&moved, &cp5, &[]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "unverified_recovery");
    assert_eq!(report["epochs_unrecorded"], 1);

    // Lost: the relay referenced Store seqs past the restored head (an
    // in-place restore the fingerprint cannot see). The record documents
    // the lost range.
    let status = json_lines(&audit_admin(Some(&verifier), &["status"]));
    let head = status[0]["head_seq"].as_i64().expect("head");
    let relay_max_seq = head + 5;
    let (line, started) = recovery_epoch(&cast, Some(&cp5), Some(relay_max_seq));
    assert_eq!(started["classification"], "restore");
    assert_eq!(started["restored_head_seq"], head);
    assert_eq!(started["lost_upper_seq"], relay_max_seq);
    std::fs::write(
        &records,
        format!(
            "{}{line}\n",
            std::fs::read_to_string(&records).expect("records")
        ),
    )
    .expect("append");
    reapply(&cast);
    let (cp6, cp6_record) = checkpoint(&verifier, &dir, "cp6.json");
    let (restored, _) = export(&verifier, &dir, "restored", &["--operation", "verify"]);
    let (code, report) = assess(&restored, &cp6, &["--recovery-records", &records_text]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "lost");
    assert_eq!(report["authenticated_through"], cp6_record["seq"]);
    assert_eq!(report["epochs_total"], 2);
    assert_eq!(report["epochs_unrecorded"], 0);
    assert_eq!(report["epochs"][1]["lost_upper"], relay_max_seq);
    assert_eq!(report["epochs"][1]["restored_head_seq"], head);
    assert_eq!(report["epochs"][1]["classification"], "restore");
    // An earlier checkpoint on the surviving history still confirms it.
    let (code, report) = assess(&restored, &cp4, &["--recovery-records", &records_text]);
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "lost");
    assert_eq!(report["authenticated_through"], cp4_record["seq"]);
    assert_eq!(report["findings"], json!({"ahead": 1}));
    // Without the records, or with a record that disagrees, the recovery
    // is unverified (suspected tampering disguised as recovery).
    let (code, report) = assess(&restored, &cp6, &[]);
    assert_eq!(
        (code, text(&report["verdict"])),
        (Some(4), "unverified_recovery".to_owned())
    );
    let disagreeing = dir.join("disagreeing.jsonl");
    std::fs::write(
        &disagreeing,
        std::fs::read_to_string(&records).expect("records").replace(
            &format!("\"lost_upper\":{relay_max_seq}"),
            &format!("\"lost_upper\":{}", relay_max_seq + 1),
        ),
    )
    .expect("write");
    let (code, report) = assess(
        &restored,
        &cp6,
        &["--recovery-records", &path_text(&disagreeing)],
    );
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "unverified_recovery");
    assert_eq!(report["unmatched_records"], 1);
    // The previous checkpoint is still authentic through its seq, and the
    // record of the later epoch lies after that export's head (neutral).
    let (code, report) = assess(&moved, &cp5, &["--recovery-records", &records_text]);
    assert_eq!(
        (code, report["records_after_head"].clone()),
        (Some(0), json!(1))
    );
    assert_eq!(cp5_record["epoch"], 2);

    // An unknown lost bound: a restore recorded without a checkpoint and
    // without the relay's highest referenced seq. The record line says so
    // (`lost_upper: null`) and the epoch is never authentic, even with a
    // checkpoint at the head of an export that crosses only this epoch.
    let (line, started) = recovery_epoch(&cast, None, None);
    assert_eq!(started["lost_upper_known"], Value::Bool(false));
    assert_eq!(started["checkpoint_classification"], Value::Null);
    let unknown_line: Value = serde_json::from_str(&line).expect("record line");
    assert_eq!(unknown_line["lost_upper"], Value::Null, "{line}");
    std::fs::write(
        &records,
        format!(
            "{}{line}\n",
            std::fs::read_to_string(&records).expect("records")
        ),
    )
    .expect("append");
    reapply(&cast);
    let (cp7, cp7_record) = checkpoint(&verifier, &dir, "cp7.json");
    let after = text(&cp6_record["seq"]);
    let (unknown, _) = export(
        &verifier,
        &dir,
        "unknown",
        &[
            "--operation",
            "verify",
            "--seq-after",
            &after,
            "--checkpoint",
            &cp6,
        ],
    );
    let (code, report) = assess(
        &unknown,
        &cp7,
        &["--anchor", &cp6, "--recovery-records", &records_text],
    );
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "lost");
    assert_eq!(report["authenticated_through"], cp7_record["seq"]);
    assert_eq!(report["epochs_total"], 1);
    assert_eq!(report["epochs"][0]["recorded"], Value::Bool(true));
    assert_eq!(report["epochs"][0]["lost_upper_known"], Value::Bool(false));
    assert_eq!(report["records_before_anchor"], 2);
    // Without the record the epoch is unverified; a record that claims the
    // bound is known (lost_upper = restored head) does not match the body.
    let (code, report) = assess(&unknown, &cp7, &["--anchor", &cp6]);
    assert_eq!(
        (code, text(&report["verdict"])),
        (Some(4), "unverified_recovery".to_owned())
    );
    let claimed = dir.join("claimed.jsonl");
    std::fs::write(
        &claimed,
        format!(
            "{}\n",
            line.replace(
                "\"lost_upper\":null",
                &format!("\"lost_upper\":{}", text(&started["restored_head_seq"]))
            )
        ),
    )
    .expect("write");
    let (code, report) = assess(
        &unknown,
        &cp7,
        &["--anchor", &cp6, "--recovery-records", &path_text(&claimed)],
    );
    assert_eq!(code, Some(4), "{report}");
    assert_eq!(report["verdict"], "unverified_recovery");
    assert_eq!(report["unmatched_records"], 1);

    // Inputs that cannot be read are failures, not verdicts.
    let malformed = dir.join("malformed.jsonl");
    std::fs::write(
        &malformed,
        "{\"format\":\"kp-audit-recovery-records-v1\"}\n",
    )
    .expect("write");
    let (code, _) = assess(
        &restored,
        &cp6,
        &["--recovery-records", &path_text(&malformed)],
    );
    assert_eq!(code, Some(1));
    let (code, _) = assess(&dir.join("missing"), &cp6, &[]);
    assert_eq!(code, Some(1));
    let output = audit_admin(None, &["assess", "--dir", &path_text(&restored)]);
    assert_eq!(output.status.code(), Some(2), "--checkpoint is required");
    db.assert_store_conforms().await;
}
