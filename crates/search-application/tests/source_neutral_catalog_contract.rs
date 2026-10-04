use std::sync::Arc;
use std::time::Duration;

use search_application::ports::{AccessDecision, BoxFuture};
use search_application::remote_registration::{
    CurrentAccessContract, RegisteredEndpoint, RemoteRegistrationLimits, RemoteSourceRegistration,
    ServerRemoteRegistrationConfig,
};
use search_application::scoped::{
    AccessContextAuthorityPort, AccessRevision, AuthorizedSourceScope, CurrentSourceVisibilityPort,
    PrincipalRef, RegistrationRevision, ScopedSourceRegistryPort, SyntheticAuthorityAdapter,
    SyntheticVisibilityAdapter, TenantId, TrustedSearchScope, VisibilityRevision,
};
use search_application::search_core::id::SourceId;
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode};
use search_application::source_registration::{
    CompleteDesiredRegistrations, ConnectedDocumentAdapterCapabilities,
    ConnectedDocumentAdapterWitness, DocumentAdapterCapabilityPort, DocumentAdapterRef,
    DocumentSourceRegistration, RegistrationNamespace, RegistrationSetRevision,
    ServerDocumentRegistrationConfig, SourceKind, SourceRegistration, SourceRegistrationCatalog,
    SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority, SyntheticRegistrationLedger,
    TrustedVisibleRegistry,
};
use uuid::Uuid;

fn source(number: u128) -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(number))
}

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).unwrap()
}

fn revision(value: u64) -> RegistrationRevision {
    RegistrationRevision::new(value).unwrap()
}

fn vis_revision(value: u64) -> VisibilityRevision {
    VisibilityRevision::new(value).unwrap()
}

fn remote_config(id: SourceId) -> ServerRemoteRegistrationConfig {
    ServerRemoteRegistrationConfig {
        tenant: tenant("tenant-a"),
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
        registration_revision: revision(1),
        visibility_revision: vis_revision(1),
    }
}

fn remote(config: ServerRemoteRegistrationConfig) -> SourceRegistration {
    SourceRegistration::Remote(RemoteSourceRegistration::from_server_config(config).unwrap())
}

fn document_config(id: SourceId) -> ServerDocumentRegistrationConfig {
    ServerDocumentRegistrationConfig {
        tenant: tenant("tenant-a"),
        source_id: id,
        document_adapter_ref: DocumentAdapterRef::new("document-binding-a").unwrap(),
        allowed_resource_kinds: vec![ResourceKind::Document],
        supported_modes: vec![DiscoveryMode::LocalDirectory],
        enumeration_semantics: EnumerationSemantics::Partial,
        retention_mode: RetentionMode::PersistentResource,
        registration_revision: revision(1),
        visibility_revision: vis_revision(1),
    }
}

struct ConnectedDocumentAdapter {
    capabilities: Option<ConnectedDocumentAdapterCapabilities>,
}

impl DocumentAdapterCapabilityPort for ConnectedDocumentAdapter {
    fn connected_capabilities<'a>(
        &'a self,
        _binding: &'a DocumentAdapterRef,
    ) -> BoxFuture<'a, Option<ConnectedDocumentAdapterCapabilities>> {
        Box::pin(async move { Ok(self.capabilities.clone()) })
    }
}

fn connected_capabilities(binding: &str) -> ConnectedDocumentAdapterCapabilities {
    ConnectedDocumentAdapterCapabilities::new(
        DocumentAdapterRef::new(binding).unwrap(),
        vec![ResourceKind::Document],
        vec![DiscoveryMode::LocalDirectory],
        vec![EnumerationSemantics::Partial],
        vec![RetentionMode::PersistentResource],
    )
    .unwrap()
}

async fn document(config: ServerDocumentRegistrationConfig) -> SourceRegistration {
    let adapter = ConnectedDocumentAdapter {
        capabilities: Some(connected_capabilities("document-binding-a")),
    };
    let witness = ConnectedDocumentAdapterWitness::from_connected_port(
        &adapter,
        &config.document_adapter_ref,
    )
    .await
    .unwrap()
    .unwrap();
    SourceRegistration::Document(
        DocumentSourceRegistration::from_server_config(config, &witness).unwrap(),
    )
}

async fn digest(
    namespace: RegistrationNamespace,
    registrations: Vec<SourceRegistration>,
) -> [u8; 32] {
    let host = SyntheticHostRegistrationAuthority::new();
    host.publish(
        namespace,
        RegistrationSetRevision::new(2).unwrap(),
        registrations,
    )
    .unwrap();
    CompleteDesiredRegistrations::capture(&host, namespace)
        .await
        .unwrap()
        .set_digest()
        .as_bytes()
}

#[tokio::test]
async fn document_requires_connected_capability_witness() {
    let config = document_config(source(1));
    let disconnected = ConnectedDocumentAdapter { capabilities: None };
    assert!(
        ConnectedDocumentAdapterWitness::from_connected_port(
            &disconnected,
            &config.document_adapter_ref
        )
        .await
        .unwrap()
        .is_none()
    );

    let wrong = ConnectedDocumentAdapter {
        capabilities: Some(connected_capabilities("other-binding")),
    };
    assert!(
        ConnectedDocumentAdapterWitness::from_connected_port(&wrong, &config.document_adapter_ref)
            .await
            .is_err()
    );
    assert!(DocumentAdapterRef::new("postgres://server/credential").is_err());
}

#[tokio::test]
async fn document_rejects_remote_or_unwired_local_mode() {
    let adapter = ConnectedDocumentAdapter {
        capabilities: Some(connected_capabilities("document-binding-a")),
    };
    let mut config = document_config(source(2));
    let witness = ConnectedDocumentAdapterWitness::from_connected_port(
        &adapter,
        &config.document_adapter_ref,
    )
    .await
    .unwrap()
    .unwrap();
    config.supported_modes = vec![DiscoveryMode::RemoteQuery];
    assert!(DocumentSourceRegistration::from_server_config(config.clone(), &witness).is_err());
    config.supported_modes = vec![DiscoveryMode::LocalContentSearch];
    assert!(DocumentSourceRegistration::from_server_config(config.clone(), &witness).is_err());
    config.supported_modes = vec![DiscoveryMode::LocalDirectory];
    config.enumeration_semantics = EnumerationSemantics::Complete;
    assert!(DocumentSourceRegistration::from_server_config(config.clone(), &witness).is_err());
    config.enumeration_semantics = EnumerationSemantics::Partial;
    config.retention_mode = RetentionMode::NoRetention;
    assert!(DocumentSourceRegistration::from_server_config(config, &witness).is_err());
}

#[tokio::test]
async fn document_descriptor_and_safe_capability_are_derived() {
    let registration = document(document_config(source(3))).await;
    let descriptor = registration.authority_descriptor();
    assert_eq!(descriptor.kind(), SourceKind::Document);
    assert_eq!(descriptor.source_id(), source(3));
    assert_eq!(descriptor.tenant(), &tenant("tenant-a"));
    let projected = registration.discoverable_source();
    assert!(projected.supports(DiscoveryMode::LocalDirectory));
    assert!(!projected.supports(DiscoveryMode::LocalContentSearch));
    assert_eq!(
        projected.enumeration_semantics,
        EnumerationSemantics::Partial
    );
    assert!(projected.authority_scope.is_none());
    assert!(projected.access_model.is_none());
    assert!(projected.freshness_policy.is_none());
    assert!(!format!("{projected:?}").contains("document-binding-a"));
}

#[tokio::test]
async fn canonical_set_digest_covers_every_remote_and_document_field() {
    let id = source(4);
    let baseline = remote_config(id);
    let expected_dto = remote(baseline.clone()).persistence_dto_v1().unwrap();
    let expected = digest(
        RegistrationNamespace::Remote,
        vec![remote(baseline.clone())],
    )
    .await;
    let mut variants = Vec::new();
    let mut value = baseline.clone();
    value.tenant = tenant("tenant-b");
    variants.push(value);
    let mut value = baseline.clone();
    value.source_id = source(5);
    variants.push(value);
    let mut value = baseline.clone();
    value.provider_kind = "provider-b".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.endpoint = RegisteredEndpoint::new("http", "example.test", 443, "/v1").unwrap();
    variants.push(value);
    let mut value = baseline.clone();
    value.endpoint = RegisteredEndpoint::new("https", "other.test", 443, "/v1").unwrap();
    variants.push(value);
    let mut value = baseline.clone();
    value.endpoint = RegisteredEndpoint::new("https", "example.test", 8443, "/v1").unwrap();
    variants.push(value);
    let mut value = baseline.clone();
    value.endpoint = RegisteredEndpoint::new("https", "example.test", 443, "/v2").unwrap();
    variants.push(value);
    let mut value = baseline.clone();
    value.supported_modes.reverse();
    variants.push(value);
    let mut value = baseline.clone();
    value.enumeration_semantics = EnumerationSemantics::Partial;
    variants.push(value);
    let mut value = baseline.clone();
    value.authority_predicates = vec!["policy-b".into()];
    variants.push(value);
    let mut value = baseline.clone();
    value.allowed_resource_kinds = vec![ResourceKind::Document];
    variants.push(value);
    let mut value = baseline.clone();
    value.current_access_contract = CurrentAccessContract::PublicReadWithFieldPolicy;
    variants.push(value);
    let mut value = baseline.clone();
    value.retention_mode = RetentionMode::NoRetention;
    variants.push(value);
    let mut value = baseline.clone();
    value.freshness_policy = None;
    variants.push(value);
    let mut value = baseline.clone();
    value.canonical_upstream_lineage = "lineage-b".into();
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.call_millis += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.evaluation_millis += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_request_bytes += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_decoded_response_bytes += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_hits_per_page += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_pages_or_requests += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_hits += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_actions += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_native_id_bytes += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_cursor_bytes += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.limits.max_json_depth += 1;
    variants.push(value);
    let mut value = baseline.clone();
    value.registration_revision = revision(2);
    variants.push(value);
    let mut value = baseline.clone();
    value.visibility_revision = vis_revision(2);
    variants.push(value);
    for (field, variant) in variants.into_iter().enumerate() {
        assert_ne!(
            expected_dto,
            remote(variant.clone()).persistence_dto_v1().unwrap(),
            "remote DTO field mutation {field}"
        );
        assert_ne!(
            expected,
            digest(RegistrationNamespace::Remote, vec![remote(variant)]).await,
            "remote field mutation {field}"
        );
    }

    let baseline = document_config(source(6));
    let expected_dto = document(baseline.clone())
        .await
        .persistence_dto_v1()
        .unwrap();
    let expected = digest(
        RegistrationNamespace::Document,
        vec![document(baseline.clone()).await],
    )
    .await;
    let mut variants = Vec::new();
    let mut value = baseline.clone();
    value.tenant = tenant("tenant-b");
    variants.push(value);
    let mut value = baseline.clone();
    value.source_id = source(7);
    variants.push(value);
    let mut value = baseline.clone();
    value.document_adapter_ref = DocumentAdapterRef::new("document-binding-b").unwrap();
    variants.push(value);
    let mut value = baseline.clone();
    value.allowed_resource_kinds = vec![ResourceKind::Document, ResourceKind::Knowledge];
    variants.push(value);
    let mut value = baseline.clone();
    value.supported_modes = vec![
        DiscoveryMode::LocalDirectory,
        DiscoveryMode::LocalContentSearch,
    ];
    variants.push(value);
    let mut value = baseline.clone();
    value.enumeration_semantics = EnumerationSemantics::Complete;
    variants.push(value);
    let mut value = baseline.clone();
    value.retention_mode = RetentionMode::SessionOnly;
    variants.push(value);
    let mut value = baseline.clone();
    value.registration_revision = revision(2);
    variants.push(value);
    let mut value = baseline.clone();
    value.visibility_revision = vis_revision(2);
    variants.push(value);
    for (field, variant) in variants.into_iter().enumerate() {
        if (2..=6).contains(&field) {
            // Changed ability is witnessed by the connected adapter.
            let adapter = ConnectedDocumentAdapter {
                capabilities: Some(
                    ConnectedDocumentAdapterCapabilities::new(
                        variant.document_adapter_ref.clone(),
                        variant.allowed_resource_kinds.clone(),
                        vec![DiscoveryMode::LocalDirectory],
                        vec![
                            EnumerationSemantics::Partial,
                            EnumerationSemantics::Complete,
                        ],
                        vec![
                            RetentionMode::PersistentResource,
                            RetentionMode::SessionOnly,
                        ],
                    )
                    .unwrap(),
                ),
            };
            let adapter = if field == 4 {
                ConnectedDocumentAdapter {
                    capabilities: Some(
                        ConnectedDocumentAdapterCapabilities::new(
                            variant.document_adapter_ref.clone(),
                            variant.allowed_resource_kinds.clone(),
                            variant.supported_modes.clone(),
                            vec![EnumerationSemantics::Partial],
                            vec![RetentionMode::PersistentResource],
                        )
                        .unwrap(),
                    ),
                }
            } else {
                adapter
            };
            let witness = ConnectedDocumentAdapterWitness::from_connected_port(
                &adapter,
                &variant.document_adapter_ref,
            )
            .await
            .unwrap()
            .unwrap();
            let changed = SourceRegistration::Document(
                DocumentSourceRegistration::from_server_config(variant, &witness).unwrap(),
            );
            assert_ne!(
                expected_dto,
                changed.persistence_dto_v1().unwrap(),
                "document DTO field mutation {field}"
            );
            assert_ne!(
                expected,
                digest(RegistrationNamespace::Document, vec![changed]).await,
                "document field mutation {field}"
            );
            continue;
        }
        let changed = document(variant).await;
        assert_ne!(
            expected_dto,
            changed.persistence_dto_v1().unwrap(),
            "document DTO field mutation {field}"
        );
        assert_ne!(
            expected,
            digest(RegistrationNamespace::Document, vec![changed]).await,
            "document field mutation {field}"
        );
    }
}

#[tokio::test]
async fn canonical_framing_distinguishes_empty_none_unicode_and_field_boundaries() {
    let mut base = remote_config(source(10));
    base.freshness_policy = None;
    let absent = digest(RegistrationNamespace::Remote, vec![remote(base.clone())]).await;
    base.freshness_policy = Some(String::new());
    let empty = digest(RegistrationNamespace::Remote, vec![remote(base.clone())]).await;
    assert_ne!(absent, empty);

    let mut left = base.clone();
    left.provider_kind = "ab".into();
    left.canonical_upstream_lineage = "c".into();
    let mut right = base.clone();
    right.provider_kind = "a".into();
    right.canonical_upstream_lineage = "bc".into();
    assert_ne!(
        digest(RegistrationNamespace::Remote, vec![remote(left)]).await,
        digest(RegistrationNamespace::Remote, vec![remote(right)]).await
    );

    base.freshness_policy = Some("本文　テスト".into());
    let unicode = digest(RegistrationNamespace::Remote, vec![remote(base.clone())]).await;
    base.freshness_policy = Some("本文 テスト".into());
    assert_ne!(
        unicode,
        digest(RegistrationNamespace::Remote, vec![remote(base)]).await
    );
}

#[tokio::test]
async fn digest_is_stable_across_map_insertion_order() {
    let a = remote(remote_config(source(8)));
    let b = remote(remote_config(source(9)));
    assert_eq!(
        digest(RegistrationNamespace::Remote, vec![a.clone(), b.clone()]).await,
        digest(RegistrationNamespace::Remote, vec![b, a]).await,
    );
    assert_ne!(
        digest(RegistrationNamespace::Remote, vec![]).await,
        digest(RegistrationNamespace::Document, vec![]).await,
    );
}

#[tokio::test]
async fn canonical_v1_digest_golden_vectors() {
    let hex =
        |bytes: [u8; 32]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    assert_eq!(
        hex(digest(RegistrationNamespace::Document, vec![]).await),
        "681ffd78855326989a36e135be5db597ec3ef0ceb1f1c4d3a3ee08ab037d91a4"
    );
    assert_eq!(
        hex(digest(RegistrationNamespace::Remote, vec![]).await),
        "e6cd600c96029b6fcab4a5c9089738e7be7ef9bfbcfa3a37c6cb230d2f89125b"
    );
    assert_eq!(
        hex(digest(
            RegistrationNamespace::Document,
            vec![document(document_config(source(6))).await]
        )
        .await),
        "662d45f33a8616e5686fe2000f96cf9a72d4bee7bf0293ba0e8922c42c1ff1c4"
    );
    assert_eq!(
        hex(digest(
            RegistrationNamespace::Remote,
            vec![remote(remote_config(source(4)))]
        )
        .await),
        "cc61a3e6154a7f3dc99c1e5411dde64c1708fbeb5367869405a024c426edd84f"
    );
}

fn remote_at(id: SourceId, tenant_name: &str, registration_revision: u64) -> SourceRegistration {
    let mut config = remote_config(id);
    config.tenant = tenant(tenant_name);
    config.registration_revision = revision(registration_revision);
    remote(config)
}

async fn complete(
    host: &SyntheticHostRegistrationAuthority,
    namespace: RegistrationNamespace,
    revision_number: u64,
    registrations: Vec<SourceRegistration>,
) -> CompleteDesiredRegistrations {
    host.publish(
        namespace,
        RegistrationSetRevision::new(revision_number).unwrap(),
        registrations,
    )
    .unwrap();
    CompleteDesiredRegistrations::capture(host, namespace)
        .await
        .unwrap()
}

async fn catalog_with(
    documents: Vec<SourceRegistration>,
    remotes: Vec<SourceRegistration>,
) -> (
    Arc<SyntheticHostRegistrationAuthority>,
    Arc<SyntheticRegistrationLedger>,
    SourceRegistrationCatalog,
) {
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let doc = complete(&host, RegistrationNamespace::Document, 1, documents).await;
    let remote = complete(&host, RegistrationNamespace::Remote, 1, remotes).await;
    let ledger = Arc::new(SyntheticRegistrationLedger::with_host(host.clone()));
    let catalog = SourceRegistrationCatalog::try_new(ledger.clone(), &doc, &remote)
        .await
        .unwrap();
    (host, ledger, catalog)
}

#[tokio::test]
async fn remote_reconcile_never_tombstones_document() {
    let doc_id = source(101);
    let remote_id = source(102);
    let doc = document(document_config(doc_id)).await;
    let remote = remote_at(remote_id, "tenant-b", 1);
    let (host, ledger, catalog) = catalog_with(vec![doc.clone()], vec![remote]).await;
    let doc_activation = ledger
        .state_for_testing()
        .unwrap()
        .activation(doc_id)
        .unwrap();
    let desired = complete(&host, RegistrationNamespace::Remote, 2, vec![]).await;
    catalog.replace_checked(&desired).await.unwrap();
    assert_eq!(
        ledger.state_for_testing().unwrap().activation(doc_id),
        Some(doc_activation)
    );
    assert!(ledger.is_current(&doc, doc_activation).await.unwrap());
    assert!(matches!(
        catalog.get_for_server(doc_id),
        Some(SourceRegistration::Document(_))
    ));
    assert!(catalog.get_for_server(remote_id).is_none());
}

#[tokio::test]
async fn partial_tenant_map_cannot_tombstone_foreign_remote() {
    let a = remote_at(source(103), "tenant-a", 1);
    let b = remote_at(source(104), "tenant-b", 1);
    let (host, ledger, _catalog) = catalog_with(vec![], vec![a.clone(), b.clone()]).await;
    let before = ledger.state_for_testing().unwrap();
    let _full = complete(&host, RegistrationNamespace::Remote, 2, vec![a.clone(), b]).await;
    let unrelated = SyntheticHostRegistrationAuthority::new();
    let partial = complete(&unrelated, RegistrationNamespace::Remote, 2, vec![a]).await;
    assert!(ledger.reconcile(&partial).await.is_err());
    assert_eq!(before, ledger.state_for_testing().unwrap());
    // The actual host still owns both tenant registrations.
    let current = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
        .await
        .unwrap();
    assert_eq!(current.len(), 2);
}

#[tokio::test]
async fn partial_foreign_capture_cannot_replace_catalog_or_tombstone_other_tenant() {
    let a = remote_at(source(119), "tenant-a", 1);
    let b = remote_at(source(120), "tenant-b", 1);
    let b_id = b.source_id();
    let (host, ledger, catalog) = catalog_with(vec![], vec![a.clone(), b.clone()]).await;
    let before = ledger.state_for_testing().unwrap();
    let b_activation = before.activation(b_id).unwrap();
    let _complete = complete(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![a.clone(), b.clone()],
    )
    .await;
    let foreign = SyntheticHostRegistrationAuthority::new();
    let partial = complete(&foreign, RegistrationNamespace::Remote, 2, vec![a]).await;

    assert!(catalog.replace_checked(&partial).await.is_err());
    assert_eq!(ledger.state_for_testing().unwrap(), before);
    assert!(ledger.is_current(&b, b_activation).await.unwrap());
    assert_eq!(
        CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn document_remote_same_source_id_is_rejected_after_tombstone() {
    let id = source(105);
    let doc = document(document_config(id)).await;
    let (host, ledger, catalog) = catalog_with(vec![doc], vec![]).await;
    let empty = complete(&host, RegistrationNamespace::Document, 2, vec![]).await;
    catalog.replace_checked(&empty).await.unwrap();
    let before = ledger.state_for_testing().unwrap();
    let remote = complete(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![remote_at(id, "tenant-a", 1)],
    )
    .await;
    assert!(ledger.reconcile(&remote).await.is_err());
    assert_eq!(before, ledger.state_for_testing().unwrap());
}

#[tokio::test]
async fn partial_or_stale_remote_desired_set_is_atomic_failure() {
    let a = remote_at(source(106), "tenant-a", 1);
    let b = remote_at(source(107), "tenant-b", 1);
    let (host, ledger, _catalog) = catalog_with(vec![], vec![a.clone(), b.clone()]).await;
    let before = ledger.state_for_testing().unwrap();
    let _full = complete(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![a.clone(), b.clone()],
    )
    .await;
    let foreign = SyntheticHostRegistrationAuthority::new();
    let partial = complete(&foreign, RegistrationNamespace::Remote, 2, vec![a.clone()]).await;
    assert!(ledger.reconcile(&partial).await.is_err());
    assert_eq!(before, ledger.state_for_testing().unwrap());

    let stale = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
        .await
        .unwrap();
    let _next = complete(&host, RegistrationNamespace::Remote, 3, vec![a, b]).await;
    assert!(ledger.reconcile(&stale).await.is_err());
    assert_eq!(before, ledger.state_for_testing().unwrap());
}

#[tokio::test]
async fn same_revision_same_digest_is_idempotent() {
    let (host, ledger, _catalog) =
        catalog_with(vec![], vec![remote_at(source(108), "tenant-a", 1)]).await;
    let desired = CompleteDesiredRegistrations::capture(&*host, RegistrationNamespace::Remote)
        .await
        .unwrap();
    let before = ledger.state_for_testing().unwrap();
    let first = ledger.reconcile(&desired).await.unwrap();
    let second = ledger.reconcile(&desired).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(before, ledger.state_for_testing().unwrap());
}

#[tokio::test]
async fn old_activation_and_whole_dto_fail_current() {
    let id = source(109);
    let original = remote_at(id, "tenant-a", 1);
    let (host, ledger, catalog) = catalog_with(vec![], vec![original.clone()]).await;
    let old_activation = ledger.state_for_testing().unwrap().activation(id).unwrap();
    let mut changed_config = remote_config(id);
    changed_config.registration_revision = revision(2);
    changed_config.endpoint = RegisteredEndpoint::new("https", "changed.test", 443, "/v1").unwrap();
    let changed = remote(changed_config);
    let desired = complete(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![changed.clone()],
    )
    .await;
    catalog.replace_checked(&desired).await.unwrap();
    let current_activation = ledger.state_for_testing().unwrap().activation(id).unwrap();
    assert_ne!(old_activation, current_activation);
    assert!(!ledger.is_current(&original, old_activation).await.unwrap());
    assert!(
        !ledger
            .is_current(&original, current_activation)
            .await
            .unwrap()
    );
    assert!(!ledger.is_current(&changed, old_activation).await.unwrap());
    assert!(
        ledger
            .is_current(&changed, current_activation)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn unknown_commit_blocks_local_projection() {
    let id = source(110);
    let original = remote_at(id, "tenant-a", 1);
    let (host, ledger, catalog) = catalog_with(vec![], vec![original]).await;
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            PrincipalRef::new("alice").unwrap(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    visibility
        .grant(
            tenant("tenant-a"),
            PrincipalRef::new("alice").unwrap(),
            id,
            revision(1),
            vis_revision(1),
        )
        .unwrap();
    assert!(visibility.bind_source(&actor, id).await.unwrap().is_some());
    let desired = complete(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![remote_at(id, "tenant-a", 2)],
    )
    .await;
    ledger.fail_after_commit_once();
    assert!(catalog.replace_checked(&desired).await.is_err());
    assert!(visibility.bind_source(&actor, id).await.unwrap().is_none());
    catalog.replace_checked(&desired).await.unwrap();
    visibility
        .grant(
            tenant("tenant-a"),
            PrincipalRef::new("alice").unwrap(),
            id,
            revision(2),
            vis_revision(1),
        )
        .unwrap();
    assert!(visibility.bind_source(&actor, id).await.unwrap().is_some());
}

#[tokio::test]
async fn union_catalog_includes_document_and_remote_with_one_scope() {
    let doc_id = source(111);
    let remote_id = source(112);
    let (_host, _ledger, catalog) = catalog_with(
        vec![document(document_config(doc_id)).await],
        vec![remote_at(remote_id, "tenant-a", 1)],
    )
    .await;
    let authority = SyntheticAuthorityAdapter::new();
    let principal = PrincipalRef::new("alice").unwrap();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal.clone(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [doc_id, remote_id] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal.clone(),
                id,
                revision(1),
                vis_revision(1),
            )
            .unwrap();
    }
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
    let snapshot = registry.visible_sources(&actor).await.unwrap();
    assert_eq!(snapshot.entries().len(), 2);
    assert!(snapshot.continuation_stamp().is_none());
    assert!(
        snapshot
            .entries()
            .iter()
            .all(|entry| entry.scope().actor() == &actor)
    );
    assert!(
        snapshot
            .entries()
            .iter()
            .any(|entry| entry.registration().kind() == SourceKind::Document)
    );
    assert!(
        snapshot
            .entries()
            .iter()
            .any(|entry| entry.registration().kind() == SourceKind::Remote)
    );
    assert_eq!(
        visibility
            .current(snapshot.entries()[0].scope())
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
}

#[tokio::test]
async fn unstamped_snapshot_has_no_cursor_authority() {
    let (_host, _ledger, catalog) = catalog_with(vec![], vec![]).await;
    let authority = SyntheticAuthorityAdapter::new();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            PrincipalRef::new("alice").unwrap(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
    let snapshot = registry.visible_sources(&actor).await.unwrap();
    assert!(snapshot.entries().is_empty());
    assert!(snapshot.continuation_stamp().is_none());
}

struct UnknownOneSource<'a, 'b> {
    inner: &'a SyntheticVisibilityAdapter<'b>,
    unknown: SourceId,
}

impl CurrentSourceVisibilityPort for UnknownOneSource<'_, '_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>> {
        self.inner.bind_source(actor, source)
    }

    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if scope.source_id() == self.unknown {
                Ok(AccessDecision::Unknown)
            } else {
                self.inner.current(scope).await
            }
        })
    }
}

#[tokio::test]
async fn visibility_denied_or_unknown_keeps_other_source() {
    let doc_id = source(113);
    let remote_id = source(114);
    let (_host, _ledger, catalog) = catalog_with(
        vec![document(document_config(doc_id)).await],
        vec![remote_at(remote_id, "tenant-a", 1)],
    )
    .await;
    let authority = SyntheticAuthorityAdapter::new();
    let principal = PrincipalRef::new("alice").unwrap();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal.clone(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    visibility
        .grant(
            tenant("tenant-a"),
            principal.clone(),
            doc_id,
            revision(1),
            vis_revision(1),
        )
        .unwrap();
    visibility
        .grant(
            tenant("tenant-a"),
            principal.clone(),
            remote_id,
            revision(1),
            vis_revision(1),
        )
        .unwrap();
    let unknown = UnknownOneSource {
        inner: &visibility,
        unknown: doc_id,
    };
    let registry = TrustedVisibleRegistry::new(&authority, &unknown, &catalog);
    let snapshot = registry.visible_sources(&actor).await.unwrap();
    assert_eq!(snapshot.entries().len(), 1);
    assert_eq!(snapshot.entries()[0].scope().source_id(), remote_id);
    visibility.revoke(&principal, doc_id).unwrap();
    let registry = TrustedVisibleRegistry::new(&authority, &visibility, &catalog);
    let snapshot = registry.visible_sources(&actor).await.unwrap();
    assert_eq!(snapshot.entries().len(), 1);
    assert_eq!(snapshot.entries()[0].scope().source_id(), remote_id);
}

struct AdvanceDocumentDuringBind<'a, 'b> {
    inner: &'a SyntheticVisibilityAdapter<'b>,
    catalog: &'b SourceRegistrationCatalog,
    host: Arc<SyntheticHostRegistrationAuthority>,
    document: SourceId,
}

impl CurrentSourceVisibilityPort for AdvanceDocumentDuringBind<'_, '_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>> {
        Box::pin(async move {
            if source == self.document {
                let mut config = document_config(source);
                config.registration_revision = revision(2);
                let updated = document(config).await;
                let desired = complete(
                    &self.host,
                    RegistrationNamespace::Document,
                    2,
                    vec![updated],
                )
                .await;
                self.catalog.replace_checked(&desired).await?;
                self.inner.grant(
                    tenant("tenant-a"),
                    PrincipalRef::new("alice").unwrap(),
                    source,
                    revision(2),
                    vis_revision(1),
                )?;
            }
            self.inner.bind_source(actor, source).await
        })
    }

    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision> {
        self.inner.current(scope)
    }
}

#[tokio::test]
async fn catalog_revision_race_omits_only_changed_source() {
    let doc_id = source(115);
    let remote_id = source(116);
    let (host, _ledger, catalog) = catalog_with(
        vec![document(document_config(doc_id)).await],
        vec![remote_at(remote_id, "tenant-a", 1)],
    )
    .await;
    let authority = SyntheticAuthorityAdapter::new();
    let principal = PrincipalRef::new("alice").unwrap();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal.clone(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [doc_id, remote_id] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal.clone(),
                id,
                revision(1),
                vis_revision(1),
            )
            .unwrap();
    }
    let advancing = AdvanceDocumentDuringBind {
        inner: &visibility,
        catalog: &catalog,
        host,
        document: doc_id,
    };
    let registry = TrustedVisibleRegistry::new(&authority, &advancing, &catalog);
    let snapshot = registry.visible_sources(&actor).await.unwrap();
    assert_eq!(snapshot.entries().len(), 1);
    assert_eq!(snapshot.entries()[0].scope().source_id(), remote_id);
}

struct FailingOneSource<'a, 'b> {
    inner: &'a SyntheticVisibilityAdapter<'b>,
    failed: SourceId,
}

struct WrongSourceScope {
    scope: AuthorizedSourceScope,
}

impl CurrentSourceVisibilityPort for WrongSourceScope {
    fn bind_source<'a>(
        &'a self,
        _actor: &'a TrustedSearchScope,
        _source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>> {
        Box::pin(async move { Ok(Some(self.scope.clone())) })
    }

    fn current<'a>(&'a self, _scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async { Ok(AccessDecision::Allowed) })
    }
}

impl CurrentSourceVisibilityPort for FailingOneSource<'_, '_> {
    fn bind_source<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<AuthorizedSourceScope>> {
        self.inner.bind_source(actor, source)
    }

    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision> {
        Box::pin(async move {
            if scope.source_id() == self.failed {
                Err(search_application::SearchError::OperationFailed(
                    "visibility unavailable".into(),
                ))
            } else {
                self.inner.current(scope).await
            }
        })
    }
}

#[tokio::test]
async fn structural_mismatch_or_infrastructure_error_fails_whole_snapshot() {
    let doc_id = source(117);
    let remote_id = source(118);
    let (_host, _ledger, catalog) = catalog_with(
        vec![document(document_config(doc_id)).await],
        vec![remote_at(remote_id, "tenant-a", 1)],
    )
    .await;
    let authority = SyntheticAuthorityAdapter::new();
    let principal = PrincipalRef::new("alice").unwrap();
    let handle = authority
        .issue_verified_identity(
            tenant("tenant-a"),
            principal.clone(),
            None,
            AccessRevision::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();
    let actor = authority.resolve(&handle).await.unwrap().unwrap();
    let visibility = SyntheticVisibilityAdapter::new(&catalog);
    for id in [doc_id, remote_id] {
        visibility
            .grant(
                tenant("tenant-a"),
                principal.clone(),
                id,
                revision(1),
                vis_revision(1),
            )
            .unwrap();
    }
    let failing = FailingOneSource {
        inner: &visibility,
        failed: doc_id,
    };
    let registry = TrustedVisibleRegistry::new(&authority, &failing, &catalog);
    assert!(matches!(
        registry.visible_sources(&actor).await,
        Err(search_application::SearchError::OperationFailed(_))
    ));
    let wrong = WrongSourceScope {
        scope: visibility
            .bind_source(&actor, remote_id)
            .await
            .unwrap()
            .unwrap(),
    };
    let registry = TrustedVisibleRegistry::new(&authority, &wrong, &catalog);
    assert!(matches!(
        registry.visible_sources(&actor).await,
        Err(search_application::SearchError::OperationFailed(_))
    ));
}

#[test]
fn durable_registration_view_keeps_all_remote_fields_and_exact_visibility() {
    let value = remote(remote_config(source(9001)));
    let dto = value.persistence_dto_v1().unwrap();
    assert_eq!(dto["dto_version"], "v1");
    assert_eq!(dto["source_kind"], "REMOTE");
    assert_eq!(dto["tenant_owner_key"], "tenant-a");
    assert_eq!(dto["registration_revision"], 1);
    assert_eq!(dto["visibility_revision"], 1);
    assert_eq!(dto["definition"].as_object().unwrap().len(), 11);
    assert_eq!(dto["definition"]["limits"].as_object().unwrap().len(), 11);
    let mut changed = dto.clone();
    changed["visibility_revision"] = serde_json::json!(2);
    assert!(value.matches_persisted_definition_v1(&changed).unwrap());
    changed["definition"]["endpoint"]["host"] = serde_json::json!("different.test");
    assert!(!value.matches_persisted_definition_v1(&changed).unwrap());
    let mut unknown = dto.clone();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(!value.matches_persisted_definition_v1(&unknown).unwrap());
}

#[tokio::test]
async fn durable_registration_digest_reuses_canonical_single_source_set() {
    let value = remote(remote_config(source(9002)));
    let host = SyntheticHostRegistrationAuthority::new();
    host.publish(
        RegistrationNamespace::Remote,
        RegistrationSetRevision::new(1).unwrap(),
        vec![value.clone()],
    )
    .unwrap();
    let desired = CompleteDesiredRegistrations::capture(&host, RegistrationNamespace::Remote)
        .await
        .unwrap();
    assert_eq!(value.persistence_digest_v1().unwrap(), desired.set_digest());
    let mut config = remote_config(source(9002));
    config.limits.max_json_depth += 1;
    assert_ne!(
        value.persistence_digest_v1().unwrap(),
        remote(config).persistence_digest_v1().unwrap()
    );
}
