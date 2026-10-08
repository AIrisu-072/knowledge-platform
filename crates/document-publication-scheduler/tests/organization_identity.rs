use document_application::{
    IdentityContextResolver, IdentityResolutionError, InvocationKind, authorize_scheduled_publish,
};
use document_domain::PrincipalRef;
use document_publication_scheduler::{StaticRequesterResolver, scheduler_executor};
use organization_server::{OrganizationProfile, SyntheticIdentityAdapter};

#[tokio::test]
async fn organization_requesters_match_the_existing_http_adapter_without_extra_authority() {
    let resolver = StaticRequesterResolver::for_runtime_mode("organization-synthetic").unwrap();
    for profile in OrganizationProfile::ALL {
        let expected = SyntheticIdentityAdapter::new(profile)
            .current_context()
            .unwrap();
        let actual = resolver.resolve(expected.principal()).await.unwrap();
        assert_eq!(actual.principal(), expected.principal());
        assert_eq!(actual.subjects(), expected.subjects());
        assert_eq!(actual.subjects().len(), 1);
        assert_eq!(actual.invocation_kind(), InvocationKind::HumanInteractive);
        assert_eq!(actual.service_executor(), None);
        assert!(actual.ensure_current().is_ok());
        let executor = scheduler_executor();
        let due = authorize_scheduled_publish(&resolver, expected.principal(), &executor)
            .await
            .unwrap();
        assert_eq!(due.principal(), expected.principal());
        assert_eq!(due.subjects(), expected.subjects());
        assert_eq!(due.invocation_kind(), InvocationKind::Service);
        assert_eq!(due.service_executor(), Some(&executor));
    }
}

#[tokio::test]
async fn runtime_selection_isolates_providers_and_rejects_unlisted_or_executor_identities() {
    for mode in ["poc", "organization-synthetic"] {
        let resolver = StaticRequesterResolver::for_runtime_mode(mode).unwrap();
        let mut invalid = vec![
            ("organization-synthetic", "unknown"),
            ("organization-synthetic", "sales-02"),
            ("organization-synthetic", "Sales-01"),
            ("organization-synthetic", "scheduler"),
            ("organization-synthetic", "poc-agent"),
            ("service", "scheduler"),
            ("other", "sales-01"),
            ("poc", "sales-01"),
        ];
        if mode == "poc" {
            invalid.extend(
                OrganizationProfile::ALL.map(|p| ("organization-synthetic", p.principal())),
            );
        } else {
            invalid.extend([("poc", "poc-human"), ("poc", "poc-agent")]);
        }
        for (provider, id) in invalid {
            assert_eq!(
                resolver
                    .resolve(&PrincipalRef::new(provider, id).unwrap())
                    .await
                    .err(),
                Some(IdentityResolutionError::InvalidIdentity),
                "{mode} must reject {provider}/{id}"
            );
        }
    }
}

#[tokio::test]
async fn organization_context_is_refreshed_for_each_due_attempt() {
    let resolver = StaticRequesterResolver::for_runtime_mode("organization-synthetic").unwrap();
    let principal = PrincipalRef::new("organization-synthetic", "sales-01").unwrap();
    let first = resolver.resolve(&principal).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = resolver.resolve(&principal).await.unwrap();
    assert!(second.valid_until() > first.valid_until());
    assert_eq!(first.subjects(), second.subjects());
    assert!(second.ensure_current().is_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn binary_selects_organization_resolver_before_storage_validation() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_document-publication-scheduler"))
        .env_clear()
        .env("KP_RUNTIME_MODE", "organization-synthetic")
        .env("DOCUMENT_DATABASE_URL", "unused-synthetic-database")
        .env("DOCUMENT_STORAGE_ROOT", root.path().join("missing"))
        .env("DSI_WORKER_EXECUTABLE", "unused-synthetic-worker")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "publication scheduler startup failed: authoritative file storage root is unavailable\n"
    );
}
