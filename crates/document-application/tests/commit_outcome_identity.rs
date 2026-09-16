use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use document_application::{
    ApplicationError, AuthoritativeDocument, Clock, ContentReader, CreateDocumentCommand,
    CreateInitialDocumentRecord, DocumentRepository, DocumentService, FileStorage, IdGenerator,
    RepositoryError, StorageError, StorageObjectInfo, StoreFileRequest, StoredFile,
};
use document_domain::{
    ContentHash, DocumentId, DocumentVersionId, FileId, FileSize, FolderId, MediaType, Metadata,
    PrincipalRef, StorageKey,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct FixedIds(Mutex<VecDeque<Uuid>>);

impl FixedIds {
    fn new() -> Self {
        Self(Mutex::new(
            (1_u128..=16).map(Uuid::from_u128).collect::<VecDeque<_>>(),
        ))
    }
}

impl IdGenerator for FixedIds {
    fn next_uuid_v7(&self) -> Uuid {
        self.0.lock().unwrap().pop_front().unwrap()
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH
    }
}

struct FinalizingStorage;

impl FileStorage for FinalizingStorage {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        Ok(StoredFile::new(
            StorageKey::new(format!("objects/00/{}", request.file_id().as_uuid())).unwrap(),
            ContentHash::from_slice(&[7_u8; 32]).unwrap(),
            FileSize::new(21).unwrap(),
            MediaType::new("application/pdf").unwrap(),
        ))
    }

    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        Err(StorageError::Internal("not used".into()))
    }

    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

struct AmbiguousRepository;

impl DocumentRepository for AmbiguousRepository {
    async fn create_initial_document(
        &self,
        _record: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        Err(RepositoryError::CommitOutcomeUnknown)
    }

    async fn get_authoritative_document(
        &self,
        _id: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(None)
    }

    async fn file_reference_exists(&self, _file_id: FileId) -> Result<bool, RepositoryError> {
        Ok(false)
    }

    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        Ok(Vec::new())
    }
}

fn command() -> CreateDocumentCommand {
    CreateDocumentCommand {
        folder_id: FolderId::from_uuid(Uuid::from_u128(100)),
        title: "Policy v1".into(),
        document_metadata: Metadata::default(),
        version_metadata: Metadata::default(),
        principal: PrincipalRef::new("windows", "principal-1").unwrap(),
        original_filename: "policy.pdf".into(),
        media_type: MediaType::new("application/pdf").unwrap(),
        content: Box::pin(Cursor::new(b"authoritative-content".to_vec())),
    }
}

#[tokio::test]
async fn ambiguous_commit_exposes_pre_generated_ids_for_safe_lookup() {
    let service = DocumentService::new(
        Arc::new(FixedIds::new()),
        Arc::new(FixedClock),
        Arc::new(FinalizingStorage),
        Arc::new(AmbiguousRepository),
    );

    let error = service.create_document(command()).await.unwrap_err();

    assert_eq!(
        error,
        ApplicationError::CommitOutcomeUnknown {
            document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
            document_version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
            file_id: FileId::from_uuid(Uuid::from_u128(3)),
        }
    );
}
