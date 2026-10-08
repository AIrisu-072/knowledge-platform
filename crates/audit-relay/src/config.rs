//! `audit-relay run` configuration from the environment.
//!
//! | variable | default |
//! |---|---|
//! | `AUDIT_SOURCE_DATABASE_URL` | required (worker login, Document DB) |
//! | `AUDIT_STORE_DATABASE_URL` | required (ingest + relay_control login) |
//! | `AUDIT_RELAY_BATCH_SIZE` | 32 |
//! | `AUDIT_RELAY_MAX_IN_FLIGHT` | 4 |
//! | `AUDIT_RELAY_LEASE_MS` | 30000 |
//! | `AUDIT_RELAY_RENEW_MS` | 9000 |
//! | `AUDIT_RELAY_MAX_PROCESSING_MS` | 900000 |
//! | `AUDIT_RELAY_DRAIN_MS` | 30000 |
//! | `AUDIT_RELAY_POLL_MS` | 250 |
//! | `AUDIT_RELAY_REAP_BATCH` | 32 |
//! | `AUDIT_RELAY_INGEST_TIMEOUT_MS` | lease / 3 − 1000 |
//! | `AUDIT_RELAY_BREAKER_INITIAL_MS` | 1000 |
//! | `AUDIT_RELAY_BREAKER_MAX_MS` | 60000 |
//! | `AUDIT_RELAY_PROGRESS_MS` | 10000 (minimum spacing of progress lines) |
//!
//! Capacity: the runner claims at most `MAX_IN_FLIGHT` rows per cycle (one
//! while the breaker is half-open), waits for them, then sleeps the poll
//! interval, so one process delivers about
//! `MAX_IN_FLIGHT / (poll + Store round trip)` events per second (about 15/s
//! with the defaults). Raise `AUDIT_RELAY_MAX_IN_FLIGHT` (runner bound 8),
//! lower the poll interval or run more relay processes for a higher
//! sustained rate; `health` shows the backlog as `pending` and
//! `oldest_pending_age_seconds`.

use std::fmt;
use std::time::Duration;

use outbox_delivery::DeliveryConfig;

use crate::breaker::BreakerConfig;
use crate::handler::HandlerConfig;
use crate::monitor::MonitorConfig;
use crate::session::redact;
use crate::source::RelayPolicy;

pub const SOURCE_URL: &str = "AUDIT_SOURCE_DATABASE_URL";
pub const STORE_URL: &str = "AUDIT_STORE_DATABASE_URL";
pub const MIGRATE_URL: &str = "AUDIT_RELAY_MIGRATE_DATABASE_URL";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{0} must be a positive integer")]
    NotANumber(&'static str),
    #[error("the delivery configuration is outside the runner bounds")]
    Delivery,
    #[error("AUDIT_RELAY_INGEST_TIMEOUT_MS must be positive and below a third of the lease")]
    IngestTimeout,
    #[error("the breaker cooldowns must satisfy 0 < initial <= max")]
    Breaker,
}

/// Everything `run` needs. `Debug` redacts the URLs.
#[derive(Clone)]
pub struct RunConfig {
    pub source_url: String,
    pub store_url: String,
    pub delivery: DeliveryConfig,
    pub handler: HandlerConfig,
    pub breaker: BreakerConfig,
    pub policy: RelayPolicy,
    pub monitor: MonitorConfig,
}

impl fmt::Debug for RunConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunConfig")
            .field("source_url", &redact(&self.source_url))
            .field("store_url", &redact(&self.store_url))
            .field("delivery", &self.delivery)
            .field("handler", &self.handler)
            .field("breaker", &self.breaker)
            .field("policy", &self.policy)
            .field("monitor", &self.monitor)
            .finish()
    }
}

fn number(
    lookup: &dyn Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<u64, ConfigError> {
    match lookup(name) {
        None => Ok(default),
        Some(text) => text
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or(ConfigError::NotANumber(name)),
    }
}

fn required(
    lookup: &dyn Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    lookup(name)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing(name))
}

impl RunConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&|name| std::env::var(name).ok())
    }

    /// Builds and validates the configuration from a variable lookup.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let source_url = required(lookup, SOURCE_URL)?;
        let store_url = required(lookup, STORE_URL)?;
        let ms = |name, default| number(lookup, name, default).map(Duration::from_millis);
        let count = |name, default| {
            number(lookup, name, default)
                .and_then(|n| u32::try_from(n).map_err(|_| ConfigError::NotANumber(name)))
        };
        let lease = ms("AUDIT_RELAY_LEASE_MS", 30_000)?;
        let delivery = DeliveryConfig {
            batch_size: count("AUDIT_RELAY_BATCH_SIZE", 32)?,
            max_in_flight: count("AUDIT_RELAY_MAX_IN_FLIGHT", 4)?,
            lease_duration: lease,
            renew_interval: ms("AUDIT_RELAY_RENEW_MS", 9_000)?,
            max_processing: ms("AUDIT_RELAY_MAX_PROCESSING_MS", 900_000)?,
            drain_timeout: ms("AUDIT_RELAY_DRAIN_MS", 30_000)?,
            poll_interval: ms("AUDIT_RELAY_POLL_MS", 250)?,
            reap_batch: count("AUDIT_RELAY_REAP_BATCH", 32)?,
        };
        delivery.validate().map_err(|_| ConfigError::Delivery)?;
        let default_ingest = (lease / 3)
            .saturating_sub(Duration::from_secs(1))
            .max(Duration::from_millis(100));
        let ingest_timeout = match lookup("AUDIT_RELAY_INGEST_TIMEOUT_MS") {
            None => default_ingest,
            Some(_) => ms("AUDIT_RELAY_INGEST_TIMEOUT_MS", 0)?,
        };
        if ingest_timeout.is_zero() || ingest_timeout >= lease / 3 {
            return Err(ConfigError::IngestTimeout);
        }
        let breaker = BreakerConfig {
            initial_cooldown: ms("AUDIT_RELAY_BREAKER_INITIAL_MS", 1_000)?,
            max_cooldown: ms("AUDIT_RELAY_BREAKER_MAX_MS", 60_000)?,
            probe_timeout: ingest_timeout,
            closed_claims: delivery.batch_size,
        };
        if breaker.initial_cooldown > breaker.max_cooldown {
            return Err(ConfigError::Breaker);
        }
        let progress_interval = ms("AUDIT_RELAY_PROGRESS_MS", 10_000)?;
        let defaults = MonitorConfig::default();
        let monitor = MonitorConfig {
            // Sample at least as often as lines may be printed.
            tick: defaults.tick.min(progress_interval),
            progress_interval,
            ..defaults
        };
        Ok(Self {
            source_url,
            store_url,
            delivery,
            handler: HandlerConfig {
                ingest_timeout,
                control_timeout: ingest_timeout,
                control_attempts: 3,
            },
            breaker,
            policy: RelayPolicy::default(),
            monitor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    const URLS: [(&str, &str); 2] = [
        (SOURCE_URL, "postgres://worker:secret@db/document"),
        (STORE_URL, "postgres://relay:secret@db/audit_store"),
    ];

    #[test]
    fn defaults_follow_design_6_3_and_validate() {
        let config = RunConfig::from_lookup(&lookup(&URLS)).expect("defaults");
        assert_eq!(config.delivery.batch_size, 32);
        assert_eq!(config.delivery.max_in_flight, 4);
        assert_eq!(config.delivery.lease_duration, Duration::from_secs(30));
        assert_eq!(config.delivery.poll_interval, Duration::from_millis(250));
        assert!(config.handler.ingest_timeout < config.delivery.lease_duration / 3);
        assert_eq!(config.policy, RelayPolicy::default());
        assert_eq!(config.monitor, MonitorConfig::default());
        let mut pairs = URLS.to_vec();
        pairs.push(("AUDIT_RELAY_PROGRESS_MS", "200"));
        let quick = RunConfig::from_lookup(&lookup(&pairs)).expect("progress");
        assert_eq!(quick.monitor.progress_interval, Duration::from_millis(200));
        assert_eq!(quick.monitor.tick, Duration::from_millis(200));
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("secret"), "{rendered}");
    }

    #[test]
    fn invalid_values_are_refused() {
        assert_eq!(
            RunConfig::from_lookup(&lookup(&URLS[..1])).err(),
            Some(ConfigError::Missing(STORE_URL))
        );
        let mut pairs = URLS.to_vec();
        pairs.push(("AUDIT_RELAY_BATCH_SIZE", "64"));
        assert_eq!(
            RunConfig::from_lookup(&lookup(&pairs)).err(),
            Some(ConfigError::Delivery)
        );
        let mut pairs = URLS.to_vec();
        pairs.push(("AUDIT_RELAY_INGEST_TIMEOUT_MS", "10000"));
        assert_eq!(
            RunConfig::from_lookup(&lookup(&pairs)).err(),
            Some(ConfigError::IngestTimeout)
        );
        let mut pairs = URLS.to_vec();
        pairs.push(("AUDIT_RELAY_POLL_MS", "soon"));
        assert_eq!(
            RunConfig::from_lookup(&lookup(&pairs)).err(),
            Some(ConfigError::NotANumber("AUDIT_RELAY_POLL_MS"))
        );
    }
}
