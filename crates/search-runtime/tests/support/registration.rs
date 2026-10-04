#![allow(dead_code)]

use std::sync::Arc;

use search_application::ports::BoxFuture;
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::scoped::{RegistrationRevision, TenantId, VisibilityRevision};
use search_application::search_core::id::SourceId;
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_application::source_registration::{
    CompleteDesiredRegistrations, ConnectedDocumentAdapterCapabilities,
    ConnectedDocumentAdapterWitness, DocumentAdapterCapabilityPort, DocumentAdapterRef,
    DocumentSourceRegistration, RegistrationNamespace, RegistrationSetRevision,
    ServerDocumentRegistrationConfig, SourceRegistration, SyntheticHostRegistrationAuthority,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use sqlx::PgPool;
use uuid::Uuid;

pub fn source(number: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(number))
}

pub fn tenant(name: &str) -> TenantId {
    TenantId::new(name).unwrap()
}

pub fn revision(number: u64) -> RegistrationRevision {
    RegistrationRevision::new(number).unwrap()
}

pub fn visibility(number: u64) -> VisibilityRevision {
    VisibilityRevision::new(number).unwrap()
}

pub fn remote(id: SourceId, tenant_name: &str, registration_revision: u64) -> SourceRegistration {
    SourceRegistration::Remote(
        RemoteSourceRegistration::from_server_config(remote_config(
            id,
            tenant_name,
            registration_revision,
        ))
        .unwrap(),
    )
}

pub fn remote_config(
    id: SourceId,
    tenant_name: &str,
    registration_revision: u64,
) -> ServerRemoteRegistrationConfig {
    ServerRemoteRegistrationConfig {
        tenant: tenant(tenant_name),
        source_id: id,
        provider_kind: "provider-a".into(),
        endpoint: RegisteredEndpoint::new("https", "example.test", 443, "/v1").unwrap(),
        supported_modes: vec![DiscoveryMode::RemoteQuery, DiscoveryMode::DirectAddress],
        enumeration_semantics: EnumerationSemantics::QueryOnly,
        authority_predicates: vec!["policy-a".into()],
        allowed_resource_kinds: vec![ResourceKind::Knowledge],
        current_access_contract: CurrentAccessContract::PerItem,
        retention_mode: RetentionMode::SessionOnly,
        freshness_policy: Some("current".into()),
        canonical_upstream_lineage: "lineage-a".into(),
        limits: RemoteRegistrationLimits::synthetic_canary(),
        registration_revision: revision(registration_revision),
        visibility_revision: visibility(1),
    }
}

struct ConnectedDocumentAdapter;

impl DocumentAdapterCapabilityPort for ConnectedDocumentAdapter {
    fn connected_capabilities<'a>(
        &'a self,
        binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>> {
        Box::pin(async move {
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

pub async fn document(id: SourceId, tenant_name: &str) -> SourceRegistration {
    document_with_revision(id, tenant_name, 1).await
}

pub async fn document_with_revision(
    id: SourceId,
    tenant_name: &str,
    registration_revision: u64,
) -> SourceRegistration {
    let binding = DocumentAdapterRef::new("document-binding-a").unwrap();
    let witness =
        ConnectedDocumentAdapterWitness::from_connected_port(&ConnectedDocumentAdapter, &binding)
            .await
            .unwrap()
            .unwrap();
    SourceRegistration::Document(
        DocumentSourceRegistration::from_server_config(
            ServerDocumentRegistrationConfig {
                tenant: tenant(tenant_name),
                source_id: id,
                document_adapter_ref: binding,
                allowed_resource_kinds: vec![ResourceKind::Document],
                supported_modes: vec![DiscoveryMode::LocalDirectory],
                enumeration_semantics: EnumerationSemantics::Partial,
                retention_mode: RetentionMode::PersistentResource,
                registration_revision: revision(registration_revision),
                visibility_revision: visibility(1),
            },
            &witness,
        )
        .unwrap(),
    )
}

pub async fn publish(
    host: &SyntheticHostRegistrationAuthority,
    namespace: RegistrationNamespace,
    set_revision: u64,
    registrations: Vec<SourceRegistration>,
) -> CompleteDesiredRegistrations {
    host.publish(
        namespace,
        RegistrationSetRevision::new(set_revision).unwrap(),
        registrations,
    )
    .unwrap();
    CompleteDesiredRegistrations::capture(host, namespace)
        .await
        .unwrap()
}

pub async fn fixture() -> (
    crate::support::postgres::DatabaseGuard,
    PgPool,
    Arc<SyntheticHostRegistrationAuthority>,
    PgSourceRegistrationLedger,
) {
    let (guard, pool, _) = crate::support::postgres::postgres("source_registration_test").await;
    search_runtime::migrate(&pool).await.unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(pool.clone(), host.clone());
    (guard, pool, host, ledger)
}
