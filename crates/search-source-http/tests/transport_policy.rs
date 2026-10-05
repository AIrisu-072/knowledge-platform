//! P4-16: fixed-origin transport policy on real local TCP. Every server,
//! body and certificate-free endpoint is generated at runtime.

use std::collections::VecDeque;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::GzEncoder;
use search_application::remote_registration::{RegisteredEndpoint, RemoteRegistrationLimits};
use search_source_http::transport::{
    AddressResolver, GuardedHttpTransport, RegisteredPath, TransportError, TransportFuture,
    TransportLimits,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Answers one queued address list per call and counts calls.
struct Resolver {
    answers: Mutex<VecDeque<Vec<SocketAddr>>>,
    calls: Mutex<usize>,
}

impl Resolver {
    fn new(answers: Vec<Vec<SocketAddr>>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into()),
            calls: Mutex::new(0),
        })
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl AddressResolver for Resolver {
    fn resolve<'a>(&'a self, _: &'a str, _: u16) -> TransportFuture<'a, Vec<SocketAddr>> {
        *self.calls.lock().unwrap() += 1;
        let answer = self.answers.lock().unwrap().pop_front();
        Box::pin(async move { answer.ok_or(TransportError::Resolution) })
    }
}

#[derive(Clone)]
enum Reply {
    Raw(Vec<u8>),
    Drop,
    Stall(Vec<u8>),
}

/// A runtime HTTP/1.1 server recording each request head.
struct Server {
    addr: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = heads.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let seen = seen.clone();
                let reply = reply.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && head.len() < 32 * 1024 {
                        if stream.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let text = String::from_utf8_lossy(&head).to_string();
                    let length = text
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    let mut body = vec![0u8; length];
                    let _ = stream.read_exact(&mut body).await;
                    seen.lock().unwrap().push(text);
                    match reply {
                        Reply::Raw(bytes) => {
                            let _ = stream.write_all(&bytes).await;
                        }
                        Reply::Drop => {}
                        Reply::Stall(bytes) => {
                            let _ = stream.write_all(&bytes).await;
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                });
            }
        });
        Self { addr, heads }
    }

    fn requests(&self) -> usize {
        self.heads.lock().unwrap().len()
    }

    fn first_line(&self, index: usize) -> String {
        self.heads.lock().unwrap()[index]
            .lines()
            .next()
            .unwrap()
            .to_owned()
    }
}

fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len()).into_bytes();
    for (name, value) in headers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn ok(body: &[u8]) -> Reply {
    Reply::Raw(response(
        "200 OK",
        &[("Content-Type", "application/json")],
        body,
    ))
}

fn canary() -> TransportLimits {
    TransportLimits::from_registration(RemoteRegistrationLimits::synthetic_canary())
}

fn short() -> TransportLimits {
    TransportLimits {
        connect_timeout: Duration::from_millis(300),
        read_timeout: Duration::from_millis(300),
        call_timeout: Duration::from_millis(600),
        ..canary()
    }
}

fn endpoint(scheme: &str, port: u16) -> RegisteredEndpoint {
    RegisteredEndpoint::new(scheme, "catalog.example.test", port, "/v1").unwrap()
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn loopback_transport(
    port: u16,
    resolver: Arc<Resolver>,
    limits: TransportLimits,
) -> GuardedHttpTransport {
    GuardedHttpTransport::new_loopback_for_test(endpoint("http", port), resolver, limits).unwrap()
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

#[tokio::test]
async fn private_and_mapped_ip_rejected() {
    let port = 443;
    let private: Vec<IpAddr> = vec![
        Ipv4Addr::new(10, 0, 0, 1).into(),
        Ipv4Addr::new(127, 0, 0, 1).into(),
        Ipv4Addr::new(169, 254, 169, 254).into(),
        Ipv4Addr::new(172, 16, 0, 1).into(),
        Ipv4Addr::new(192, 168, 1, 1).into(),
        Ipv4Addr::new(100, 64, 0, 1).into(),
        Ipv6Addr::LOCALHOST.into(),
        "::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap().into(),
        "fd00::1".parse::<Ipv6Addr>().unwrap().into(),
        "fe80::1".parse::<Ipv6Addr>().unwrap().into(),
    ];
    for ip in private {
        // One public and one private answer: the whole answer is refused.
        let resolver = Resolver::new(vec![vec![
            SocketAddr::new(Ipv4Addr::new(93, 184, 216, 34).into(), port),
            SocketAddr::new(ip, port),
        ]]);
        let transport = GuardedHttpTransport::new_production(
            endpoint("https", port),
            resolver.clone(),
            canary(),
        )
        .unwrap();
        assert_eq!(
            transport
                .request(&RegisteredPath::search(), b"{}", soon())
                .await
                .unwrap_err(),
            TransportError::AddressDenied,
            "{ip}"
        );
        assert_eq!(resolver.calls(), 1);
    }
    // Production refuses plain HTTP and IP-literal origins outright.
    let resolver = Resolver::new(vec![]);
    assert!(
        GuardedHttpTransport::new_production(endpoint("http", port), resolver.clone(), canary())
            .is_err()
    );
    let literal = RegisteredEndpoint::new("https", "127.0.0.1", port, "/v1").unwrap();
    assert!(GuardedHttpTransport::new_production(literal, resolver, canary()).is_err());
}

#[tokio::test]
async fn rebinding_and_pool_reuse_rechecked() {
    let server = Server::start(ok(br#"{"ok":true}"#)).await;
    let port = server.addr.port();
    let resolver = Resolver::new(vec![
        vec![loopback(port)],
        vec![SocketAddr::new(Ipv4Addr::new(10, 0, 0, 7).into(), port)],
        vec![loopback(port)],
    ]);
    let transport = loopback_transport(port, resolver.clone(), canary());
    let path = RegisteredPath::search();
    assert!(transport.request(&path, b"{}", soon()).await.is_ok());
    // The resolver changed its answer: no connection is reused past it.
    assert_eq!(
        transport.request(&path, b"{}", soon()).await.unwrap_err(),
        TransportError::AddressDenied
    );
    assert!(transport.request(&path, b"{}", soon()).await.is_ok());
    assert_eq!(resolver.calls(), 3);
    assert_eq!(server.requests(), 2);
}

#[tokio::test]
async fn redirect_and_proxy_denied() {
    let target = Server::start(ok(b"{}")).await;
    let location = format!(
        "http://catalog.example.test:{}/v1/elsewhere",
        target.addr.port()
    );
    let redirect = Server::start(Reply::Raw(response(
        "302 Found",
        &[("Location", location.as_str())],
        b"",
    )))
    .await;
    let port = redirect.addr.port();
    let transport = loopback_transport(
        port,
        Resolver::new(vec![vec![loopback(port)], vec![loopback(port)]]),
        canary(),
    );
    assert_eq!(
        transport
            .request(&RegisteredPath::search(), b"{}", soon())
            .await
            .unwrap_err(),
        TransportError::Redirect
    );
    assert_eq!(redirect.requests(), 1);
    assert_eq!(target.requests(), 0);

    // An ambient proxy is never consulted.
    let proxy = Server::start(ok(b"{}")).await;
    let direct = Server::start(ok(b"{}")).await;
    let proxy_url = format!("http://{}", proxy.addr);
    // SAFETY: this test binary reads no proxy variable elsewhere; the guard
    // under test must ignore them regardless.
    unsafe {
        for name in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            std::env::set_var(name, &proxy_url);
        }
    }
    let port = direct.addr.port();
    let transport = loopback_transport(port, Resolver::new(vec![vec![loopback(port)]]), canary());
    assert!(
        transport
            .request(&RegisteredPath::search(), b"{}", soon())
            .await
            .is_ok()
    );
    assert_eq!(direct.requests(), 1);
    assert_eq!(proxy.requests(), 0);
}

#[tokio::test]
async fn caller_or_provider_url_never_fetched() {
    let server = Server::start(ok(b"{}")).await;
    let port = server.addr.port();
    let transport = loopback_transport(
        port,
        Resolver::new(vec![vec![loopback(port)], vec![loopback(port)]]),
        canary(),
    );
    let hostile = "https://elsewhere.example.test/raw?x=1#frag";
    transport
        .request(&RegisteredPath::content(hostile), b"", soon())
        .await
        .unwrap();
    transport
        .request(
            &RegisteredPath::catalog(Some("../../admin?all")),
            b"",
            soon(),
        )
        .await
        .unwrap();
    // Only the registered origin and fixed paths, the values encoded.
    assert_eq!(
        server.first_line(0),
        "GET /v1/content/https%3A%2F%2Felsewhere.example.test%2Fraw%3Fx%3D1%23frag HTTP/1.1"
    );
    assert_eq!(
        server.first_line(1),
        "GET /v1/catalog?cursor=..%2F..%2Fadmin%3Fall HTTP/1.1"
    );
    for head in server.heads.lock().unwrap().iter() {
        assert!(
            head.to_ascii_lowercase()
                .contains(&format!("host: catalog.example.test:{port}"))
        );
        assert!(!head.contains("elsewhere.example.test:"));
    }
}

#[tokio::test]
async fn decoded_oversize_and_timeout_close_body() {
    // The canary bounds come from the registration.
    let limits = canary();
    assert_eq!(limits.max_request_bytes, 16 * 1024);
    assert_eq!(limits.max_decoded_response_bytes, 1024 * 1024);
    assert_eq!(limits.call_timeout, Duration::from_secs(2));

    let big = Server::start(ok(&vec![b'a'; 1024 * 1024 + 1])).await;
    let port = big.addr.port();
    let transport = loopback_transport(port, Resolver::new(vec![vec![loopback(port)]]), limits);
    assert_eq!(
        transport
            .request(&RegisteredPath::search(), b"{}", soon())
            .await
            .unwrap_err(),
        TransportError::ContentLengthExceeded
    );
    // Small on the wire, oversized once decoded.
    let mut gzip = GzEncoder::new(Vec::new(), Compression::best());
    gzip.write_all(&vec![b'a'; 2 * 1024 * 1024]).unwrap();
    let compressed = gzip.finish().unwrap();
    let bomb = Server::start(Reply::Raw(response(
        "200 OK",
        &[("Content-Encoding", "gzip")],
        &compressed,
    )))
    .await;
    let port = bomb.addr.port();
    let transport = loopback_transport(port, Resolver::new(vec![vec![loopback(port)]]), limits);
    assert_eq!(
        transport
            .request(&RegisteredPath::search(), b"{}", soon())
            .await
            .unwrap_err(),
        TransportError::DecodedLimitExceeded
    );
    // Headers, then a stalled body: bounded by the read/call deadline.
    let stall = Server::start(Reply::Stall(
        b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\n\r\n{".to_vec(),
    ))
    .await;
    let port = stall.addr.port();
    let transport = loopback_transport(port, Resolver::new(vec![vec![loopback(port)]]), short());
    let started = Instant::now();
    assert!(matches!(
        transport
            .request(&RegisteredPath::search(), b"{}", soon())
            .await
            .unwrap_err(),
        TransportError::Timeout | TransportError::DeadlineExceeded
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    // Oversized request and an expired deadline never reach the network.
    let quiet = Server::start(ok(b"{}")).await;
    let port = quiet.addr.port();
    let resolver = Resolver::new(vec![vec![loopback(port)], vec![loopback(port)]]);
    let transport = loopback_transport(port, resolver.clone(), limits);
    assert_eq!(
        transport
            .request(&RegisteredPath::search(), &vec![b'x'; 16 * 1024], soon())
            .await
            .unwrap_err(),
        TransportError::RequestLimitExceeded
    );
    assert_eq!(
        transport
            .request(&RegisteredPath::search(), b"{}", Instant::now())
            .await
            .unwrap_err(),
        TransportError::DeadlineExceeded
    );
    assert_eq!(resolver.calls(), 0);
    assert_eq!(quiet.requests(), 0);
}

#[tokio::test]
async fn no_automatic_retry() {
    for (reply, expected) in [
        (Reply::Drop, None),
        (
            Reply::Raw(response("503 Service Unavailable", &[], b"")),
            Some(TransportError::HttpStatus(503)),
        ),
        (
            Reply::Raw(response(
                "429 Too Many Requests",
                &[("Retry-After", "0")],
                b"",
            )),
            Some(TransportError::HttpStatus(429)),
        ),
    ] {
        let server = Server::start(reply).await;
        let port = server.addr.port();
        let transport = loopback_transport(
            port,
            Resolver::new(vec![vec![loopback(port)], vec![loopback(port)]]),
            canary(),
        );
        let error = transport
            .request(&RegisteredPath::search(), b"{}", soon())
            .await
            .unwrap_err();
        if let Some(expected) = expected {
            assert_eq!(error, expected);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(server.requests(), 1);
    }
}
