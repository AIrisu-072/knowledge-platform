//! `audit-admin` end to end: operator commands, 0600 files, refusal of
//! privileged sessions and URL redaction in errors.

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
    assert_eq!(manifest[0]["checkpoint"]["comparison"], "ahead");
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

    // Usage errors exit 2.
    assert_eq!(audit_admin(None, &["status"]).status.code(), Some(2));
    assert_eq!(
        audit_admin(Some(&verifier), &["nope"]).status.code(),
        Some(2)
    );
    db.assert_store_conforms().await;
}
