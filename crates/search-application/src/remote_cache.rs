//! P4-08: expiry cache for `CACHE_WITH_EXPIRY` Sources.
//!
//! Entries are keyed by tenant, Source, principal, the actor's access
//! revision, the Source registration and visibility revisions and the
//! normalized operation, and expire at `min(provider TTL, registration
//! ceiling, actor deadline)`. A hit is raw provider input only: it is observed
//! and verified again under the new evaluation and sealed into a new
//! generation, never reused as an old generation. Other retention modes and
//! durable conversion are refused.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use search_core::id::SourceId;
use search_core::source::RetentionMode;

use crate::SearchError;
use crate::remote::{RemoteOperation, RemoteResponseInput, TrustedRemoteContext};
use crate::remote_lease::{LeaseClock, RemoteOwner, lease_unavailable};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CacheKey {
    tenant: String,
    source: SourceId,
    principal: String,
    access_revision: u64,
    registration_revision: u64,
    visibility_revision: u64,
    operation: String,
}

struct CacheEntry {
    input: RemoteResponseInput,
    deadline: Instant,
}

fn operation_key(operation: &RemoteOperation) -> String {
    match operation {
        RemoteOperation::Enumerate { cursor } => format!(
            "enumerate\u{1f}{}",
            cursor.as_ref().map(|c| c.as_str()).unwrap_or("")
        ),
        RemoteOperation::Query { input } => format!(
            "query\u{1f}{}\u{1f}{}\u{1f}{:?}",
            input.text(),
            input.window(),
            input
                .facets()
                .iter()
                .map(|f| (&f.facet, &f.expected))
                .collect::<Vec<_>>()
        ),
        RemoteOperation::Lookup { native_id } => format!("lookup\u{1f}{}", native_id.as_str()),
        RemoteOperation::Live { input } => format!(
            "live\u{1f}{}\u{1f}{}",
            input.query_input().map(|q| q.text()).unwrap_or(""),
            input.native_id().map(|id| id.as_str()).unwrap_or("")
        ),
    }
}

fn key(context: &TrustedRemoteContext, operation: &RemoteOperation) -> CacheKey {
    let actor = context.binding().actor();
    let scope = context.source_scope();
    CacheKey {
        tenant: actor.tenant().as_str().into(),
        source: scope.source_id(),
        principal: actor.principal().as_str().into(),
        access_revision: actor.access_revision().get(),
        registration_revision: scope.registration_revision().get(),
        visibility_revision: scope.visibility_revision().get(),
        operation: operation_key(operation),
    }
}

pub struct RemoteResultCache {
    entries: BTreeMap<CacheKey, CacheEntry>,
    ceiling: Duration,
    max_entries: usize,
    clock: Arc<dyn LeaseClock>,
}

impl fmt::Debug for RemoteResultCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteResultCache(<lease-owned>)")
    }
}

impl RemoteResultCache {
    pub fn new(ceiling: Duration, max_entries: usize, clock: Arc<dyn LeaseClock>) -> Self {
        Self {
            entries: BTreeMap::new(),
            ceiling,
            max_entries,
            clock,
        }
    }

    fn admitted(context: &TrustedRemoteContext) -> Result<(), SearchError> {
        if context.registration().retention_mode() != RetentionMode::CacheWithExpiry
            || !context.binding().actor().is_live()
        {
            return Err(lease_unavailable());
        }
        Ok(())
    }

    /// Raw input for a fresh observation under `context`, or nothing.
    pub fn get(
        &mut self,
        context: &TrustedRemoteContext,
        operation: &RemoteOperation,
        now: Instant,
    ) -> Result<Option<RemoteResponseInput>, SearchError> {
        Self::admitted(context)?;
        let key = key(context, operation);
        match self.entries.get(&key) {
            Some(entry) if now < entry.deadline => Ok(Some(entry.input.clone())),
            Some(_) => {
                self.entries.remove(&key);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    pub fn put(
        &mut self,
        context: &TrustedRemoteContext,
        operation: &RemoteOperation,
        input: RemoteResponseInput,
        provider_expiry: Instant,
    ) -> Result<(), SearchError> {
        Self::admitted(context)?;
        let now = self.clock.now();
        let actor_deadline = context.binding().actor().deadline();
        let deadline = provider_expiry.min(now + self.ceiling).min(actor_deadline);
        if deadline <= now {
            return Ok(());
        }
        let key = key(context, operation);
        if !self.entries.contains_key(&key) && self.entries.len() >= self.max_entries {
            // Drop expired entries before refusing.
            self.entries.retain(|_, entry| now < entry.deadline);
            if self.entries.len() >= self.max_entries {
                return Err(lease_unavailable());
            }
        }
        self.entries.insert(key, CacheEntry { input, deadline });
        Ok(())
    }

    /// Every entry of the owner's tenant, Source and principal goes.
    pub fn invalidate_owner(&mut self, owner: &RemoteOwner) {
        let tenant = owner.actor().tenant().as_str();
        let principal = owner.actor().principal().as_str();
        let source = owner.source().source_id();
        self.entries.retain(|key, _| {
            !(key.tenant == tenant && key.principal == principal && key.source == source)
        });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
