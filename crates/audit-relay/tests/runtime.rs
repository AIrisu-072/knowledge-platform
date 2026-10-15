//! What a running relay shows outside its process (design §12): the
//! circuit-breaker state every `audit-relay run` reports for `health`, and
//! its bounded progress lines on stderr, which never carry payload, subject,
//! actor, resource, event id, event type or reason content.

mod support;

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use audit_relay::breaker::Breaker;
use audit_relay::health::{HealthOptions, health};
use audit_relay::monitor::RuntimeReporter;
use audit_relay::session::posture;
use serde_json::{Value, json};
use support::*;

const CONVERGE: Duration = Duration::from_secs(30);

async fn health_report(env: &Env) -> Value {
    health(
        &env.worker.pool,
        Arc::new(env.operator_client().await),
        HealthOptions::default(),
    )
    .await
    .expect("health")
}

fn alarms(report: &Value) -> Vec<String> {
    report["alarms"]
        .as_array()
        .expect("alarms")
        .iter()
        .map(|a| a.as_str().expect("code").to_owned())
        .collect()
}

#[tokio::test]
async fn health_reports_the_circuit_of_each_running_relay() {
    let env = Env::start().await;
    let report = health_report(&env).await;
    assert_eq!(
        report["circuit"],
        json!({
            "state": null, "gate": null, "outage_streak": null, "outages": null,
            "running": 0, "stale": 0, "last_report_age_seconds": null,
        }),
        "no relay runs"
    );

    // An open breaker after a Store outage.
    let breaker = Breaker::new(test_breaker());
    let reporter = RuntimeReporter::new(env.worker.pool.clone());
    breaker.record_outage();
    reporter
        .report(&breaker.snapshot())
        .await
        .expect("the worker reports");
    let report = health_report(&env).await;
    let circuit = &report["circuit"];
    assert_eq!(circuit["state"], "open");
    assert_eq!(circuit["gate"], "unknown");
    assert_eq!(circuit["outage_streak"], 1);
    assert_eq!(circuit["outages"], 1);
    assert_eq!(circuit["running"], 1);
    assert!(circuit["last_report_age_seconds"].as_f64().expect("age") < 60.0);
    assert!(alarms(&report).contains(&"circuit_open".to_owned()));

    // A structured verdict closes it and ends the streak.
    breaker.record_verdict();
    reporter.report(&breaker.snapshot()).await.expect("report");
    let report = health_report(&env).await;
    assert_eq!(
        (
            &report["circuit"]["state"],
            &report["circuit"]["outage_streak"],
            &report["circuit"]["outages"]
        ),
        (&json!("closed"), &json!(0), &json!(1))
    );
    assert!(!alarms(&report).contains(&"circuit_open".to_owned()));

    // A second process, still half-open: the worst state wins.
    let other = RuntimeReporter::new(env.worker.pool.clone());
    other
        .report(&Breaker::new(test_breaker()).snapshot())
        .await
        .expect("report");
    let report = health_report(&env).await;
    assert_eq!(report["circuit"]["state"], "half_open");
    assert_eq!(report["circuit"]["running"], 2);

    // A process that stopped reporting (crashed) is stale, not running.
    env.force(
        "UPDATE audit_relay.relay_runtime SET reported_at = reported_at - interval '5 minutes' \
         WHERE circuit = 'half_open'",
    )
    .await;
    let report = health_report(&env).await;
    assert_eq!(
        (
            &report["circuit"]["state"],
            &report["circuit"]["running"],
            &report["circuit"]["stale"]
        ),
        (&json!("closed"), &json!(1), &json!(1))
    );
    // A clean stop deletes its row; a row silent for a day is pruned by
    // the next report.
    reporter.stop(&breaker.snapshot()).await.expect("stop");
    let report = health_report(&env).await;
    assert_eq!(
        (&report["circuit"]["running"], &report["circuit"]["stale"]),
        (&json!(0), &json!(1))
    );
    env.force("UPDATE audit_relay.relay_runtime SET reported_at = reported_at - interval '2 days'")
        .await;
    reporter.report(&breaker.snapshot()).await.expect("report");
    let report = health_report(&env).await;
    assert_eq!(
        (&report["circuit"]["running"], &report["circuit"]["stale"]),
        (&json!(1), &json!(0))
    );

    // Codes only; only the worker reports; the matrix stays clean.
    let error = sqlx::query(
        "SELECT audit_relay.report_runtime(gen_random_uuid(), 'ajar', 'ok', 0, 0, false)",
    )
    .execute(&env.worker.pool)
    .await
    .expect_err("unknown circuit state");
    assert_eq!(sqlstate(&error), "22023");
    let error = sqlx::query(
        "SELECT audit_relay.report_runtime(gen_random_uuid(), 'open', 'Free Text', 0, 0, false)",
    )
    .execute(&env.worker.pool)
    .await
    .expect_err("not a code");
    assert_eq!(sqlstate(&error), "22023");
    let error = sqlx::query(
        "SELECT audit_relay.report_runtime(gen_random_uuid(), 'open', 'ok', 0, 0, false)",
    )
    .execute(&env.operator.pool)
    .await
    .expect_err("the operator does not run the relay");
    assert_eq!(sqlstate(&error), "42501");
    assert_eq!(posture(&env.worker.pool).await.expect("posture"), vec![]);
}

/// Values of the synthetic event that must never reach the output.
fn forbidden(row: &Staged, reason: &str) -> Vec<String> {
    let mut values = vec![
        row.event_id.to_string(),
        row.resource_id.to_string(),
        row.subject.clone(),
        row.actor_pid.clone(),
        row.event_type.clone(),
        reason.to_owned(),
        "withdrawn".to_owned(),
        "base_inspection_unavailable".to_owned(),
    ];
    values.extend(row.resource_version_id.map(|id| id.to_string()));
    values
}

#[tokio::test]
async fn run_prints_bounded_progress_and_reports_its_circuit() {
    let env = Env::start().await;
    let reason = "synthetic-reason-for-the-progress-test";
    let row = Staged::withdrawn(reason);
    env.insert(&row).await;
    let child = Command::new(env!("CARGO_BIN_EXE_audit-relay"))
        .arg("run")
        .env("AUDIT_SOURCE_DATABASE_URL", &env.worker.url)
        .env("AUDIT_STORE_DATABASE_URL", &env.relay_store.url)
        .env("AUDIT_RELAY_LEASE_MS", "6000")
        .env("AUDIT_RELAY_RENEW_MS", "1000")
        .env("AUDIT_RELAY_POLL_MS", "50")
        .env("AUDIT_RELAY_DRAIN_MS", "3000")
        .env("AUDIT_RELAY_BREAKER_INITIAL_MS", "20")
        .env("AUDIT_RELAY_BREAKER_MAX_MS", "200")
        .env("AUDIT_RELAY_PROGRESS_MS", "200")
        .env_remove("PGOPTIONS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn audit-relay run");
    let pid = child.id().to_string();

    let deadline = tokio::time::Instant::now() + CONVERGE;
    while delivered_count(&env).await < 1 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "run did not deliver"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // The ingest verdict closed the breaker; the process reports it.
    loop {
        let report = health_report(&env).await;
        let circuit = &report["circuit"];
        if circuit["running"] == 1 && circuit["state"] == "closed" {
            assert_eq!(circuit["gate"], "ok");
            assert_eq!(circuit["outage_streak"], 0);
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no circuit report: {circuit}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Idle for a while: no more lines than the startup transitions.
    tokio::time::sleep(Duration::from_millis(1_000)).await;

    let status = Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .expect("send SIGTERM");
    assert!(status.success());
    let output = tokio::task::spawn_blocking(move || child.wait_with_output())
        .await
        .expect("join")
        .expect("wait for audit-relay run");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    println!("audit-relay run stderr:\n{stderr}");
    // A SIGTERM that lands while a runner step is pending ends the run as
    // unknown (the runner never claims a clean stop it cannot prove); the
    // monitor still prints its last line and removes its report.
    const UNKNOWN_STOP: &str = "audit-relay: delivery stopped: outbox store result is unknown";
    match output.status.code() {
        Some(0) => {
            let summary: Value = serde_json::from_str(&stdout).expect("summary JSON");
            assert!(summary["settled"].as_u64().expect("settled") >= 1);
        }
        Some(1) => assert_eq!(stderr.lines().last(), Some(UNKNOWN_STOP), "{stderr}"),
        other => panic!("audit-relay run exited with {other:?}: {stderr}"),
    }

    // Every progress line is bounded: fixed keys, code values.
    let lines: Vec<&str> = stderr.lines().filter(|l| *l != UNKNOWN_STOP).collect();
    assert!(!lines.is_empty(), "progress lines");
    assert!(lines.len() <= 6, "an idle relay is quiet: {stderr}");
    let mut delivered = 0;
    for line in &lines {
        let fields = line
            .strip_prefix("audit-relay: event=")
            .unwrap_or_else(|| panic!("unexpected stderr line: {line}"));
        for field in format!("event={fields}").split(' ') {
            let (key, value) = field.split_once('=').expect("key=value");
            assert!(
                audit_core::kinds::is_code(key) && audit_core::kinds::is_code(value),
                "{line}"
            );
            if key == "delivered" {
                delivered += value.parse::<u64>().expect("count");
            }
        }
    }
    assert_eq!(delivered, 1, "{stderr}");
    for value in forbidden(&row, reason) {
        assert!(!stderr.contains(&value), "{value} in stderr: {stderr}");
        assert!(!stdout.contains(&value), "{value} in stdout: {stdout}");
    }

    // A clean stop removes the report.
    let report = health_report(&env).await;
    assert_eq!(
        (&report["circuit"]["running"], &report["circuit"]["state"]),
        (&json!(0), &Value::Null)
    );
}
