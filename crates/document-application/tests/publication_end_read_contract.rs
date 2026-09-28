use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use document_application::{
    ApplicationError, AuthoritativeDocument, Clock, ContentReader, CreateInitialDocumentRecord,
    DocumentRepository, DocumentService, FileStorage, IdGenerator, RepositoryError, StorageError,
    StorageObjectInfo, StoreFileRequest, StoredFile,
};
use document_domain::{
    ContentHash, CreateInitialDocument, DocumentId, DocumentVersionId, FileId, FileSize, FolderId,
    InitialDocument, MediaType, Metadata, PrincipalRef, StorageKey, StoredFileDescriptor, Title,
};
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

struct Ids;
impl IdGenerator for Ids {
    fn next_uuid_v7(&self) -> Uuid {
        Uuid::now_v7()
    }
}
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH
    }
}
#[derive(Default)]
struct Storage {
    opens: Mutex<usize>,
}
impl FileStorage for Storage {
    async fn put_immutable(&self, _: StoreFileRequest) -> Result<StoredFile, StorageError> {
        Err(StorageError::Internal("unused".to_owned()))
    }
    async fn open(&self, _: &StorageKey) -> Result<ContentReader, StorageError> {
        *self.opens.lock().unwrap() += 1;
        Ok(Box::pin(Cursor::new(b"content".to_vec())))
    }
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(vec![])
    }
}

#[derive(Default)]
struct Repository {
    internal: Mutex<Option<AuthoritativeDocument>>,
    authoring: Mutex<Option<AuthoritativeDocument>>,
    published: Mutex<Option<AuthoritativeDocument>>,
}
impl DocumentRepository for Repository {
    async fn create_initial_document(
        &self,
        _: CreateInitialDocumentRecord,
    ) -> Result<(), RepositoryError> {
        Err(RepositoryError::Internal("unused".to_owned()))
    }
    async fn get_authoritative_document(
        &self,
        _: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(self.internal.lock().unwrap().clone())
    }
    async fn get_authoring_document(
        &self,
        _: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(self.authoring.lock().unwrap().clone())
    }
    async fn get_current_published_document(
        &self,
        _: DocumentId,
    ) -> Result<Option<AuthoritativeDocument>, RepositoryError> {
        Ok(self.published.lock().unwrap().clone())
    }
    async fn is_current_published_version(
        &self,
        _: DocumentId,
        _: DocumentVersionId,
    ) -> Result<bool, RepositoryError> {
        Ok(self.published.lock().unwrap().is_some())
    }
    async fn list_current_published_versions(
        &self,
        _: Option<DocumentId>,
        _: i64,
    ) -> Result<Vec<document_application::CurrentPublishedVersionRef>, RepositoryError> {
        Ok(vec![])
    }
    async fn file_reference_exists(&self, _: FileId) -> Result<bool, RepositoryError> {
        Ok(false)
    }
    async fn list_referenced_file_ids(&self) -> Result<Vec<FileId>, RepositoryError> {
        Ok(vec![])
    }
}

fn working() -> AuthoritativeDocument {
    AuthoritativeDocument::from_initial(
        InitialDocument::create(CreateInitialDocument {
            document_id: DocumentId::from_uuid(Uuid::from_u128(1)),
            version_id: DocumentVersionId::from_uuid(Uuid::from_u128(2)),
            file_id: FileId::from_uuid(Uuid::from_u128(3)),
            folder_id: FolderId::from_uuid(Uuid::from_u128(4)),
            title: Title::new("Draft").unwrap(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: PrincipalRef::new("test", "actor").unwrap(),
            stored_file: StoredFileDescriptor::new(
                StorageKey::new("objects/draft").unwrap(),
                ContentHash::from_slice(&[1; 32]).unwrap(),
                FileSize::new(7).unwrap(),
                MediaType::new("text/plain").unwrap(),
            ),
            original_filename: "draft.txt".to_owned(),
            created_at: OffsetDateTime::UNIX_EPOCH,
        })
        .unwrap(),
    )
}

#[tokio::test]
async fn authoring_read_preserves_working_then_hides_ended_document_without_opening_storage() {
    let repository = Arc::new(Repository::default());
    let storage = Arc::new(Storage::default());
    let draft = working();
    let id = draft.document().document_id();
    *repository.internal.lock().unwrap() = Some(draft.clone());
    *repository.authoring.lock().unwrap() = Some(draft);
    let service = DocumentService::new(
        Arc::new(Ids),
        Arc::new(FixedClock),
        storage.clone(),
        repository.clone(),
    );
    assert_eq!(
        service
            .get_document(id)
            .await
            .unwrap()
            .version()
            .title()
            .as_str(),
        "Draft"
    );
    let mut reader = service.open_primary_file(id).await.unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"content");
    assert!(matches!(
        service.get_current_published_document(id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    *repository.authoring.lock().unwrap() = None;
    assert!(matches!(
        service.get_document(id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(matches!(
        service.open_primary_file(id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert_eq!(*storage.opens.lock().unwrap(), 1);
    assert!(service.lookup_create_outcome(id).await.unwrap().is_some());
}

#[tokio::test]
async fn published_read_and_open_use_only_the_published_port() {
    let repository = Arc::new(Repository::default());
    let storage = Arc::new(Storage::default());
    let draft = working();
    let id = draft.document().document_id();
    let mut document = draft.document().clone();
    let mut version = draft.version().clone();
    document
        .publish_initial_version(&mut version, OffsetDateTime::UNIX_EPOCH)
        .unwrap();
    let published = AuthoritativeDocument::from_parts(
        document,
        version,
        draft.file().clone(),
        draft.version_file().clone(),
    );
    *repository.published.lock().unwrap() = Some(published);
    let service = DocumentService::new(
        Arc::new(Ids),
        Arc::new(FixedClock),
        storage.clone(),
        repository,
    );
    assert_eq!(
        service
            .get_current_published_document(id)
            .await
            .unwrap()
            .version()
            .title()
            .as_str(),
        "Draft"
    );
    let mut reader = service.open_current_primary_file(id).await.unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"content");
    assert_eq!(*storage.opens.lock().unwrap(), 1);
}
