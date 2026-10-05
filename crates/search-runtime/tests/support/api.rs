//! Host wiring shared by the P5-08 runtime and server tests: the host's
//! verified session resolver, its Source grant policy, the Bearer credential
//! verifier that turns a known fixture credential into an opaque session,
//! the factory inputs, and in-process and real-TCP HTTP clients.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use search_api_http::auth::{
    CredentialError, CredentialFuture, SearchAuthSchemeBinding, SearchCredentialVerifierPort,
    StaticBearerChallenge,
};
use search_api_http::router::ApiFuture;
use search_application::api_scope::ApiError;
use search_application::discovery_service::{DiscoveryConfig, TemporalPolicy};
use search_application::materialization::ProbeBudget;
use search_application::ports::BoxFuture;
use search_application::retrieval::{RetrievalInputs, RetrieverProfile, RetrieverSupport};
use search_application::routing::RoutingConstraints;
use search_application::scoped::{
    AccessRevision, CheckedAuthorityAdapter, PrincipalRef, RegistrationRevision, TenantId,
    TrustedSearchScope, VerifiedActorDescriptor, VerifiedActorResolverPort, VerifiedSourceGrant,
    VerifiedSourceVisibilityPort, VisibilityRevision,
};
use search_application::search_core::id::{SessionId, SourceId};
use search_application::source_registration::HostRegistrationSnapshotPort;
use search_application::visible_claim::InMemoryClaimCatalog;
use search_runtime::api::{
    ActorPorts, ActorPortsFactory, RemoteTransportFactory, SearchApiDurablePorts,
    SearchApiHostConfig, SearchApiIdentityScheme,
};
use serde_json::Value;
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;
use uuid::Uuid;

struct Session {
    tenant: String,
    principal: String,
    session: Uuid,
    issued_at: Instant,
    deadline: Instant,
}

/// The host identity provider's verified sessions, keyed by raw handle.
#[derive(Default)]
pub struct Sessions(Mutex<BTreeMap<String, Session>>);

impl Sessions {
    pub fn open(&self, tenant: &str, principal: &str) -> String {
        let raw = format!("session-{}", Uuid::new_v4().simple());
        let issued_at = Instant::now();
        self.0.lock().unwrap().insert(
            raw.clone(),
            Session {
                tenant: tenant.into(),
                principal: principal.into(),
                session: Uuid::now_v7(),
                issued_at,
                deadline: issued_at + Duration::from_secs(600),
            },
        );
        raw
    }

    pub fn close(&self, raw: &str) {
        self.0.lock().unwrap().remove(raw);
    }
}

impl VerifiedActorResolverPort for Sessions {
    fn resolve_verified<'a>(
        &'a self,
        raw_handle: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedActorDescriptor>> {
        Box::pin(async move {
            let sessions = self.0.lock().unwrap();
            let Some(session) = sessions.get(raw_handle) else {
                return Ok(None);
            };
            Ok(Some(VerifiedActorDescriptor::new(
                TenantId::new(session.tenant.clone())?,
                PrincipalRef::new(session.principal.clone())?,
                Some(SessionId::from_uuid(session.session)),
                AccessRevision::new(1)?,
                session.issued_at,
                session.deadline,
            )?))
        })
    }
}

/// The host's Source grant policy for each principal.
#[derive(Default)]
pub struct Grants(Mutex<BTreeMap<(String, SourceId), VerifiedSourceGrant>>);

impl Grants {
    pub fn grant(&self, tenant: &str, principal: &str, source: SourceId) {
        self.grant_at(tenant, principal, source, 1);
    }

    pub fn grant_at(&self, tenant: &str, principal: &str, source: SourceId, revision: u64) {
        self.0.lock().unwrap().insert(
            (principal.into(), source),
            VerifiedSourceGrant::new(
                TenantId::new(tenant).unwrap(),
                source,
                RegistrationRevision::new(revision).unwrap(),
                VisibilityRevision::new(1).unwrap(),
            ),
        );
    }

    pub fn revoke(&self, principal: &str, source: SourceId) {
        self.0.lock().unwrap().remove(&(principal.into(), source));
    }
}

impl VerifiedSourceVisibilityPort for Grants {
    fn grant_for<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<VerifiedSourceGrant>> {
        Box::pin(async move {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(actor.principal().as_str().to_owned(), source))
                .cloned())
        })
    }
}

/// Known fixture credentials; each becomes the opaque session it names.
pub struct Credentials {
    sessions: Arc<Sessions>,
    tokens: Mutex<BTreeMap<String, String>>,
}

impl SearchCredentialVerifierPort for Credentials {
    fn verify<'a>(&'a self, token: &'a str) -> CredentialFuture<'a> {
        Box::pin(async move {
            let raw = self.tokens.lock().unwrap().get(token).cloned();
            let Some(raw) = raw else {
                return Ok(None);
            };
            let scope = CheckedAuthorityAdapter::new(&*self.sessions)
                .authenticate_handle(&raw)
                .await
                .map_err(|_| CredentialError::Unavailable)?;
            Ok(scope.map(|scope| scope.access_handle().clone()))
        })
    }
}

/// Everything the host owns besides the Source state.
pub struct Host {
    pub sessions: Arc<Sessions>,
    pub grants: Arc<Grants>,
    pub credentials: Arc<Credentials>,
    pub claims: Arc<InMemoryClaimCatalog>,
}

impl Host {
    pub fn new() -> Self {
        let sessions = Arc::new(Sessions::default());
        Self {
            credentials: Arc::new(Credentials {
                sessions: sessions.clone(),
                tokens: Mutex::new(BTreeMap::new()),
            }),
            sessions,
            grants: Arc::new(Grants::default()),
            claims: Arc::new(InMemoryClaimCatalog::new(vec![])),
        }
    }

    /// A known credential for `principal`, bound to a fresh opaque session.
    pub fn login(&self, token: &str, tenant: &str, principal: &str) -> String {
        let raw = self.sessions.open(tenant, principal);
        self.credentials
            .tokens
            .lock()
            .unwrap()
            .insert(token.into(), raw.clone());
        raw
    }

    pub fn config(
        &self,
        registrations: Arc<dyn HostRegistrationSnapshotPort>,
        transports: Option<Arc<dyn RemoteTransportFactory>>,
        config: DiscoveryConfig,
    ) -> SearchApiHostConfig {
        SearchApiHostConfig {
            actors: self.sessions.clone(),
            visibility: self.grants.clone(),
            registrations,
            claims: Some(self.claims.clone()),
            remote_transports: transports,
            config,
            operation_timeout: Duration::from_secs(20),
            disclosure_ttl: Duration::from_secs(30),
        }
    }

    pub fn identity(&self) -> SearchApiIdentityScheme {
        SearchApiIdentityScheme {
            credentials: Some(self.credentials.clone()),
            auth: Some(SearchAuthSchemeBinding::bearer(Arc::new(
                StaticBearerChallenge,
            ))),
        }
    }
}

pub fn durable(pool: &PgPool, ports: Arc<dyn ActorPortsFactory>) -> SearchApiDurablePorts {
    SearchApiDurablePorts {
        pool: pool.clone(),
        actor_ports: Some(ports),
    }
}

/// For tests that read only the Source catalog.
pub struct NoActorPorts;

impl ActorPortsFactory for NoActorPorts {
    fn for_actor<'a>(&'a self, _: &'a TrustedSearchScope) -> ApiFuture<'a, ActorPorts> {
        Box::pin(async { Err(ApiError::DependencyUnavailable) })
    }
}

/// Lexical/Directory retrieval for Documents plus the given remote support.
pub fn discovery_config(remote: RetrieverSupport, inputs: RetrievalInputs) -> DiscoveryConfig {
    DiscoveryConfig {
        routing: RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: vec![],
            max_initial_optional_sources: 0,
        },
        retriever_profile: RetrieverProfile::Capability,
        retriever_support: RetrieverSupport {
            directory: true,
            lexical: true,
            ..remote
        },
        retrieval_inputs: RetrievalInputs {
            max_initial_retrievers_per_source: 2,
            ..inputs
        },
        structured_filters: vec![],
        discriminators: vec![],
        lexical_query: None,
        temporal_policy: TemporalPolicy::default(),
        probe_budget: ProbeBudget {
            max_content_bytes: 0,
            max_latency_ms: 0,
            max_remote_calls: 0,
            max_monetary_cost_minor_units: 0,
            currency: "USD".into(),
        },
        max_actions: 8,
        evaluation_currency: "USD".into(),
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Value,
}

/// One in-process request through the router.
pub async fn call(router: &Router, request: Request<Body>) -> Reply {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

pub fn get(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// The visible Source IDs, or the Problem status.
pub async fn visible_sources(router: &Router, token: &str) -> Result<Vec<String>, u16> {
    let reply = call(router, get("/v1/sources?pageSize=100", token)).await;
    if reply.status != StatusCode::OK {
        return Err(reply.status.as_u16());
    }
    let mut ids: Vec<String> = reply.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["sourceId"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    Ok(ids)
}

/// A raw HTTP/1.1 exchange over a real socket, `Connection: close`.
pub struct Wire {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Wire {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

pub fn raw_request(method: &str, path: &str, token: Option<&str>, body: &str) -> Vec<u8> {
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{authorization}\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

pub async fn exchange(addr: SocketAddr, request: &[u8]) -> Wire {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    parse(&bytes)
}

pub fn parse(bytes: &[u8]) -> Wire {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let head = String::from_utf8_lossy(&bytes[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect();
    let raw = &bytes[split + 4..];
    let chunked = headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    });
    let body = if chunked { dechunk(raw) } else { raw.to_vec() };
    Wire {
        status,
        headers,
        body,
    }
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line = raw.windows(2).position(|window| window == b"\r\n").unwrap();
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&raw[..line])
                .split(';')
                .next()
                .unwrap()
                .trim(),
            16,
        )
        .unwrap();
        raw = &raw[line + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}
