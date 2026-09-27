#[path = "support/versioning.rs"]
mod support;

use std::{io::Cursor, sync::Arc};

use document_application::{
    ApplicationError, CreateDocumentCommand, DocumentPublicationEndService, DocumentRepository,
    DocumentService, EndDocumentPublicationCommand, PublicationEndOperationId,
    VersioningRepository,
};
use document_domain::{DocumentVersionId, FolderId, LifecycleState, MediaType, Metadata};
use support::{Fixture, TestIds, actor, fixture};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

fn end_service(
    f: &Fixture,
) -> DocumentPublicationEndService<
    TestIds,
    support::TestClock,
    document_repository_postgres::PostgresDocumentRepository,
> {
    DocumentPublicationEndService::new(Arc::new(TestIds), f.clock.clone(), f.repository.clone())
}

fn service(
    f: &Fixture,
) -> DocumentService<
    TestIds,
    support::TestClock,
    support::TestStorage,
    document_repository_postgres::PostgresDocumentRepository,
> {
    DocumentService::new(
        Arc::new(TestIds),
        f.clock.clone(),
        f.storage.clone(),
        f.repository.clone(),
    )
}

async fn end(f: &Fixture) {
    end_service(f)
        .end_document_publication(
            EndDocumentPublicationCommand::new(
                PublicationEndOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                f.document_id,
                1,
                f.base_id,
                actor(),
                "end publication".to_owned(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn published_reads_and_search_checks_drop_ended_document_but_keep_internal_history() {
    let f = fixture().await;
    let service = service(&f);
    assert_eq!(
        service
            .get_current_published_document(f.document_id)
            .await
            .unwrap()
            .version()
            .document_version_id(),
        f.base_id
    );
    let mut reader = service
        .open_current_primary_file(f.document_id)
        .await
        .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, vec![1; 3]);
    assert!(
        f.repository
            .is_current_published_version(f.document_id, f.base_id)
            .await
            .unwrap()
    );
    assert!(
        !f.repository
            .is_current_published_version(
                f.document_id,
                DocumentVersionId::from_uuid(Uuid::now_v7())
            )
            .await
            .unwrap()
    );
    assert_eq!(
        f.repository
            .list_current_published_versions(None, 1000)
            .await
            .unwrap()
            .len(),
        1
    );

    end(&f).await;
    f.storage.remove("objects/base");
    assert!(matches!(
        service.get_document(f.document_id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(matches!(
        service.open_primary_file(f.document_id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(matches!(
        service.get_current_published_document(f.document_id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(matches!(
        service.open_current_primary_file(f.document_id).await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(
        !f.repository
            .is_current_published_version(f.document_id, f.base_id)
            .await
            .unwrap()
    );
    assert!(
        f.repository
            .list_current_published_versions(None, 1000)
            .await
            .unwrap()
            .is_empty()
    );
    let explicit = f
        .repository
        .get_version_snapshot(f.document_id, f.base_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        explicit.version().lifecycle_state(),
        LifecycleState::Published
    );
    let internal = service
        .lookup_create_outcome(f.document_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(internal.version().document_version_id(), f.base_id);
}

#[tokio::test]
async fn unended_working_initial_remains_authoring_readable_but_not_public() {
    let f = fixture().await;
    let created = service(&f)
        .create_document(CreateDocumentCommand {
            folder_id: FolderId::from_uuid(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID),
            title: "Draft".to_owned(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: actor(),
            original_filename: "draft.txt".to_owned(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![7_u8; 3])),
        })
        .await
        .unwrap();
    let service = service(&f);
    let draft = service.get_document(created.document_id()).await.unwrap();
    assert_eq!(draft.version().lifecycle_state(), LifecycleState::Working);
    let mut reader = service
        .open_primary_file(created.document_id())
        .await
        .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, vec![7_u8; 3]);
    assert!(matches!(
        service
            .get_current_published_document(created.document_id())
            .await,
        Err(ApplicationError::DocumentNotFound)
    ));
    assert!(
        !f.repository
            .is_current_published_version(created.document_id(), created.document_version_id())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn broken_nonnull_current_is_integrity_error_and_listing_is_paged() {
    let f = fixture().await;
    let second = Uuid::now_v7();
    let second_version = Uuid::now_v7();
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',to_timestamp(0))")
        .bind(second).bind(document_repository_postgres::SYSTEM_ROOT_FOLDER_ID).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state,title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) VALUES ($1,$2,1,'PUBLISHED','Second',to_timestamp(1),'test-idp','editor','{}',to_timestamp(0))")
        .bind(second_version).bind(second).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE documents SET current_version_id = $1 WHERE document_id = $2")
        .bind(second_version)
        .bind(second)
        .execute(&f.pool)
        .await
        .unwrap();
    let page = f
        .repository
        .list_current_published_versions(None, 1)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    let next = f
        .repository
        .list_current_published_versions(Some(page[0].document_id()), 1)
        .await
        .unwrap();
    assert_eq!(next.len(), 1);
    assert!(page[0].document_id().as_uuid() < next[0].document_id().as_uuid());
    assert!(
        f.repository
            .list_current_published_versions(Some(next[0].document_id()), 1)
            .await
            .unwrap()
            .is_empty()
    );

    sqlx::query("UPDATE document_versions SET lifecycle_state = 'WORKING', published_at = NULL WHERE document_version_id = $1")
        .bind(f.base_id.as_uuid())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(matches!(
        service(&f)
            .get_current_published_document(f.document_id)
            .await,
        Err(ApplicationError::IntegrityViolation)
    ));
}
