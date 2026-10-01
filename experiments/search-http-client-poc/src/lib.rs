//! Credential-free, isolated qualification of a guarded reqwest transport.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::time::Duration;

use reqwest::header::CONTENT_LENGTH;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

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
    TotalTimeout,
    Cancelled,
    Network,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_request_bytes: usize,
    pub max_decoded_response_bytes: usize,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub total_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: 16 * 1024,
            max_decoded_response_bytes: 1024 * 1024,
            connect_timeout: Duration::from_millis(500),
            read_timeout: Duration::from_millis(500),
            total_timeout: Duration::from_secs(2),
        }
    }
}

pub trait AddressResolver: Send + Sync {
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> BoxFuture<'_, Result<Vec<SocketAddr>, TransportError>>;
}

#[derive(Clone, Copy)]
pub struct SystemResolver;

impl AddressResolver for SystemResolver {
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> BoxFuture<'_, Result<Vec<SocketAddr>, TransportError>> {
        let host = host.to_owned();
        Box::pin(async move {
            tokio::net::lookup_host((host.as_str(), port))
                .await
                .map(|addresses| addresses.collect())
                .map_err(|_| TransportError::Resolution)
        })
    }
}

pub struct GuardedTransport<R: AddressResolver> {
    endpoint: reqwest::Url,
    resolver: R,
    limits: Limits,
    allow_loopback: bool,
    test_root: Option<reqwest::Certificate>,
}

impl GuardedTransport<SystemResolver> {
    pub fn new_production(endpoint: &str, limits: Limits) -> Result<Self, TransportError> {
        Self::new(endpoint, SystemResolver, limits, false, None)
    }
}

impl<R: AddressResolver> GuardedTransport<R> {
    /// Isolated PoC hook. The production adapter must gate this behind cfg(test).
    pub fn new_loopback_for_test(
        endpoint: &str,
        resolver: R,
        limits: Limits,
        test_root: Option<reqwest::Certificate>,
    ) -> Result<Self, TransportError> {
        Self::new(endpoint, resolver, limits, true, test_root)
    }

    fn new(
        endpoint: &str,
        resolver: R,
        limits: Limits,
        allow_loopback: bool,
        test_root: Option<reqwest::Certificate>,
    ) -> Result<Self, TransportError> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|_| TransportError::Endpoint)?;
        let host = endpoint.host_str().ok_or(TransportError::Endpoint)?;
        // GET only, no caller headers or body. Reserve room for Host, encoding and
        // HTTP framing in addition to the canonical URL before any network call.
        if host.len() > 253
            || endpoint
                .as_str()
                .len()
                .saturating_add(host.len())
                .saturating_add(512)
                > limits.max_request_bytes
        {
            return Err(TransportError::RequestLimitExceeded);
        }
        if host.parse::<IpAddr>().is_ok()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
            || (endpoint.scheme() != "https" && !(allow_loopback && endpoint.scheme() == "http"))
            || limits.max_decoded_response_bytes == 0
            || limits.connect_timeout.is_zero()
            || limits.read_timeout.is_zero()
            || limits.total_timeout.is_zero()
        {
            return Err(TransportError::Endpoint);
        }
        Ok(Self {
            endpoint,
            resolver,
            limits,
            allow_loopback,
            test_root,
        })
    }

    /// The URL is fixed by the trusted constructor; no provider URL or body enters.
    pub async fn fetch<C>(&self, cancel: C) -> Result<Vec<u8>, TransportError>
    where
        C: Future<Output = ()> + Send,
    {
        let operation = async {
            let host = self.endpoint.host_str().ok_or(TransportError::Endpoint)?;
            let port = self
                .endpoint
                .port_or_known_default()
                .ok_or(TransportError::Endpoint)?;
            let addrs = self.resolver.resolve(host, port).await?;
            if addrs.is_empty()
                || addrs.iter().any(|addr| {
                    addr.port() != port || !allowed_address(addr.ip(), self.allow_loopback)
                })
            {
                return Err(TransportError::AddressDenied);
            }
            // Fresh client per call prevents unchecked pool reuse. The checked addresses
            // are the sole DNS override for this hostname; TLS still sees the hostname.
            let mut builder = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .pool_max_idle_per_host(0)
                .pool_idle_timeout(None)
                .connect_timeout(self.limits.connect_timeout)
                .read_timeout(self.limits.read_timeout)
                .tls_backend_rustls()
                .gzip(true)
                .resolve_to_addrs(host, &addrs);
            if let Some(root) = &self.test_root {
                builder = builder.tls_certs_only([root.clone()]);
            }
            let client = builder.build().map_err(|_| TransportError::Network)?;
            // reqwest does not expose the TCP/TLS and response-header phases separately.
            // The outer connect bound conservatively includes response headers.
            let mut response = tokio::time::timeout(
                self.limits.connect_timeout,
                client.get(self.endpoint.clone()).send(),
            )
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(map_reqwest_error)?;
            if response.status().is_redirection() {
                return Err(TransportError::Redirect);
            }
            if !response.status().is_success() {
                return Err(TransportError::HttpStatus(response.status().as_u16()));
            }
            // reqwest removes Content-Length after automatic decompression. Check the
            // header when present, then count actual decoded chunks in every case.
            if let Some(raw) = response.headers().get(CONTENT_LENGTH) {
                let len: u64 = raw
                    .to_str()
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .ok_or(TransportError::Network)?;
                if len > self.limits.max_decoded_response_bytes as u64 {
                    return Err(TransportError::ContentLengthExceeded);
                }
            }
            let mut body = Vec::new();
            loop {
                let chunk = tokio::time::timeout(self.limits.read_timeout, response.chunk())
                    .await
                    .map_err(|_| TransportError::Timeout)?
                    .map_err(map_reqwest_error)?;
                let Some(chunk) = chunk else { break };
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
            Ok(body)
        };
        tokio::select! {
            biased;
            _ = cancel => Err(TransportError::Cancelled),
            result = tokio::time::timeout(self.limits.total_timeout, operation) => {
                result.map_err(|_| TransportError::TotalTimeout)?
            }
        }
    }
}

fn map_reqwest_error(error: reqwest::Error) -> TransportError {
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
    let seg = ip.segments();
    if (seg[0] & 0xe000) != 0x2000 {
        return false;
    }
    if seg[0] == 0x2001 && ((seg[1] & 0xfe00) == 0 || seg[1] == 0x0db8) {
        return false;
    }
    if seg[0] == 0x2002 || (seg[0] == 0x3fff && (seg[1] & 0xf000) == 0) {
        return false;
    }
    true
}
