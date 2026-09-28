use std::sync::Arc;

use document_domain::{
    AuditEventId, DocumentId, DocumentVersionId, DomainError, EventId, LifecycleState,
    SemanticContentItem, VersionManifest,
};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};
use serde_json::json;
use time::OffsetDateTime;

use crate::{
    AUDIT_DOCUMENT_VERSION_PUBLISHED, ApplicationError, AuditEventRecord, AuthoritativeDocument,
    CancelScheduleCommand, CancelScheduleRecord, CancelScheduleResult, Clock, CreateVersionCommand,
    DOCUMENT_VERSION_PUBLISHED, DocumentPublishRepository, DocumentRepository, DomainEventRecord,
    DueExecutionOutcome, DueTerminalRecord, EnsureSemanticInspection, FileStorage, IdGenerator,
    InspectionExecutionError, PreparedManifest, PublicationScheduleRepository,
    PublishCommandIdentity, PublishDocumentCommand, PublishDocumentResult,
    PublishInitialVersionRecord, PublishOperationId, PublishOperationRecord, PublishVersionRecord,
    RebaseWorkingVersionCommand, RepositoryError, SchedulePublishCommand, SchedulePublishRecord,
    SchedulePublishResult, SemanticInspectionExecutor, SemanticInspectionRepository,
    UpdateWorkingVersionCommand, VersionCommandIdentity, VersionMutationRecord,
    VersionOperationKind, VersionOperationResult, VersioningPreflight, VersioningRepository,
    WithdrawVersionCommand, WithdrawVersionRecord, WithdrawVersionResult,
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

    pub async fn publish_document(
        &self,
        command: PublishDocumentCommand,
    ) -> Result<PublishDocumentResult, ApplicationError>
    where
        R: DocumentPublishRepository,
    {
        self.publish_document_mode(command, None).await
    }

    async fn publish_document_mode(
        &self,
        command: PublishDocumentCommand,
        due_at: Option<OffsetDateTime>,
    ) -> Result<PublishDocumentResult, ApplicationError>
    where
        R: DocumentPublishRepository,
    {
        let scheduled_due = due_at.is_some();
        let identity = PublishCommandIdentity::from_command(&command);
        if let Some(stored) = self
            .repository
            .get_publish_operation(command.publish_operation_id())
            .await?
        {
            return if stored.matches_identity(&identity) {
                Ok(stored.result().clone())
            } else {
                Err(ApplicationError::Conflict)
            };
        }
        let target = self
            .repository
            .get_version_snapshot(command.document_id(), command.target_document_version_id())
            .await?
            .ok_or(ApplicationError::DocumentVersionNotFound)?;
        if target.document().revision() != command.expected_document_revision() {
            return Err(ApplicationError::Conflict);
        }
        if target.version().lifecycle_state() != LifecycleState::Working
            || target.requires_content_classification()
        {
            return Err(ApplicationError::BusinessRule);
        }
        let preflight = VersioningPreflight::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        );
        let prepared_target = preflight.inspect_existing(&target).await?;
        preflight.check_publish_quality(&prepared_target).await?;
        let published_at = due_at.unwrap_or_else(|| self.clock.now());
        let mut document = target.document().clone();
        let mut version = target.version().clone();
        let (transition, base) = if version.version_no().get() == 1 {
            if document.current_version_id().is_some() {
                return Err(ApplicationError::Conflict);
            }
            (
                document
                    .publish_initial_version(&mut version, published_at)
                    .map_err(map_publish_transition_error)?,
                None,
            )
        } else {
            let current = self
                .load_current(command.document_id(), command.expected_document_revision())
                .await?;
            if version.base_document_version_id() != Some(current.version().document_version_id()) {
                return Err(ApplicationError::Conflict);
            }
            let prepared_base = preflight.inspect_existing(&current).await?;
            ensure_publish_difference(prepared_base.manifest(), prepared_target.manifest())?;
            let transition = document
                .publish_next_version(&mut version, published_at)
                .map_err(map_publish_transition_error)?;
            (
                transition,
                Some((
                    current.version().document_version_id(),
                    prepared_base.identity_digest(),
                )),
            )
        };
        let result = PublishDocumentResult::from_persisted(
            command.publish_operation_id(),
            command.document_id(),
            command.target_document_version_id(),
            transition.resulting_document_revision(),
            published_at,
        );
        let domain_event = DomainEventRecord::new(
            EventId::from_uuid(self.ids.next_uuid_v7()),
            DOCUMENT_VERSION_PUBLISHED,
            command.document_id(),
            json!({
                "documentId": command.document_id().as_uuid().to_string(),
                "documentVersionId": command.target_document_version_id().as_uuid().to_string(),
                "resultingDocumentRevision": result.resulting_document_revision(),
                "publishedAt": published_at,
                "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
            }),
            published_at,
        );
        let mut audit_payload = json!({
            "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
            "expectedDocumentRevision": command.expected_document_revision(),
            "resultingDocumentRevision": result.resulting_document_revision(),
            "result": "success", "publishedAt": published_at,
        });
        if scheduled_due {
            audit_payload["serviceExecutor"] = json!("document-publication-scheduler");
        }
        let audit_event = AuditEventRecord::new(
            AuditEventId::from_uuid(self.ids.next_uuid_v7()),
            AUDIT_DOCUMENT_VERSION_PUBLISHED,
            command.principal().clone(),
            command.document_id(),
            Some(command.target_document_version_id()),
            audit_payload,
            published_at,
        );
        let operation = PublishOperationRecord::new(identity, result);
        let persisted = match base {
            Some((base_id, base_digest)) => {
                let mut record = PublishVersionRecord::new(
                    operation,
                    domain_event,
                    audit_event,
                    base_id,
                    base_digest,
                    prepared_target.identity_digest(),
                );
                if scheduled_due {
                    record = record.for_due();
                }
                self.repository.publish_next_version(record).await
            }
            None => {
                let mut record =
                    PublishInitialVersionRecord::new(operation, domain_event, audit_event);
                if scheduled_due {
                    record = record.for_due();
                }
                self.repository.publish_initial_version(record).await
            }
        };
        match persisted {
            Ok(result) => Ok(result),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::PublishCommitOutcomeUnknown {
                    publish_operation_id: command.publish_operation_id(),
                    document_id: command.document_id(),
                    document_version_id: command.target_document_version_id(),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn execute_due(
        &self,
        publish_operation_id: PublishOperationId,
    ) -> Result<DueExecutionOutcome, ApplicationError>
    where
        R: DocumentPublishRepository + PublicationScheduleRepository,
    {
        let Some(schedule) = self.repository.get_schedule(publish_operation_id).await? else {
            return Ok(DueExecutionOutcome::Inactive);
        };
        let command = PublishDocumentCommand::new(
            publish_operation_id,
            schedule.command.document_id(),
            schedule.command.target_version_id(),
            schedule.result.accepted_revision,
            schedule.command.actor().clone(),
        )?;
        let identity = PublishCommandIdentity::from_command(&command);
        if let Some(stored) = self
            .repository
            .get_publish_operation(publish_operation_id)
            .await?
        {
            return if stored.matches_identity(&identity) {
                Ok(DueExecutionOutcome::Published(stored.result().clone()))
            } else {
                Err(ApplicationError::IntegrityViolation)
            };
        }
        if schedule.status != "PENDING" {
            return Ok(DueExecutionOutcome::Inactive);
        }
        if !self.repository.is_due(publish_operation_id).await? {
            if let Some(stored) = self
                .repository
                .get_publish_operation(publish_operation_id)
                .await?
            {
                return if stored.matches_identity(&identity) {
                    Ok(DueExecutionOutcome::Published(stored.result().clone()))
                } else {
                    Err(ApplicationError::IntegrityViolation)
                };
            }
            return Ok(DueExecutionOutcome::NotDue);
        }
        let database_now = self.repository.database_now().await?;
        match self
            .publish_document_mode(command, Some(database_now))
            .await
        {
            Ok(result) => Ok(DueExecutionOutcome::Published(result)),
            Err(error) => {
                if let Some(stored) = self
                    .repository
                    .get_publish_operation(publish_operation_id)
                    .await?
                {
                    if stored.matches_identity(&identity) {
                        return Ok(DueExecutionOutcome::Published(stored.result().clone()));
                    }
                    return Err(ApplicationError::IntegrityViolation);
                }
                if due_failure_is_transient(&error) {
                    return match self.repository.record_retry(publish_operation_id).await {
                        Ok(next) => Ok(DueExecutionOutcome::RetryScheduled(next)),
                        Err(RepositoryError::BusinessRule) => Ok(DueExecutionOutcome::Inactive),
                        Err(error) => Err(error.into()),
                    };
                }
                let reason = due_terminal_reason(&error).to_owned();
                let record = DueTerminalRecord {
                    publish_operation_id,
                    reason: reason.clone(),
                    domain_event_id: EventId::from_uuid(self.ids.next_uuid_v7()),
                    audit_event_id: AuditEventId::from_uuid(self.ids.next_uuid_v7()),
                    occurred_at: database_now,
                };
                match self.repository.terminalize(record).await {
                    Ok(()) => Ok(DueExecutionOutcome::Terminal(reason)),
                    Err(RepositoryError::BusinessRule) => Ok(DueExecutionOutcome::Inactive),
                    Err(error) => Err(error.into()),
                }
            }
        }
    }

    pub async fn withdraw_version(
        &self,
        command: WithdrawVersionCommand,
    ) -> Result<WithdrawVersionResult, ApplicationError> {
        if let Some(stored) = self
            .repository
            .get_withdraw_operation(command.operation_id())
            .await?
        {
            return if stored.command_digest == command.command_digest() {
                Ok(stored.result)
            } else {
                Err(ApplicationError::Conflict)
            };
        }
        let target = self
            .repository
            .get_version_snapshot(command.document_id(), command.target_version_id())
            .await?
            .ok_or(ApplicationError::DocumentVersionNotFound)?;
        if target.document().revision() != command.expected_revision() {
            return Err(ApplicationError::Conflict);
        }
        if target.version().lifecycle_state() != LifecycleState::Published {
            return Err(ApplicationError::BusinessRule);
        }
        let is_current =
            target.document().current_version_id() == Some(command.target_version_id());
        let mut eligible_base = None;
        let mut eligible_base_manifest_digest = None;
        let mut eligible_snapshot = None;
        let mut withheld_reason = None;
        if is_current && let Some(base_id) = target.version().base_document_version_id() {
            let candidate = self
                .repository
                .get_version_snapshot(command.document_id(), base_id)
                .await;
            match candidate {
                Ok(Some(candidate))
                    if candidate.version().lifecycle_state() == LifecycleState::Published =>
                {
                    let preflight = VersioningPreflight::new(
                        self.repository.clone(),
                        self.storage.clone(),
                        self.executor.clone(),
                        self.clock.clone(),
                    );
                    match preflight.inspect_existing(&candidate).await {
                        Ok(prepared) => match preflight.check_publish_quality(&prepared).await {
                            Ok(()) => {
                                eligible_base = Some(base_id);
                                eligible_base_manifest_digest = Some(prepared.identity_digest());
                                eligible_snapshot = Some(candidate);
                            }
                            Err(_) => {
                                withheld_reason =
                                    Some("base_quality_or_storage_unavailable".to_owned())
                            }
                        },
                        Err(_) => withheld_reason = Some("base_inspection_unavailable".to_owned()),
                    }
                }
                _ => withheld_reason = Some("base_unavailable_or_not_published".to_owned()),
            }
        }
        let mut document = target.document().clone();
        let mut version = target.version().clone();
        document
            .withdraw_version(
                &mut version,
                eligible_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.version()),
                self.clock.now(),
            )
            .map_err(map_publish_transition_error)?;
        let record = WithdrawVersionRecord {
            command: command.clone(),
            eligible_base_id: eligible_base,
            eligible_base_manifest_digest,
            restoration_withheld_reason: withheld_reason,
            domain_event_id: EventId::from_uuid(self.ids.next_uuid_v7()),
            audit_event_id: AuditEventId::from_uuid(self.ids.next_uuid_v7()),
            withdrawn_at: self.clock.now(),
        };
        match self.repository.withdraw_version(record).await {
            Ok(result) => Ok(result),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::VersionCommitOutcomeUnknown {
                    operation_id: command.operation_id(),
                    document_id: command.document_id(),
                    document_version_id: command.target_version_id(),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn schedule_publish(
        &self,
        command: SchedulePublishCommand,
    ) -> Result<SchedulePublishResult, ApplicationError>
    where
        R: PublicationScheduleRepository,
    {
        if let Some(stored) = self
            .repository
            .get_schedule(command.publish_operation_id())
            .await?
        {
            return if stored.command == command {
                Ok(stored.result)
            } else {
                Err(ApplicationError::Conflict)
            };
        }
        if command.scheduled_publish_at() <= self.clock.now() {
            return Err(ApplicationError::Validation(
                "scheduled publication must be in the future".to_owned(),
            ));
        }
        let target = self
            .repository
            .get_version_snapshot(command.document_id(), command.target_version_id())
            .await?
            .ok_or(ApplicationError::DocumentVersionNotFound)?;
        if target.document().revision() != command.expected_revision() {
            return Err(ApplicationError::Conflict);
        }
        if target.version().lifecycle_state() != LifecycleState::Working
            || target.requires_content_classification()
        {
            return Err(ApplicationError::BusinessRule);
        }
        let preflight = VersioningPreflight::new(
            self.repository.clone(),
            self.storage.clone(),
            self.executor.clone(),
            self.clock.clone(),
        );
        let prepared = preflight.inspect_existing(&target).await?;
        preflight.check_publish_quality(&prepared).await?;
        let expected_current = if target.version().version_no().get() == 1 {
            if target.document().current_version_id().is_some() {
                return Err(ApplicationError::Conflict);
            }
            None
        } else {
            let current = self
                .load_current(command.document_id(), command.expected_revision())
                .await?;
            if target.version().base_document_version_id()
                != Some(current.version().document_version_id())
            {
                return Err(ApplicationError::Conflict);
            }
            let base = preflight.inspect_existing(&current).await?;
            ensure_publish_difference(base.manifest(), prepared.manifest())?;
            Some(current.version().document_version_id())
        };
        let record = SchedulePublishRecord {
            command: command.clone(),
            expected_current_version_id: expected_current,
            manifest_digest: prepared.identity_digest(),
            domain_event_id: EventId::from_uuid(self.ids.next_uuid_v7()),
            audit_event_id: AuditEventId::from_uuid(self.ids.next_uuid_v7()),
            occurred_at: self.clock.now(),
        };
        match self.repository.reserve(record).await {
            Ok(result) => Ok(result),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::ScheduleCommitOutcomeUnknown {
                    publish_operation_id: command.publish_operation_id(),
                    document_id: command.document_id(),
                    document_version_id: command.target_version_id(),
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn cancel_schedule(
        &self,
        command: CancelScheduleCommand,
    ) -> Result<CancelScheduleResult, ApplicationError>
    where
        R: PublicationScheduleRepository,
    {
        if let Some(stored) = self
            .repository
            .get_cancel_operation(command.operation_id())
            .await?
        {
            return if stored.command_digest == command.command_digest() {
                Ok(stored.result)
            } else {
                Err(ApplicationError::Conflict)
            };
        }
        let record = CancelScheduleRecord {
            command: command.clone(),
            domain_event_id: EventId::from_uuid(self.ids.next_uuid_v7()),
            audit_event_id: AuditEventId::from_uuid(self.ids.next_uuid_v7()),
            occurred_at: self.clock.now(),
        };
        match self.repository.cancel(record).await {
            Ok(result) => Ok(result),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::VersionCommitOutcomeUnknown {
                    operation_id: command.operation_id(),
                    document_id: command.document_id(),
                    document_version_id: command.target_version_id(),
                })
            }
            Err(error) => Err(error.into()),
        }
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
            }) && (new.format_id() != old.format_id()
                || new.inspection_profile_id() != old.inspection_profile_id())
            {
                return Err(ApplicationError::BusinessRule);
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

fn ensure_publish_difference(
    base: &VersionManifest,
    candidate: &VersionManifest,
) -> Result<(), ApplicationError> {
    for old in base.items() {
        if let Some(new) = candidate.items().iter().find(|item| {
            item.ordinal() == old.ordinal() && item.logical_path() == old.logical_path()
        }) && (old.format_id() != new.format_id()
            || old.inspection_profile_id() != new.inspection_profile_id())
        {
            return Err(ApplicationError::BusinessRule);
        }
    }
    if base.identity_digest() == candidate.identity_digest() {
        return Err(ApplicationError::BusinessRule);
    }
    Ok(())
}

fn map_publish_transition_error(error: DomainError) -> ApplicationError {
    match error {
        DomainError::VersionDocumentMismatch | DomainError::RevisionOverflow => {
            ApplicationError::IntegrityViolation
        }
        DomainError::CurrentVersionAlreadySet
        | DomainError::StaleVersionBase
        | DomainError::NoCurrentPublishedVersion => ApplicationError::Conflict,
        DomainError::VersionNotWorking => ApplicationError::BusinessRule,
        other => ApplicationError::Validation(other.to_string()),
    }
}

fn due_failure_is_transient(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::RepositoryUnavailable
            | ApplicationError::StorageUnavailable
            | ApplicationError::StorageWriteFailed
            | ApplicationError::StorageSyncFailed
            | ApplicationError::StorageFinalizeFailed
            | ApplicationError::Internal(_)
            | ApplicationError::PublishCommitOutcomeUnknown { .. }
            | ApplicationError::InspectionFailed(
                InspectionExecutionError::InspectionTimeout
                    | InspectionExecutionError::InspectionResourceLimitExceeded
                    | InspectionExecutionError::ExtractorUnavailable
            )
    )
}

fn due_terminal_reason(error: &ApplicationError) -> &'static str {
    match error {
        ApplicationError::PublishQualityRejected(_) => "publish_quality_rejected",
        ApplicationError::Conflict => "stale_publication_intent",
        ApplicationError::BusinessRule => "publication_business_rule",
        ApplicationError::IntegrityViolation
        | ApplicationError::SemanticInspectionDeterminismViolation
        | ApplicationError::InvalidWorkerResult => "publication_integrity_failure",
        ApplicationError::InspectionFailed(_) => "semantic_inspection_rejected",
        _ => "publication_preflight_rejected",
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
