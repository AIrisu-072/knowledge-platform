//! Bounded delivery configuration and deterministic retry policy.

use std::time::Duration;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::model::DeliveryError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryPolicy {
    pub revision: i64,
    pub max_attempts: i32,
    pub lease_min_ms: i64,
    pub lease_max_ms: i64,
    pub backoff_min_ms: i64,
    pub backoff_max_ms: i64,
}

impl Default for DeliveryPolicy {
    fn default() -> Self {
        Self {
            revision: 1,
            max_attempts: 8,
            lease_min_ms: 1_000,
            lease_max_ms: 120_000,
            backoff_min_ms: 1_000,
            backoff_max_ms: 300_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryConfig {
    pub batch_size: u32,
    pub max_in_flight: u32,
    pub lease_duration: Duration,
    pub renew_interval: Duration,
    pub max_processing: Duration,
    pub drain_timeout: Duration,
    pub poll_interval: Duration,
    pub reap_batch: u32,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            batch_size: 32,
            max_in_flight: 8,
            lease_duration: Duration::from_secs(120),
            renew_interval: Duration::from_secs(30),
            max_processing: Duration::from_secs(15 * 60),
            drain_timeout: Duration::from_secs(30),
            poll_interval: Duration::from_secs(1),
            reap_batch: 32,
        }
    }
}

impl DeliveryConfig {
    /// Validate v0 dispatch bounds before any claim or lease operation.
    pub fn validate(&self) -> Result<(), DeliveryError> {
        let lease_min = Duration::from_secs(1);
        let lease_max = Duration::from_secs(120);
        let processing_max = Duration::from_secs(24 * 60 * 60);
        let valid = (1..=32).contains(&self.batch_size)
            && (1..=8).contains(&self.max_in_flight)
            && (1..=32).contains(&self.reap_batch)
            && (lease_min..=lease_max).contains(&self.lease_duration)
            && !self.renew_interval.is_zero()
            && self.renew_interval < self.lease_duration / 3
            && !self.max_processing.is_zero()
            && self.max_processing <= processing_max
            && !self.drain_timeout.is_zero()
            && self.drain_timeout <= self.max_processing
            && !self.poll_interval.is_zero()
            && self.poll_interval <= self.lease_duration;
        if valid {
            Ok(())
        } else {
            Err(DeliveryError::InvalidConfig)
        }
    }
}

/// Exponential seconds plus a stable SHA-256 jitter, capped at five minutes.
/// Attempts at or below zero use the first-attempt delay. The hash input is
/// UUID bytes followed by the positive attempt number in big-endian order.
pub fn retry_delay(event_id: Uuid, attempt: i32) -> Duration {
    const CAP_MS: u64 = 300_000;
    let attempt = attempt.max(1);
    if attempt >= 10 {
        return Duration::from_millis(CAP_MS);
    }

    let base_ms = 1_000_u64 << (attempt - 1);
    let mut hasher = Sha256::new();
    hasher.update(event_id.as_bytes());
    hasher.update(attempt.to_be_bytes());
    let digest = hasher.finalize();
    let modulus = base_ms / 4 + 1;
    let mut jitter_ms = 0_u64;
    for &byte in digest.iter() {
        jitter_ms = (jitter_ms * 256 + u64::from(byte)) % modulus;
    }
    Duration::from_millis(base_ms.saturating_add(jitter_ms).clamp(1_000, CAP_MS))
}

#[cfg(test)]
mod tests {
    use super::{DeliveryConfig, DeliveryPolicy, retry_delay};
    use crate::model::{DeliveryError, ErrorCode};
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn rejects_unbounded_or_unsafe_config() {
        let good = DeliveryConfig::default();
        assert_eq!(good.validate(), Ok(()));
        assert_eq!(DeliveryPolicy::default().max_attempts, 8);
        assert_eq!(DeliveryPolicy::default().lease_max_ms, 120_000);

        let mut bad = good;
        bad.batch_size = 0;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad.batch_size = 33;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad = good;
        bad.max_in_flight = 0;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad.max_in_flight = 9;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad = good;
        bad.reap_batch = 0;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad.reap_batch = 33;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));

        bad = good;
        bad.renew_interval = Duration::from_secs(40);
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad.renew_interval = Duration::ZERO;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad = good;
        bad.lease_duration = Duration::ZERO;
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));
        bad.lease_duration = Duration::from_secs(121);
        assert_eq!(bad.validate(), Err(DeliveryError::InvalidConfig));

        for changed in [
            DeliveryConfig {
                max_processing: Duration::ZERO,
                ..good
            },
            DeliveryConfig {
                drain_timeout: Duration::ZERO,
                ..good
            },
            DeliveryConfig {
                poll_interval: Duration::ZERO,
                ..good
            },
            DeliveryConfig {
                max_processing: Duration::from_secs(24 * 60 * 60 + 1),
                ..good
            },
            DeliveryConfig {
                drain_timeout: Duration::from_secs(24 * 60 * 60 + 1),
                ..good
            },
            DeliveryConfig {
                poll_interval: Duration::from_secs(121),
                ..good
            },
        ] {
            assert_eq!(changed.validate(), Err(DeliveryError::InvalidConfig));
        }
    }

    #[test]
    fn retry_delay_uses_full_sha256_digest_for_exact_vectors() {
        let id = Uuid::from_u128(0x21213141727182818284590452353602);
        // Independently calculated from SHA-256(UUID bytes || signed i32 BE attempt).
        for (attempt, expected_ms) in [(1, 1_055), (2, 2_153), (9, 289_301)] {
            assert_eq!(retry_delay(id, attempt), Duration::from_millis(expected_ms));
        }
    }

    #[test]
    fn retry_delay_is_stable_bounded_and_increases() {
        let id = Uuid::from_u128(0x21213141727182818284590452353602);
        let other = Uuid::from_u128(0x31213141727182818284590452353602);
        let first = retry_delay(id, 1);
        assert_eq!(first, retry_delay(id, 1));
        assert!((Duration::from_secs(1)..=Duration::from_millis(1_250)).contains(&first));
        assert_ne!(first, retry_delay(other, 1));
        let mut previous = first;
        for attempt in 2..=16 {
            let next = retry_delay(id, attempt);
            assert!(next >= previous, "attempt {attempt} reduced retry delay");
            assert!(next <= Duration::from_secs(300));
            previous = next;
        }
        assert_eq!(retry_delay(id, i32::MAX), Duration::from_secs(300));
        assert!(retry_delay(id, 0) >= Duration::from_secs(1));
        assert!(retry_delay(id, i32::MIN) >= Duration::from_secs(1));
    }

    #[test]
    fn error_codes_are_allowlisted() {
        let expected = [
            "unsupported_event",
            "invalid_envelope",
            "source_unavailable",
            "indexing_failed",
            "handler_timeout",
            "delivery_unknown",
            "delivery_unknown_at_limit",
        ];
        assert_eq!(ErrorCode::ALL.map(ErrorCode::as_str), expected);
        for code in ErrorCode::ALL {
            assert_eq!(code.as_str().parse::<ErrorCode>(), Ok(code));
        }
        assert!("payload=password=secret".parse::<ErrorCode>().is_err());
        assert!("unsupported_event\nsecret".parse::<ErrorCode>().is_err());
        assert_eq!(
            DeliveryError::StoreUnknown.to_string(),
            "outbox store result is unknown"
        );
    }
}
