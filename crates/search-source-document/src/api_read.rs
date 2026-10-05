//! P5-05: Document current reads for the Search API.
//!
//! A Document Source's durable Resource is its current published Version.
//! The locator answers only inside this Source's visible scope and only for
//! a Version that is still current, published and not ended (T10); no
//! history or Document-ID fallback. The reader rechecks the existing
//! Document Read authorization before and after reading the current
//! Version, so a revocation or new Version during the read returns nothing.

use std::sync::Arc;

use search_application::ports::{AccessDecision, BoxFuture, CurrentAccessEvaluatorPort};
use search_application::resource_read::{
    CurrentResourceReadPort, ResourceCoverage, ResourceLocatorPort, ResourceSnapshot,
    VisibleResourceBinding,
};
use search_application::scoped::{AuthorizedSourceScope, TrustedSearchScope};
use search_core::id::{ResourceId, ResourceVersionId, SourceId};
use search_core::resource::ResourceKind;
use sqlx::PgPool;
use uuid::Uuid;

use crate::postgres::DocumentCurrentAccessAdapter;

const CURRENT_VERSION: &str = concat!(
    "SELECT d.document_id, v.title, ",
    "EXISTS (SELECT 1 FROM content_items c WHERE c.document_version_id = v.document_version_id) ",
    "FROM document_versions v JOIN documents d ON d.document_id = v.document_id ",
    "WHERE v.document_version_id = $1 AND d.current_version_id = v.document_version_id ",
    "AND v.lifecycle_state = 'PUBLISHED' ",
    "AND NOT EXISTS (SELECT 1 FROM document_publication_end_operations e ",
    "WHERE e.document_id = d.document_id)"
);

fn unavailable(error: sqlx::Error) -> search_application::SearchError {
    search_application::SearchError::SourceUnavailable(error.to_string())
}

pub struct DocumentApiRead {
    source_id: SourceId,
    pool: PgPool,
    access: Arc<DocumentCurrentAccessAdapter>,
    access_context: String,
}

impl std::fmt::Debug for DocumentApiRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DocumentApiRead(<source-owned>)")
    }
}

impl DocumentApiRead {
    /// `access` is the request's session-scoped Document access adapter and
    /// `access_context` its exact binding key.
    pub fn new(
        source_id: SourceId,
        pool: PgPool,
        access: Arc<DocumentCurrentAccessAdapter>,
        access_context: String,
    ) -> Self {
        Self {
            source_id,
            pool,
            access,
            access_context,
        }
    }

    async fn current(
        &self,
        version: Uuid,
    ) -> Result<Option<(Uuid, Option<String>, bool)>, sqlx::Error> {
        sqlx::query_as(CURRENT_VERSION)
            .bind(version)
            .fetch_optional(&self.pool)
            .await
    }

    async fn allowed(&self, resource: ResourceId) -> Result<bool, search_application::SearchError> {
        Ok(
            CurrentAccessEvaluatorPort::evaluate(&*self.access, resource, &self.access_context)
                .await?
                == AccessDecision::Allowed,
        )
    }
}

impl ResourceLocatorPort for DocumentApiRead {
    fn resolve_visible<'a>(
        &'a self,
        _actor: &'a TrustedSearchScope,
        visible: &'a [AuthorizedSourceScope],
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Vec<VisibleResourceBinding>> {
        Box::pin(async move {
            let Some(scope) = visible
                .iter()
                .find(|scope| scope.source_id() == self.source_id)
            else {
                return Ok(vec![]);
            };
            let current = self
                .current(resource_id.as_uuid())
                .await
                .map_err(unavailable)?;
            Ok(current
                .map(|(document, _, _)| {
                    VisibleResourceBinding::new(scope.clone(), resource_id, document.to_string())
                })
                .into_iter()
                .collect())
        })
    }
}

impl CurrentResourceReadPort for DocumentApiRead {
    fn read_current<'a>(
        &'a self,
        _actor: &'a TrustedSearchScope,
        binding: &'a VisibleResourceBinding,
    ) -> BoxFuture<'a, Option<ResourceSnapshot>> {
        Box::pin(async move {
            let resource = binding.resource_id();
            if binding.scope().source_id() != self.source_id || !self.allowed(resource).await? {
                return Ok(None);
            }
            let Some((document, title, has_parts)) = self
                .current(resource.as_uuid())
                .await
                .map_err(unavailable)?
            else {
                return Ok(None);
            };
            // The parent Document is still the one located, and Read still holds.
            if document.to_string() != binding.locator() || !self.allowed(resource).await? {
                return Ok(None);
            }
            Ok(Some(ResourceSnapshot {
                resource_id: resource,
                source_id: self.source_id,
                resource_type: ResourceKind::Knowledge,
                resource_version: Some(ResourceVersionId::from_uuid(resource.as_uuid())),
                title,
                coverage: if has_parts {
                    ResourceCoverage::BodyUnknown
                } else {
                    ResourceCoverage::TitleAndPermittedMetadata
                },
            }))
        })
    }
}
