#![allow(dead_code)]

use std::sync::Arc;

use document_application::{InvocationKind, VerifiedActorContext};
use document_domain::{DocumentId, FolderId, PolicySubject, PolicySubjectKind, PrincipalRef};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub(super) struct Fixture {
    _container: testcontainers::ContainerAsync<GenericImage>,
    pub pool: PgPool,
    pub repository: Arc<PostgresDocumentRepository>,
    pub document_id: DocumentId,
    pub root_id: FolderId,
}

pub(super) fn actor() -> PrincipalRef {
    PrincipalRef::new("test-idp", "policy-admin").unwrap()
}

pub(super) fn context() -> VerifiedActorContext {
    let actor = actor();
    let subject = PolicySubject::new(
        PolicySubjectKind::Principal,
        actor.identity_provider(),
        actor.principal_id(),
    )
    .unwrap();
    VerifiedActorContext::from_trusted_adapter(
        actor,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

pub(super) async fn fixture() -> Fixture {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "management_transaction_test")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/management_transaction_test"
        ))
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    let document_id = DocumentId::from_uuid(Uuid::now_v7());
    sqlx::query("INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) VALUES ($1,$2,NULL,1,'{}',now())")
        .bind(document_id.as_uuid())
        .bind(SYSTEM_ROOT_FOLDER_ID)
        .execute(&pool)
        .await
        .unwrap();
    Fixture {
        _container: container,
        repository: Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
            pool.clone(),
            actor(),
        )),
        pool,
        document_id,
        root_id: FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
    }
}
