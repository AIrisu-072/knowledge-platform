//! Shared trusted remote fixture for the P4 contract tests: one server-owned
//! remote registration in a host catalog, a synthetic actor, a visibility
//! grant and a host snapshot verifier. Runtime-generated values only.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use search_application::ports::BoxFuture;
use search_application::remote::{
    PlannedRemoteAction, RemoteActionOutcome, RemoteActionResponse, RemoteOperation, RemotePage,
    RemoteResponseInput, RemoteResponseStatus, TrustedRemoteContext, UntrustedRemoteHit,
};
use search_application::remote_observation::{
    CheckedRemoteObservationAdapter, RemoteSnapshotVerifierPort, SnapshotAttestation,
    SnapshotExtent,
};
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::retrieval::{OpaqueNativeId, RemoteQueryInput};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessContextHandle, AccessRevision, CurrentSourceVisibilityPort,
    PrincipalRef, RegistrationRevision, ScopedSourceRegistryPort, SyntheticAuthorityAdapter,
    SyntheticVisibilityAdapter, TenantId, TrustedDiscoveryBinding, VisibilityRevision,
    VisibleSourceRegistration,
};
use search_application::source_registration::{
    CompleteDesiredRegistrations, RegistrationNamespace, RegistrationSetRevision,
    SourceRegistration, SourceRegistrationCatalog, SyntheticHostRegistrationAuthority,
    SyntheticRegistrationLedger, TrustedVisibleRegistry,
};
use search_core::id::{DiscoveryEvaluationId, SessionId, SourceId};
use search_core::predicate::TypedValue;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn native(value: &str) -> OpaqueNativeId {
    OpaqueNativeId::new(value).unwrap()
}

pub fn registration(retention: RetentionMode) -> RemoteSourceRegistration {
    RemoteSourceRegistration::from_server_config(ServerRemoteRegistrationConfig {
        tenant: TenantId::new("tenant-a").unwrap(),
        source_id: SourceId::from_uuid(Uuid::from_u128(4_001)),
        provider_kind: "synthetic".into(),
        endpoint: RegisteredEndpoint::new("https", "catalog.example.test", 443, "/v1").unwrap(),
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

pub struct Remote {
    pub registration: RemoteSourceRegistration,
    pub authority: SyntheticAuthorityAdapter,
    pub catalog: SourceRegistrationCatalog,
    pub handle: AccessContextHandle,
    pub binding: TrustedDiscoveryBinding,
    pub visible: VisibleSourceRegistration,
}

impl Remote {
    pub async fn new(retention: RetentionMode) -> Self {
        Self::build(retention, None).await
    }

    /// The same fixture for an actor with a trusted session.
    pub async fn with_session(retention: RetentionMode) -> Self {
        Self::build(retention, Some(SessionId::from_uuid(Uuid::now_v7()))).await
    }

    async fn build(retention: RetentionMode, session: Option<SessionId>) -> Self {
        let registration = registration(retention);
        let host = Arc::new(SyntheticHostRegistrationAuthority::new());
        for (namespace, values) in [
            (RegistrationNamespace::Document, vec![]),
            (
                RegistrationNamespace::Remote,
                vec![SourceRegistration::Remote(registration.clone())],
            ),
        ] {
            host.publish(namespace, RegistrationSetRevision::new(1).unwrap(), values)
                .unwrap();
        }
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
        let authority = SyntheticAuthorityAdapter::new();
        let handle = authority
            .issue_verified_identity(
                registration.tenant().clone(),
                PrincipalRef::new("reader").unwrap(),
                session,
                AccessRevision::new(1).unwrap(),
                Duration::from_secs(60),
            )
            .unwrap();
        let actor = authority.resolve(&handle).await.unwrap().unwrap();
        let binding = authority
            .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::now_v7()))
            .await
            .unwrap()
            .unwrap();
        let visibility = SyntheticVisibilityAdapter::new(&catalog);
        grant(&visibility, &registration, &binding);
        let visible = TrustedVisibleRegistry::new(&authority, &visibility, &catalog)
            .visible_sources(&actor)
            .await
            .unwrap()[0]
            .clone();
        Self {
            registration,
            authority,
            catalog,
            handle,
            binding,
            visible,
        }
    }

    pub fn visibility(&self) -> SyntheticVisibilityAdapter<'_> {
        let visibility = SyntheticVisibilityAdapter::new(&self.catalog);
        grant(&visibility, &self.registration, &self.binding);
        visibility
    }

    pub async fn context(
        &self,
        visibility: &dyn CurrentSourceVisibilityPort,
    ) -> TrustedRemoteContext {
        TrustedRemoteContext::bind(
            self.binding.clone(),
            &self.visible,
            &self.authority,
            visibility,
        )
        .await
        .unwrap()
    }

    pub fn evaluation(&self) -> DiscoveryEvaluationId {
        self.binding.evaluation()
    }

    /// Re-resolves the handle (e.g. after an access revision change) and binds
    /// a fresh evaluation and visible registration for that actor.
    pub async fn rebind(
        &self,
        handle: &AccessContextHandle,
        visibility: &SyntheticVisibilityAdapter<'_>,
    ) -> TrustedRemoteContext {
        let actor = self.authority.resolve(handle).await.unwrap().unwrap();
        let binding = self
            .authority
            .bind_discovery(&actor, DiscoveryEvaluationId::from_uuid(Uuid::now_v7()))
            .await
            .unwrap()
            .unwrap();
        visibility
            .grant(
                self.registration.tenant().clone(),
                actor.principal().clone(),
                self.registration.source_id(),
                self.registration.registration_revision(),
                self.registration.visibility_revision(),
            )
            .unwrap();
        let visible = TrustedVisibleRegistry::new(&self.authority, visibility, &self.catalog)
            .visible_sources(&actor)
            .await
            .unwrap()[0]
            .clone();
        TrustedRemoteContext::bind(binding, &visible, &self.authority, visibility)
            .await
            .unwrap()
    }

    /// Another principal of the same tenant with its own visibility grant.
    pub async fn other_principal(
        &self,
        principal: &str,
        visibility: &SyntheticVisibilityAdapter<'_>,
    ) -> TrustedRemoteContext {
        let handle = self
            .authority
            .issue_verified_identity(
                self.registration.tenant().clone(),
                PrincipalRef::new(principal).unwrap(),
                None,
                AccessRevision::new(1).unwrap(),
                Duration::from_secs(60),
            )
            .unwrap();
        self.rebind(&handle, visibility).await
    }
}

/// A clock the test moves by hand.
pub struct ManualClock(std::sync::Mutex<std::time::Instant>);

impl ManualClock {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(std::sync::Mutex::new(std::time::Instant::now())))
    }
    pub fn advance(&self, by: Duration) {
        let mut now = self.0.lock().unwrap();
        *now += by;
    }
}

impl search_application::remote_lease::LeaseClock for ManualClock {
    fn now(&self) -> std::time::Instant {
        *self.0.lock().unwrap()
    }
}

pub fn grant(
    visibility: &SyntheticVisibilityAdapter<'_>,
    registration: &RemoteSourceRegistration,
    binding: &TrustedDiscoveryBinding,
) {
    visibility
        .grant(
            registration.tenant().clone(),
            binding.actor().principal().clone(),
            registration.source_id(),
            registration.registration_revision(),
            registration.visibility_revision(),
        )
        .unwrap();
}

/// Host snapshot verifier: one token and extent for every response.
pub struct Verifier {
    pub token: String,
    pub extent: SnapshotExtent,
}

impl Verifier {
    pub fn shared(token: &str) -> Self {
        Self {
            token: token.into(),
            extent: SnapshotExtent::PartialSource,
        }
    }
    pub fn single(token: &str) -> Self {
        Self {
            token: token.into(),
            extent: SnapshotExtent::SingleResponse,
        }
    }
}

impl RemoteSnapshotVerifierPort for Verifier {
    fn verify<'a>(
        &'a self,
        _context: &'a TrustedRemoteContext,
        _action: &'a PlannedRemoteAction,
        _input: &'a RemoteResponseInput,
    ) -> BoxFuture<'a, Option<SnapshotAttestation>> {
        Box::pin(async move {
            Ok(Some(SnapshotAttestation::new(
                self.token.clone(),
                self.extent,
                OffsetDateTime::now_utc(),
                vec![],
            )?))
        })
    }
}

/// A provider hit with optional version, digest and typed fields.
pub fn hit(
    id: Option<&str>,
    version: Option<&str>,
    digest: Option<&str>,
    fields: &[(&str, &str, Option<&str>)],
) -> UntrustedRemoteHit {
    UntrustedRemoteHit::new(
        id.map(native),
        version.map(Into::into),
        digest.map(Into::into),
    )
    .unwrap()
    .with_projection(
        Some(ResourceKind::Knowledge),
        Some("規程".into()),
        fields
            .iter()
            .map(|(name, value, provenance)| {
                (
                    (*name).to_owned(),
                    TypedValue::String((*value).to_owned()),
                    provenance.map(str::to_owned),
                )
            })
            .collect(),
    )
    .unwrap()
}

pub fn query() -> RemoteOperation {
    RemoteOperation::Query {
        input: RemoteQueryInput::new("規程", vec![], 10, &[]).unwrap(),
    }
}

pub fn lookup(id: &str) -> RemoteOperation {
    RemoteOperation::Lookup {
        native_id: native(id),
    }
}

/// Runs one observed action through the checked adapter.
pub async fn observe(
    remote: &Remote,
    visibility: &dyn CurrentSourceVisibilityPort,
    verifier: &dyn RemoteSnapshotVerifierPort,
    context: &TrustedRemoteContext,
    retriever: &str,
    operation: RemoteOperation,
    hits: Vec<UntrustedRemoteHit>,
) -> RemoteActionResponse {
    let action = PlannedRemoteAction::new(context, retriever, operation).unwrap();
    let adapter = CheckedRemoteObservationAdapter::new(
        &remote.registration,
        &remote.authority,
        visibility,
        verifier,
    );
    let input = RemoteResponseInput::new(
        RemoteResponseStatus::Success,
        RemotePage::Unpaged,
        hits,
        None,
    )
    .unwrap();
    match adapter.observe(context, &action, input).await.unwrap() {
        RemoteActionOutcome::Completed(response) => *response,
        other => panic!("expected a completed action: {other:?}"),
    }
}
