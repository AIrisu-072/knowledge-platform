//! Guarded transport to one operator-registered origin.
//!
//! Every call resolves the registered hostname, rejects the whole answer if
//! any A/AAAA address is not public, pins the checked addresses for a fresh
//! client (so no unchecked pooled connection is reused), keeps hostname TLS
//! verification, ignores ambient proxies, never follows redirects or retries,
//! and bounds request bytes, decoded response bytes and the connect/read/call
//! deadlines. Only `RegisteredPath` values built by this crate reach the URL.

use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use search_application::SearchError;
use search_application::remote_registration::{RegisteredEndpoint, RemoteRegistrationLimits};

pub type TransportFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TransportError>> + Send + 'a>>;

/// Low-cardinality transport failures; none carries provider text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Endpoint,
    RequestLimitExceeded,
    Resolution,
    AddressDenied,
    Redirect,
    HttpStatus(u16),
    ContentLengthExceeded,
    DecodedLimitExceeded,
    Timeout,
    DeadlineExceeded,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportLimits {
    pub max_request_bytes: usize,
    pub max_decoded_response_bytes: usize,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub call_timeout: Duration,
}

impl TransportLimits {
    /// The operator registration's bounds: one call within `call_millis`,
    /// with the connect and read phases bounded by the same call budget.
    pub fn from_registration(limits: RemoteRegistrationLimits) -> Self {
        let call = Duration::from_millis(limits.call_millis);
        Self {
            max_request_bytes: limits.max_request_bytes,
            max_decoded_response_bytes: limits.max_decoded_response_bytes,
            connect_timeout: call,
            read_timeout: call,
            call_timeout: call,
        }
    }

    fn valid(self) -> bool {
        self.max_request_bytes > 0
            && self.max_decoded_response_bytes > 0
            && !self.connect_timeout.is_zero()
            && !self.read_timeout.is_zero()
            && !self.call_timeout.is_zero()
    }
}

pub trait AddressResolver: Send + Sync {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> TransportFuture<'a, Vec<SocketAddr>>;
}

/// The operating system resolver, consulted again on every call.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl AddressResolver for SystemResolver {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> TransportFuture<'a, Vec<SocketAddr>> {
        Box::pin(async move {
            tokio::net::lookup_host((host, port))
                .await
                .map(|addresses| addresses.collect())
                .map_err(|_| TransportError::Resolution)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Get,
    Post,
}

/// One of the adapter's fixed protocol paths under the registered base path.
/// Caller or provider strings enter only as percent-encoded segments/values.
#[derive(Clone, PartialEq, Eq)]
pub struct RegisteredPath {
    method: Method,
    suffix: String,
}

impl fmt::Debug for RegisteredPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RegisteredPath(<fixed>)")
    }
}

fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

impl RegisteredPath {
    pub fn catalog(cursor: Option<&str>) -> Self {
        Self {
            method: Method::Get,
            suffix: match cursor {
                Some(cursor) => format!("catalog?cursor={}", encode(cursor)),
                None => "catalog?cursor=".into(),
            },
        }
    }
    pub fn search() -> Self {
        Self {
            method: Method::Post,
            suffix: "search".into(),
        }
    }
    pub fn lookup() -> Self {
        Self {
            method: Method::Post,
            suffix: "lookup".into(),
        }
    }
    pub fn live() -> Self {
        Self {
            method: Method::Post,
            suffix: "live".into(),
        }
    }
    pub fn authorize() -> Self {
        Self {
            method: Method::Post,
            suffix: "authorize".into(),
        }
    }
    pub fn content(native_id: &str) -> Self {
        Self {
            method: Method::Get,
            suffix: format!("content/{}", encode(native_id)),
        }
    }
}

/// A complete 2xx response body, bounded by decoded bytes. It is consumed by
/// the protocol decoder and never logged.
pub struct BoundedResponse {
    body: Vec<u8>,
}

impl fmt::Debug for BoundedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoundedResponse({} bytes)", self.body.len())
    }
}

impl BoundedResponse {
    pub fn as_bytes(&self) -> &[u8] {
        &self.body
    }
}

pub struct GuardedHttpTransport {
    endpoint: RegisteredEndpoint,
    origin: reqwest::Url,
    base_path: String,
    host: String,
    port: u16,
    resolver: Arc<dyn AddressResolver>,
    limits: TransportLimits,
    allow_loopback: bool,
}

impl fmt::Debug for GuardedHttpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GuardedHttpTransport(<registered origin>)")
    }
}

fn invalid_endpoint() -> SearchError {
    SearchError::InvalidRequest("remote transport endpoint is invalid".into())
}

impl GuardedHttpTransport {
    /// HTTPS to the registered hostname only; no loopback or private address
    /// can be allowed through this constructor.
    pub fn new_production(
        endpoint: RegisteredEndpoint,
        resolver: Arc<dyn AddressResolver>,
        limits: TransportLimits,
    ) -> Result<Self, SearchError> {
        if endpoint.scheme() != "https" {
            return Err(invalid_endpoint());
        }
        Self::new(endpoint, resolver, limits, false)
    }

    /// Synthetic qualification only: plain HTTP to a resolver-pinned
    /// loopback address. Compiled only with `synthetic-loopback-test-only`.
    #[cfg(feature = "synthetic-loopback-test-only")]
    pub fn new_loopback_for_test(
        endpoint: RegisteredEndpoint,
        resolver: Arc<dyn AddressResolver>,
        limits: TransportLimits,
    ) -> Result<Self, SearchError> {
        Self::new(endpoint, resolver, limits, true)
    }

    fn new(
        endpoint: RegisteredEndpoint,
        resolver: Arc<dyn AddressResolver>,
        limits: TransportLimits,
        allow_loopback: bool,
    ) -> Result<Self, SearchError> {
        let host = endpoint.host().to_owned();
        if host.parse::<IpAddr>().is_ok() || host.contains(':') || !limits.valid() {
            return Err(invalid_endpoint());
        }
        let origin = reqwest::Url::parse(&format!(
            "{}://{}:{}/",
            endpoint.scheme(),
            host,
            endpoint.port()
        ))
        .map_err(|_| invalid_endpoint())?;
        if origin.host_str() != Some(host.as_str()) || origin.query().is_some() {
            return Err(invalid_endpoint());
        }
        Ok(Self {
            base_path: endpoint.base_path().trim_end_matches('/').to_owned(),
            port: endpoint.port(),
            endpoint,
            origin,
            host,
            resolver,
            limits,
            allow_loopback,
        })
    }

    pub const fn limits(&self) -> TransportLimits {
        self.limits
    }

    pub fn endpoint(&self) -> &RegisteredEndpoint {
        &self.endpoint
    }

    /// One bounded call. `body` is the adapter's own encoded request; an
    /// empty body is a GET. Nothing is retried, spooled or logged.
    pub fn request<'a>(
        &'a self,
        path: &'a RegisteredPath,
        body: &'a [u8],
        deadline: Instant,
    ) -> TransportFuture<'a, BoundedResponse> {
        Box::pin(async move {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::DeadlineExceeded);
            }
            let target = format!("{}/{}", self.base_path, path.suffix);
            let url = self
                .origin
                .join(&target)
                .map_err(|_| TransportError::Endpoint)?;
            // A `.`/`..` segment from a provider ID would be normalized away
            // to an unregistered path; the path must survive the join intact.
            let expected_path = target.split('?').next().unwrap_or_default();
            if url.host_str() != Some(self.host.as_str())
                || url.port_or_known_default() != Some(self.port)
                || url.scheme() != self.origin.scheme()
                || url.path() != expected_path
            {
                return Err(TransportError::Endpoint);
            }
            // Reserve room for the request line, Host and framing headers.
            if url
                .as_str()
                .len()
                .saturating_add(body.len())
                .saturating_add(512)
                > self.limits.max_request_bytes
            {
                return Err(TransportError::RequestLimitExceeded);
            }
            let total = remaining.min(self.limits.call_timeout);
            tokio::time::timeout(total, self.call(url, path.method, body))
                .await
                .map_err(|_| TransportError::DeadlineExceeded)?
        })
    }

    async fn call(
        &self,
        url: reqwest::Url,
        method: Method,
        body: &[u8],
    ) -> Result<BoundedResponse, TransportError> {
        let addresses = self.resolver.resolve(&self.host, self.port).await?;
        if addresses.is_empty()
            || addresses.iter().any(|address| {
                address.port() != self.port || !allowed_address(address.ip(), self.allow_loopback)
            })
        {
            return Err(TransportError::AddressDenied);
        }
        // A fresh client per call: the checked addresses are the only DNS
        // answer for this hostname, and no pooled connection outlives them.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(None)
            .connect_timeout(self.limits.connect_timeout)
            .read_timeout(self.limits.read_timeout)
            .tls_backend_rustls()
            .gzip(true)
            .resolve_to_addrs(&self.host, &addresses)
            .build()
            .map_err(|_| TransportError::Network)?;
        let request = match method {
            Method::Get => client.get(url),
            Method::Post => client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec()),
        };
        let mut response = tokio::time::timeout(self.limits.connect_timeout, request.send())
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(map_error)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(TransportError::Redirect);
        }
        if !status.is_success() {
            return Err(TransportError::HttpStatus(status.as_u16()));
        }
        // Decompression can remove Content-Length; check it when present and
        // always count the decoded bytes actually received.
        if let Some(raw) = response.headers().get(CONTENT_LENGTH) {
            let length: u64 = raw
                .to_str()
                .ok()
                .and_then(|value| value.parse().ok())
                .ok_or(TransportError::Network)?;
            if length > self.limits.max_decoded_response_bytes as u64 {
                return Err(TransportError::ContentLengthExceeded);
            }
        }
        let mut body = Vec::new();
        while let Some(chunk) = tokio::time::timeout(self.limits.read_timeout, response.chunk())
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(map_error)?
        {
            if chunk.len()
                > self
                    .limits
                    .max_decoded_response_bytes
                    .saturating_sub(body.len())
            {
                return Err(TransportError::DecodedLimitExceeded);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(BoundedResponse { body })
    }
}

fn map_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Network
    }
}

fn allowed_address(ip: IpAddr, allow_loopback: bool) -> bool {
    match ip {
        IpAddr::V4(ip) => (allow_loopback && ip.is_loopback()) || public_v4(ip),
        IpAddr::V6(ip) => (allow_loopback && ip.is_loopback()) || public_v6(ip),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 0 || b == 168 || (b == 88 && c == 99)))
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        || (a == 203 && b == 0 && c == 113))
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if ip.to_ipv4_mapped().is_some() {
        return false;
    }
    let segments = ip.segments();
    if (segments[0] & 0xe000) != 0x2000 {
        return false;
    }
    if segments[0] == 0x2001 && ((segments[1] & 0xfe00) == 0 || segments[1] == 0x0db8) {
        return false;
    }
    !(segments[0] == 0x2002 || (segments[0] == 0x3fff && (segments[1] & 0xf000) == 0))
}
