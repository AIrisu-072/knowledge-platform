//! Assembly of the relay: `outbox_delivery::DeliveryRunner` (unchanged) over
//! [`RelayOutboxStore`], [`AuditDeliveryHandler`] and [`BreakerAdmission`].

use std::sync::Arc;
use std::time::Duration;

use audit_core::AuditStore;
use audit_store_postgres::admin::AuditAdmin;
use audit_store_postgres::{PostgresAuditStore, SessionError};
use outbox_delivery::runner::{DeliveryRunner, RunSummary};
use outbox_delivery::{DeliveryConfig, DeliveryError};
use sqlx::PgPool;
use tokio::sync::watch;

use crate::breaker::{Breaker, BreakerAdmission, BreakerConfig};
use crate::config::RunConfig;
use crate::handler::{AuditDeliveryHandler, HandlerConfig, Projector};
use crate::ledger::DeliveryLedger;
use crate::session::{
    Side, StartupError, connect, refuse_privileged_source, refuse_same_database, require_posture,
};
use crate::source::{RelayOutboxStore, RelayPolicy};
use crate::store_admin::{PgStoreAdmin, StoreAdmin};

pub type Runner = DeliveryRunner<RelayOutboxStore, AuditDeliveryHandler, BreakerAdmission>;

/// A ready runner and the breaker it shares with its handler.
pub struct Relay {
    pub runner: Runner,
    pub breaker: Arc<Breaker>,
    pub ledger: Arc<DeliveryLedger>,
}

/// The parts of a relay; tests substitute the Store client or admin.
pub struct RelayParts {
    pub source: PgPool,
    pub store: Arc<dyn AuditStore>,
    pub admin: Arc<dyn StoreAdmin>,
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
        let outbox = RelayOutboxStore::new(parts.source.clone(), parts.policy, ledger.clone());
        let mut handler = AuditDeliveryHandler::new(
            parts.store.clone(),
            parts.admin.clone(),
            parts.source.clone(),
            ledger.clone(),
            breaker.clone(),
            parts.handler,
        );
        if let Some(projector) = parts.projector {
            handler = handler.with_projector(projector);
        }
        let admission =
            BreakerAdmission::new(breaker.clone(), parts.store, parts.admin, parts.source);
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

/// Connected and checked pools plus the Store clients.
pub struct Connections {
    pub source: PgPool,
    pub store_pool: PgPool,
    pub store: PostgresAuditStore,
    pub admin: PgStoreAdmin,
}

/// Connects both databases and runs the startup checks. `posture` is false
/// only for `health`, which reports violations instead of refusing.
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
    let admin = PgStoreAdmin::new(
        AuditAdmin::connect(store_pool.clone())
            .await
            .map_err(store_session)?,
    );
    Ok(Connections {
        source,
        store_pool,
        store,
        admin,
    })
}

/// `audit-relay run`: deliver until `shutdown` turns true.
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
    let relay = Relay::assemble(RelayParts {
        source: connections.source,
        store: Arc::new(connections.store),
        admin: Arc::new(connections.admin),
        delivery: config.delivery,
        handler: config.handler,
        breaker: config.breaker,
        policy: config.policy,
        projector: None,
    })?;
    Ok(relay.runner.run_until_shutdown(shutdown).await?)
}
