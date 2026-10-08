//! The in-process `DeliveryLedger` (design §6.1).
//!
//! `outbox_delivery::ErrorCode` is a closed set of seven codes, so the handler
//! passes the Store receipt, the Audit-specific detail code and the outage
//! mark to the store implementation through this bounded map keyed by
//! `(event_id, lease_token)`. Losing an entry (process crash, eviction) is
//! safe: an ack without a receipt is refused and a failure without a note is
//! settled with the generic runner code, which consumes the attempt.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use audit_core::IngestReceipt;
use uuid::Uuid;

/// Default bound; the runner keeps at most eight claims in flight.
pub const DEFAULT_CAPACITY: usize = 1024;

/// What the handler learned about one claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The Store accepted the event (stored or duplicate). `store_epoch` is
    /// the Store recovery epoch observed by the admission probe.
    Receipt {
        receipt: IngestReceipt,
        store_epoch: i64,
    },
    /// The claim failed with a bounded detail code.
    Failure(FailureNote),
}

/// A failure detail. `outage` returns the attempt; `streak_countable` marks
/// a residual unexpected error that may count toward `outage_streak`;
/// `relay_hold` marks a hold on the relay side (the Store was not reached):
/// it returns the attempt too, but backs off on the row's own hold count
/// and leaves the Store outage state alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureNote {
    pub code: String,
    pub outage: bool,
    pub streak_countable: bool,
    pub relay_hold: bool,
}

impl FailureNote {
    /// A Store outage (or an unknown Store outcome).
    pub fn outage(code: &str, streak_countable: bool) -> Self {
        Self {
            code: code.to_owned(),
            outage: true,
            streak_countable,
            relay_hold: false,
        }
    }

    /// A relay-side hold (catalog skew, projection or source bookkeeping).
    pub fn relay_hold(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            outage: true,
            streak_countable: false,
            relay_hold: true,
        }
    }

    pub fn verdict(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            outage: false,
            streak_countable: false,
            relay_hold: false,
        }
    }
}

#[derive(Default)]
struct Inner {
    entries: HashMap<(Uuid, Uuid), Note>,
    order: VecDeque<(Uuid, Uuid)>,
}

/// Bounded map from `(event_id, lease_token)` to the handler's [`Note`].
pub struct DeliveryLedger {
    capacity: usize,
    inner: Mutex<Inner>,
}

impl Default for DeliveryLedger {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl DeliveryLedger {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Records (or replaces) the note of one claim, evicting the oldest
    /// entries beyond the capacity.
    pub fn record(&self, event_id: Uuid, lease_token: Uuid, note: Note) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = (event_id, lease_token);
        if inner.entries.insert(key, note).is_none() {
            inner.order.push_back(key);
        }
        while inner.entries.len() > self.capacity {
            match inner.order.pop_front() {
                Some(old) => {
                    inner.entries.remove(&old);
                }
                None => break,
            }
        }
    }

    /// Removes and returns the note of one claim.
    pub fn take(&self, event_id: Uuid, lease_token: Uuid) -> Option<Note> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = (event_id, lease_token);
        let note = inner.entries.remove(&key);
        if note.is_some() {
            inner.order.retain(|entry| *entry != key);
        }
        note
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_core::IngestOutcome;

    fn receipt(seq: i64) -> Note {
        Note::Receipt {
            receipt: IngestReceipt {
                seq,
                envelope_digest: [7; 32],
                outcome: IngestOutcome::Stored,
                adapter_version: 1,
            },
            store_epoch: 1,
        }
    }

    #[test]
    fn keyed_by_event_and_token() {
        let ledger = DeliveryLedger::new(8);
        let (event, token, other) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        ledger.record(event, token, receipt(4));
        assert_eq!(
            ledger.take(event, other),
            None,
            "a stale token reads nothing"
        );
        assert_eq!(ledger.take(event, token), Some(receipt(4)));
        assert_eq!(ledger.take(event, token), None, "taken once");
    }

    #[test]
    fn bounded_eviction_drops_the_oldest() {
        let ledger = DeliveryLedger::new(2);
        for n in 0..3_u128 {
            ledger.record(Uuid::from_u128(n), Uuid::from_u128(100), receipt(1));
        }
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger.take(Uuid::from_u128(0), Uuid::from_u128(100)), None);
        assert!(
            ledger
                .take(Uuid::from_u128(2), Uuid::from_u128(100))
                .is_some()
        );
    }
}
