use std::{collections::HashSet, sync::Arc};

use document_domain::{
    AuditEventId, CreateInitialDocument, DocumentId, DocumentVersionId, DomainError, EventId,
    FileId, InitialDocument, Title,
};
use serde_json::json;
use time::Duration;

use crate::{
    AUDIT_DOCUMENT_CREATED, AUDIT_DOCUMENT_VERSION_CREATED, AUDIT_DOCUMENT_VERSION_PUBLISHED,
    ApplicationError, AuditEventRecord, AuthoritativeDocument, Clock, ContentReader,
    CreateDocumentCommand, CreateDocumentResult, CreateInitialDocumentRecord, DOCUMENT_CREATED,
    DOCUMENT_VERSION_CREATED, DOCUMENT_VERSION_PUBLISHED, DocumentPublishRepository,
    DocumentRepository, DomainEventRecord, FileStorage, IdGenerator, PublishCommandIdentity,
    PublishDocumentCommand, PublishDocumentResult, PublishInitialVersionRecord,
    PublishOperationRecord, ReconciliationFinding, RepositoryError, StorageObjectKind,
    StoreFileRequest, classify,
};

pub struct DocumentService<I, C, F, R> {
    ids: Arc<I>,
    clock: Arc<C>,
    storage: Arc<F>,
    repository: Arc<R>,
}

impl<I, C, F, R> DocumentService<I, C, F, R>
where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
{
    pub fn new(ids: Arc<I>, clock: Arc<C>, storage: Arc<F>, repository: Arc<R>) -> Self {
        Self {
            ids,
            clock,
            storage,
            repository,
        }
    }
}

impl<I, C, F, R> DocumentService<I, C, F, R>
where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
    R: DocumentRepository,
{
    pub async fn create_document(
        &self,
        command: CreateDocumentCommand,
    ) -> Result<CreateDocumentResult, ApplicationError> {
        let CreateDocumentCommand {
            folder_id,
            title,
            document_metadata,
            version_metadata,
            principal,
            original_filename,
            media_type,
            content,
        } = command;

        let title = Title::new(title)?;
        if original_filename.trim().is_empty() {
            return Err(ApplicationError::Validation(
                "original filename cannot be blank".to_owned(),
            ));
        }

        let occurred_at = self.clock.now();
        let document_id = DocumentId::from_uuid(self.ids.next_uuid_v7());
        let document_version_id = DocumentVersionId::from_uuid(self.ids.next_uuid_v7());
        let file_id = FileId::from_uuid(self.ids.next_uuid_v7());
        let document_created_id = EventId::from_uuid(self.ids.next_uuid_v7());
        let version_created_id = EventId::from_uuid(self.ids.next_uuid_v7());
        let audit_document_created_id = AuditEventId::from_uuid(self.ids.next_uuid_v7());
        let audit_version_created_id = AuditEventId::from_uuid(self.ids.next_uuid_v7());

        let stored_file = self
            .storage
            .put_immutable(StoreFileRequest::new(file_id, content, media_type))
            .await?;

        let initial = InitialDocument::create(CreateInitialDocument {
            document_id,
            version_id: document_version_id,
            file_id,
            folder_id,
            title,
            document_metadata,
            version_metadata,
            principal: principal.clone(),
            stored_file: stored_file.into_descriptor(),
            original_filename,
            created_at: occurred_at,
        })?;
        let authoritative = AuthoritativeDocument::from_initial(initial);

        let domain_events = vec![
            DomainEventRecord::new(
                document_created_id,
                DOCUMENT_CREATED,
                document_id,
                json!({"documentId": document_id.as_uuid().to_string()}),
                occurred_at,
            ),
            DomainEventRecord::new(
                version_created_id,
                DOCUMENT_VERSION_CREATED,
                document_id,
                json!({
                    "documentId": document_id.as_uuid().to_string(),
                    "documentVersionId": document_version_id.as_uuid().to_string(),
                    "versionNo": 1
                }),
                occurred_at,
            ),
        ];
        let audit_events = vec![
            AuditEventRecord::new(
                audit_document_created_id,
                AUDIT_DOCUMENT_CREATED,
                principal.clone(),
                document_id,
                None,
                json!({"documentId": document_id.as_uuid().to_string()}),
                occurred_at,
            ),
            AuditEventRecord::new(
                audit_version_created_id,
                AUDIT_DOCUMENT_VERSION_CREATED,
                principal,
                document_id,
                Some(document_version_id),
                json!({
                    "documentId": document_id.as_uuid().to_string(),
                    "documentVersionId": document_version_id.as_uuid().to_string()
                }),
                occurred_at,
            ),
        ];

        let record = CreateInitialDocumentRecord::new(authoritative, domain_events, audit_events);
        match self.repository.create_initial_document(record).await {
            Ok(()) => Ok(CreateDocumentResult::new(
                document_id,
                document_version_id,
                file_id,
            )),
            Err(RepositoryError::CommitOutcomeUnknown) => {
                Err(ApplicationError::CommitOutcomeUnknown {
                    document_id,
                    document_version_id,
                    file_id,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn lookup_create_outcome(
        &self,
        document_id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, ApplicationError> {
        self.repository
            .get_authoritative_document(document_id)
            .await
            .map_err(Into::into)
    }

    pub async fn reconcile_storage(
        &self,
        grace: Duration,
    ) -> Result<Vec<ReconciliationFinding>, ApplicationError> {
        let now = self.clock.now();
        let objects = self.storage.list_objects().await?;
        let referenced_ids = self.repository.list_referenced_file_ids().await?;
        let referenced: HashSet<FileId> = referenced_ids.iter().copied().collect();
        let mut findings = Vec::new();

        for file_id in referenced_ids {
            let final_object = objects
                .iter()
                .find(|object| {
                    object.kind() == StorageObjectKind::Final && object.file_id() == Some(file_id)
                })
                .cloned();
            let classification = classify(true, final_object.as_ref(), now, grace)
                .expect("referenced files always have a reconciliation classification");
            findings.push(ReconciliationFinding::new(
                file_id,
                final_object,
                classification,
            ));
        }

        for object in objects {
            let Some(file_id) = object.file_id() else {
                continue;
            };
            if referenced.contains(&file_id) {
                continue;
            }
            if let Some(classification) = classify(false, Some(&object), now, grace) {
                findings.push(ReconciliationFinding::new(
                    file_id,
                    Some(object),
                    classification,
                ));
            }
        }

        Ok(findings)
    }

    pub async fn get_document(
        &self,
        document_id: DocumentId,
    ) -> Result<AuthoritativeDocument, ApplicationError> {
        self.repository
            .get_authoritative_document(document_id)
            .await?
            .ok_or(ApplicationError::DocumentNotFound)
    }

    pub async fn open_primary_file(
        &self,
        document_id: DocumentId,
    ) -> Result<ContentReader, ApplicationError> {
        let authoritative = self.get_document(document_id).await?;
        let storage_key = authoritative.file().storage_key().clone();
        self.storage.open(&storage_key).await.map_err(Into::into)
    }
}

impl<I, C, F, R> DocumentService<I, C, F, R>
where
    I: IdGenerator,
    C: Clock,
    F: FileStorage,
    R: DocumentPublishRepository,
{
    pub async fn publish_document(
        &self,
        command: PublishDocumentCommand,
    ) -> Result<PublishDocumentResult, ApplicationError> {
        let identity = PublishCommandIdentity::from_command(&command);

        if let Some(stored) = self
            .repository
            .get_publish_operation(command.publish_operation_id())
            .await?
        {
            if stored.matches_identity(&identity) {
                return Ok(stored.result().clone());
            }
            return Err(ApplicationError::Conflict);
        }

        let candidate = self
            .repository
            .get_publish_candidate(command.document_id(), command.target_document_version_id())
            .await?;
        let published_at = self.clock.now();
        let (mut document, mut version, file, _version_file) = candidate.into_parts();
        let transition = document
            .publish_initial_version(&mut version, published_at)
            .map_err(map_publish_domain_error)?;

        let storage_key = file.storage_key().clone();
        let reader = self.storage.open(&storage_key).await?;
        drop(reader);

        let domain_event_id = EventId::from_uuid(self.ids.next_uuid_v7());
        let audit_event_id = AuditEventId::from_uuid(self.ids.next_uuid_v7());
        let result = PublishDocumentResult::from_persisted(
            command.publish_operation_id(),
            command.document_id(),
            command.target_document_version_id(),
            transition.resulting_document_revision(),
            published_at,
        );

        let domain_event = DomainEventRecord::new(
            domain_event_id,
            DOCUMENT_VERSION_PUBLISHED,
            command.document_id(),
            json!({
                "documentId": command.document_id().as_uuid().to_string(),
                "documentVersionId": command.target_document_version_id().as_uuid().to_string(),
                "resultingDocumentRevision": transition.resulting_document_revision(),
                "publishedAt": published_at,
                "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
            }),
            published_at,
        );
        let audit_event = AuditEventRecord::new(
            audit_event_id,
            AUDIT_DOCUMENT_VERSION_PUBLISHED,
            command.principal().clone(),
            command.document_id(),
            Some(command.target_document_version_id()),
            json!({
                "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
                "expectedDocumentRevision": command.expected_document_revision(),
                "resultingDocumentRevision": transition.resulting_document_revision(),
                "result": "success",
                "publishedAt": published_at,
            }),
            published_at,
        );
        let operation = PublishOperationRecord::new(identity, result.clone());
        let record = PublishInitialVersionRecord::new(operation, domain_event, audit_event);

        match self.repository.publish_initial_version(record).await {
            Ok(persisted) => Ok(persisted),
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
}

fn map_publish_domain_error(error: DomainError) -> ApplicationError {
    match error {
        DomainError::VersionDocumentMismatch | DomainError::RevisionOverflow => {
            ApplicationError::IntegrityViolation
        }
        DomainError::CurrentVersionAlreadySet => ApplicationError::Conflict,
        DomainError::VersionNotWorking => ApplicationError::BusinessRule,
        other => ApplicationError::Validation(other.to_string()),
    }
}
