//! Same-origin `/v1` forwarding to exactly one configured loopback backend.
//!
//! The page keeps calling relative `/v1/...` URLs as in the browser build. The
//! shell forwards only those paths, only to the origin named by
//! `KNOWLEDGE_PLATFORM_API_ORIGIN` (a literal loopback `http://` origin), with
//! an allowlist of request and response headers, no redirects, no cookies, no
//! system proxy and bounded sizes and time. Nothing else is reachable.

use std::net::IpAddr;
use std::time::Duration;

use tauri::http::{
    HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode, header,
};
use url::Url;

pub const ORIGIN_ENV: &str = "KNOWLEDGE_PLATFORM_API_ORIGIN";

/// Matches the Document API v0 multipart bound (1 GiB) plus envelope room.
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024 * 1024 + 1024 * 1024;
/// Matches the Document API v0 file bound (256 MiB) plus envelope room.
pub const MAX_RESPONSE_BODY_BYTES: usize = 256 * 1024 * 1024 + 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Longer than the server's own multipart operation budget (120 s).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Request headers the existing frontend sends and the backend reads. Origin,
/// Referer, Cookie, Authorization and every identity/claim header are dropped.
const REQUEST_HEADERS: [HeaderName; 4] = [
    header::ACCEPT,
    header::ACCEPT_LANGUAGE,
    header::CONTENT_TYPE,
    HeaderName::from_static("traceparent"),
];

/// Response headers passed back to the page. Set-Cookie, Location, CORS and
/// hop-by-hop headers never reach the WebView.
const RESPONSE_HEADERS: [HeaderName; 7] = [
    header::CONTENT_TYPE,
    header::CONTENT_DISPOSITION,
    header::CONTENT_LANGUAGE,
    header::CACHE_CONTROL,
    header::ETAG,
    header::LAST_MODIFIED,
    header::RETRY_AFTER,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginError {
    Missing,
    Invalid,
    NotLoopback,
}

/// Accepts only `http://127.0.0.1:<port>` style literal loopback origins
/// (IPv4 127/8 or `[::1]`), with an explicit port and nothing else.
pub fn parse_backend_origin(raw: Option<&str>) -> Result<Url, OriginError> {
    let raw = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(OriginError::Missing)?;
    let url = Url::parse(raw).map_err(|_| OriginError::Invalid)?;
    // The text must be exactly `http://<host>:<port>` (an explicit port, even
    // the default 80), so nothing else in it can change where requests go.
    let canonical = match (url.host_str(), url.port_or_known_default()) {
        (Some(host), Some(port)) => format!("http://{host}:{port}"),
        _ => return Err(OriginError::Invalid),
    };
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || raw.trim_end_matches('/') != canonical
    {
        return Err(OriginError::Invalid);
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        _ => false,
    };
    if !loopback {
        return Err(OriginError::NotLoopback);
    }
    Ok(url)
}

/// True for the raw request paths the shell forwards (before normalization).
pub fn is_api_path(path: &str) -> bool {
    path == "/v1" || path.starts_with("/v1/")
}

/// The backend URL for a page request, or None when the normalized target
/// would leave the configured origin or the `/v1` tree.
pub fn api_target(origin: &Url, path_and_query: &str) -> Option<Url> {
    if !path_and_query.starts_with('/') || path_and_query.contains('#') {
        return None;
    }
    let base = origin.as_str().trim_end_matches('/');
    let target = Url::parse(&format!("{base}{path_and_query}")).ok()?;
    let within = target.origin() == origin.origin()
        && target.username().is_empty()
        && target.password().is_none()
        && is_api_path(target.path());
    within.then_some(target)
}

pub fn is_forwardable_method(method: &Method) -> bool {
    [
        Method::GET,
        Method::HEAD,
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
    ]
    .contains(method)
}

/// Defense in depth: an Origin or Referer, when the WebView sends one, must be
/// the bundled app. (The scheme itself is served only to the main window,
/// which can only load bundled app URLs.)
pub fn origin_allowed(headers: &HeaderMap, app_origin: &str) -> bool {
    let origin_ok = headers
        .get_all(header::ORIGIN)
        .iter()
        .all(|value| value.as_bytes() == app_origin.as_bytes());
    let referer_ok = headers.get_all(header::REFERER).iter().all(|value| {
        value
            .to_str()
            .ok()
            .and_then(|text| Url::parse(text).ok())
            .is_some_and(|url| serialized_origin(&url).as_deref() == Some(app_origin))
    });
    origin_ok && referer_ok
}

/// `scheme://host[:port]`, also for non-special schemes such as `tauri:`
/// (whose WHATWG origin is opaque and would serialize as "null").
pub fn serialized_origin(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    })
}

pub fn forwarded_request_headers(headers: &HeaderMap) -> HeaderMap {
    let mut kept = HeaderMap::new();
    for name in &REQUEST_HEADERS {
        for value in headers.get_all(name) {
            kept.append(name.clone(), value.clone());
        }
    }
    kept
}

pub fn returned_response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut kept = HeaderMap::new();
    for name in &RESPONSE_HEADERS {
        for value in headers.get_all(name) {
            kept.append(name.clone(), value.clone());
        }
    }
    kept
}

/// RFC 9457 problem body without any backend, OS or path detail.
pub fn problem(status: StatusCode, detail: &str, app_origin: &str) -> Response<Vec<u8>> {
    let body = serde_json::json!({
        "type": "about:blank",
        "title": status.canonical_reason().unwrap_or("Error"),
        "status": status.as_u16(),
        "detail": detail,
    });
    let mut response = Response::new(body.to_string().into_bytes());
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    secure_headers(headers, app_origin);
    response
}

/// A `/v1` response is data for the page, never code or a document of the
/// app's origin: `script-src 'self'` (Tauri always adds `'self'`) would
/// otherwise run a document original uploaded as JavaScript, with access to
/// the broker IPC. Only data types keep their type; with `nosniff` anything
/// else cannot load as script or style, and the sandbox CSP stops it from
/// rendering as an app-origin page.
pub fn data_only(headers: &mut HeaderMap) {
    let essence = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
    let data = essence.as_deref().is_some_and(|essence| {
        matches!(
            essence,
            "application/json"
                | "text/plain"
                | "application/octet-stream"
                | "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/webp"
        ) || (essence.starts_with("application/") && essence.ends_with("+json"))
    });
    if !data {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
    }
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
}

/// The answer for every `/v1` request when no usable backend is configured.
pub fn unconfigured(error: OriginError, app_origin: &str) -> Response<Vec<u8>> {
    let detail = match error {
        OriginError::Missing => "サーバーの接続先が設定されていません。",
        OriginError::Invalid | OriginError::NotLoopback => {
            "サーバーの接続先の形式が正しくありません（http://127.0.0.1:<port> の形で指定してください）。"
        }
    };
    problem(StatusCode::SERVICE_UNAVAILABLE, detail, app_origin)
}

pub fn secure_headers(headers: &mut HeaderMap, app_origin: &str) {
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(origin) = HeaderValue::from_str(app_origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
}

pub struct Proxy {
    origin: Result<Url, OriginError>,
    client: Option<reqwest::Client>,
    app_origin: &'static str,
}

impl Proxy {
    pub fn new(origin: Result<Url, OriginError>, app_origin: &'static str) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self {
            origin,
            client,
            app_origin,
        }
    }

    pub fn from_env(app_origin: &'static str) -> Self {
        let origin = parse_backend_origin(std::env::var(ORIGIN_ENV).ok().as_deref());
        // Local configuration only (no secret): say once why /v1 will fail.
        match origin {
            Err(OriginError::Missing) => eprintln!(
                "knowledge-platform-desktop: {ORIGIN_ENV} is not set; /v1 requests answer 503"
            ),
            Err(_) => eprintln!(
                "knowledge-platform-desktop: {ORIGIN_ENV} must be exactly http://127.0.0.1:<port> or http://[::1]:<port>; /v1 requests answer 503"
            ),
            Ok(_) => {}
        }
        Self::new(origin, app_origin)
    }

    pub async fn forward(&self, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let app_origin = self.app_origin;
        let origin = match &self.origin {
            Ok(origin) => origin,
            Err(error) => return unconfigured(*error, app_origin),
        };
        let Some(client) = &self.client else {
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "サーバーへの接続を準備できません。",
                app_origin,
            );
        };
        let (parts, body) = request.into_parts();
        if !is_forwardable_method(&parts.method) {
            return problem(
                StatusCode::METHOD_NOT_ALLOWED,
                "この操作は利用できません。",
                app_origin,
            );
        }
        if !origin_allowed(&parts.headers, app_origin) {
            return problem(
                StatusCode::FORBIDDEN,
                "この画面からの要求ではありません。",
                app_origin,
            );
        }
        let Some(target) = parts
            .uri
            .path_and_query()
            .and_then(|value| api_target(origin, value.as_str()))
        else {
            return problem(
                StatusCode::BAD_REQUEST,
                "要求の宛先が正しくありません。",
                app_origin,
            );
        };
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return problem(
                StatusCode::PAYLOAD_TOO_LARGE,
                "送信できる大きさを超えています。",
                app_origin,
            );
        }
        let mut outbound = client
            .request(parts.method.clone(), target)
            .headers(forwarded_request_headers(&parts.headers));
        if !body.is_empty() {
            outbound = outbound.body(body);
        }
        let mut upstream = match outbound.send().await {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                return problem(
                    StatusCode::GATEWAY_TIMEOUT,
                    "サーバーの応答がありません。",
                    app_origin,
                );
            }
            Err(_) => {
                return problem(
                    StatusCode::BAD_GATEWAY,
                    "サーバーに接続できません。",
                    app_origin,
                );
            }
        };
        if upstream
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
        {
            return problem(
                StatusCode::BAD_GATEWAY,
                "サーバーの応答が大きすぎます。",
                app_origin,
            );
        }
        let status = upstream.status();
        let headers = returned_response_headers(upstream.headers());
        let mut bytes = Vec::new();
        loop {
            match upstream.chunk().await {
                Ok(Some(chunk)) => {
                    if bytes.len() + chunk.len() > MAX_RESPONSE_BODY_BYTES {
                        return problem(
                            StatusCode::BAD_GATEWAY,
                            "サーバーの応答が大きすぎます。",
                            app_origin,
                        );
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(error) if error.is_timeout() => {
                    return problem(
                        StatusCode::GATEWAY_TIMEOUT,
                        "サーバーの応答がありません。",
                        app_origin,
                    );
                }
                Err(_) => {
                    return problem(
                        StatusCode::BAD_GATEWAY,
                        "サーバーの応答が途中で切れました。",
                        app_origin,
                    );
                }
            }
        }
        let mut response = Response::new(bytes);
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        data_only(response.headers_mut());
        secure_headers(response.headers_mut(), app_origin);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Url {
        parse_backend_origin(Some("http://127.0.0.1:8080")).unwrap()
    }

    #[test]
    fn backend_origin_must_be_a_literal_loopback_http_origin_with_port() {
        assert!(parse_backend_origin(Some("http://127.0.0.1:8080")).is_ok());
        assert!(parse_backend_origin(Some("http://127.0.0.1:8080/")).is_ok());
        assert!(parse_backend_origin(Some("http://[::1]:9000")).is_ok());
        // An explicit default port is still an explicit port.
        let port80 = parse_backend_origin(Some("http://127.0.0.1:80")).unwrap();
        assert_eq!(
            api_target(&port80, "/v1/x").unwrap().as_str(),
            "http://127.0.0.1/v1/x"
        );
        assert_eq!(parse_backend_origin(None), Err(OriginError::Missing));
        assert_eq!(parse_backend_origin(Some("  ")), Err(OriginError::Missing));
        for invalid in [
            "https://127.0.0.1:8080",
            "http://127.0.0.1",
            "http://127.0.0.1:8080/v1",
            "http://127.0.0.1:8080/?a=1",
            "http://127.0.0.1:8080/#x",
            "http://user:pw@127.0.0.1:8080",
            "file:///etc/passwd",
            "127.0.0.1:8080",
        ] {
            assert_eq!(
                parse_backend_origin(Some(invalid)),
                Err(OriginError::Invalid),
                "{invalid}"
            );
        }
        for remote in [
            "http://localhost:8080",
            "http://10.0.0.1:8080",
            "http://example.com:80",
            "http://0.0.0.0:8080",
        ] {
            assert!(parse_backend_origin(Some(remote)).is_err(), "{remote}");
        }
    }

    #[test]
    fn only_the_v1_tree_of_the_configured_origin_is_reachable() {
        let origin = origin();
        assert_eq!(
            api_target(&origin, "/v1/documents?view=authoring")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:8080/v1/documents?view=authoring"
        );
        assert_eq!(
            api_target(&origin, "/v1").unwrap().as_str(),
            "http://127.0.0.1:8080/v1"
        );
        for escape in [
            "/v1/../health/ready",
            "/v1/%2e%2e/health/ready",
            "/v1/./../health",
            "//evil.example/v1/x",
            "/v1\\..\\health",
            "/v2/x",
            "/v1x",
            "v1/x",
            "/v1/x#frag",
            "/v1/../../etc/passwd",
            "/v1/../../../v1x",
        ] {
            assert_eq!(api_target(&origin, escape), None, "{escape}");
        }
        // An encoded slash stays one literal segment inside /v1 on the same origin.
        let literal = api_target(&origin, "/v1/..%2fhealth").unwrap();
        assert_eq!(
            (literal.host_str(), literal.path()),
            (Some("127.0.0.1"), "/v1/..%2fhealth")
        );
    }

    #[test]
    fn raw_path_selection_is_exact() {
        assert!(is_api_path("/v1"));
        assert!(is_api_path("/v1/organization/tasks"));
        assert!(!is_api_path("/v1x"));
        assert!(!is_api_path("/documents"));
        assert!(!is_api_path("/"));
    }

    #[test]
    fn a_present_origin_or_referer_must_be_the_app() {
        let app = "tauri://localhost";
        let with = |pairs: &[(&'static str, &'static str)]| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(
                    HeaderName::from_static(name),
                    HeaderValue::from_static(value),
                );
            }
            headers
        };
        // WebKitGTK sends neither Origin nor Sec-Fetch-* for same-origin
        // custom-scheme requests (observed in the real shell); the scheme is
        // only served to the main window, which only loads bundled app URLs.
        assert!(origin_allowed(&with(&[]), app));
        assert!(origin_allowed(
            &with(&[("referer", "tauri://localhost/documents?x=1")]),
            app
        ));
        assert!(origin_allowed(
            &with(&[("origin", "tauri://localhost")]),
            app
        ));
        for foreign in [
            &[("origin", "null")][..],
            &[("origin", "https://evil.example")],
            &[("origin", "tauri://localhost.evil")],
            &[("referer", "https://evil.example/tauri://localhost/")],
            &[("referer", "tauri://localhostevil/")],
            &[("referer", "not a url")],
            &[
                ("origin", "tauri://localhost"),
                ("referer", "https://evil.example/"),
            ],
        ] {
            assert!(!origin_allowed(&with(foreign), app), "{foreign:?}");
        }
    }

    #[test]
    fn methods_outside_the_api_set_are_refused() {
        for allowed in [
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ] {
            assert!(is_forwardable_method(&allowed));
        }
        for refused in [Method::OPTIONS, Method::TRACE, Method::CONNECT] {
            assert!(!is_forwardable_method(&refused));
        }
    }

    #[test]
    fn headers_are_allowlisted_in_both_directions() {
        let mut request = HeaderMap::new();
        for (name, value) in [
            ("accept", "application/json"),
            ("content-type", "multipart/form-data; boundary=x"),
            (
                "traceparent",
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            ),
            ("origin", "tauri://localhost"),
            ("cookie", "a=b"),
            ("authorization", "Bearer x"),
            ("x-principal-id", "someone"),
            ("x-acting-principal", "someone"),
            ("x-organization-profile", "approver-01"),
            ("host", "evil.example"),
            ("referer", "tauri://localhost/documents"),
        ] {
            request.insert(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        let kept = forwarded_request_headers(&request);
        let mut names: Vec<_> = kept.keys().map(HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["accept", "content-type", "traceparent"]);

        let mut response = HeaderMap::new();
        for (name, value) in [
            ("content-type", "application/json"),
            ("content-disposition", "attachment; filename=\"a.txt\""),
            ("set-cookie", "a=b"),
            ("location", "http://evil.example/"),
            ("access-control-allow-origin", "*"),
            ("connection", "close"),
            ("cache-control", "no-store"),
        ] {
            response.insert(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        let kept = returned_response_headers(&response);
        let mut names: Vec<_> = kept.keys().map(HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["cache-control", "content-disposition", "content-type"]
        );
    }

    #[test]
    fn a_wrong_origin_is_reported_as_wrong_not_as_missing() {
        let missing = unconfigured(OriginError::Missing, "tauri://localhost");
        let wrong = unconfigured(OriginError::NotLoopback, "tauri://localhost");
        let invalid = unconfigured(OriginError::Invalid, "tauri://localhost");
        let detail = |response: &Response<Vec<u8>>| {
            serde_json::from_slice::<serde_json::Value>(response.body()).unwrap()["detail"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(detail(&missing), "サーバーの接続先が設定されていません。");
        assert!(detail(&wrong).contains("http://127.0.0.1:<port>"));
        assert_eq!(detail(&wrong), detail(&invalid));
        for response in [&missing, &wrong, &invalid] {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    #[test]
    fn api_responses_never_become_app_origin_code_or_documents() {
        let typed = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(value));
            data_only(&mut headers);
            headers
        };
        // A document original uploaded as script/markup/style is served as bytes.
        for active in [
            "text/javascript",
            "application/javascript; charset=utf-8",
            "TEXT/JAVASCRIPT",
            "text/css",
            "text/html; charset=utf-8",
            "image/svg+xml",
            "application/xhtml+xml",
            "text/xml",
            "application/wasm",
            "application/pdf",
        ] {
            assert_eq!(
                typed(active)[header::CONTENT_TYPE],
                "application/octet-stream",
                "{active}"
            );
        }
        for data in [
            "application/json",
            "application/problem+json",
            "application/vnd.example+json; charset=utf-8",
            "text/plain; charset=utf-8",
            "application/octet-stream",
            "image/png",
            "image/jpeg",
        ] {
            assert_eq!(typed(data)[header::CONTENT_TYPE], data, "{data}");
        }
        let headers = typed("application/json");
        assert_eq!(
            headers[header::CONTENT_SECURITY_POLICY],
            "sandbox; default-src 'none'"
        );
        let mut untyped = HeaderMap::new();
        data_only(&mut untyped);
        assert_eq!(untyped[header::CONTENT_TYPE], "application/octet-stream");
    }

    #[test]
    fn problems_carry_no_backend_detail() {
        let response = problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "サーバーの接続先が設定されていません。",
            "tauri://localhost",
        );
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        let body: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(body["status"], 503);
    }
}
