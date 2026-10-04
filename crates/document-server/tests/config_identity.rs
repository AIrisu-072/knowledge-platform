use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::extract::Extension;
use axum::http::Request;
use axum::routing::get;
use axum::{Json, Router};
use document_api_http::router::protect_routes;
use document_application::VerifiedActorContext;
use document_server::config::{Command, ConfigSource, RuntimeConfig};
use document_server::identity::{PoCIdentityProfile, StaticPoCIdentityAdapter};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Environment(BTreeMap<&'static str, String>);
impl ConfigSource for Environment {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}
fn valid() -> Environment {
    Environment(BTreeMap::from([
        ("KP_RUNTIME_MODE", "poc".into()),
        ("KP_IDENTITY_PROFILE", "poc-human".into()),
        (
            "KP_DATABASE_URL",
            "postgresql://synthetic_user:synthetic_secret@127.0.0.1/synthetic_poc".into(),
        ),
        ("KP_STORAGE_ROOT", "/synthetic/private-storage".into()),
        ("KP_DSI_WORKER", "/synthetic/dsi-worker".into()),
        ("KP_DIFF_WORKER", "/synthetic/diff-worker".into()),
        ("KP_WEB_DIST", "/synthetic/web-dist".into()),
    ]))
}

#[test]
fn unknown_and_production_modes_fail() {
    for mode in [
        None,
        Some("production"),
        Some("POC"),
        Some(""),
        Some("unknown"),
    ] {
        let mut env = valid();
        env.0.remove("KP_RUNTIME_MODE");
        if let Some(mode) = mode {
            env.0.insert("KP_RUNTIME_MODE", mode.into());
        }
        for command in [Command::Serve, Command::Migrate, Command::BootstrapPoc] {
            assert!(RuntimeConfig::from_env(&env, command).is_err());
        }
    }
    for profile in ["unknown", "poc-scheduler", "poc-human,poc-agent", ""] {
        let mut env = valid();
        env.0.insert("KP_IDENTITY_PROFILE", profile.into());
        assert!(RuntimeConfig::from_env(&env, Command::Serve).is_err());
    }
}

#[test]
fn bind_override_is_explicit() {
    for address in ["0.0.0.0:8080", "192.0.2.1:8080", "[::]:8080"] {
        let mut env = valid();
        env.0.insert("KP_BIND", address.into());
        assert!(RuntimeConfig::from_env(&env, Command::Serve).is_err());
        env.0.insert("KP_POC_ALLOW_NON_LOOPBACK", "true".into());
        let config = RuntimeConfig::from_env(&env, Command::Serve).unwrap();
        assert!(config.serve().unwrap().non_loopback_warning());
    }
    for value in ["TRUE", "1", "yes", "", " false "] {
        let mut env = valid();
        env.0.insert("KP_POC_ALLOW_NON_LOOPBACK", value.into());
        assert!(RuntimeConfig::from_env(&env, Command::Serve).is_err());
    }
    for address in ["localhost:8080", "127.0.0.1", "bad", "127.0.0.1:65536"] {
        let mut env = valid();
        env.0.insert("KP_BIND", address.into());
        assert!(RuntimeConfig::from_env(&env, Command::Serve).is_err());
    }
    for address in ["127.0.0.1:8080", "[::1]:8080"] {
        let mut env = valid();
        env.0.insert("KP_BIND", address.into());
        assert!(
            !RuntimeConfig::from_env(&env, Command::Serve)
                .unwrap()
                .serve()
                .unwrap()
                .non_loopback_warning()
        );
    }
}

#[test]
fn command_configuration_is_narrow_and_fail_fast() {
    let mut env = valid();
    let config = RuntimeConfig::from_env(&env, Command::Serve).unwrap();
    assert_eq!(config.profile(), Some(PoCIdentityProfile::Human));
    assert_eq!(config.serve().unwrap().bind().to_string(), "127.0.0.1:8080");
    assert!(config.serve().unwrap().web_dist().is_some());
    env.0.insert("KP_IDENTITY_PROFILE", "poc-agent".into());
    env.0.remove("KP_WEB_DIST");
    let config = RuntimeConfig::from_env(&env, Command::Serve).unwrap();
    assert_eq!(config.serve().unwrap().bind().to_string(), "127.0.0.1:8081");
    assert!(config.serve().unwrap().web_dist().is_none());
    for required in [
        "KP_DATABASE_URL",
        "KP_STORAGE_ROOT",
        "KP_DSI_WORKER",
        "KP_DIFF_WORKER",
        "KP_WEB_DIST",
    ] {
        let mut env = valid();
        env.0.remove(required);
        assert!(RuntimeConfig::from_env(&env, Command::Serve).is_err());
    }
    let mut env = valid();
    env.0
        .retain(|key, _| ["KP_RUNTIME_MODE", "KP_DATABASE_URL"].contains(key));
    assert!(RuntimeConfig::from_env(&env, Command::Migrate).is_ok());
    assert!(RuntimeConfig::from_env(&env, Command::BootstrapPoc).is_err());
    env.0.insert("KP_IDENTITY_PROFILE", "poc-human".into());
    assert!(RuntimeConfig::from_env(&env, Command::BootstrapPoc).is_ok());
    env.0.insert("KP_IDENTITY_PROFILE", "poc-agent".into());
    assert!(RuntimeConfig::from_env(&env, Command::BootstrapPoc).is_err());
}

#[test]
fn errors_redact_url_and_paths() {
    let env = valid();
    let config = RuntimeConfig::from_env(&env, Command::Serve).unwrap();
    let diagnostic = format!("{config:?} {:?}", config.serve().unwrap());
    for secret in [
        "synthetic_secret",
        "synthetic_user",
        "synthetic_poc",
        "/synthetic",
    ] {
        assert!(!diagnostic.contains(secret));
    }
    for (name, value) in [
        ("KP_DATABASE_URL", "https://synthetic_secret@invalid"),
        ("KP_BIND", "/synthetic_secret"),
        ("KP_POC_ALLOW_NON_LOOPBACK", "synthetic_secret"),
        ("KP_IDENTITY_PROFILE", "synthetic_secret"),
    ] {
        let mut env = valid();
        env.0.insert(name, value.into());
        let error = RuntimeConfig::from_env(&env, Command::Serve).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("synthetic_secret"));
    }
}

async fn context(Extension(actor): Extension<VerifiedActorContext>) -> Json<Value> {
    Json(
        json!({"provider":actor.principal().identity_provider(),"principal":actor.principal().principal_id(),"subjects":actor.subjects(),"kind":actor.invocation_kind().as_str(),"executor":actor.service_executor()}),
    )
}

#[tokio::test]
async fn profiles_are_fixed_and_headers_cannot_override() {
    for (profile, principal, group, kind) in [
        (
            PoCIdentityProfile::Human,
            "poc-human",
            "poc-users",
            "human_interactive",
        ),
        (
            PoCIdentityProfile::Agent,
            "poc-agent",
            "poc-agents",
            "agent",
        ),
    ] {
        let router = protect_routes(
            Router::new().route("/identity", get(context)),
            Some(Arc::new(StaticPoCIdentityAdapter::new(profile))),
        )
        .unwrap();
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/identity?principal=poc-human&group=poc-users")
                    .header("x-principal", "poc-human")
                    .header("x-groups", "poc-users")
                    .header("authorization", "Bearer synthetic-spoof")
                    .header("cookie", "profile=poc-human")
                    .body(Body::from(
                        r#"{"principal":"poc-human","invocationKind":"human_interactive"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"provider":"poc","principal":principal,"subjects":[{"kind":"principal","identity_provider":"poc","subject_id":principal},{"kind":"group","identity_provider":"poc","subject_id":group}],"kind":kind,"executor":null})
        );
    }
}
