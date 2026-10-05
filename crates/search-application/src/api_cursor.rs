//! P5-03: RAM-only public cursors.
//!
//! A cursor is a random UUID v4 handle with no payload. Everything it
//! continues is held server-side and bound to the actor (tenant, principal,
//! session, access revision), the request digest, the authoritative visible
//! set stamp, every pinned Source generation and the S1 position. Any
//! mismatch, expiry (absolute 5 minutes, idle 1 minute), reuse or restart
//! is `CursorStale`. A handle is consumed by its first use.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use search_core::id::SourceId;
use search_core::projection::ProjectionGenerationKey;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api_scope::ApiError;
use crate::remote_lease::LeaseClock;
use crate::scoped::{TrustedSearchScope, VisibleSetStamp};

pub const CURSOR_ABSOLUTE: Duration = Duration::from_secs(300);
pub const CURSOR_IDLE: Duration = Duration::from_secs(60);
const MAX_CURSORS: usize = 4_096;

/// The opaque public handle: a UUID v4 and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CursorHandle(Uuid);

impl fmt::Debug for CursorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CursorHandle(<redacted>)")
    }
}

impl CursorHandle {
    /// Accepts only the canonical lowercase hyphenated UUID v4 form.
    pub fn parse(value: &str) -> Option<Self> {
        let uuid = Uuid::parse_str(value).ok()?;
        (uuid.get_version_num() == 4 && uuid.hyphenated().to_string() == value)
            .then_some(Self(uuid))
    }

    pub fn to_wire(self) -> String {
        self.0.hyphenated().to_string()
    }
}

/// What a cursor continues; compared whole on every use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorBinding {
    owner: [u8; 32],
    request: [u8; 32],
    stamp: VisibleSetStamp,
    generations: BTreeMap<SourceId, ProjectionGenerationKey>,
}

/// Digest of the actor fields a cursor is bound to.
pub fn owner_digest(actor: &TrustedSearchScope) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in [
        actor.tenant().as_str().as_bytes(),
        actor.principal().as_str().as_bytes(),
        &actor
            .session()
            .map(|session| *session.as_uuid().as_bytes())
            .unwrap_or_default(),
        &actor.access_revision().get().to_be_bytes(),
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

impl CursorBinding {
    pub fn new(
        actor: &TrustedSearchScope,
        request_digest: [u8; 32],
        stamp: VisibleSetStamp,
        generations: BTreeMap<SourceId, ProjectionGenerationKey>,
    ) -> Self {
        Self {
            owner: owner_digest(actor),
            request: request_digest,
            stamp,
            generations,
        }
    }
}

struct Entry {
    binding: CursorBinding,
    position: usize,
    issued: Instant,
    last_used: Instant,
}

pub struct PublicCursorStore {
    entries: Mutex<BTreeMap<CursorHandle, Entry>>,
    clock: Arc<dyn LeaseClock>,
}

impl fmt::Debug for PublicCursorStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicCursorStore(<ram>)")
    }
}

impl PublicCursorStore {
    pub fn new(clock: Arc<dyn LeaseClock>) -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            clock,
        }
    }

    fn live(&self, entry: &Entry, now: Instant) -> bool {
        now < entry.issued + CURSOR_ABSOLUTE && now < entry.last_used + CURSOR_IDLE
    }

    pub fn issue(&self, binding: CursorBinding, position: usize) -> Result<CursorHandle, ApiError> {
        let now = self.clock.now();
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| ApiError::ServiceUnavailable)?;
        if entries.len() >= MAX_CURSORS {
            entries.retain(|_, entry| {
                now < entry.issued + CURSOR_ABSOLUTE && now < entry.last_used + CURSOR_IDLE
            });
            if entries.len() >= MAX_CURSORS {
                return Err(ApiError::ServiceUnavailable);
            }
        }
        let handle = loop {
            let candidate = CursorHandle(Uuid::new_v4());
            if !entries.contains_key(&candidate) {
                break candidate;
            }
        };
        entries.insert(
            handle,
            Entry {
                binding,
                position,
                issued: now,
                last_used: now,
            },
        );
        Ok(handle)
    }

    /// Consumes the handle; returns its S1 position only for an identical,
    /// unexpired binding.
    pub fn consume(
        &self,
        handle: &CursorHandle,
        binding: &CursorBinding,
    ) -> Result<usize, ApiError> {
        let now = self.clock.now();
        let entry = self
            .entries
            .lock()
            .map_err(|_| ApiError::ServiceUnavailable)?
            .remove(handle)
            .ok_or(ApiError::CursorStale)?;
        if !self.live(&entry, now) || entry.binding != *binding {
            return Err(ApiError::CursorStale);
        }
        Ok(entry.position)
    }

    /// Every cursor of the actor goes, e.g. after an access revision change.
    pub fn invalidate_owner(&self, actor: &TrustedSearchScope) {
        let owner = owner_digest(actor);
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|_, entry| entry.binding.owner != owner);
        }
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
