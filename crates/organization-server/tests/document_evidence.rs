//! Pure provider-boundary tests: no database, listener, storage, or network.
use document_application::{
    DocumentHistoryEntry, DocumentHistoryRepository, DocumentRevisionDetail,
    DocumentRevisionDetailQuery, DocumentRevisionPageQuery, DocumentRevisionReadRepository,
    DocumentRevisionSummary, HistoryPageQuery, InvocationKind, Page, RepositoryError,
    RevisionComparisonAuditRequest, VerifiedActorContext, VersionDetail, VersionFileSummary,
    VersionPageQuery, VersionPurpose, VersionRequest, VersionSummary,
};
use document_domain::DocumentVersionId;
use organization_server::DocumentEvidenceSource;
use std::{future::pending, sync::Arc, time::Duration};
use time::OffsetDateTime;
use uuid::Uuid;
use work_application::{EvidenceSourcePort, EvidenceSourcePurpose};
use work_domain::{AuthoritativeLocator, EvidenceSource, SourceRef, VerifiedActor, WorkError};

struct ReadRepository {
    actor: VerifiedActor,
    source: EvidenceSource,
    published: bool,
    returned_revision: Uuid,
    returned_version: Uuid,
    files: Vec<VersionFileSummary>,
    revision_error: Option<RepositoryError>,
    file_error: Option<RepositoryError>,
    stalled: bool,
}

impl ReadRepository {
    fn fixture(actor: VerifiedActor) -> Self {
        let source = EvidenceSource {
            source_ref: SourceRef {
                provider_id: "document".into(),
                resource_id: Uuid::from_u128(1),
                revision_id: Uuid::from_u128(2),
                version_id: Uuid::from_u128(3),
            },
            authoritative_locator: AuthoritativeLocator {
                kind: "contentItem".into(),
                content_item_id: Uuid::from_u128(4),
                representation_id: Uuid::from_u128(5),
            },
        };
        Self {
            actor,
            returned_revision: source.source_ref.revision_id,
            returned_version: source.source_ref.version_id,
            files: vec![VersionFileSummary {
                content_item_id: source.authoritative_locator.content_item_id,
                representation_id: source.authoritative_locator.representation_id,
                logical_path: "synthetic-input".into(),
                ordinal: 0,
                role: "AUTHORITATIVE".into(),
                safe_display_name: "synthetic-input.txt".into(),
                media_type: "text/plain".into(),
                size_bytes: 1,
            }],
            source,
            published: true,
            revision_error: None,
            file_error: None,
            stalled: false,
        }
    }

    fn authorize_actor(&self, ctx: &VerifiedActorContext) -> Result<(), RepositoryError> {
        if ctx.ensure_current().is_err()
            || ctx.principal().identity_provider() != "organization-synthetic"
            || ctx.principal().principal_id() != self.actor.principal_id()
            || ctx.invocation_kind() != InvocationKind::HumanInteractive
            || ctx.service_executor().is_some()
        {
            return Err(RepositoryError::Forbidden);
        }
        Ok(())
    }
}

impl DocumentRevisionReadRepository for ReadRepository {
    async fn get_document_revision(
        &self,
        ctx: &VerifiedActorContext,
        query: DocumentRevisionDetailQuery,
    ) -> Result<DocumentRevisionDetail, RepositoryError> {
        self.authorize_actor(ctx)?;
        if self.stalled {
            pending::<()>().await;
        }
        if let Some(error) = &self.revision_error {
            return Err(error.clone());
        }
        if query.document_id.as_uuid() != self.source.source_ref.resource_id
            || query.revision_id != self.source.source_ref.revision_id
        {
            return Err(RepositoryError::DocumentRevisionNotFound);
        }
        Ok(DocumentRevisionDetail {
            summary: DocumentRevisionSummary {
                revision_id: self.returned_revision,
                document_version_id: DocumentVersionId::from_uuid(self.returned_version),
                major_no: 1,
                minor_no: 0,
                metadata_snapshot_status: "AVAILABLE".into(),
                source_kind: "synthetic".into(),
                created_at: OffsetDateTime::UNIX_EPOCH,
            },
            metadata_snapshot: Some(serde_json::json!({"title":"not copied to Work"})),
            actor: None,
            reason: None,
        })
    }

    async fn list_document_revisions(
        &self,
        _: &VerifiedActorContext,
        _: DocumentRevisionPageQuery,
    ) -> Result<Page<DocumentRevisionSummary>, RepositoryError> {
        panic!("evidence must resolve the exact revision, never fall back to a list")
    }

    async fn authorize_and_audit_revision_comparison(
        &self,
        _: &VerifiedActorContext,
        _: RevisionComparisonAuditRequest,
    ) -> Result<Uuid, RepositoryError> {
        panic!("evidence authorization does not run comparison or infer truth")
    }
}

impl DocumentHistoryRepository for ReadRepository {
    async fn list_version_files(
        &self,
        ctx: &VerifiedActorContext,
        request: VersionRequest,
    ) -> Result<Vec<VersionFileSummary>, RepositoryError> {
        self.authorize_actor(ctx)?;
        if let Some(error) = &self.file_error {
            return Err(error.clone());
        }
        if request.document_id.as_uuid() != self.source.source_ref.resource_id
            || request.document_version_id.as_uuid() != self.source.source_ref.version_id
            || request.purpose == VersionPurpose::Authoring
            || (request.purpose == VersionPurpose::Published && !self.published)
        {
            return Err(RepositoryError::DocumentVersionNotFound);
        }
        Ok(self.files.clone())
    }

    async fn list_document_versions(
        &self,
        _: &VerifiedActorContext,
        _: VersionPageQuery,
    ) -> Result<Page<VersionSummary>, RepositoryError> {
        panic!("evidence must not fall back to another version")
    }

    async fn list_document_history(
        &self,
        _: &VerifiedActorContext,
        _: HistoryPageQuery,
    ) -> Result<Page<DocumentHistoryEntry>, RepositoryError> {
        panic!("evidence needs no general history disclosure")
    }

    async fn get_document_version(
        &self,
        _: &VerifiedActorContext,
        _: VersionRequest,
    ) -> Result<VersionDetail, RepositoryError> {
        panic!("evidence needs no version metadata disclosure")
    }
}

#[tokio::test]
async fn published_authoritative_source_accepts_each_actual_human_actor() {
    for actor in [VerifiedActor::Sales01, VerifiedActor::Office01] {
        let repository = ReadRepository::fixture(actor);
        let source = repository.source.clone();
        let adapter = DocumentEvidenceSource::new(Arc::new(repository));
        assert_eq!(
            adapter
                .authorize(actor, source, EvidenceSourcePurpose::RegisterPublished)
                .await,
            Ok(())
        );
    }
}

#[tokio::test]
async fn caller_cannot_borrow_the_other_profile_provider_authority() {
    let repository = ReadRepository::fixture(VerifiedActor::Sales01);
    let source = repository.source.clone();
    let adapter = DocumentEvidenceSource::new(Arc::new(repository));
    assert_eq!(
        adapter
            .authorize(
                VerifiedActor::Office01,
                source,
                EvidenceSourcePurpose::ReadHistory
            )
            .await,
        Err(WorkError::EvidenceNotFound)
    );
}

#[tokio::test]
async fn retained_pinned_version_uses_history_but_cannot_be_newly_registered() {
    let mut repository = ReadRepository::fixture(VerifiedActor::Office01);
    repository.published = false;
    let source = repository.source.clone();
    let adapter = DocumentEvidenceSource::new(Arc::new(repository));
    assert_eq!(
        adapter
            .authorize(
                VerifiedActor::Office01,
                source.clone(),
                EvidenceSourcePurpose::ReadHistory
            )
            .await,
        Ok(())
    );
    assert_eq!(
        adapter
            .authorize(
                VerifiedActor::Office01,
                source,
                EvidenceSourcePurpose::RegisterPublished
            )
            .await,
        Err(WorkError::EvidenceNotFound)
    );
}

#[tokio::test]
async fn document_revision_and_version_must_all_match_without_fallback() {
    for field in ["document", "revision", "version"] {
        let repository = ReadRepository::fixture(VerifiedActor::Sales01);
        let mut source = repository.source.clone();
        match field {
            "document" => source.source_ref.resource_id = Uuid::from_u128(100),
            "revision" => source.source_ref.revision_id = Uuid::from_u128(100),
            "version" => source.source_ref.version_id = Uuid::from_u128(100),
            _ => unreachable!(),
        }
        let adapter = DocumentEvidenceSource::new(Arc::new(repository));
        assert_eq!(
            adapter
                .authorize(
                    VerifiedActor::Sales01,
                    source,
                    EvidenceSourcePurpose::RegisterPublished
                )
                .await,
            Err(WorkError::EvidenceNotFound),
            "mismatched {field}"
        );
    }
}

#[tokio::test]
async fn inconsistent_returned_revision_or_version_is_rejected() {
    for change_revision in [true, false] {
        let mut repository = ReadRepository::fixture(VerifiedActor::Sales01);
        if change_revision {
            repository.returned_revision = Uuid::from_u128(100);
        } else {
            repository.returned_version = Uuid::from_u128(100);
        }
        let source = repository.source.clone();
        let adapter = DocumentEvidenceSource::new(Arc::new(repository));
        assert_eq!(
            adapter
                .authorize(
                    VerifiedActor::Sales01,
                    source,
                    EvidenceSourcePurpose::ReadHistory
                )
                .await,
            Err(WorkError::EvidenceNotFound)
        );
    }
}

#[tokio::test]
async fn locator_requires_exact_content_item_representation_and_authoritative_role() {
    for mismatch in ["item", "representation", "role", "empty"] {
        let mut repository = ReadRepository::fixture(VerifiedActor::Sales01);
        let source = repository.source.clone();
        match mismatch {
            "item" => repository.files[0].content_item_id = Uuid::from_u128(100),
            "representation" => repository.files[0].representation_id = Uuid::from_u128(100),
            "role" => repository.files[0].role = "DERIVED".into(),
            "empty" => repository.files.clear(),
            _ => unreachable!(),
        }
        let adapter = DocumentEvidenceSource::new(Arc::new(repository));
        assert_eq!(
            adapter
                .authorize(
                    VerifiedActor::Sales01,
                    source,
                    EvidenceSourcePurpose::ReadHistory
                )
                .await,
            Err(WorkError::EvidenceNotFound),
            "mismatched {mismatch}"
        );
    }
}

#[tokio::test]
async fn unsupported_provider_or_locator_has_no_fallback() {
    for change_provider in [true, false] {
        let repository = ReadRepository::fixture(VerifiedActor::Sales01);
        let mut source = repository.source.clone();
        if change_provider {
            source.source_ref.provider_id = "search".into();
        } else {
            source.authoritative_locator.kind = "file".into();
        }
        let adapter = DocumentEvidenceSource::new(Arc::new(repository));
        assert_eq!(
            adapter
                .authorize(
                    VerifiedActor::Sales01,
                    source.clone(),
                    EvidenceSourcePurpose::RegisterPublished
                )
                .await,
            Err(WorkError::ValidationFailed)
        );
        assert_eq!(
            adapter
                .authorize(
                    VerifiedActor::Sales01,
                    source,
                    EvidenceSourcePurpose::ReadHistory
                )
                .await,
            Err(WorkError::EvidenceNotFound)
        );
    }
}

#[tokio::test]
async fn current_denial_and_missing_source_are_indistinguishable() {
    for error in [
        RepositoryError::Forbidden,
        RepositoryError::DocumentNotFound,
        RepositoryError::DocumentRevisionNotFound,
        RepositoryError::DocumentVersionNotFound,
        RepositoryError::FileObjectNotFound,
        RepositoryError::StaleVersion,
    ] {
        for fail_files in [true, false] {
            let mut repository = ReadRepository::fixture(VerifiedActor::Office01);
            if fail_files {
                repository.file_error = Some(error.clone());
            } else {
                repository.revision_error = Some(error.clone());
            }
            let source = repository.source.clone();
            let adapter = DocumentEvidenceSource::new(Arc::new(repository));
            assert_eq!(
                adapter
                    .authorize(
                        VerifiedActor::Office01,
                        source,
                        EvidenceSourcePurpose::ReadHistory
                    )
                    .await,
                Err(WorkError::EvidenceNotFound)
            );
        }
    }
}

#[tokio::test]
async fn unknown_provider_failure_is_unavailable_without_internal_metadata() {
    for error in [
        RepositoryError::Unavailable,
        RepositoryError::IntegrityViolation,
        RepositoryError::Internal("private database detail".into()),
    ] {
        for fail_files in [true, false] {
            let mut repository = ReadRepository::fixture(VerifiedActor::Office01);
            if fail_files {
                repository.file_error = Some(error.clone());
            } else {
                repository.revision_error = Some(error.clone());
            }
            let source = repository.source.clone();
            let adapter = DocumentEvidenceSource::new(Arc::new(repository));
            assert_eq!(
                adapter
                    .authorize(
                        VerifiedActor::Office01,
                        source,
                        EvidenceSourcePurpose::ReadHistory
                    )
                    .await,
                Err(WorkError::DependencyUnavailable)
            );
        }
    }
}

#[tokio::test]
async fn stalled_provider_is_bounded_and_never_authorizes() {
    let mut repository = ReadRepository::fixture(VerifiedActor::Sales01);
    repository.stalled = true;
    let source = repository.source.clone();
    let adapter = DocumentEvidenceSource::new(Arc::new(repository));
    let result = tokio::time::timeout(
        Duration::from_secs(6),
        adapter.authorize(
            VerifiedActor::Sales01,
            source,
            EvidenceSourcePurpose::ReadHistory,
        ),
    )
    .await
    .expect("provider authorization must have its own deadline");
    assert_eq!(result, Err(WorkError::DependencyUnavailable));
}
