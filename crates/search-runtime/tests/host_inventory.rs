//! B5 (P7-R01P/R02): the host registration inventory publisher and its read
//! adapter on real PostgreSQL. A revision commits every tenant and both
//! namespaces with its Audit event or nothing; the head only moves forward;
//! the read adapter rebuilds and re-digests what it serves and feeds the
//! durable Source registration ledger.

mod support;

#[path = "support/registration.rs"]
mod registration;

use std::sync::Arc;

use search_application::ports::BoxFuture;
use search_application::scoped::TenantId;
use search_application::search_core::id::SourceId;
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_application::source_registration::{
    ConnectedDocumentAdapterCapabilities, DocumentAdapterCapabilityPort, DocumentAdapterRef,
    ServerDocumentRegistrationConfig, SourceRegistrationLedgerPort,
};
use search_runtime::host_inventory::{
    HostRegistrationInputV1, InventoryError, PgHostInventoryPublisher, PgHostRegistrationSource,
    PublishOutcome,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use sqlx::PgPool;

struct Connected;

impl DocumentAdapterCapabilityPort for Connected {
    fn connected_capabilities<'a>(
        &'a self,
        binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>> {
        Box::pin(async move {
            if binding.as_str() != "document-binding-a" {
                return Ok(None);
            }
            Ok(Some(ConnectedDocumentAdapterCapabilities::new(
                binding.clone(),
                vec![ResourceKind::Document],
                vec![DiscoveryMode::LocalDirectory],
                vec![EnumerationSemantics::Partial],
                vec![RetentionMode::PersistentResource],
            )?))
        })
    }
}

fn document(id: SourceId, tenant: &str, binding: &str) -> ServerDocumentRegistrationConfig {
    ServerDocumentRegistrationConfig {
        tenant: registration::tenant(tenant),
        source_id: id,
        document_adapter_ref: DocumentAdapterRef::new(binding).unwrap(),
        allowed_resource_kinds: vec![ResourceKind::Document],
        supported_modes: vec![DiscoveryMode::LocalDirectory],
        enumeration_semantics: EnumerationSemantics::Partial,
        retention_mode: RetentionMode::PersistentResource,
        registration_revision: registration::revision(1),
        visibility_revision: registration::visibility(1),
    }
}

fn input(revision: u64) -> HostRegistrationInputV1 {
    HostRegistrationInputV1 {
        deployment_epoch: 1,
        authority_revision: revision,
        tenants: ["tenant-a", "tenant-b", "tenant-c"]
            .into_iter()
            .map(|name| TenantId::new(name).unwrap())
            .collect(),
        documents: vec![document(
            registration::source(8_101),
            "tenant-a",
            "document-binding-a",
        )],
        remotes: vec![registration::remote_config(
            registration::source(8_201),
            "tenant-c",
            1,
        )],
        writer_ref: "operator:release-7".into(),
    }
}

async fn start() -> (support::postgres::DatabaseGuard, PgPool) {
    let (guard, pool, _) = support::postgres::postgres("host_inventory").await;
    search_runtime::migrate(&pool).await.unwrap();
    (guard, pool)
}

async fn count(pool: &PgPool, statement: &'static str) -> i64 {
    sqlx::query_scalar(statement).fetch_one(pool).await.unwrap()
}

async fn head(pool: &PgPool) -> Option<(i64, i64)> {
    sqlx::query_as("SELECT deployment_epoch, authority_revision FROM search_host_inventory_head")
        .fetch_optional(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn both_namespaces_and_empty_tenants_publish_together_and_feed_the_ledger() {
    let (_guard, pool) = start().await;
    let adapters: Arc<dyn DocumentAdapterCapabilityPort> = Arc::new(Connected);
    let publisher = PgHostInventoryPublisher::new(pool.clone(), adapters.clone());
    assert_eq!(
        publisher.publish(&input(1)).await.unwrap(),
        PublishOutcome::Published
    );
    assert_eq!(head(&pool).await, Some((1, 1)));
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM search_audit_outbox_events \
             WHERE event_type='host.registration.changed' AND subject_kind='HostInventory' \
             AND actor_ref='operator:release-7' AND delivered_at IS NULL",
        )
        .await,
        1
    );

    let source = Arc::new(PgHostRegistrationSource::new(pool.clone(), adapters));
    let (documents, remotes) = source.capture().await.unwrap();
    assert_eq!(documents.deployment_revision().get(), 1);
    assert_eq!(remotes.deployment_revision().get(), 1);
    assert!(
        documents
            .registrations()
            .contains_key(&registration::source(8_101))
    );
    assert!(
        remotes
            .registrations()
            .contains_key(&registration::source(8_201))
    );
    let tenants = source.tenants().await.unwrap();
    assert_eq!(tenants.len(), 3, "the empty tenant-b stays in the roster");

    // The durable ledger takes the adapter as its host authority.
    let ledger = PgSourceRegistrationLedger::new(pool.clone(), source.clone());
    ledger.reconcile(&documents).await.unwrap();
    ledger.reconcile(&remotes).await.unwrap();
    assert_eq!(
        count(&pool, "SELECT count(*) FROM search_source_ownership").await,
        2
    );
}

#[tokio::test]
async fn the_head_moves_forward_only_and_one_revision_has_one_content() {
    let (_guard, pool) = start().await;
    let publisher = PgHostInventoryPublisher::new(pool.clone(), Arc::new(Connected));
    publisher.publish(&input(1)).await.unwrap();
    // The same revision and content again is not a second publication.
    assert_eq!(
        publisher.publish(&input(1)).await.unwrap(),
        PublishOutcome::AlreadyCurrent
    );
    let mut changed = input(1);
    changed.remotes.clear();
    assert_eq!(
        publisher.publish(&changed).await,
        Err(InventoryError::Conflict)
    );
    let mut next = input(2);
    next.remotes.clear();
    assert_eq!(
        publisher.publish(&next).await.unwrap(),
        PublishOutcome::Published
    );
    assert_eq!(
        publisher.publish(&input(1)).await,
        Err(InventoryError::Stale)
    );
    assert_eq!(head(&pool).await, Some((1, 2)));
    assert_eq!(
        count(&pool, "SELECT count(*) FROM search_audit_outbox_events").await,
        2
    );
    let source = PgHostRegistrationSource::new(pool.clone(), Arc::new(Connected));
    let (_, remotes) = source.capture().await.unwrap();
    assert!(remotes.is_empty());
}

#[tokio::test]
async fn incomplete_or_unconnected_input_writes_nothing() {
    let (_guard, pool) = start().await;
    let publisher = PgHostInventoryPublisher::new(pool.clone(), Arc::new(Connected));
    let mut outside = input(1);
    outside
        .tenants
        .retain(|tenant| tenant.as_str() != "tenant-c");
    let mut duplicate = input(1);
    duplicate.remotes[0].source_id = registration::source(8_101);
    let mut unconnected = input(1);
    unconnected.documents[0] = document(
        registration::source(8_101),
        "tenant-a",
        "document-binding-x",
    );
    let mut no_writer = input(1);
    no_writer.writer_ref = "bad writer".into();
    for bad in [outside, duplicate, unconnected, no_writer] {
        assert!(matches!(
            publisher.publish(&bad).await,
            Err(InventoryError::Invalid(_))
        ));
    }
    assert_eq!(head(&pool).await, None);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM search_host_inventory_revision").await,
        0
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM search_audit_outbox_events").await,
        0
    );
}

#[tokio::test]
async fn an_audit_failure_rolls_the_revision_and_head_back() {
    let (_guard, pool) = start().await;
    let publisher = PgHostInventoryPublisher::new(pool.clone(), Arc::new(Connected));
    publisher.publish(&input(1)).await.unwrap();
    sqlx::raw_sql(
        "CREATE FUNCTION test_refuse_audit() RETURNS TRIGGER LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'audit sink refused'; END; $$; \
         CREATE TRIGGER test_refuse_audit BEFORE INSERT ON search_audit_outbox_events \
         FOR EACH ROW EXECUTE FUNCTION test_refuse_audit();",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        publisher.publish(&input(2)).await,
        Err(InventoryError::Store)
    );
    assert_eq!(head(&pool).await, Some((1, 1)));
    assert_eq!(
        count(&pool, "SELECT count(*) FROM search_host_inventory_revision").await,
        1
    );
}

#[tokio::test]
async fn published_revisions_and_the_head_cannot_be_rewritten() {
    let (_guard, pool) = start().await;
    let publisher = PgHostInventoryPublisher::new(pool.clone(), Arc::new(Connected));
    publisher.publish(&input(1)).await.unwrap();
    publisher.publish(&input(2)).await.unwrap();
    for statement in [
        "UPDATE search_host_inventory_revision SET writer_ref='someone-else'",
        "DELETE FROM search_host_inventory_revision",
        "UPDATE search_host_inventory_head SET authority_revision=1",
        "DELETE FROM search_host_inventory_head",
        "UPDATE search_audit_outbox_events SET reason_code='REWRITTEN'",
        "DELETE FROM search_audit_outbox_events",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        let code = error
            .as_database_error()
            .and_then(|db| db.code())
            .map(|code| code.to_string());
        assert!(
            matches!(code.as_deref(), Some("23514") | Some("23503")),
            "{statement}: {error}"
        );
    }
    assert_eq!(head(&pool).await, Some((1, 2)));
}
