//! In-memory Source registry for bootstrap and contract tests, without persistence semantics.

use std::collections::BTreeMap;

use search_core::id::SourceId;
use search_core::source::DiscoverableSource;

use crate::ports::{BoxFuture, SourceRegistryPort};

#[derive(Debug, Clone, Default)]
pub struct InMemorySourceRegistry {
    sources: BTreeMap<SourceId, DiscoverableSource>,
}

impl InMemorySourceRegistry {
    pub fn insert(&mut self, source: DiscoverableSource) -> Option<DiscoverableSource> {
        self.sources.insert(source.source_id, source)
    }
}

impl SourceRegistryPort for InMemorySourceRegistry {
    fn get_source<'a>(&'a self, source_id: SourceId) -> BoxFuture<'a, Option<DiscoverableSource>> {
        Box::pin(async move { Ok(self.sources.get(&source_id).cloned()) })
    }

    fn list_sources<'a>(&'a self) -> BoxFuture<'a, Vec<DiscoverableSource>> {
        Box::pin(async move { Ok(self.sources.values().cloned().collect()) })
    }
}
