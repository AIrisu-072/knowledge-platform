//! P5-08: the Search API production factory on the durable PostgreSQL
//! Source registration ledger. Startup refuses missing wiring or a partial
//! namespace before any durable write; the union catalog follows each
//! namespace reconcile and closes every current gate after an ambiguous
//! commit until the host's state is rechecked.

mod support;

#[path = "support/api.rs"]
mod api;
#[path = "support/registration.rs"]
mod registration;

use std::sync::Arc;
use std::time::Duration;

use api::{Host, NoActorPorts, discovery_config, durable, visible_sources};
use registration::*;
use search_api_http::router::StartupError as HttpStartupError;
use search_application::retrieval::{RetrievalInputs, RetrieverSupport};
use search_application::search_core::id::SourceId;
use search_application::source_registration::{
    HostRegistrationSnapshotPort, RegistrationNamespace, SyntheticHostRegistrationAuthority,
};
use search_runtime::api::{
    ActorPortsFactory, ProductionTransports, RemoteTransportFactory, SearchApiRuntime,
    StartupError, build_search_api_runtime,
};
use search_source_http::transport::SystemResolver;
use sqlx::PgPool;

fn transports() -> Option<Arc<dyn RemoteTransportFactory>> {
    Some(Arc::new(ProductionTransports::new(Arc::new(
        SystemResolver,
    ))))
}

fn no_ports() -> Arc<dyn ActorPortsFactory> {
    Arc::new(NoActorPorts)
}

fn id(source: SourceId) -> String {
    source.as_uuid().to_string()
}

async fn start(
    host: &Host,
    registrations: &Arc<SyntheticHostRegistrationAuthority>,
    pool: &PgPool,
) -> Result<SearchApiRuntime, StartupError> {
    let registrations: Arc<dyn HostRegistrationSnapshotPort> = registrations.clone();
    build_search_api_runtime(
        host.config(
            registrations,
            transports(),
            discovery_config(RetrieverSupport::default(), RetrievalInputs::default()),
        ),
        durable(pool, no_ports()),
        host.identity(),
    )
    .await
}

#[tokio::test]
async fn missing_identity_challenge_durable_ledger_or_namespace_reconcile_rejects_start() {
    let (_guard, pool, registrations, _) = fixture().await;
    let document_id = source(8_001);
    let remote_id = source(8_002);
    publish(
        &registrations,
        RegistrationNamespace::Document,
        1,
        vec![document(document_id, "tenant-a").await],
    )
    .await;
    publish(&registrations, RegistrationNamespace::Remote, 1, vec![]).await;
    let host = Host::new();
    let snapshot: Arc<dyn HostRegistrationSnapshotPort> = registrations.clone();
    let config = || {
        host.config(
            snapshot.clone(),
            transports(),
            discovery_config(RetrieverSupport::default(), RetrievalInputs::default()),
        )
    };

    let mut identity = host.identity();
    identity.credentials = None;
    assert_eq!(
        build_search_api_runtime(config(), durable(&pool, no_ports()), identity)
            .await
            .unwrap_err(),
        StartupError::Http(HttpStartupError::CredentialVerifierUnwired)
    );
    let mut identity = host.identity();
    identity.auth = None;
    assert_eq!(
        build_search_api_runtime(config(), durable(&pool, no_ports()), identity)
            .await
            .unwrap_err(),
        StartupError::Http(HttpStartupError::ChallengeUnwired)
    );
    let mut unwired = config();
    unwired.claims = None;
    assert_eq!(
        build_search_api_runtime(unwired, durable(&pool, no_ports()), host.identity())
            .await
            .unwrap_err(),
        StartupError::ClaimCatalogUnwired
    );
    let mut ports = durable(&pool, no_ports());
    ports.actor_ports = None;
    assert_eq!(
        build_search_api_runtime(config(), ports, host.identity())
            .await
            .unwrap_err(),
        StartupError::ActorPortsUnwired
    );

    // No durable ledger schema: the reconcile fails, startup is refused.
    let (_bare_guard, bare, _) = support::postgres::postgres("search_api_bare").await;
    assert_eq!(
        build_search_api_runtime(config(), durable(&bare, no_ports()), host.identity())
            .await
            .unwrap_err(),
        StartupError::Registration
    );
    // Only one namespace's complete set: never a partial union catalog.
    let partial = Arc::new(SyntheticHostRegistrationAuthority::new());
    publish(
        &partial,
        RegistrationNamespace::Document,
        1,
        vec![document(document_id, "tenant-a").await],
    )
    .await;
    assert_eq!(
        start(&host, &partial, &pool).await.unwrap_err(),
        StartupError::Registration
    );

    // A registered remote Source needs its transport.
    publish(
        &registrations,
        RegistrationNamespace::Remote,
        2,
        vec![remote(remote_id, "tenant-a", 1)],
    )
    .await;
    let mut no_transport = config();
    no_transport.remote_transports = None;
    assert_eq!(
        build_search_api_runtime(no_transport, durable(&pool, no_ports()), host.identity())
            .await
            .unwrap_err(),
        StartupError::RemoteTransportUnwired
    );

    // Fully wired: both namespaces reconcile and the union is served.
    let runtime = start(&host, &registrations, &pool).await.unwrap();
    host.login("reader-token", "tenant-a", "reader");
    host.grants.grant("tenant-a", "reader", document_id);
    host.grants.grant("tenant-a", "reader", remote_id);
    let mut expected = vec![id(document_id), id(remote_id)];
    expected.sort();
    assert_eq!(
        visible_sources(&runtime.router(), "reader-token").await,
        Ok(expected)
    );
    assert_eq!(
        visible_sources(&runtime.router(), "unknown-token").await,
        Err(401)
    );
}

#[tokio::test]
async fn tenant_kind_collision_tombstone_old_scope_rejected_after_restart() {
    let (_guard, pool, registrations, _) = fixture().await;
    let source_id = source(8_101);
    publish(
        &registrations,
        RegistrationNamespace::Document,
        1,
        vec![document(source_id, "tenant-a").await],
    )
    .await;
    publish(&registrations, RegistrationNamespace::Remote, 1, vec![]).await;
    let host = Host::new();
    host.login("reader-token", "tenant-a", "reader");
    host.grants.grant("tenant-a", "reader", source_id);
    let runtime = start(&host, &registrations, &pool).await.unwrap();
    assert_eq!(
        visible_sources(&runtime.router(), "reader-token").await,
        Ok(vec![id(source_id)])
    );

    // The Document namespace drops the Source: its old scope is gone even
    // though the host grant still names it.
    let tombstone = publish(&registrations, RegistrationNamespace::Document, 2, vec![]).await;
    runtime.reconcile(&tombstone).await.unwrap();
    assert_eq!(
        visible_sources(&runtime.router(), "reader-token").await,
        Ok(vec![])
    );
    drop(runtime);

    // After a restart the tombstoned ID cannot come back as another kind…
    publish(
        &registrations,
        RegistrationNamespace::Remote,
        2,
        vec![remote(source_id, "tenant-a", 1)],
    )
    .await;
    assert_eq!(
        start(&host, &registrations, &pool).await.unwrap_err(),
        StartupError::Registration
    );
    publish(&registrations, RegistrationNamespace::Remote, 3, vec![]).await;
    // …nor for another tenant.
    publish(
        &registrations,
        RegistrationNamespace::Document,
        3,
        vec![document_with_revision(source_id, "tenant-b", 2).await],
    )
    .await;
    host.login("foreign-token", "tenant-b", "reader");
    assert_eq!(
        start(&host, &registrations, &pool).await.unwrap_err(),
        StartupError::Registration
    );
    publish(&registrations, RegistrationNamespace::Document, 4, vec![]).await;
    let restarted = start(&host, &registrations, &pool).await.unwrap();
    for token in ["reader-token", "foreign-token"] {
        assert_eq!(
            visible_sources(&restarted.router(), token).await,
            Ok(vec![])
        );
    }
}

#[tokio::test]
async fn remote_reconcile_keeps_document_source_and_reverse() {
    let (_guard, pool, registrations, _) = fixture().await;
    let (first_document, second_document) = (source(8_201), source(8_202));
    let (first_remote, second_remote) = (source(8_203), source(8_204));
    publish(
        &registrations,
        RegistrationNamespace::Document,
        1,
        vec![document(first_document, "tenant-a").await],
    )
    .await;
    publish(
        &registrations,
        RegistrationNamespace::Remote,
        1,
        vec![remote(first_remote, "tenant-a", 1)],
    )
    .await;
    let host = Host::new();
    host.login("reader-token", "tenant-a", "reader");
    for granted in [first_document, second_document, first_remote, second_remote] {
        host.grants.grant("tenant-a", "reader", granted);
    }
    let runtime = start(&host, &registrations, &pool).await.unwrap();
    let visible = || visible_sources_sorted(&runtime);
    assert_eq!(visible().await, ids(&[first_document, first_remote]));

    // A remote-only reconcile leaves the Document Source current.
    let remote_only = publish(&registrations, RegistrationNamespace::Remote, 2, vec![]).await;
    runtime.reconcile(&remote_only).await.unwrap();
    assert_eq!(visible().await, ids(&[first_document]));
    let remote_back = publish(
        &registrations,
        RegistrationNamespace::Remote,
        3,
        vec![remote(second_remote, "tenant-a", 1)],
    )
    .await;
    runtime.reconcile(&remote_back).await.unwrap();
    assert_eq!(visible().await, ids(&[first_document, second_remote]));

    // And the reverse: a Document reconcile leaves the remote Source current.
    let documents = publish(
        &registrations,
        RegistrationNamespace::Document,
        2,
        vec![
            document(first_document, "tenant-a").await,
            document(second_document, "tenant-a").await,
        ],
    )
    .await;
    runtime.reconcile(&documents).await.unwrap();
    assert_eq!(
        visible().await,
        ids(&[first_document, second_document, second_remote])
    );
    let documents_only = publish(&registrations, RegistrationNamespace::Document, 3, vec![]).await;
    runtime.reconcile(&documents_only).await.unwrap();
    assert_eq!(visible().await, ids(&[second_remote]));
}

fn ids(sources: &[SourceId]) -> Result<Vec<String>, u16> {
    let mut out: Vec<String> = sources.iter().map(|source| id(*source)).collect();
    out.sort();
    Ok(out)
}

async fn visible_sources_sorted(runtime: &SearchApiRuntime) -> Result<Vec<String>, u16> {
    visible_sources(&runtime.router(), "reader-token").await
}

#[tokio::test]
async fn ambiguous_commit_closes_current_until_recheck() {
    let (_guard, pool, registrations, _) = fixture().await;
    let source_id = source(8_301);
    publish(
        &registrations,
        RegistrationNamespace::Document,
        1,
        vec![document(source_id, "tenant-a").await],
    )
    .await;
    publish(&registrations, RegistrationNamespace::Remote, 1, vec![]).await;
    let host = Host::new();
    host.login("reader-token", "tenant-a", "reader");
    host.grants.grant("tenant-a", "reader", source_id);
    let runtime = Arc::new(start(&host, &registrations, &pool).await.unwrap());
    assert_eq!(visible_sources_sorted(&runtime).await, ids(&[source_id]));

    // A second connection holds the commit of the next reconcile open.
    sqlx::raw_sql("CREATE FUNCTION registration_commit_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(8301); RETURN NULL; END $$; CREATE CONSTRAINT TRIGGER registration_commit_barrier AFTER UPDATE ON search_registration_serial DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION registration_commit_barrier();")
        .execute(&pool)
        .await
        .unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(8301)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let desired = publish(
        &registrations,
        RegistrationNamespace::Document,
        2,
        vec![document_with_revision(source_id, "tenant-a", 2).await],
    )
    .await;
    let task_runtime = runtime.clone();
    let task_desired = desired.clone();
    let task = tokio::spawn(async move { task_runtime.reconcile(&task_desired).await });
    let reached_commit = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype='advisory' AND database=(SELECT oid FROM pg_database WHERE datname=current_database()) AND objid=8301 AND NOT granted)")
                .fetch_one(&pool)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    // The caller gives up: whether the commit happened is unknown.
    task.abort();
    let stopped = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("cancelled reconcile must stop");
    if reached_commit.is_err() {
        blocker.rollback().await.unwrap();
        panic!("commit must reach the deferred barrier");
    }
    assert!(stopped.unwrap_err().is_cancelled());
    assert_eq!(visible_sources_sorted(&runtime).await, Err(503));
    tokio::time::timeout(Duration::from_secs(5), blocker.rollback())
        .await
        .expect("barrier rollback must stop")
        .unwrap();
    // Releasing the database alone reopens nothing.
    assert_eq!(visible_sources_sorted(&runtime).await, Err(503));

    // A fresh reconcile of the host's state is the only recheck.
    tokio::time::timeout(Duration::from_secs(10), runtime.reconcile(&desired))
        .await
        .expect("recheck must finish")
        .unwrap();
    assert_eq!(visible_sources_sorted(&runtime).await, ids(&[]));
    host.grants.grant_at("tenant-a", "reader", source_id, 2);
    assert_eq!(visible_sources_sorted(&runtime).await, ids(&[source_id]));
}
