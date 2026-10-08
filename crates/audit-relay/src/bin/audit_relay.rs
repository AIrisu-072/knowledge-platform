//! `audit-relay`: Document audit outbox → Audit Store (design §6, §12).
//!
//! ```text
//! audit-relay migrate                      AUDIT_RELAY_MIGRATE_DATABASE_URL
//! audit-relay run                          AUDIT_SOURCE_DATABASE_URL + AUDIT_STORE_DATABASE_URL
//! audit-relay health [--forecast] [--reconcile]
//! audit-relay reconcile [--repair]
//! audit-relay replay --event-id <uuid>
//! ```
//!
//! Output is JSON on stdout; errors are codes on stderr. URLs and
//! credentials are never printed. `run` uses the relay service's Store login
//! (ingest + relay_control + reconciler); `health`, `reconcile` and `replay`
//! are meant for the operator's Store login (relay_control + reconciler):
//! `replay` and `reconcile --repair` refuse a Store login holding
//! `audit_store_ingest`. `health --forecast` needs the worker's source login
//! (the forecast projects staged content).

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use audit_relay::config::{MIGRATE_URL, RunConfig, SOURCE_URL, STORE_URL};
use audit_relay::health::{HealthOptions, health};
use audit_relay::reconcile::Reconciler;
use audit_relay::relay::{connect_checked, run};
use audit_relay::replay::replay;
use audit_relay::session::{Side, connect, refuse_service_store_login};
use serde_json::json;
use tokio::sync::watch;
use uuid::Uuid;

const USAGE: &str = "usage: audit-relay <migrate | run | health [--forecast] [--reconcile] | \
                     reconcile [--repair] | replay --event-id <uuid>>";

/// Bound on one Store call outside `run` (reconcile, replay, health).
const ADMIN_TIMEOUT: Duration = Duration::from_secs(8);

fn env(name: &'static str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn print(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

async fn shutdown_signal(sender: watch::Sender<bool>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(term) => term,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                let _ = sender.send(true);
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    let _ = sender.send(true);
}

async fn command(args: &[String]) -> Result<(), String> {
    let flags: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match args.first().map(String::as_str) {
        Some("migrate") if flags.is_empty() => {
            let url = env(MIGRATE_URL)?;
            let pool = connect(Side::Source, &url, 2, Duration::from_secs(30))
                .await
                .map_err(|e| e.to_string())?;
            audit_relay::migrate(&pool)
                .await
                .map_err(|e| e.to_string())?;
            print(&json!({"migrated": true, "ledger": audit_relay::MIGRATION_LEDGER}));
            Ok(())
        }
        Some("run") if flags.is_empty() => {
            let config = RunConfig::from_env().map_err(|e| e.to_string())?;
            let (sender, receiver) = watch::channel(false);
            tokio::spawn(shutdown_signal(sender));
            let summary = run(&config, receiver).await.map_err(|e| e.to_string())?;
            print(&json!({
                "cycles": summary.cycles,
                "claimed": summary.claimed,
                "settled": summary.settled,
                "lost": summary.lost,
                "reaped": summary.reaped,
                "outages": summary.outages,
            }));
            Ok(())
        }
        Some("health")
            if flags
                .iter()
                .all(|f| matches!(*f, "--forecast" | "--reconcile")) =>
        {
            let options = HealthOptions {
                forecast: flags.contains(&"--forecast"),
                reconcile: flags.contains(&"--reconcile"),
            };
            let connections =
                connect_checked(&env(SOURCE_URL)?, &env(STORE_URL)?, ADMIN_TIMEOUT, false)
                    .await
                    .map_err(|e| e.to_string())?;
            let report = health(&connections.source, Arc::new(connections.store), options)
                .await
                .map_err(|e| e.to_string())?;
            print(&report);
            Ok(())
        }
        Some("reconcile") if flags.is_empty() || flags == ["--repair"] => {
            let repair = flags == ["--repair"];
            let connections =
                connect_checked(&env(SOURCE_URL)?, &env(STORE_URL)?, ADMIN_TIMEOUT, true)
                    .await
                    .map_err(|e| e.to_string())?;
            if repair {
                refuse_service_store_login(&connections.store_pool)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            let report = Reconciler::new(connections.source, Arc::new(connections.store))
                .run(repair)
                .await
                .map_err(|e| e.to_string())?;
            print(&serde_json::to_value(&report).map_err(|e| e.to_string())?);
            Ok(())
        }
        Some("replay") if flags.len() == 2 && flags[0] == "--event-id" => {
            let event_id =
                Uuid::parse_str(flags[1]).map_err(|_| "invalid --event-id".to_owned())?;
            let connections =
                connect_checked(&env(SOURCE_URL)?, &env(STORE_URL)?, ADMIN_TIMEOUT, true)
                    .await
                    .map_err(|e| e.to_string())?;
            refuse_service_store_login(&connections.store_pool)
                .await
                .map_err(|e| e.to_string())?;
            let outcome = replay(&connections.source, &connections.store, event_id)
                .await
                .map_err(|e| e.to_string())?;
            print(&serde_json::to_value(&outcome).map_err(|e| e.to_string())?);
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match command(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) if message == USAGE => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(message) => {
            eprintln!("audit-relay: {message}");
            ExitCode::FAILURE
        }
    }
}
