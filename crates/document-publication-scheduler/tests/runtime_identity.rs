use document_application::{
    IdentityContextResolver, IdentityResolutionError, InvocationKind, authorize_scheduled_publish,
};
use document_domain::{PolicySubjectKind, PrincipalRef};
use document_publication_scheduler::{StaticRequesterResolver, scheduler_executor};

#[test]
fn static_requester_resolution_is_explicitly_poc_only() {
    for mode in ["", "production", "test", "POC", "poc "] {
        assert!(StaticRequesterResolver::for_runtime_mode(mode).is_err());
    }
    assert!(StaticRequesterResolver::for_runtime_mode("poc").is_ok());
}

#[tokio::test]
async fn fixed_requesters_are_resolved_without_executor_authority() {
    let resolver = StaticRequesterResolver::for_runtime_mode("poc").unwrap();
    let executor = scheduler_executor();
    assert_eq!(executor, PrincipalRef::new("service", "scheduler").unwrap());
    for (id, group, invocation) in [
        ("poc-human", "poc-users", InvocationKind::HumanInteractive),
        ("poc-agent", "poc-agents", InvocationKind::Agent),
    ] {
        let principal = PrincipalRef::new("poc", id).unwrap();
        let resolved = resolver.resolve(&principal).await.unwrap();
        assert_eq!(resolved.principal(), &principal);
        assert_eq!(resolved.invocation_kind(), invocation);
        assert_eq!(resolved.service_executor(), None);
        assert_eq!(resolved.subjects().len(), 2);
        assert!(
            resolved
                .subjects()
                .iter()
                .any(|s| s.kind() == PolicySubjectKind::Principal && s.subject_id() == id)
        );
        assert!(
            resolved
                .subjects()
                .iter()
                .any(|s| s.kind() == PolicySubjectKind::Group && s.subject_id() == group)
        );
        let due = authorize_scheduled_publish(&resolver, &principal, &executor)
            .await
            .unwrap();
        assert_eq!(due.principal(), &principal);
        assert_eq!(due.invocation_kind(), InvocationKind::Service);
        assert_eq!(due.service_executor(), Some(&executor));
        assert_eq!(due.subjects(), resolved.subjects());
        assert!(
            due.subjects()
                .iter()
                .all(|s| s.identity_provider() == "poc" && s.subject_id() != "scheduler")
        );
    }
}

#[tokio::test]
async fn unknown_requester_and_executor_are_never_login_identities() {
    let resolver = StaticRequesterResolver::for_runtime_mode("poc").unwrap();
    for (provider, principal) in [
        ("poc", "unknown"),
        ("poc", "scheduler"),
        ("service", "scheduler"),
        ("other", "poc-human"),
        ("poc", "poc-users"),
    ] {
        assert_eq!(
            resolver
                .resolve(&PrincipalRef::new(provider, principal).unwrap())
                .await
                .err(),
            Some(IdentityResolutionError::InvalidIdentity)
        );
    }
}

#[tokio::test]
async fn original_requester_is_refreshed_at_each_resolution() {
    let resolver = StaticRequesterResolver::for_runtime_mode("poc").unwrap();
    let principal = PrincipalRef::new("poc", "poc-human").unwrap();
    let first = resolver.resolve(&principal).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = resolver.resolve(&principal).await.unwrap();
    assert!(second.valid_until() > first.valid_until());
    assert!(second.ensure_current().is_ok());
    assert_eq!(first.principal(), second.principal());
    assert_eq!(first.subjects(), second.subjects());
}

#[test]
fn binary_rejects_non_poc_mode_before_dependency_access() {
    for mode in [None, Some("production"), Some("unknown")] {
        let mut command =
            std::process::Command::new(env!("CARGO_BIN_EXE_document-publication-scheduler"));
        command.env_clear();
        if let Some(mode) = mode {
            command.env("KP_RUNTIME_MODE", mode);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "publication scheduler requires KP_RUNTIME_MODE=poc\n"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn binary_wires_the_fixed_resolver_before_storage_validation() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_document-publication-scheduler"))
        .env_clear()
        .env("KP_RUNTIME_MODE", "poc")
        .env("DOCUMENT_DATABASE_URL", "unused-secret-database")
        .env("DOCUMENT_STORAGE_ROOT", root.path().join("missing"))
        .env("DSI_WORKER_EXECUTABLE", "unused-private-worker")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "publication scheduler startup failed: authoritative file storage root is unavailable\n"
    );
}
