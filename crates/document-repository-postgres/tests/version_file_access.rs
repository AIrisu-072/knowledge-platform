#[path = "support/management.rs"]
mod support;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use document_application::{
    ApplicationError, BootstrapRootPolicy, ContentReader, DocumentHistoryService, FileStorage,
    StorageError, StorageObjectInfo, StoreFileRequest, StoredFile, VersionFileAccessService,
    VersionFileRequest, VersionPurpose, VersionRequest,
};
use document_domain::{
    Action, DocumentVersionId, PolicyGrant, PolicySubject, PolicySubjectKind, StorageKey,
};
use support::{context, fixture};
use uuid::Uuid;

#[derive(Default)]
struct CountingStorage {
    opens: AtomicUsize,
}

impl FileStorage for CountingStorage {
    async fn put_immutable(&self, _request: StoreFileRequest) -> Result<StoredFile, StorageError> {
        unreachable!()
    }
    async fn open(&self, _key: &StorageKey) -> Result<ContentReader, StorageError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(std::io::Cursor::new(b"bytes".to_vec())))
    }
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn audit_is_committed_before_storage_open_and_membership_is_checked() {
    let f = fixture().await;
    let grant = PolicyGrant::new(
        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "policy-admin").unwrap(),
        [Action::Read, Action::ReadHistory],
    )
    .unwrap();
    f.repository
        .initialize_root_policy(&context(), vec![grant])
        .await
        .unwrap();
    let version_id = DocumentVersionId::from_uuid(Uuid::now_v7());
    let item_id = Uuid::now_v7();
    let representation_id = Uuid::now_v7();
    let file_id = Uuid::now_v7();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','File',now(),'test-idp','policy-admin','{}',now())")
        .bind(version_id.as_uuid()).bind(f.document_id.as_uuid()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(version_id.as_uuid())
        .bind(f.document_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) VALUES ($1,$2,'text/plain',5,'objects/test-file',now())")
        .bind(file_id).bind(vec![1_u8;32]).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) VALUES ($1,$2,'primary',0,$3)")
        .bind(item_id).bind(version_id.as_uuid()).bind(representation_id)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) VALUES ($1,$2,$3,'AUTHORITATIVE','file.txt')")
        .bind(representation_id).bind(item_id).bind(file_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let storage = Arc::new(CountingStorage::default());
    let service = VersionFileAccessService::new(f.repository.clone(), storage.clone());
    let request = VersionFileRequest {
        version: VersionRequest {
            document_id: f.document_id,
            document_version_id: version_id,
            purpose: VersionPurpose::Published,
        },
        content_item_id: item_id,
        representation_id,
    };
    let wrong = service
        .open_version_file(
            &context(),
            VersionFileRequest {
                representation_id: Uuid::now_v7(),
                ..request
            },
        )
        .await;
    assert!(matches!(wrong, Err(ApplicationError::FileObjectNotFound)));
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);
    sqlx::query("CREATE FUNCTION reject_file_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type = 'document.file.access_granted' THEN RAISE EXCEPTION 'blocked'; END IF; RETURN NEW; END; $$")
        .execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_file_audit BEFORE INSERT ON audit_outbox_events FOR EACH ROW EXECUTE FUNCTION reject_file_audit()")
        .execute(&f.pool).await.unwrap();
    assert!(
        service
            .open_version_file(&context(), request)
            .await
            .is_err()
    );
    assert_eq!(storage.opens.load(Ordering::SeqCst), 0);
    sqlx::query("DROP TRIGGER reject_file_audit ON audit_outbox_events")
        .execute(&f.pool)
        .await
        .unwrap();
    let opened = service
        .open_version_file(&context(), request)
        .await
        .unwrap();
    assert_eq!(opened.media_type.as_str(), "text/plain");
    assert_eq!(opened.safe_display_name, "file.txt");
    assert_eq!(storage.opens.load(Ordering::SeqCst), 1);
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_outbox_events WHERE event_type = 'document.file.access_granted'")
        .fetch_one(&f.pool).await.unwrap();
    assert_eq!(audits, 1);
    let files = DocumentHistoryService::new(f.repository.clone())
        .list_version_files(&context(), request.version)
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].content_item_id, item_id);
    assert_eq!(files[0].representation_id, representation_id);
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
        .bind(f.document_id.as_uuid())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO document_publication_end_operations (operation_id,document_id,command_digest,expected_document_revision,expected_current_version_id,actor_identity_provider,actor_principal_id,reason,former_current_version_id,resulting_document_revision,ended_at) VALUES ($1,$2,$3,$4,$5,'test-idp','policy-admin','end',$5,$6,now())")
        .bind(Uuid::now_v7()).bind(f.document_id.as_uuid()).bind(vec![0_u8;32])
        .bind(revision).bind(version_id.as_uuid()).bind(revision + 1)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = NULL, revision = revision + 1 WHERE document_id = $1")
        .bind(f.document_id.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        service.open_version_file(&context(), request).await,
        Err(ApplicationError::DocumentVersionNotFound)
    ));
    assert_eq!(storage.opens.load(Ordering::SeqCst), 1);
    let history_request = VersionFileRequest {
        version: VersionRequest {
            purpose: VersionPurpose::History,
            ..request.version
        },
        ..request
    };
    service
        .open_version_file(&context(), history_request)
        .await
        .unwrap();
    assert_eq!(storage.opens.load(Ordering::SeqCst), 2);
}
