//! Reference-only source checks through Document's existing current authority.
//! These separate provider reads do not form an atomic transaction with Work.
use crate::{OrganizationProfile, SyntheticIdentityAdapter};
use document_application::{
    ApplicationError, DocumentHistoryRepository, DocumentHistoryService,
    DocumentRevisionDetailQuery, DocumentRevisionReadRepository, DocumentRevisionReadService,
    InvocationKind, VerifiedActorContext, VersionPurpose, VersionRequest,
};
use document_domain::{DocumentId, DocumentVersionId};
use document_server::identity::{PoCIdentityProfile, StaticPoCIdentityAdapter};
use std::{sync::Arc, time::Duration};
use work_application::{AgentSourcePort, EvidenceSourcePort, EvidenceSourcePurpose, WorkFuture};
use work_domain::{AgentDispatchContext, EvidenceSource, RevisionRef, VerifiedActor, WorkError};

const AUTHORIZATION_BUDGET: Duration = Duration::from_secs(5);

pub struct DocumentEvidenceSource<R> {
    repository: Arc<R>,
}
impl<R> DocumentEvidenceSource<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

/// A separate fixed provider principal intersects the actual requester's rights.
/// This is a Document Application adapter, not an MCP client or model execution.
pub struct DocumentAgentSource<R> {
    repository: Arc<R>,
}
impl<R> DocumentAgentSource<R> {
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

#[derive(Clone, Copy)]
enum SourceIdentity {
    Requester(VerifiedActor),
    DocumentAgent,
}
impl SourceIdentity {
    fn current_context(self) -> Result<VerifiedActorContext, WorkError> {
        let ctx = match self {
            Self::Requester(actor) => {
                let profile = OrganizationProfile::parse(actor.principal_id())
                    .map_err(|_| WorkError::DependencyUnavailable)?;
                let ctx = SyntheticIdentityAdapter::new(profile)
                    .current_context()
                    .map_err(|_| WorkError::DependencyUnavailable)?;
                if ctx.principal().identity_provider() != "organization-synthetic"
                    || ctx.principal().principal_id() != actor.principal_id()
                    || ctx.invocation_kind() != InvocationKind::HumanInteractive
                    || ctx.service_executor().is_some()
                {
                    return Err(WorkError::DependencyUnavailable);
                }
                ctx
            }
            Self::DocumentAgent => {
                let ctx = StaticPoCIdentityAdapter::new(PoCIdentityProfile::Agent)
                    .current_context()
                    .map_err(|_| WorkError::DependencyUnavailable)?;
                verify_provider_context(&ctx)?;
                ctx
            }
        };
        ctx.ensure_current()
            .map_err(|_| WorkError::DependencyUnavailable)?;
        Ok(ctx)
    }
}

fn verify_provider_context(ctx: &VerifiedActorContext) -> Result<(), WorkError> {
    if ctx.ensure_current().is_err()
        || ctx.principal().identity_provider() != "poc"
        || ctx.principal().principal_id() != "poc-agent"
        || ctx.invocation_kind() != InvocationKind::Agent
        || ctx.service_executor().is_some()
    {
        return Err(WorkError::DependencyUnavailable);
    }
    Ok(())
}

impl<R: DocumentRevisionReadRepository + DocumentHistoryRepository> EvidenceSourcePort
    for DocumentEvidenceSource<R>
{
    fn authorize(
        &self,
        actor: VerifiedActor,
        source: EvidenceSource,
        purpose: EvidenceSourcePurpose,
    ) -> WorkFuture<'_, ()> {
        Box::pin(async move {
            tokio::time::timeout(
                AUTHORIZATION_BUDGET,
                authorize_source(
                    self.repository.clone(),
                    SourceIdentity::Requester(actor),
                    &source,
                    purpose,
                ),
            )
            .await
            .map_err(|_| WorkError::DependencyUnavailable)?
        })
    }
}

impl<R: DocumentRevisionReadRepository + DocumentHistoryRepository> AgentSourcePort
    for DocumentAgentSource<R>
{
    fn authorize(
        &self,
        context: AgentDispatchContext,
        reference: RevisionRef,
        remaining: Duration,
    ) -> WorkFuture<'_, ()> {
        Box::pin(async move {
            context.validate_scope()?;
            let source = &context
                .evidence
                .iter()
                .find(|record| record.id == reference.id && record.revision == reference.revision)
                .ok_or(WorkError::EvidenceNotFound)?
                .source;
            if remaining.is_zero() {
                return Err(WorkError::DependencyUnavailable);
            }
            // The repository checks current Work/cancel/context before and after
            // this single exact-source operation, outside every Work row lock.
            tokio::time::timeout(remaining.min(AUTHORIZATION_BUDGET), async {
                authorize_source(
                    self.repository.clone(),
                    SourceIdentity::Requester(context.execution.requested_by),
                    source,
                    EvidenceSourcePurpose::ReadHistory,
                )
                .await?;
                authorize_source(
                    self.repository.clone(),
                    SourceIdentity::DocumentAgent,
                    source,
                    EvidenceSourcePurpose::ReadHistory,
                )
                .await
            })
            .await
            .map_err(|_| WorkError::DependencyUnavailable)?
        })
    }
}

/// Reuse the existing exact source resolution for both independently verified
/// identities. Refresh/verify principal and invocation before each service use.
async fn authorize_source<R: DocumentRevisionReadRepository + DocumentHistoryRepository>(
    repository: Arc<R>,
    identity: SourceIdentity,
    source: &EvidenceSource,
    purpose: EvidenceSourcePurpose,
) -> Result<(), WorkError> {
    if source.source_ref.provider_id != "document"
        || source.authoritative_locator.kind != "contentItem"
    {
        return Err(match purpose {
            EvidenceSourcePurpose::RegisterPublished => WorkError::ValidationFailed,
            EvidenceSourcePurpose::ReadHistory => WorkError::EvidenceNotFound,
        });
    }
    let ctx = identity.current_context()?;
    let revision = DocumentRevisionReadService::new(repository.clone())
        .get_document_revision(
            &ctx,
            DocumentRevisionDetailQuery {
                document_id: DocumentId::from_uuid(source.source_ref.resource_id),
                revision_id: source.source_ref.revision_id,
            },
        )
        .await
        .map_err(source_error)?;
    if revision.summary.revision_id != source.source_ref.revision_id
        || revision.summary.document_version_id.as_uuid() != source.source_ref.version_id
    {
        return Err(WorkError::EvidenceNotFound);
    }
    let ctx = identity.current_context()?;
    let files = DocumentHistoryService::new(repository)
        .list_version_files(
            &ctx,
            VersionRequest {
                document_id: DocumentId::from_uuid(source.source_ref.resource_id),
                document_version_id: DocumentVersionId::from_uuid(source.source_ref.version_id),
                purpose: match purpose {
                    EvidenceSourcePurpose::RegisterPublished => VersionPurpose::Published,
                    EvidenceSourcePurpose::ReadHistory => VersionPurpose::History,
                },
            },
        )
        .await
        .map_err(source_error)?;
    if !files.iter().any(|file| {
        file.content_item_id == source.authoritative_locator.content_item_id
            && file.representation_id == source.authoritative_locator.representation_id
            && file.role == "AUTHORITATIVE"
    }) {
        return Err(WorkError::EvidenceNotFound);
    }
    // No metadata, body, fragment, comparison verdict or inferred truth leaves this port.
    Ok(())
}

fn source_error(error: ApplicationError) -> WorkError {
    match error {
        ApplicationError::Forbidden
        | ApplicationError::FolderNotFound
        | ApplicationError::DocumentNotFound
        | ApplicationError::DocumentRevisionNotFound
        | ApplicationError::DocumentVersionNotFound
        | ApplicationError::FileObjectNotFound
        | ApplicationError::StaleVersion => WorkError::EvidenceNotFound,
        _ => WorkError::DependencyUnavailable,
    }
}

#[cfg(test)]
mod agent_identity_tests {
    use super::*;
    use document_application::{InvocationKind, VerifiedActorContext};
    use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};

    fn context(provider: &str, principal: &str, kind: InvocationKind) -> VerifiedActorContext {
        VerifiedActorContext::from_trusted_adapter(
            PrincipalRef::new(provider, principal).unwrap(),
            vec![PolicySubject::new(PolicySubjectKind::Principal, provider, principal).unwrap()],
            time::OffsetDateTime::now_utc() + time::Duration::minutes(1),
            kind,
            None,
        )
        .unwrap()
    }

    #[test]
    fn agent_provider_is_fixed_independently_from_the_organization_executor() {
        assert_eq!(
            verify_provider_context(&context("poc", "poc-agent", InvocationKind::Agent)),
            Ok(())
        );
        for ctx in [
            context("organization-synthetic", "agent-01", InvocationKind::Agent),
            context("poc", "poc-human", InvocationKind::Agent),
            context("poc", "poc-agent", InvocationKind::HumanInteractive),
            context("poc", "poc-agent", InvocationKind::Service),
        ] {
            assert_eq!(
                verify_provider_context(&ctx),
                Err(WorkError::DependencyUnavailable)
            );
        }
    }
}
