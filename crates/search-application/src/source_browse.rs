//! P5-05: the visible Source catalog page.
//!
//! Items come only from the actor's complete visible catalog and expose
//! connected capabilities only: no endpoint, authority, retention or
//! credential. Without an authoritative visible-set stamp there is no
//! cursor: the whole catalog must fit one bounded response, otherwise the
//! page is a generic dependency failure rather than a truncated 200. With a
//! stamp, pages continue by a RAM cursor bound to the stamp, so a visibility
//! change between pages is `CursorStale`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use search_core::id::SourceId;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoveryMode, EnumerationSemantics};
use sha2::{Digest, Sha256};

use crate::api_cursor::{CursorBinding, CursorHandle, PublicCursorStore};
use crate::api_scope::{ApiError, SearchOperationContext};
use crate::remote_disclosure::{
    Disclosable, DisclosedFields, DisclosureOwner, TransientDisclosure,
};
use crate::remote_lease::LeaseClock;
use crate::scoped::VisibleCatalogSnapshot;
use crate::source_registration::SourceKind;

/// A conservative serialized size per Source item for the one-response bound.
pub const SOURCE_ITEM_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceCoverageKind {
    TitleAndPermittedMetadata,
    BodySearchWithPerItemCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItemView {
    pub source_id: SourceId,
    pub source_type: SourceKind,
    pub resource_types: Vec<ResourceKind>,
    pub discovery_modes: Vec<DiscoveryMode>,
    pub enumeration_semantics: EnumerationSemantics,
    pub coverage: Vec<SourceCoverageKind>,
}

pub struct SourceView {
    items: Vec<SourceItemView>,
    next_cursor: Option<CursorHandle>,
}

impl fmt::Debug for SourceView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceView(<transient>)")
    }
}

impl SourceView {
    pub fn items(&self) -> &[SourceItemView] {
        &self.items
    }
    pub fn next_cursor(&self) -> Option<CursorHandle> {
        self.next_cursor
    }
}

impl Disclosable for SourceView {
    fn disclosed_fields(&self) -> DisclosedFields {
        DisclosedFields {
            resources: vec![],
            resource_sources: vec![],
            claims: vec![],
        }
    }
}

pub struct SourceBrowseService<'a> {
    cursors: &'a PublicCursorStore,
    clock: Arc<dyn LeaseClock>,
    disclosure_ttl: Duration,
    max_response_bytes: usize,
}

impl fmt::Debug for SourceBrowseService<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceBrowseService(<wired>)")
    }
}

impl<'a> SourceBrowseService<'a> {
    pub fn new(
        cursors: &'a PublicCursorStore,
        clock: Arc<dyn LeaseClock>,
        disclosure_ttl: Duration,
        max_response_bytes: usize,
    ) -> Self {
        Self {
            cursors,
            clock,
            disclosure_ttl,
            max_response_bytes,
        }
    }

    pub async fn page(
        &self,
        context: &SearchOperationContext,
        snapshot: &VisibleCatalogSnapshot,
        page_size: usize,
        cursor: Option<CursorHandle>,
    ) -> Result<TransientDisclosure<SourceView>, ApiError> {
        if page_size == 0 || page_size > 100 {
            return Err(ApiError::ValidationFailed);
        }
        context.check_live()?;
        let mut items: Vec<SourceItemView> = snapshot
            .entries()
            .iter()
            .map(|entry| {
                let source = entry.discoverable_source();
                let body = source
                    .discovery_modes
                    .contains(&DiscoveryMode::LocalContentSearch);
                SourceItemView {
                    source_id: entry.scope().source_id(),
                    source_type: entry.registration().kind(),
                    resource_types: source.resource_types,
                    discovery_modes: source.discovery_modes,
                    enumeration_semantics: source.enumeration_semantics,
                    coverage: if body {
                        vec![
                            SourceCoverageKind::TitleAndPermittedMetadata,
                            SourceCoverageKind::BodySearchWithPerItemCoverage,
                        ]
                    } else {
                        vec![SourceCoverageKind::TitleAndPermittedMetadata]
                    },
                }
            })
            .collect();
        items.sort_by_key(|item| item.source_id);
        let binding = snapshot.continuation_stamp().map(|stamp| {
            let mut hasher = Sha256::new();
            hasher.update(b"search-sources:v1");
            hasher.update((page_size as u64).to_be_bytes());
            CursorBinding::new(
                context.actor(),
                hasher.finalize().into(),
                stamp.clone(),
                BTreeMap::new(),
            )
        });
        let (page, next_cursor) = match &binding {
            None => {
                if cursor.is_some() {
                    return Err(ApiError::CursorStale);
                }
                // No continuation authority: all or nothing in one response.
                if items.len() > page_size
                    || items.len().saturating_mul(SOURCE_ITEM_BYTES) > self.max_response_bytes
                {
                    return Err(ApiError::DependencyUnavailable);
                }
                (items, None)
            }
            Some(binding) => {
                let start = match &cursor {
                    Some(handle) => self.cursors.consume(handle, binding)?,
                    None => 0,
                };
                let end = (start + page_size).min(items.len());
                let next = (end < items.len())
                    .then(|| self.cursors.issue(binding.clone(), end))
                    .transpose()?;
                (items[start.min(items.len())..end].to_vec(), next)
            }
        };
        context.check_live()?;
        Ok(TransientDisclosure::new(
            SourceView {
                items: page,
                next_cursor,
            },
            DisclosureOwner::new(
                context.actor().clone(),
                snapshot
                    .entries()
                    .iter()
                    .map(|entry| entry.scope().clone())
                    .collect(),
            ),
            self.disclosure_ttl,
            self.clock.clone(),
            true,
        ))
    }
}
