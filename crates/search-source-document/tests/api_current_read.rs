//! P5-05: Document current reads for the Search API on a real PostgreSQL
//! Document Source and a real filesystem store.

#[path = "../../search-application/tests/support/api.rs"]
mod api;

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use api::ApiWorld;
use document_application::{
    BootstrapRootPolicy, DocumentAccessCheckService, FileStorage, InvocationKind, StoreFileRequest,
    VerifiedActorContext,
};
use document_domain::{
    Action, FileId, MediaType, PolicyGrant, PolicySubject, PolicySubjectKind, PrincipalRef,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use document_storage_fs::FileSystemStorage;
use search_application::api_scope::{
    ApiError, SearchOperationContext, prepare_api_visible_sources,
};
use search_application::ports::BoxFuture;
use search_application::remote_disclosure::{
    CurrentDisclosureAccessPort, DisclosedFields, DisclosureOwner,
};
use search_application::remote_lease::SystemLeaseClock;
use search_application::resource_read::{ResourceCoverage, ResourceReadService, ResourceSnapshot};
use search_application::source_registration::TrustedVisibleRegistry;
use search_core::id::{ResourceId, ResourceVersionId};
use search_source_document::{DocumentApiRead, DocumentCurrentAccessAdapter};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use time::OffsetDateTime;
use uuid::Uuid;

const ACCESS_CONTEXT: &str = "search-api-access";

struct Open;
impl CurrentDisclosureAccessPort for Open {
    fn authorize<'a>(
        &'a self,
        _: &'a DisclosureOwner,
        _: &'a DisclosedFields,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn principal() -> PrincipalRef {
    PrincipalRef::new("test-idp", "editor").unwrap()
}

fn actor() -> VerifiedActorContext {
    VerifiedActorContext::from_trusted_adapter(
        principal(),
        vec![PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap()],
        OffsetDateTime::now_utc() + time::Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

struct Database {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pool: PgPool,
    root: PathBuf,
    storage: FileSystemStorage,
    repository: Arc<PostgresDocumentRepository>,
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Database {
    async fn start() -> Self {
        let container = GenericImage::new("postgres", "18.6-bookworm")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_DB", "api_current_read")
            .start()
            .await
            .unwrap();
        let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&format!(
                "postgres://postgres:postgres@127.0.0.1:{port}/api_current_read"
            ))
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
            pool.clone(),
            principal(),
        ));
        repository
            .initialize_root_policy(
                &actor(),
                vec![
                    PolicyGrant::new(
                        PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor")
                            .unwrap(),
                        [Action::Read],
                    )
                    .unwrap(),
                ],
            )
            .await
            .unwrap();
        let root = std::env::temp_dir().join(format!("search-api-read-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        Self {
            _container: container,
            pool,
            storage: FileSystemStorage::new(&root),
            root,
            repository,
        }
    }

    /// Publishes a new current Version (with one stored Part when given).
    async fn publish(
        &self,
        document: Option<Uuid>,
        title: &str,
        part: Option<&[u8]>,
    ) -> (Uuid, Uuid) {
        let document_id = match document {
            Some(document) => document,
            None => {
                let document = Uuid::now_v7();
                sqlx::query(
                    "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
                     VALUES ($1,$2,NULL,1,'{}'::jsonb,now())",
                )
                .bind(document)
                .bind(SYSTEM_ROOT_FOLDER_ID)
                .execute(&self.pool)
                .await
                .unwrap();
                document
            }
        };
        let version_id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
             title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
             VALUES ($1,$2,(SELECT COUNT(*) + 1 FROM document_versions WHERE document_id = $2), \
                     'PUBLISHED',$3,now(),'test-idp','editor','{}'::jsonb,now())",
        )
        .bind(version_id)
        .bind(document_id)
        .bind(title)
        .execute(&self.pool)
        .await
        .unwrap();
        if let Some(bytes) = part {
            let file_id = FileId::from_uuid(Uuid::now_v7());
            let stored = self
                .storage
                .put_immutable(StoreFileRequest::new(
                    file_id,
                    Box::pin(Cursor::new(bytes.to_vec())),
                    MediaType::new("text/plain").unwrap(),
                ))
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO file_objects (file_id,content_hash,media_type,size_bytes,storage_locator,created_at) \
                 VALUES ($1,$2,'text/plain',$3,$4,now())",
            )
            .bind(file_id.as_uuid())
            .bind(stored.content_hash().as_bytes().to_vec())
            .bind(stored.size_bytes().get())
            .bind(stored.storage_key().as_str())
            .execute(&self.pool)
            .await
            .unwrap();
            let (item, representation) = (Uuid::now_v7(), Uuid::now_v7());
            let mut tx = self.pool.begin().await.unwrap();
            sqlx::query(
                "INSERT INTO content_items (content_item_id,document_version_id,logical_path,ordinal,authoritative_representation_id) \
                 VALUES ($1,$2,'body/part-0',0,$3)",
            )
            .bind(item)
            .bind(version_id)
            .bind(representation)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO content_representations (content_representation_id,content_item_id,file_id,role,original_filename) \
                 VALUES ($1,$2,$3,'AUTHORITATIVE','source')",
            )
            .bind(representation)
            .bind(item)
            .bind(file_id.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
        }
        sqlx::query(
            "UPDATE documents SET current_version_id = $1, revision = revision + 1 WHERE document_id = $2",
        )
        .bind(version_id)
        .bind(document_id)
        .execute(&self.pool)
        .await
        .unwrap();
        (document_id, version_id)
    }

    async fn end_publication(&self, document: Uuid, version: Uuid) {
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM documents WHERE document_id = $1")
                .bind(document)
                .fetch_one(&self.pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO document_publication_end_operations \
             (operation_id, document_id, command_digest, expected_document_revision, \
              expected_current_version_id, actor_identity_provider, actor_principal_id, reason, \
              former_current_version_id, resulting_document_revision, ended_at) \
             VALUES ($1, $2, $3, $4, $5, 'test-idp', 'editor', 'synthetic end', $5, $6, now())",
        )
        .bind(Uuid::now_v7())
        .bind(document)
        .bind(vec![1_u8; 32])
        .bind(revision)
        .bind(version)
        .bind(revision + 1)
        .execute(&self.pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn real_db_fs_parent_part_raw_current_read_and_t10() {
    let database = Database::start().await;
    let world = ApiWorld::new().await;
    let visibility = world.visibility();
    let handle = world
        .actor("tenant-a", "reader", &visibility, &[world.document])
        .await;
    let registry = TrustedVisibleRegistry::new(&world.authority, &visibility, &world.catalog);
    let access = Arc::new(DocumentCurrentAccessAdapter::new(
        world.document,
        database.pool.clone(),
        DocumentAccessCheckService::new(database.repository.clone()),
        actor(),
        ACCESS_CONTEXT.into(),
    ));
    let reader = DocumentApiRead::new(
        world.document,
        database.pool.clone(),
        access,
        ACCESS_CONTEXT.into(),
    );
    let service = ResourceReadService::new(
        &reader,
        &reader,
        Arc::new(SystemLeaseClock),
        std::time::Duration::from_secs(30),
    );
    let read = |id: Uuid| {
        let service = &service;
        let world = &world;
        let registry = &registry;
        let handle = &handle;
        async move {
            let context = SearchOperationContext::authenticate(
                &world.authority,
                handle,
                Instant::now() + std::time::Duration::from_secs(10),
            )
            .await
            .unwrap();
            let snapshot = prepare_api_visible_sources(&world.authority, registry, &context)
                .await
                .unwrap();
            let mut disclosure = service
                .read(&context, &snapshot, ResourceId::from_uuid(id))
                .await?;
            let mut detail: Option<ResourceSnapshot> = None;
            disclosure
                .disclose_with(&Open, |view| {
                    detail = Some(view.snapshot().clone());
                    Ok(())
                })
                .await
                .unwrap();
            Ok::<_, ApiError>(detail.unwrap())
        }
    };

    // A current Version with a stored Part: title, version and body state.
    let (document, first) = database
        .publish(None, "規程", Some(b"synthetic body"))
        .await;
    let detail = read(first).await.unwrap();
    assert_eq!(detail.resource_id, ResourceId::from_uuid(first));
    assert_eq!(detail.source_id, world.document);
    assert_eq!(detail.title.as_deref(), Some("規程"));
    assert_eq!(
        detail.resource_version,
        Some(ResourceVersionId::from_uuid(first))
    );
    assert_eq!(detail.coverage, ResourceCoverage::BodyUnknown);
    // The Document ID is not a Resource alias.
    assert_eq!(
        read(document).await.unwrap_err(),
        ApiError::ResourceNotFound
    );

    // A new Version: the old one is history, never a fallback.
    let (_, second) = database.publish(Some(document), "規程 改訂", None).await;
    assert_eq!(read(first).await.unwrap_err(), ApiError::ResourceNotFound);
    let detail = read(second).await.unwrap();
    assert_eq!(detail.title.as_deref(), Some("規程 改訂"));
    assert_eq!(detail.coverage, ResourceCoverage::TitleAndPermittedMetadata);

    // Publication ended (T10): the same generic not-found.
    database.end_publication(document, second).await;
    assert_eq!(read(second).await.unwrap_err(), ApiError::ResourceNotFound);

    // Another actor's view without this Source sees nothing either.
    let (_, other) = database.publish(None, "別文書", None).await;
    let outsider = world.actor("tenant-a", "outsider", &visibility, &[]).await;
    let context = SearchOperationContext::authenticate(
        &world.authority,
        &outsider,
        Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .unwrap();
    let snapshot = prepare_api_visible_sources(&world.authority, &registry, &context)
        .await
        .unwrap();
    assert_eq!(
        service
            .read(&context, &snapshot, ResourceId::from_uuid(other))
            .await
            .unwrap_err(),
        ApiError::ResourceNotFound
    );
}
