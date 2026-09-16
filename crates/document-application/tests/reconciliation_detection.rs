use std::sync::Arc;

use document_application::{
    AuthoritativeDocument, Clock, ContentReader, CreateInitialDocumentRecord, DocumentRepository,
    DocumentService, FileStorage, IdGenerator, ReconciliationClassification, RepositoryError,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
};
use document_domain::{DocumentId, FileId, StorageKey};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

struct NoopIds;

impl IdGenerator for NoopIds {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::from_u128(1)
    }
}

struct FixedClock(OffsetDateTime);

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

struct EmptyStorage;

impl FileStorage for EmptyStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        Err(StorageError::Internal(
            "not used by reconciliation test".into(),
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Err(StorageError::Internal(
            "not used by reconciliation test".into(),
        ))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

struct ReferencedRepository {
    file_id: FileId,
}

impl DocumentRepository for ReferencedRepository {
    async fn create_initial_document(
        &self,
        _record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        Err(RepositoryError::Internal(
            "not used by reconciliation test".into(),
        ))
    }

    async fn get_authoritative_document(
        &self,
        _id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(None)
    }

    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError> {
        Ok(file_id == self.file_id)
    }
}

#[tokio::test]
async fn reconciliation_detects_authoritative_reference_whose_final_object_is_missing() {
    let file_id = FileId::from_uuid(Uuid::from_u128(42));
    let service = DocumentService::new(
        Arc::new(NoopIds),
        Arc::new(FixedClock(OffsetDateTime::UNIX_EPOCH + Duration::hours(2))),
        Arc::new(EmptyStorage),
        Arc::new(ReferencedRepository { file_id }),
    );

    let findings = service
        .reconcile_storage(Duration::hours(1))
        .await
        .expect("reconciliation scan should complete");

    assert_eq!(
        findings.len(),
        1,
        "a DB-referenced file with no final storage object must be reported"
    );
    assert_eq!(
        findings[0].classification(),
        ReconciliationClassification::IntegrityViolation
    );
}
