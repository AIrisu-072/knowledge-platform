use document_server::config::{Command, ConfigSource};
use organization_server::{OrganizationConfig, OrganizationProfile, SyntheticIdentityAdapter};
use std::collections::BTreeMap;

struct Environment(BTreeMap<&'static str, String>);
impl ConfigSource for Environment {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}
fn environment() -> Environment {
    Environment(BTreeMap::from([
        ("KP_RUNTIME_MODE", "organization-synthetic".into()),
        ("KP_ORGANIZATION_PROFILE", "sales-01".into()),
        (
            "KP_DATABASE_URL",
            "postgres://synthetic@127.0.0.1/synthetic_poc".into(),
        ),
        ("KP_STORAGE_ROOT", "/synthetic/storage".into()),
        ("KP_DSI_WORKER", "/synthetic/dsi".into()),
        ("KP_DIFF_WORKER", "/synthetic/diff".into()),
        ("KP_WEB_DIST", "/synthetic/dist".into()),
    ]))
}
#[test]
fn synthetic_identity_is_fixed_and_reused_for_document() {
    for (profile, name) in [
        (OrganizationProfile::Sales, "sales-01"),
        (OrganizationProfile::Office, "office-01"),
    ] {
        let actor = SyntheticIdentityAdapter::new(profile)
            .current_context()
            .unwrap();
        assert_eq!(
            actor.principal().identity_provider(),
            "organization-synthetic"
        );
        assert_eq!(actor.principal().principal_id(), name);
        assert_eq!(
            actor.invocation_kind(),
            document_application::InvocationKind::HumanInteractive
        );
        assert!(
            actor
                .subjects()
                .iter()
                .all(|subject| subject.identity_provider() == "organization-synthetic")
        );
    }
}
#[test]
fn only_two_explicit_profiles_and_synthetic_mode_are_accepted() {
    let mut env = environment();
    for profile in ["sales-01", "office-01"] {
        env.0.insert("KP_ORGANIZATION_PROFILE", profile.into());
        assert!(OrganizationConfig::from_env(&env, Command::Serve).is_ok());
    }
    for profile in [
        "poc-human",
        "production",
        "agent-01",
        "sales-01,office-01",
        "",
    ] {
        env.0.insert("KP_ORGANIZATION_PROFILE", profile.into());
        assert!(OrganizationConfig::from_env(&env, Command::Serve).is_err());
    }
    for mode in ["poc", "production", ""] {
        let mut env = environment();
        env.0.insert("KP_RUNTIME_MODE", mode.into());
        assert!(OrganizationConfig::from_env(&env, Command::Serve).is_err());
    }
}
#[test]
fn organization_config_keeps_fixed_profiles_off_public_interfaces() {
    let mut env = environment();
    let config = OrganizationConfig::from_env(&env, Command::Serve).unwrap();
    assert_eq!(
        config.document().serve().unwrap().bind().to_string(),
        "127.0.0.1:8090"
    );
    env.0.insert("KP_ORGANIZATION_PROFILE", "office-01".into());
    let config = OrganizationConfig::from_env(&env, Command::Serve).unwrap();
    assert_eq!(
        config.document().serve().unwrap().bind().to_string(),
        "127.0.0.1:8091"
    );
    env.0.insert("KP_BIND", "0.0.0.0:8090".into());
    env.0.insert("KP_POC_ALLOW_NON_LOOPBACK", "true".into());
    assert!(OrganizationConfig::from_env(&env, Command::Serve).is_err());
}

#[tokio::test]
async fn document_composition_factory_rejects_non_serve_before_io() {
    use document_server::composition::{StartupError, compose_runtime_with_identity};
    use std::sync::Arc;
    let config = OrganizationConfig::from_env(&environment(), Command::Migrate).unwrap();
    let result = compose_runtime_with_identity(
        config.document(),
        Arc::new(SyntheticIdentityAdapter::new(OrganizationProfile::Sales)),
    )
    .await;
    assert!(matches!(result, Err(StartupError::Configuration)));
}
