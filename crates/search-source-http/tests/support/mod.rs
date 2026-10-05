//! Trusted in-process wiring for the real-TCP tests: server-owned remote
//! registrations on one host catalog, a synthetic authority, actors with
//! visibility grants, and the loopback-only transport constructor.
#![allow(dead_code)]

pub mod discovery;
pub mod synthetic_catalog;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use search_application::remote::TrustedRemoteContext;
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessContextHandle, AccessRevision, PrincipalRef,
    RegistrationRevision, ScopedSourceRegistryPort, SyntheticAuthorityAdapter,
    SyntheticVisibilityAdapter, TenantId, TrustedDiscoveryBinding, VisibilityRevision,
    VisibleSourceRegistration,
};
use search_application::source_registration::{
    CompleteDesiredRegistrations, RegistrationNamespace, RegistrationSetRevision,
    SourceRegistration, SourceRegistrationCatalog, SyntheticHostRegistrationAuthority,
    SyntheticRegistrationLedger, TrustedVisibleRegistry,
};
use search_core::id::{DiscoveryEvaluationId, SessionId, SourceId};
use search_core::resource::ResourceKind;
use search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_source_http::adapter::HttpRemoteSourceAdapter;
use search_source_http::transport::{
    AddressResolver, GuardedHttpTransport, TransportFuture, TransportLimits,
};
use uuid::Uuid;

/// A lease clock the test moves by hand.
pub struct ManualClock(std::sync::Mutex<std::time::Instant>);

impl ManualClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(std::time::Instant::now())))
    }
    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl search_application::remote_lease::LeaseClock for ManualClock {
    fn now(&self) -> std::time::Instant {
        *self.0.lock().unwrap()
    }
}

/// Resolves the registered hostname to the synthetic catalog's loopback port.
pub struct Loopback(pub u16);

impl AddressResolver for Loopback {
    fn resolve<'a>(&'a self, _: &'a str, _: u16) -> TransportFuture<'a, Vec<SocketAddr>> {
        let port = self.0;
        Box::pin(async move { Ok(vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)]) })
    }
}

pub fn source(value: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(value))
}

pub fn registration(
    tenant: &str,
    source_id: SourceId,
    port: u16,
    base_path: &str,
    retention: RetentionMode,
) -> RemoteSourceRegistration {
    RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
        tenant: TenantId::new(tenant).unwrap(),
        source_id,
        provider_kind: "synthetic".into(),
        endpoint: RegisteredEndpoint::new("http", "catalog.example.test", port, base_path).unwrap(),
        supported_modes: vec![
            DiscoveryMode::RemoteEnumeration,
            DiscoveryMode::RemoteQuery,
            DiscoveryMode::DirectAddress,
            DiscoveryMode::LiveOnly,
        ],
        enumeration_semantics: EnumerationSemantics::Complete,
        authority_predicates: vec!["catalog.title".into()],
        allowed_resource_kinds: vec![ResourceKind::Knowledge],
        current_access_contract: CurrentAccessContract::PerItem,
        retention_mode: retention,
        freshness_policy: None,
        canonical_upstream_lineage: "synthetic-catalog".into(),
        limits: RemoteRegistrationLimits::synthetic_canary(),
        registration_revision: RegistrationRevision::new(1).unwrap(),
        visibility_revision: VisibilityRevision::new(1).unwrap(),
    })
    .unwrap()
}

/// One host catalog with every remote registration of the test.
pub struct World {
    pub authority: SyntheticAuthorityAdapter,
    pub catalog: SourceRegistrationCatalog,
    pub registrations: Vec<RemoteSourceRegistration>,
}

/// One actor's server-issued identity and Discovery binding.
pub struct Actor {
    pub handle: AccessContextHandle,
    pub binding: TrustedDiscoveryBinding,
}

impl World {
    pub async fn new(registrations: Vec<RemoteSourceRegistration>) -> Self {
        let host = Arc::new(SyntheticHostRegistrationAuthority::new());
        host.publish(
            RegistrationNamespace::Document,
            RegistrationSetRevision::new(1).unwrap(),
            vec![],
        )
        .unwrap();
        host.publish(
            RegistrationNamespace::Remote,
            RegistrationSetRevision::new(1).unwrap(),
            registrations
                .iter()
                .cloned()
                .map(SourceRegistration::Remote)
                .collect(),
        )
        .unwrap();
        let document =
            CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Document)
                .await
                .unwrap();
        let remote = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
            .await
            .unwrap();
        let catalog = SourceRegistrationCatalog::try_new(
            Arc::new(SyntheticRegistrationLedger::with_host(host)),
            &document,
            &remote,
        )
        .await
        .unwrap();
        Self {
            authority: SyntheticAuthorityAdapter::new(),
            catalog,
            registrations,
        }
    }

    pub fn visibility(&self) -> SyntheticVisibilityAdapter<'_> {
        SyntheticVisibilityAdapter::new(&self.catalog)
    }

    /// Issues an actor of `tenant` and grants it every registration of that
    /// tenant on `visibility`.
    pub async fn actor(
        &self,
        tenant: &str,
        principal: &str,
        session: bool,
        visibility: &SyntheticVisibilityAdapter<'_>,
    ) -> Actor {
        let tenant = TenantId::new(tenant).unwrap();
        let handle = self
            .authority
            .issue_verified_identity(
                tenant.clone(),
                PrincipalRef::new(principal).unwrap(),
                session.then(|| SessionId::from_uuid(Uuid::now_v7())),
                AccessRevision::new(1).unwrap(),
                Duration::from_secs(120),
            )
            .unwrap();
        let actor = self.authority.resolve(&handle).await.unwrap().unwrap();
        for registration in self.registrations.iter().filter(|r| r.tenant() == &tenant) {
            visibility
                .grant(
                    tenant.clone(),
                    actor.principal().clone(),
                    registration.source_id(),
                    registration.registration_revision(),
                    registration.visibility_revision(),
                )
                .unwrap();
        }
        let binding = self
            .authority
            .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::now_v7()))
            .await
            .unwrap()
            .unwrap();
        Actor { handle, binding }
    }

    /// A fresh evaluation binding for the same actor.
    pub async fn rebind(&self, actor: &Actor) -> TrustedDiscoveryBinding {
        let scope = self
            .authority
            .resolve(&actor.handle)
            .await
            .unwrap()
            .unwrap();
        self.authority
            .bind_discovery(&scope, DiscoveryEvaluationId::from_uuid(Uuid::now_v7()))
            .await
            .unwrap()
            .unwrap()
    }

    pub async fn visible(
        &self,
        binding: &TrustedDiscoveryBinding,
        visibility: &SyntheticVisibilityAdapter<'_>,
    ) -> Vec<VisibleSourceRegistration> {
        TrustedVisibleRegistry::new(&self.authority, visibility, &self.catalog)
            .visible_sources(binding.actor())
            .await
            .unwrap()
            .entries()
            .to_vec()
    }

    pub async fn context(
        &self,
        binding: &TrustedDiscoveryBinding,
        source_id: SourceId,
        visibility: &SyntheticVisibilityAdapter<'_>,
    ) -> TrustedRemoteContext {
        let visible = self.visible(binding, visibility).await;
        let entry = visible
            .iter()
            .find(|entry| entry.scope().source_id() == source_id)
            .unwrap();
        TrustedRemoteContext::bind(binding.clone(), entry, &self.authority, visibility)
            .await
            .unwrap()
    }

    pub fn adapter<'a>(
        &'a self,
        registration: &RemoteSourceRegistration,
        visibility: &'a SyntheticVisibilityAdapter<'a>,
    ) -> HttpRemoteSourceAdapter<'a> {
        let transport = GuardedHttpTransport::new_loopback_for_test(
            registration.endpoint().clone(),
            Arc::new(Loopback(registration.endpoint().port())),
            TransportLimits::from_registration(registration.limits()),
        )
        .unwrap();
        HttpRemoteSourceAdapter::new(registration.clone(), transport, &self.authority, visibility)
            .unwrap()
    }
}
