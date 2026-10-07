//! `audit-admin` end to end: operator commands, 0600 files, refusal of
//! privileged sessions, URLs with `options` and sessions without
//! synchronous_commit, URL redaction in errors, chain exports over several
//! intents, and the recovery commands.

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use audit_core::AuditStore;
use audit_store_postgres::PostgresAuditStore;
use serde_json::Value;
use support::*;
use uuid::Uuid;

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

fn scratch_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("audit-admin-{}", Uuid::now_v7().simple()));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test]
async fn operator_commands_write_private_files_and_refuse_privileged_sessions() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = PostgresAuditStore::new(cast.relay.pool.clone(), Duration::from_secs(10))
        .await
        .expect("relay");
    for n in 0..3 {
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
    let verifier = cast.verifier.url.clone();
    let status = json_lines(&audit_admin(Some(&verifier), &["status"]));
    assert_eq!(status[0]["recovery_mode"], Value::Bool(false));
    let posture = json_lines(&audit_admin(Some(&verifier), &["posture"]));
    assert_eq!(posture[0]["status"], "clean");

    let dir = scratch_dir();
    let checkpoint = dir.join("checkpoint.json");
    let written = json_lines(&audit_admin(
        Some(&verifier),
        &["checkpoint", "--out", checkpoint.to_str().expect("path")],
    ));
    assert_eq!(written[0]["format"], "kp-audit-checkpoint-v1");
    assert_eq!(mode(&checkpoint), 0o600);

    let export_dir = dir.join("export");
    std::fs::create_dir(&export_dir).expect("dir");
    let manifest = json_lines(&audit_admin(
        Some(&verifier),
        &[
            "export",
            "--dir",
            export_dir.to_str().expect("path"),
            "--operation",
            "verify",
            "--checkpoint",
            checkpoint.to_str().expect("path"),
        ],
    ));
    assert_eq!(manifest[0]["anchored"], Value::Bool(true));
    // The checkpoint is its own verified record, the head the export's
    // first intent fixes as its watermark.
    assert_eq!(manifest[0]["checkpoint"]["comparison"], "match");
    assert_eq!(mode(&export_dir.join("export.jsonl")), 0o600);
    assert_eq!(mode(&export_dir.join("manifest.json")), 0o600);

    let verified = json_lines(&audit_admin(Some(&verifier), &["verify"]));
    assert_eq!(verified[0]["outcome"], "ok");

    // The reader investigates: bounded JSON lines, relay events only.
    let lines = json_lines(&audit_admin(
        Some(&cast.reader.url),
        &["investigate", "--page-size", "2", "--max-pages", "1"],
    ));
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l["origin"] == "relay"));

    // Superuser and owner-member sessions are refused; the URL is not echoed.
    for url in [db.superuser_url(&db.database), cast.dba.url.clone()] {
        let output = audit_admin(Some(&url), &["status"]);
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("refusing"), "{stderr}");
        assert!(!stderr.contains(PASSWORD) && !stderr.contains("postgres:postgres"));
    }
    // ...but bind requires the owner member.
    let bound = json_lines(&audit_admin(
        Some(&cast.dba.url),
        &["unbind", "--db-role", &cast.reader.role],
    ));
    assert_eq!(bound[0]["status"], "unbound");
    let output = audit_admin(
        Some(&verifier),
        &[
            "bind",
            "--db-role",
            "x",
            "--issuer",
            "i",
            "--principal",
            "p",
        ],
    );
    assert_eq!(output.status.code(), Some(1));

    // Source services are registered by the owner member only.
    let registered = json_lines(&audit_admin(
        Some(&cast.dba.url),
        &[
            "register-source-service",
            "--issuer",
            "service",
            "--principal",
            "audit-relay-standby",
            "--source",
            "urn:knowledge-platform:document-platform",
        ],
    ));
    assert_eq!(registered[0]["status"], "registered");
    assert_eq!(
        audit_admin(
            Some(&verifier),
            &[
                "register-source-service",
                "--issuer",
                "service",
                "--principal",
                "p",
                "--source",
                "urn:knowledge-platform:document-platform",
            ],
        )
        .status
        .code(),
        Some(1)
    );

    // A URL carrying `options` (e.g. to weaken synchronous_commit) is
    // refused before connecting, without echoing the URL.
    for query in [
        "?options=-c%20synchronous_commit%3Doff",
        "?sslmode=disable&%6Fptions=-c%20x%3Dy",
    ] {
        let output = audit_admin(Some(&format!("{verifier}{query}")), &["status"]);
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("options"), "{stderr}");
        assert!(!stderr.contains(PASSWORD), "{stderr}");
    }
    // A login whose session runs without synchronous_commit is refused.
    let lazy = db.login("lazy", &["audit_store_verifier"]).await;
    db.exec(&format!(
        "ALTER ROLE {} IN DATABASE {} SET synchronous_commit = off",
        lazy.role, db.database
    ))
    .await;
    let output = audit_admin(Some(&lazy.url), &["status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("synchronous_commit"));
    db.exec(&format!(
        "ALTER ROLE {} IN DATABASE {} SET synchronous_commit = on",
        lazy.role, db.database
    ))
    .await;
    assert_eq!(
        json_lines(&audit_admin(Some(&verifier), &["posture"]))[0]["status"],
        "clean"
    );

    // Usage errors exit 2; rebind no longer exists.
    assert_eq!(audit_admin(None, &["status"]).status.code(), Some(2));
    assert_eq!(
        audit_admin(Some(&verifier), &["nope"]).status.code(),
        Some(2)
    );
    assert_eq!(
        audit_admin(Some(&cast.maintainer.url), &["rebind-fingerprint"])
            .status
            .code(),
        Some(2)
    );
    db.assert_store_conforms().await;
}

#[tokio::test]
async fn chain_exports_and_recovery_commands_run_end_to_end() {
    let db = TestDb::start().await;
    let cast = Cast::new(&db).await;
    let store = cast.relay.store().await;
    for n in 0..3 {
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
    let verifier = cast.verifier.url.clone();
    let maintainer = cast.maintainer.url.clone();
    let dir = scratch_dir();
    let path = |name: &str| {
        let p = dir.join(name);
        p.to_str().expect("path").to_owned()
    };

    // --identity-chain loops over intents until the first watermark.
    std::fs::create_dir(dir.join("chain")).expect("dir");
    let manifest = json_lines(&audit_admin(
        Some(&verifier),
        &[
            "export",
            "--identity-chain",
            "--dir",
            &path("chain"),
            "--page-size",
            "4",
            "--max-pages",
            "1",
        ],
    ));
    let manifest = &manifest[0];
    assert_eq!(manifest["operation"], "identity_chain");
    assert_eq!(manifest["complete"], Value::Bool(true));
    assert_eq!(manifest["anchored"], Value::Bool(true));
    let intents = manifest["intents"].as_array().expect("intents");
    assert!(intents.len() >= 4, "{}", intents.len());
    assert_eq!(manifest["rows"], manifest["watermark"]);
    // Explicit seq ranges are passed through.
    std::fs::create_dir(dir.join("range")).expect("dir");
    let ranged = json_lines(&audit_admin(
        Some(&verifier),
        &[
            "export",
            "--identity-chain",
            "--dir",
            &path("range"),
            "--seq-through",
            "5",
        ],
    ));
    assert_eq!(ranged[0]["rows"], 5);
    assert_eq!(ranged[0]["intents"][0]["seq_through"], 5);

    // Declared recovery: recovery reads work, publication is refused.
    json_lines(&audit_admin(
        Some(&verifier),
        &["checkpoint", "--out", &path("checkpoint.json")],
    ));
    let declared = json_lines(&audit_admin(
        Some(&maintainer),
        &["declare-recovery-pending", "--incident-code", "incident_7"],
    ));
    assert_eq!(declared[0]["status"], "recovery_pending");
    let check = json_lines(&audit_admin(Some(&verifier), &["verify", "--recovery"]));
    assert_eq!(check[0]["recovery_mode"], Value::Bool(true));
    assert_eq!(check[0]["outcome"], "ok");
    std::fs::create_dir(dir.join("recovery")).expect("dir");
    let recovered = json_lines(&audit_admin(
        Some(&maintainer),
        &[
            "export",
            "--identity-chain",
            "--recovery",
            "--dir",
            &path("recovery"),
            "--checkpoint",
            &path("checkpoint.json"),
        ],
    ));
    assert_eq!(recovered[0]["checkpoint"]["comparison"], "match");
    assert_eq!(recovered[0]["complete"], Value::Bool(true));
    let refused = audit_admin(Some(&verifier), &["verify"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("requires recovery"));

    // The epoch: preview the restored state, then start it with the
    // out-of-band record; a record that disagrees is refused.
    let preview = json_lines(&audit_admin(
        Some(&maintainer),
        &[
            "begin-recovery-epoch",
            "--checkpoint",
            &path("checkpoint.json"),
            "--preview",
        ],
    ));
    let preview = &preview[0];
    assert_eq!(preview["status"], "preview");
    assert_eq!(preview["checkpoint_classification"], "match");
    let text = |value: &Value| match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let expect = |lost_upper: &str| {
        audit_admin(
            Some(&maintainer),
            &[
                "begin-recovery-epoch",
                "--checkpoint",
                &path("checkpoint.json"),
                "--expect-old-epoch",
                &text(&preview["old_epoch"]),
                "--expect-head-seq",
                &text(&preview["restored_head_seq"]),
                "--expect-head-chain",
                &text(&preview["restored_head_chain"]),
                "--expect-lost-upper",
                lost_upper,
            ],
        )
    };
    let wrong = (preview["lost_upper_seq"].as_i64().expect("upper") + 1).to_string();
    let refused = expect(&wrong);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("expectation_mismatch"));
    let missing = audit_admin(Some(&maintainer), &["begin-recovery-epoch"]);
    assert_eq!(missing.status.code(), Some(2), "expectations are required");
    let started = json_lines(&expect(&text(&preview["lost_upper_seq"])));
    assert_eq!(started[0]["new_epoch"], 2);
    assert_eq!(started[0]["checkpoint_classification"], "match");
    assert_eq!(
        started[0]["restored_head_chain"],
        preview["restored_head_chain"]
    );
    let closed = audit_admin(Some(&cast.reader.url), &["investigate"]);
    assert_eq!(closed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&closed.stderr).contains("access_reapply_pending"));
    let access = json_lines(&audit_admin(
        Some(&cast.admin.url),
        &["record-access-reapplied"],
    ));
    assert_eq!(access[0]["status"], "recorded");
    let retention = json_lines(&audit_admin(
        Some(&maintainer),
        &["confirm-retention-reapplied"],
    ));
    assert_eq!(retention[0]["status"], "recorded");
    let lines = json_lines(&audit_admin(
        Some(&cast.reader.url),
        &["investigate", "--page-size", "2"],
    ));
    assert_eq!(lines.len(), 2);
    let status = json_lines(&audit_admin(Some(&verifier), &["status"]));
    assert_eq!(status[0]["recovery_epoch"], 2);
    assert_eq!(status[0]["access_reapply_pending"], Value::Bool(false));
    db.assert_store_conforms().await;
}
