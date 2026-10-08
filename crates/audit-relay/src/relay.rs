//! Assembly of the relay: `outbox_delivery::DeliveryRunner` (unchanged) over
//! [`RelayOutboxStore`], [`AuditDeliveryHandler`] and [`BreakerAdmission`],
//! and the [`Monitor`] that `run` keeps beside it (circuit-breaker report for
//! `health`, progress lines on stderr).

use std::sync::Arc;
use std::time::Duration;

use audit_core::{AuditStore, OutageCode};
use audit_store_postgres::{PostgresAuditStore, SessionError};
use outbox_delivery::runner::{DeliveryRunner, RunSummary};
use outbox_delivery::{DeliveryConfig, DeliveryError};
use sqlx::PgPool;
use tokio::sync::watch;

use crate::breaker::{Breaker, BreakerAdmission, BreakerConfig};
use crate::config::RunConfig;
use crate::handler::{AuditDeliveryHandler, HandlerConfig, Projector};
use crate::ledger::DeliveryLedger;
use crate::monitor::{Monitor, Progress, RuntimeReporter};
use crate::session::{
    Side, StartupError, connect, refuse_privileged_source, refuse_same_database, require_posture,
};
use crate::source::{RelayOutboxStore, RelayPolicy};
use crate::store::{RelayStore, UnreachableStore};

pub type Runner = DeliveryRunner<RelayOutboxStore, AuditDeliveryHandler, BreakerAdmission>;

/// A ready runner and the breaker and outcome counters it shares with its
/// handler.
pub struct Relay {
    pub runner: Runner,
    pub breaker: Arc<Breaker>,
    pub ledger: Arc<DeliveryLedger>,
    pub progress: Arc<Progress>,
}

/// The parts of a relay; tests substitute the Store client. The Store login
/// is the relay service's (ingest + relay_control + reconciler).
pub struct RelayParts {
    pub source: PgPool,
    pub store: Arc<dyn AuditStore>,
    pub delivery: DeliveryConfig,
    pub handler: HandlerConfig,
    pub breaker: BreakerConfig,
    pub policy: RelayPolicy,
    pub projector: Option<Projector>,
}

impl Relay {
    pub fn assemble(parts: RelayParts) -> Result<Self, DeliveryError> {
        let ledger = Arc::new(DeliveryLedger::default());
        let breaker = Arc::new(Breaker::new(parts.breaker));
        let progress = Arc::new(Progress::default());
        let outbox = RelayOutboxStore::new(parts.source.clone(), parts.policy, ledger.clone());
        let mut handler = AuditDeliveryHandler::new(
            parts.store.clone(),
            parts.source.clone(),
            ledger.clone(),
            breaker.clone(),
            parts.handler,
        )
        .with_progress(progress.clone());
        if let Some(projector) = parts.projector {
            handler = handler.with_projector(projector);
        }
        let admission = BreakerAdmission::new(breaker.clone(), parts.store, parts.source);
        let runner = DeliveryRunner::new(
            Arc::new(outbox),
            Arc::new(handler),
            Arc::new(admission),
            parts.delivery,
        )?;
        Ok(Self {
            runner,
            breaker,
            ledger,
            progress,
        })
    }
}

/// Why `run` could not start or stopped.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Startup(#[from] StartupError),
    #[error("delivery stopped: {0}")]
    Delivery(#[from] DeliveryError),
}

fn store_session(error: SessionError) -> StartupError {
    match error {
        SessionError::Privileged | SessionError::NotOwnerMember => {
            StartupError::Privileged(Side::Store)
        }
        SessionError::UrlOptions => StartupError::UrlOptions { side: Side::Store },
        SessionError::EnvironmentOptions => StartupError::EnvironmentOptions,
        SessionError::UrlInvalid => StartupError::UrlInvalid { side: Side::Store },
        SessionError::SynchronousCommitOff => {
            StartupError::SynchronousCommitOff { side: Side::Store }
        }
        SessionError::Database(error) => StartupError::from_sqlx(Side::Store, &error),
    }
}

/// Startup checks shared by run, reconcile and replay (design §3, §10.1):
/// non-privileged source session, clean `audit_relay` posture, distinct
/// databases.
pub async fn startup_checks(source: &PgPool, store: &PgPool) -> Result<(), StartupError> {
    refuse_privileged_source(source).await?;
    refuse_same_database(source, store).await?;
    require_posture(source).await
}

/// Connected and checked pools plus the Store client (`run`: the relay
/// service login; `reconcile`, `replay`, `health`: the operator's login).
pub struct Connections {
    pub source: PgPool,
    pub store_pool: PgPool,
    pub store: PostgresAuditStore,
}

/// Connects both databases and runs the startup checks. `posture` false
/// skips the posture refusal (health reports violations instead; the CLI's
/// health uses [`connect_for_health`]).
pub async fn connect_checked(
    source_url: &str,
    store_url: &str,
    ingest_timeout: Duration,
    posture: bool,
) -> Result<Connections, StartupError> {
    let source = connect(Side::Source, source_url, 8, Duration::from_secs(10)).await?;
    let store_pool = connect(Side::Store, store_url, 8, Duration::from_secs(10)).await?;
    refuse_privileged_source(&source).await?;
    refuse_same_database(&source, &store_pool).await?;
    if posture {
        require_posture(&source).await?;
    }
    let store = PostgresAuditStore::new(store_pool.clone(), ingest_timeout)
        .await
        .map_err(store_session)?;
    Ok(Connections {
        source,
        store_pool,
        store,
    })
}

/// The outage a failed Store connection stands for, when it is one: the
/// connection could not be made (transport, timeouts) or the server refused
/// it for availability (SQLSTATE classes 08, 53, 57 and 55000, e.g. a
/// database with `ALLOW_CONNECTIONS false`). Configuration refusals
/// (authentication, unknown database, URL or session checks) are not
/// outages and stay errors.
pub fn store_connect_outage(error: &StartupError) -> Option<OutageCode> {
    match error {
        StartupError::Unavailable {
            side: Side::Store,
            code,
        } => Some(match code.as_str() {
            "transport" => OutageCode::Transport,
            "connect_timeout" | "pool_timeout" => OutageCode::Timeout,
            _ => OutageCode::Other,
        }),
        StartupError::Database {
            side: Side::Store,
            sqlstate,
        } if sqlstate == "55000"
            || ["08", "53", "57"]
                .iter()
                .any(|class| sqlstate.starts_with(class)) =>
        {
            Some(audit_core::classify_sqlstate(sqlstate))
        }
        _ => None,
    }
}

/// `audit-relay health`: the source pool and a Store client. The source is
/// required (every count comes from it) and must not be privileged. A Store
/// that cannot be reached ([`store_connect_outage`]) becomes an
/// [`UnreachableStore`], so the report shows `stored.available = false` and
/// the outage code instead of failing; the same-database check then has
/// nothing to compare and is skipped (health writes nothing).
pub async fn connect_for_health(
    source_url: &str,
    store_url: &str,
    timeout: Duration,
) -> Result<(PgPool, Arc<dyn RelayStore>), StartupError> {
    let source = connect(Side::Source, source_url, 8, Duration::from_secs(10)).await?;
    refuse_privileged_source(&source).await?;
    let store: Arc<dyn RelayStore> =
        match connect(Side::Store, store_url, 8, Duration::from_secs(10)).await {
            Ok(store_pool) => {
                refuse_same_database(&source, &store_pool).await?;
                Arc::new(
                    PostgresAuditStore::new(store_pool, timeout)
                        .await
                        .map_err(store_session)?,
                )
            }
            Err(error) => match store_connect_outage(&error) {
                Some(code) => Arc::new(UnreachableStore::new(code)),
                None => return Err(error),
            },
        };
    Ok((source, store))
}

/// `audit-relay run`: deliver until `shutdown` turns true. The monitor
/// reports the breaker for `health` and prints progress lines on stderr
/// while the runner works, then removes its report after the drain.
pub async fn run(
    config: &RunConfig,
    shutdown: watch::Receiver<bool>,
) -> Result<RunSummary, RunError> {
    let connections = connect_checked(
        &config.source_url,
        &config.store_url,
        config.handler.ingest_timeout,
        true,
    )
    .await?;
    let source = connections.source.clone();
    let relay = Relay::assemble(RelayParts {
        source: connections.source,
        store: Arc::new(connections.store),
        delivery: config.delivery,
        handler: config.handler,
        breaker: config.breaker,
        policy: config.policy,
        projector: None,
    })?;
    let monitor = Monitor::new(
        relay.breaker.clone(),
        relay.progress.clone(),
        config.monitor,
    )
    .with_reporter(RuntimeReporter::new(source));
    let (stop, stopped) = watch::channel(false);
    let monitor = tokio::spawn(monitor.run(stopped));
    let result = relay.runner.run_until_shutdown(shutdown).await;
    let _ = stop.send(true);
    let _ = monitor.await;
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(side: Side, sqlstate: &str) -> StartupError {
        StartupError::Database {
            side,
            sqlstate: sqlstate.to_owned(),
        }
    }

    fn unavailable(side: Side, code: &str) -> StartupError {
        StartupError::Unavailable {
            side,
            code: code.to_owned(),
        }
    }

    #[test]
    fn only_store_availability_failures_are_reported_as_outages() {
        for (error, code) in [
            (unavailable(Side::Store, "transport"), OutageCode::Transport),
            (
                unavailable(Side::Store, "connect_timeout"),
                OutageCode::Timeout,
            ),
            (
                unavailable(Side::Store, "pool_timeout"),
                OutageCode::Timeout,
            ),
            (unavailable(Side::Store, "unclassified"), OutageCode::Other),
            (refused(Side::Store, "55000"), OutageCode::Other),
            (refused(Side::Store, "08006"), OutageCode::Connection),
            (refused(Side::Store, "53300"), OutageCode::Resources),
            (refused(Side::Store, "57P03"), OutageCode::Shutdown),
        ] {
            assert_eq!(store_connect_outage(&error), Some(code), "{error:?}");
        }
        for error in [
            refused(Side::Store, "28P01"),
            refused(Side::Store, "28000"),
            refused(Side::Store, "3D000"),
            refused(Side::Store, "42501"),
            unavailable(Side::Source, "transport"),
            refused(Side::Source, "55000"),
            StartupError::UrlOptions { side: Side::Store },
            StartupError::UrlInvalid { side: Side::Store },
            StartupError::SynchronousCommitOff { side: Side::Store },
            StartupError::Privileged(Side::Store),
            StartupError::SameDatabase,
        ] {
            assert_eq!(store_connect_outage(&error), None, "{error:?}");
        }
    }

    #[tokio::test]
    async fn an_unreachable_store_fails_every_call_with_its_outage() {
        let store = UnreachableStore::new(OutageCode::Transport);
        let outage = Some(OutageCode::Transport);
        assert_eq!(
            store
                .store_status()
                .await
                .err()
                .and_then(|e| e.outage_code()),
            outage
        );
        assert_eq!(
            store
                .lookup_receipts(&[uuid::Uuid::nil()])
                .await
                .err()
                .and_then(|e| e.outage_code()),
            outage
        );
        assert_eq!(
            store
                .lookup_lost_ranges()
                .await
                .err()
                .and_then(|e| e.outage_code()),
            outage
        );
    }
}
