#![forbid(unsafe_code)]
//! Local origin guard for the unauthenticated PoC servers.
//!
//! With no authentication, a server bound to the loopback address is still
//! reachable from web pages the user opens: a cross-site form can POST to it,
//! and DNS rebinding can make another site's name resolve to 127.0.0.1. This
//! guard refuses every request whose `Host` is not one of the configured
//! names, and every state-changing request a browser marks as coming from
//! another site (`Origin` not configured, or `Sec-Fetch-Site` cross-site or
//! same-site). Clients that send neither header (curl, agents) are unaffected.
//! It is not authentication.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    Empty,
    InvalidHost,
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "the allowed host list is empty",
            Self::InvalidHost => {
                "an allowed host must be host or host:port without a scheme or path"
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    MissingHost,
    UnknownHost,
    ForeignOrigin,
    CrossSiteRequest,
}

/// The `Host` values (`name[:port]`, compared case-insensitively) this
/// process answers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOriginPolicy {
    hosts: Vec<String>,
}

impl LocalOriginPolicy {
    /// Parses a comma-separated list such as `127.0.0.1:8080,localhost:8080`.
    pub fn parse(list: &str) -> Result<Self, PolicyError> {
        let mut hosts = Vec::new();
        for entry in list.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if entry.contains("://")
                || entry.contains('/')
                || entry.contains('@')
                || entry.chars().any(|c| c.is_whitespace() || c.is_control())
            {
                return Err(PolicyError::InvalidHost);
            }
            hosts.push(entry.to_ascii_lowercase());
        }
        if hosts.is_empty() {
            return Err(PolicyError::Empty);
        }
        Ok(Self { hosts })
    }

    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    fn allows_host(&self, value: &str) -> bool {
        let value = value.trim().to_ascii_lowercase();
        self.hosts.contains(&value)
    }

    fn allows_origin(&self, origin: &str) -> bool {
        let origin = origin.trim().to_ascii_lowercase();
        let Some(rest) = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
        else {
            return false;
        };
        self.hosts.iter().any(|host| host == rest)
    }

    /// Decides one request from its method and headers.
    pub fn check(&self, method: &Method, headers: &HeaderMap) -> Result<(), Rejection> {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .ok_or(Rejection::MissingHost)?;
        if !self.allows_host(host) {
            return Err(Rejection::UnknownHost);
        }
        if matches!(
            *method,
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
        ) {
            return Ok(());
        }
        if let Some(site) = headers
            .get("sec-fetch-site")
            .and_then(|value| value.to_str().ok())
            && (site.eq_ignore_ascii_case("cross-site") || site.eq_ignore_ascii_case("same-site"))
        {
            return Err(Rejection::CrossSiteRequest);
        }
        if let Some(origin) = headers.get(header::ORIGIN) {
            let allowed = origin
                .to_str()
                .is_ok_and(|origin| self.allows_origin(origin));
            if !allowed {
                return Err(Rejection::ForeignOrigin);
            }
        }
        Ok(())
    }
}

async fn guard(
    State(policy): State<Arc<LocalOriginPolicy>>,
    request: Request,
    next: Next,
) -> Response {
    match policy.check(request.method(), request.headers()) {
        Ok(()) => next.run(request).await,
        // No detail: the response must not help probe the configuration.
        Err(_) => (
            StatusCode::FORBIDDEN,
            "forbidden by the local origin guard\n",
        )
            .into_response(),
    }
}

/// Wraps every route of `router` in the guard.
pub fn guard_router(router: Router, policy: LocalOriginPolicy) -> Router {
    router.layer(middleware::from_fn_with_state(Arc::new(policy), guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::HeaderValue;
    use axum::routing::any;
    use tower::ServiceExt;

    fn policy() -> LocalOriginPolicy {
        LocalOriginPolicy::parse(" 127.0.0.1:8080, LOCALHOST:8080 ,").unwrap()
    }

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn parses_names_and_rejects_urls() {
        assert_eq!(policy().hosts(), ["127.0.0.1:8080", "localhost:8080"]);
        assert_eq!(
            LocalOriginPolicy::parse(" , ").unwrap_err(),
            PolicyError::Empty
        );
        assert_eq!(
            LocalOriginPolicy::parse("http://127.0.0.1:8080").unwrap_err(),
            PolicyError::InvalidHost
        );
        assert_eq!(
            LocalOriginPolicy::parse("a/b").unwrap_err(),
            PolicyError::InvalidHost
        );
    }

    #[test]
    fn host_must_be_configured_for_every_method() {
        let p = policy();
        assert_eq!(
            p.check(&Method::GET, &headers(&[])),
            Err(Rejection::MissingHost)
        );
        assert_eq!(
            p.check(&Method::GET, &headers(&[("host", "evil.example:8080")])),
            Err(Rejection::UnknownHost)
        );
        // DNS rebinding: another name pointing at 127.0.0.1.
        assert_eq!(
            p.check(&Method::GET, &headers(&[("host", "127.0.0.1.nip.io:8080")])),
            Err(Rejection::UnknownHost)
        );
        assert_eq!(
            p.check(&Method::GET, &headers(&[("host", "Localhost:8080")])),
            Ok(())
        );
        assert_eq!(
            p.check(&Method::GET, &headers(&[("host", "127.0.0.1:9999")])),
            Err(Rejection::UnknownHost)
        );
    }

    #[test]
    fn browsers_cannot_write_from_another_site() {
        let p = policy();
        let host = ("host", "127.0.0.1:8080");
        assert_eq!(
            p.check(
                &Method::POST,
                &headers(&[host, ("origin", "https://evil.example")])
            ),
            Err(Rejection::ForeignOrigin)
        );
        assert_eq!(
            p.check(&Method::POST, &headers(&[host, ("origin", "null")])),
            Err(Rejection::ForeignOrigin)
        );
        assert_eq!(
            p.check(
                &Method::PUT,
                &headers(&[host, ("sec-fetch-site", "cross-site")])
            ),
            Err(Rejection::CrossSiteRequest)
        );
        assert_eq!(
            p.check(
                &Method::DELETE,
                &headers(&[host, ("sec-fetch-site", "same-site")])
            ),
            Err(Rejection::CrossSiteRequest)
        );
        // The served GUI itself, and clients without browser headers.
        assert_eq!(
            p.check(
                &Method::POST,
                &headers(&[
                    host,
                    ("origin", "http://127.0.0.1:8080"),
                    ("sec-fetch-site", "same-origin")
                ])
            ),
            Ok(())
        );
        assert_eq!(p.check(&Method::POST, &headers(&[host])), Ok(()));
        // Reads stay allowed; the browser keeps cross-site responses unreadable.
        assert_eq!(
            p.check(
                &Method::GET,
                &headers(&[host, ("sec-fetch-site", "cross-site")])
            ),
            Ok(())
        );
    }

    #[tokio::test]
    async fn the_layer_answers_403_before_the_route() {
        let router = guard_router(Router::new().route("/x", any(|| async { "ok" })), policy());
        let request = |host: &'static str| {
            Request::builder()
                .method(Method::POST)
                .uri("/x")
                .header("host", host)
                .body(Body::empty())
                .unwrap()
        };
        let ok = router
            .clone()
            .oneshot(request("127.0.0.1:8080"))
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let refused = router.oneshot(request("evil.example")).await.unwrap();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    }
}
