use search_application::ports::SourceRegistryPort;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::id::SourceId;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

#[tokio::test]
async fn registry_returns_capabilities_without_source_body() {
    let mut registry = InMemorySourceRegistry::default();
    let mut source = DiscoverableSource::new(
        source_id(),
        "remote-policy",
        EnumerationSemantics::QueryOnly,
        RetentionMode::NoRetention,
    );
    source.discovery_modes.push(DiscoveryMode::RemoteQuery);
    registry.insert(source);
    let resolved = registry.get_source(source_id()).await.unwrap().unwrap();
    assert!(resolved.supports(DiscoveryMode::RemoteQuery));
    assert_eq!(resolved.retention_mode, RetentionMode::NoRetention);
}
