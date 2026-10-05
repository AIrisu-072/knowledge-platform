//! Trusted in-process wiring for the P5 API contract tests: one union host
//! catalog with Document and Remote registrations for two tenants, the
//! synthetic authority, and actors with explicit visibility grants.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use search_application::ports::BoxFuture;
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessContextHandle, AccessRevision, PrincipalRef,
    RegistrationRevision, SyntheticAuthorityAdapter, SyntheticVisibilityAdapter, TenantId,
    TrustedSearchScope, VisibilityRevision,
};
use search_application::source_registration::{
    CompleteDesiredRegistrations, ConnectedDocumentAdapterCapabilities,
    ConnectedDocumentAdapterWitness, DocumentAdapterCapabilityPort, DocumentAdapterRef,
    DocumentSourceRegistration, RegistrationNamespace, RegistrationSetRevision,
    ServerDocumentRegistrationConfig, SourceRegistration, SourceRegistrationCatalog,
    SyntheticHostRegistrationAuthority, SyntheticRegistrationLedger,
};
use search_core::id::{SessionId, SourceId};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use uuid::Uuid;

pub fn source(number: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(number))
}

pub fn tenant(value: &str) -> TenantId {
    TenantId::new(value).unwrap()
}

pub fn remote_registration(id: SourceId, tenant_name: &str) -> RemoteSourceRegistration {
    RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
        tenant: tenant(tenant_name),
        source_id: id,
        provider_kind: "synthetic".into(),
        endpoint: RegisteredEndpoint::new("https", "catalog.example.test", 443, "/v1").unwrap(),
        supported_modes: vec![DiscoveryMode::RemoteQuery, DiscoveryMode::DirectAddress],
        enumeration_semantics: EnumerationSemantics::QueryOnly,
        authority_predicates: vec!["catalog.title".into()],
        allowed_resource_kinds: vec![ResourceKind::Knowledge],
        current_access_contract: CurrentAccessContract::PerItem,
        retention_mode: RetentionMode::NoRetention,
        freshness_policy: None,
        canonical_upstream_lineage: "synthetic-catalog".into(),
        limits: RemoteRegistrationLimits::synthetic_canary(),
        registration_revision: RegistrationRevision::new(1).unwrap(),
        visibility_revision: VisibilityRevision::new(1).unwrap(),
    })
    .unwrap()
}

struct Connected;

impl DocumentAdapterCapabilityPort for Connected {
    fn connected_capabilities<'a>(
        &'a self,
        binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>> {
        Box::pin(async move {
            Ok(Some(ConnectedDocumentAdapterCapabilities::new(
                binding.clone(),
                vec![ResourceKind::Document],
                vec![
                    DiscoveryMode::LocalDirectory,
                    DiscoveryMode::LocalContentSearch,
                ],
                vec![EnumerationSemantics::Partial],
                vec![RetentionMode::PersistentResource],
            )?))
        })
    }
}

pub async fn document_registration(id: SourceId, tenant_name: &str) -> DocumentSourceRegistration {
    let config = ServerDocumentRegistrationConfig {
        tenant: tenant(tenant_name),
        source_id: id,
        document_adapter_ref: DocumentAdapterRef::new("document-binding").unwrap(),
        allowed_resource_kinds: vec![ResourceKind::Document],
        supported_modes: vec![
            DiscoveryMode::LocalDirectory,
            DiscoveryMode::LocalContentSearch,
        ],
        enumeration_semantics: EnumerationSemantics::Partial,
        retention_mode: RetentionMode::PersistentResource,
        registration_revision: RegistrationRevision::new(1).unwrap(),
        visibility_revision: VisibilityRevision::new(1).unwrap(),
    };
    let witness = ConnectedDocumentAdapterWitness::from_connected_port(
        &Connected,
        &config.document_adapter_ref,
    )
    .await
    .unwrap()
    .unwrap();
    DocumentSourceRegistration::from_server_config(config, &witness).unwrap()
}

/// tenant-a: one Document and one Remote Source; tenant-b: one Remote.
pub struct ApiWorld {
    pub authority: SyntheticAuthorityAdapter,
    pub catalog: SourceRegistrationCatalog,
    pub document: SourceId,
    pub remote: SourceId,
    pub foreign: SourceId,
}

impl ApiWorld {
    pub async fn new() -> Self {
        let (document, remote, foreign) = (source(5_001), source(5_002), source(5_003));
        let host = Arc::new(SyntheticHostRegistrationAuthority::new());
        host.publish(
            RegistrationNamespace::Document,
            RegistrationSetRevision::new(1).unwrap(),
            vec![SourceRegistration::Document(
                document_registration(document, "tenant-a").await,
            )],
        )
        .unwrap();
        host.publish(
            RegistrationNamespace::Remote,
            RegistrationSetRevision::new(1).unwrap(),
            vec![
                SourceRegistration::Remote(remote_registration(remote, "tenant-a")),
                SourceRegistration::Remote(remote_registration(foreign, "tenant-b")),
            ],
        )
        .unwrap();
        let documents =
            CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
                .await
                .unwrap();
        let remotes = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
            .await
            .unwrap();
        let catalog = SourceRegistrationCatalog::try_new(
            Arc::new(SyntheticRegistrationLedger::with_host(host)),
            &documents,
            &remotes,
        )
        .await
        .unwrap();
        Self {
            authority: SyntheticAuthorityAdapter::new(),
            catalog,
            document,
            remote,
            foreign,
        }
    }

    pub fn visibility(&self) -> SyntheticVisibilityAdapter<'_> {
        SyntheticVisibilityAdapter::new(&self.catalog)
    }

    /// Issues a verified handle and grants it `sources` of its tenant.
    pub async fn actor(
        &self,
        tenant_name: &str,
        principal: &str,
        visibility: &SyntheticVisibilityAdapter<'_>,
        sources: &[SourceId],
    ) -> AccessContextHandle {
        let handle = self
            .authority
            .issue_verified_identity(
                tenant(tenant_name),
                PrincipalRef::new(principal).unwrap(),
                Some(SessionId::from_uuid(Uuid::now_v7())),
                AccessRevision::new(1).unwrap(),
                Duration::from_secs(120),
            )
            .unwrap();
        let actor: TrustedSearchScope = self.authority.resolve(&handle).await.unwrap().unwrap();
        for source in sources {
            // Every synthetic registration is at revision 1/1.
            visibility
                .grant(
                    actor.tenant().clone(),
                    actor.principal().clone(),
                    *source,
                    RegistrationRevision::new(1).unwrap(),
                    VisibilityRevision::new(1).unwrap(),
                )
                .unwrap();
        }
        handle
    }
}
