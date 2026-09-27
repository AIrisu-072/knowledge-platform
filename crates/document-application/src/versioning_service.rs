use std::sync::Arc;

use document_domain::{
    AuditEventId, DocumentId, DocumentVersionId, EventId, LifecycleState, SemanticContentItem,
    VersionManifest,
};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};

use crate::{
    ApplicationError, AuthoritativeDocument, Clock, CreateVersionCommand, DocumentRepository,
    EnsureSemanticInspection, FileStorage, IdGenerator, PreparedManifest,
    RebaseWorkingVersionCommand, RepositoryError, SemanticInspectionExecutor,
    SemanticInspectionRepository, UpdateWorkingVersionCommand, VersionCommandIdentity,
    VersionMutationRecord, VersionOperationKind, VersionOperationResult, VersioningRepository,
};

pub struct DocumentVersionService<I, C, F, E, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    executor: Arc<E>,
    repository: Arc<R>,
}

impl<I, C, F, E, R> DocumentVersionService<I, C, F, E, R>
where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
    E: SemanticInspectionExecutor,
    R: DocumentRepository + VersioningRepository + SemanticInspectionRepository,
{
    pub fn new(
        ids: Arc<I>,
        clock: Arc<C>,
        storage: Arc<F>,
        executor: Arc<E>,
        repository: Arc<R>,
    ) -> Self {
        Self {
            ids,
            clock,
            storage,
            executor,
            repository,
        }
    }

    pub async fn create_version(
        &self,
        command: CreateVersionCommand,
        prepared: PreparedManifest,
    ) -> Result<VersionOperationResult, ApplicationError> {
        let identity = VersionCommandIdentity::new(
            command.operation_id(),
            VersionOperationKind::Create,
            command.document_id(),
            command.target_version_id(),
            command.expected_revision(),
            command.actor().clone(),
            Some(&prepared),
        )?;
        if let Some(result) = self.replay(&identity).await? {
            return Ok(result);
        }
        let current = self
            .load_current(command.document_id(), command.expected_revision())
            .await?;
        let base_digest = self
            .compare_to_base(&current, &prepared.manifest().clone())
            .await?;
        let record = self.record(
            identity,
            Some(prepared),
            current.version().document_version_id(),
            base_digest,
        );
        self.map_mutation_result(
            command.operation_id(),
            command.document_id(),
            command.target_version_id(),
            self.repository.create_version(record).await,
        )
    }

    pub async fn update_working(
        &self,
        command: UpdateWorkingVersionCommand,
        prepared: PreparedManifest,
    ) -> Result<VersionOperationResult, ApplicationError> {
        let identity = VersionCommandIdentity::new(
            command.operation_id(),
            VersionOperationKind::Update,
            command.document_id(),
            command.target_version_id(),
            command.expected_revision(),
            command.actor().clone(),
            Some(&prepared),
        )?;
        if let Some(result) = self.replay(&identity).await? {
            return Ok(result);
        }
        let current = self
            .load_current(command.document_id(), command.expected_revision())
            .await?;
        let working = self
            .load_working(command.document_id(), command.target_version_id())
            .await?;
        if working.version().base_document_version_id()
            != Some(current.version().document_version_id())
        {
            return Err(ApplicationError::Conflict);
        }
        let base_digest = self.compare_to_base(&current, prepared.manifest()).await?;
        let record = self.record(
            identity,
            Some(prepared),
            current.version().document_version_id(),
            base_digest,
        );
        self.map_mutation_result(
            command.operation_id(),
            command.document_id(),
            command.target_version_id(),
            self.repository.update_working(record).await,
        )
    }

    pub async fn rebase_working(
        &self,
        command: RebaseWorkingVersionCommand,
    ) -> Result<VersionOperationResult, ApplicationError> {
        let identity = VersionCommandIdentity::new(
            command.operation_id(),
            VersionOperationKind::Rebase,
            command.document_id(),
            command.target_version_id(),
            command.expected_revision(),
            command.actor().clone(),
            None,
        )?;
        if let Some(result) = self.replay(&identity).await? {
            return Ok(result);
        }
        let current = self
            .load_current(command.document_id(), command.expected_revision())
            .await?;
        let working = self
            .load_working(command.document_id(), command.target_version_id())
            .await?;
        if working.version().base_document_version_id()
            == Some(current.version().document_version_id())
        {
            return Err(ApplicationError::BusinessRule);
        }
        let working_manifest = self.ensure_manifest(&working).await?;
        let base_digest = self.compare_to_base(&current, &working_manifest).await?;
        let record = self.record(
            identity,
            None,
            current.version().document_version_id(),
            base_digest,
        );
        self.map_mutation_result(
            command.operation_id(),
            command.document_id(),
            command.target_version_id(),
            self.repository.rebase_working(record).await,
        )
    }

    async fn replay(
        &self,
        identity: &VersionCommandIdentity,
    ) -> Result<Option<VersionOperationResult>, ApplicationError> {
        let stored = self
            .repository
            .get_version_operation(identity.operation_id())
            .await?;
        match stored {
            Some(stored) if stored.matches_identity(identity) => Ok(Some(stored.result().clone())),
            Some(_) => Err(ApplicationError::Conflict),
            None => Ok(None),
        }
    }

    async fn load_current(
        &self,
        document_id: DocumentId,
        expected_revision: i64,
    ) -> Result<AuthoritativeDocument, ApplicationError> {
        let current = self
            .repository
            .get_authoritative_document(document_id)
            .await?
            .ok_or(ApplicationError::DocumentNotFound)?;
        if current.document().revision() != expected_revision {
            return Err(ApplicationError::Conflict);
        }
        if current.document().current_version_id() != Some(current.version().document_version_id())
            || current.version().lifecycle_state() != LifecycleState::Published
        {
            return Err(ApplicationError::BusinessRule);
        }
        if current.requires_content_classification() {
            return Err(ApplicationError::BusinessRule);
        }
        Ok(current)
    }

    async fn load_working(
        &self,
        document_id: DocumentId,
        version_id: DocumentVersionId,
    ) -> Result<AuthoritativeDocument, ApplicationError> {
        let working = self
            .repository
            .get_version_snapshot(document_id, version_id)
            .await?
            .ok_or(ApplicationError::DocumentVersionNotFound)?;
        if working.version().lifecycle_state() != LifecycleState::Working {
            return Err(ApplicationError::BusinessRule);
        }
        if working.requires_content_classification() {
            return Err(ApplicationError::BusinessRule);
        }
        Ok(working)
    }

    async fn compare_to_base(
        &self,
        current: &AuthoritativeDocument,
        candidate: &VersionManifest,
    ) -> Result<[u8; 32], ApplicationError> {
        let base = self.ensure_manifest(current).await?;
        for old in base.items() {
            if let Some(new) = candidate.items().iter().find(|item| {
                item.logical_path() == old.logical_path() && item.ordinal() == old.ordinal()
            }) {
                if new.format_id() != old.format_id()
                    || new.inspection_profile_id() != old.inspection_profile_id()
                {
                    return Err(ApplicationError::BusinessRule);
                }
            }
        }
        let base_digest = base.identity_digest();
        if base_digest == candidate.identity_digest() {
            return Err(ApplicationError::BusinessRule);
        }
        Ok(base_digest)
    }

    async fn ensure_manifest(
        &self,
        snapshot: &AuthoritativeDocument,
    ) -> Result<VersionManifest, ApplicationError> {
        if snapshot.content_items().is_empty() || snapshot.requires_content_classification() {
            return Err(ApplicationError::BusinessRule);
        }
        let ensure = EnsureSemanticInspection::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        );
        let mut items = Vec::with_capacity(snapshot.content_items().len());
        for item in snapshot.content_items() {
            let inspected = ensure
                .ensure(item.file().file_id(), InspectionProfileVersion::DsiV0)
                .await?;
            let response = inspected.response();
            items.push(SemanticContentItem::new(
                item.logical_path().clone(),
                item.ordinal(),
                format_id(response.detected_format),
                InspectionProfileVersion::DsiV0.as_str(),
                *response.semantic_fingerprint.digest(),
            )?);
        }
        VersionManifest::new(snapshot.version().title().clone(), items).map_err(Into::into)
    }

    fn record(
        &self,
        identity: VersionCommandIdentity,
        prepared: Option<PreparedManifest>,
        base_id: DocumentVersionId,
        base_digest: [u8; 32],
    ) -> VersionMutationRecord {
        VersionMutationRecord::new(
            identity,
            prepared,
            base_id,
            base_digest,
            EventId::from_uuid(self.ids.next_uuid_v7()),
            AuditEventId::from_uuid(self.ids.next_uuid_v7()),
            self.clock.now(),
        )
    }

    fn map_mutation_result(
        &self,
        operation_id: crate::VersionOperationId,
        document_id: DocumentId,
        version_id: DocumentVersionId,
        result: Result<VersionOperationResult, RepositoryError>,
    ) -> Result<VersionOperationResult, ApplicationError> {
        match result {
            Ok(result) => Ok(result),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::VersionCommitOutcomeUnknown {
                    operation_id,
                    document_id,
                    document_version_id: version_id,
                })
            }
            Err(error) => Err(error.into()),
        }
    }
}

const fn format_id(format: FormatId) -> &'static str {
    match format {
        FormatId::Docx => "docx",
        FormatId::Xlsx => "xlsx",
        FormatId::Xlsm => "xlsm",
        FormatId::Pptx => "pptx",
        FormatId::Pdf => "pdf",
        FormatId::Txt => "txt",
        FormatId::Csv => "csv",
        FormatId::Html => "html",
    }
}
