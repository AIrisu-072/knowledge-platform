//! Pure provider-boundary tests: no database, listener, storage, or network.
use document_application::{
    DocumentHistoryEntry, DocumentHistoryRepository, DocumentRevisionDetail,
    DocumentRevisionDetailQuery, DocumentRevisionPageQuery, DocumentRevisionReadRepository,
    DocumentRevisionSummary, HistoryPageQuery, InvocationKind, Page, RepositoryError,
    RevisionComparisonAuditRequest, VerifiedActorContext, VersionDetail, VersionFileSummary,
    VersionPageQuery, VersionPurpose, VersionRequest, VersionSummary,
};
use document_domain::DocumentVersionId;
use organization_server::{DocumentAgentSource, DocumentEvidenceSource};
use std::{
    future::pending,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use time::OffsetDateTime;
use uuid::Uuid;
use work_application::{AgentSourcePort, EvidenceSourcePort, EvidenceSourcePurpose};
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
    requester_allowed: bool,
    provider_allowed: bool,
    provider_revoked: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<String>>>,
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
            requester_allowed: true,
            provider_allowed: false,
            provider_revoked: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn authorize_actor(&self, ctx: &VerifiedActorContext) -> Result<(), RepositoryError> {
        self.calls.lock().unwrap().push(format!(
            "{}/{}:{}",
            ctx.principal().identity_provider(),
            ctx.principal().principal_id(),
            ctx.invocation_kind().as_str()
        ));
        let requester = self.requester_allowed
            && ctx.principal().identity_provider() == "organization-synthetic"
            && ctx.principal().principal_id() == self.actor.principal_id()
            && ctx.invocation_kind() == InvocationKind::HumanInteractive;
        let provider = self.provider_allowed
            && !self.provider_revoked.load(Ordering::SeqCst)
            && ctx.principal().identity_provider() == "poc"
            && ctx.principal().principal_id() == "poc-agent"
            && ctx.invocation_kind() == InvocationKind::Agent;
        if ctx.ensure_current().is_ok()
            && ctx.service_executor().is_none()
            && (requester || provider)
        {
            Ok(())
        } else {
            Err(RepositoryError::Forbidden)
        }
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

fn agent_context(source: EvidenceSource) -> work_domain::AgentDispatchContext {
    use work_domain::*;
    let mut workflow = Workflow::synthetic(Some(source.source_ref.resource_id));
    let context = |revision| CommandContext {
        operation_id: Uuid::now_v7(),
        expected_revision: revision,
        acting_assignment_id: SALES_ASSIGNMENT_ID,
    };
    workflow
        .apply(
            VerifiedActor::Sales01,
            &Command::RegisterEvidence {
                task_id: SALES_TASK_ID,
                context: context(workflow.source.revision),
                expected_attempt_id: SALES_ATTEMPT_ID,
                source,
                relevant_location: "private location must not become generated output".into(),
            },
            "2026-10-04T13:00:00Z",
        )
        .unwrap();
    let request = Command::RequestAgentExecution {
        task_id: SALES_TASK_ID,
        context: context(workflow.source.revision),
        expected_attempt_id: SALES_ATTEMPT_ID,
        purpose: "private prompt must not become generated output".into(),
        evidence_revision_refs: vec![RevisionRef {
            id: workflow.evidence[0].id,
            revision: 1,
        }],
    };
    let execution = match workflow
        .apply(VerifiedActor::Sales01, &request, "2026-10-04T13:00:00Z")
        .unwrap()
    {
        MutationResult::AgentExecutionRequested { execution, .. } => execution,
        _ => panic!("request must produce its exact durable execution"),
    };
    workflow
        .start_agent_execution(VerifiedActor::Sales01, execution.id, "2026-10-04T13:00:00Z")
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn agent_source_requires_both_actual_requester_and_distinct_document_provider() {
    for (requester_allowed, provider_allowed, expected) in [
        (true, true, Ok(())),
        (false, true, Err(WorkError::EvidenceNotFound)),
        (true, false, Err(WorkError::EvidenceNotFound)),
    ] {
        let mut repository = ReadRepository::fixture(VerifiedActor::Sales01);
        repository.requester_allowed = requester_allowed;
        repository.provider_allowed = provider_allowed;
        let context = agent_context(repository.source.clone());
        let calls = repository.calls.clone();
        let adapter = DocumentAgentSource::new(Arc::new(repository));
        assert_eq!(authorize_agent(&adapter, context).await, expected);
        if requester_allowed && provider_allowed {
            assert_eq!(
                *calls.lock().unwrap(),
                [
                    "organization-synthetic/sales-01:human_interactive",
                    "organization-synthetic/sales-01:human_interactive",
                    "poc/poc-agent:agent",
                    "poc/poc-agent:agent",
                ]
            );
        }
    }
}

#[tokio::test]
async fn agent_source_reauthorizes_disclosure_and_cannot_change_exact_source_or_provider_binding() {
    let mut repository = ReadRepository::fixture(VerifiedActor::Sales01);
    repository.provider_allowed = true;
    let context = agent_context(repository.source.clone());
    let revoked = repository.provider_revoked.clone();
    let adapter = DocumentAgentSource::new(Arc::new(repository));
    assert_eq!(authorize_agent(&adapter, context.clone()).await, Ok(()));
    for field in [
        "resource",
        "revision",
        "version",
        "item",
        "representation",
        "executor",
        "provider",
        "invocation",
    ] {
        let mut changed = context.clone();
        match field {
            "resource" => changed.evidence[0].source.source_ref.resource_id = Uuid::from_u128(999),
            "revision" => changed.evidence[0].source.source_ref.revision_id = Uuid::from_u128(999),
            "version" => changed.evidence[0].source.source_ref.version_id = Uuid::from_u128(999),
            "item" => {
                changed.evidence[0]
                    .source
                    .authoritative_locator
                    .content_item_id = Uuid::from_u128(999)
            }
            "representation" => {
                changed.evidence[0]
                    .source
                    .authoritative_locator
                    .representation_id = Uuid::from_u128(999)
            }
            "executor" => changed.execution.executed_by = "poc/poc-agent".into(),
            "provider" => {
                changed.execution.provider_principal_bindings[0].principal_id =
                    "organization-synthetic/agent-01".into()
            }
            "invocation" => {
                changed.execution.provider_principal_bindings[0].invocation_kind =
                    "human_interactive".into()
            }
            _ => unreachable!(),
        }
        assert!(
            authorize_agent(&adapter, changed).await.is_err(),
            "changed {field}"
        );
    }
    // A successful prior use grants no cached provider authority for disclosure.
    revoked.store(true, Ordering::SeqCst);
    assert_eq!(
        authorize_agent(&adapter, context).await,
        Err(WorkError::EvidenceNotFound)
    );
}

async fn authorize_agent(
    adapter: &DocumentAgentSource<ReadRepository>,
    context: work_domain::AgentDispatchContext,
) -> Result<(), WorkError> {
    let reference = context.execution.evidence_revision_refs[0].clone();
    adapter
        .authorize(context, reference, Duration::from_secs(5))
        .await
}

#[tokio::test]
async fn agent_source_cannot_expand_selection_or_exceed_remaining_authorization_budget() {
    let mut repository = ReadRepository::fixture(VerifiedActor::Sales01);
    repository.provider_allowed = true;
    let calls = repository.calls.clone();
    let context = agent_context(repository.source.clone());
    let adapter = DocumentAgentSource::new(Arc::new(repository));
    assert!(
        adapter
            .authorize(
                context.clone(),
                work_domain::RevisionRef {
                    id: Uuid::now_v7(),
                    revision: 1
                },
                Duration::from_secs(5)
            )
            .await
            .is_err()
    );
    assert_eq!(
        adapter
            .authorize(
                context.clone(),
                context.execution.evidence_revision_refs[0].clone(),
                Duration::ZERO
            )
            .await,
        Err(WorkError::DependencyUnavailable)
    );
    assert!(calls.lock().unwrap().is_empty());
}
