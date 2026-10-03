use std::collections::VecDeque;
use std::future::pending;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flate2::{Compression, write::GzEncoder};
use search_http_client_poc::{
    AddressResolver, BoxFuture, GuardedTransport, Limits, TransportError,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

#[derive(Clone)]
struct SequenceResolver {
    answers: Arc<Mutex<VecDeque<Vec<SocketAddr>>>>,
    calls: Arc<AtomicUsize>,
}

impl SequenceResolver {
    fn new(answers: Vec<Vec<SocketAddr>>) -> Self {
        Self {
            answers: Arc::new(Mutex::new(answers.into())),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl AddressResolver for SequenceResolver {
    fn resolve(
        &self,
        _host: &str,
        _port: u16,
    ) -> BoxFuture<'_, Result<Vec<SocketAddr>, TransportError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let answer = self.answers.lock().unwrap().pop_front();
        Box::pin(async move { answer.ok_or(TransportError::Resolution) })
    }
}

fn loopback_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn response(status: &str, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len()).into_bytes();
    for (name, value) in headers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn synthetic_body(tag: u8) -> Vec<u8> {
    let mut body = vec![tag];
    body.extend_from_slice(
        &SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_be_bytes(),
    );
    body
}

async fn read_headers<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<()> {
    let mut recent = [0_u8; 4];
    for _ in 0..32_768 {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).await?;
        recent.rotate_left(1);
        recent[3] = byte[0];
        if recent == *b"\r\n\r\n" {
            return Ok(());
        }
    }
    Err(std::io::Error::other("request headers too large"))
}

enum Reply {
    Raw(Vec<u8>),
    DropConnection,
    HeadersThenStall(Vec<u8>, Duration),
    Trickle(Vec<u8>, Duration, usize, u8),
}

async fn spawn_http(reply: Reply) -> (SocketAddr, Arc<AtomicUsize>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let seen_task = Arc::clone(&seen);
    let reply = Arc::new(reply);
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let seen = Arc::clone(&seen_task);
            let reply = Arc::clone(&reply);
            tokio::spawn(async move {
                loop {
                    if read_headers(&mut stream).await.is_err() {
                        break;
                    }
                    seen.fetch_add(1, Ordering::SeqCst);
                    match reply.as_ref() {
                        Reply::Raw(bytes) => {
                            if stream.write_all(bytes).await.is_err() {
                                break;
                            }
                        }
                        Reply::DropConnection => break,
                        Reply::HeadersThenStall(headers, delay) => {
                            let _ = stream.write_all(headers).await;
                            tokio::time::sleep(*delay).await;
                            break;
                        }
                        Reply::Trickle(headers, interval, count, byte) => {
                            if stream.write_all(headers).await.is_err() {
                                break;
                            }
                            for _ in 0..*count {
                                tokio::time::sleep(*interval).await;
                                if stream.write_all(&[*byte]).await.is_err() {
                                    break;
                                }
                            }
                            break;
                        }
                    }
                }
            });
        }
    });
    (addr, seen, task)
}

fn transport(
    endpoint: &str,
    resolver: SequenceResolver,
    limits: Limits,
    root: Option<reqwest::Certificate>,
) -> GuardedTransport<SequenceResolver> {
    GuardedTransport::new_loopback_for_test(endpoint, resolver, limits, root).unwrap()
}

async fn fetch<R: AddressResolver>(
    client: &GuardedTransport<R>,
) -> Result<Vec<u8>, TransportError> {
    client.fetch(pending::<()>()).await
}

#[tokio::test]
async fn redirect_never_followed() {
    let (target_addr, target_seen, target_task) =
        spawn_http(Reply::Raw(response("200 OK", &[], &synthetic_body(b'T')))).await;
    let redirect = response(
        "302 Found",
        &[(
            "Location",
            format!("http://127.0.0.1:{}/", target_addr.port()),
        )],
        b"",
    );
    let (addr, seen, task) = spawn_http(Reply::Raw(redirect)).await;
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        Limits::default(),
        None,
    );
    assert_eq!(fetch(&client).await, Err(TransportError::Redirect));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(target_seen.load(Ordering::SeqCst), 0);
    task.abort();
    target_task.abort();
}

#[tokio::test]
async fn proxy_environment_ignored() {
    let (direct_addr, direct_seen, direct_task) =
        spawn_http(Reply::Raw(response("200 OK", &[], &synthetic_body(b'D')))).await;
    let (proxy_addr, proxy_seen, proxy_task) =
        spawn_http(Reply::Raw(response("200 OK", &[], &synthetic_body(b'P')))).await;
    let output = tokio::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("proxy_environment_ignored_child")
        .arg("--nocapture")
        .env("POC_PROXY_CHILD", "1")
        .env("POC_DIRECT_PORT", direct_addr.port().to_string())
        .env(
            "HTTP_PROXY",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        )
        .env(
            "http_proxy",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        )
        .env(
            "ALL_PROXY",
            format!("http://127.0.0.1:{}", proxy_addr.port()),
        )
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "child status: {}; stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(direct_seen.load(Ordering::SeqCst), 1);
    assert_eq!(proxy_seen.load(Ordering::SeqCst), 0);
    direct_task.abort();
    proxy_task.abort();
}

#[tokio::test]
async fn proxy_environment_ignored_child() {
    if std::env::var("POC_PROXY_CHILD").as_deref() != Ok("1") {
        return;
    }
    let port: u16 = std::env::var("POC_DIRECT_PORT").unwrap().parse().unwrap();
    let client = transport(
        &format!("http://p4.invalid:{port}/v1/search"),
        SequenceResolver::new(vec![vec![loopback_addr(port)]]),
        Limits::default(),
        None,
    );
    assert_eq!(fetch(&client).await.unwrap().first().copied(), Some(b'D'));
}

#[tokio::test]
async fn all_a_aaaa_checked_and_pinned_against_rebind() {
    let pinned_body = synthetic_body(b'R');
    let (addr, seen, task) = spawn_http(Reply::Raw(response("200 OK", &[], &pinned_body))).await;
    let denied = SocketAddr::new("169.254.169.254".parse().unwrap(), addr.port());
    let denied_mapped = SocketAddr::new("::ffff:169.254.169.254".parse().unwrap(), addr.port());
    let allowed_v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), addr.port());
    let resolver = SequenceResolver::new(vec![
        vec![addr, denied],
        vec![allowed_v6, denied_mapped],
        vec![addr],
        vec![denied],
    ]);
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", addr.port()),
        resolver.clone(),
        Limits::default(),
        None,
    );
    assert_eq!(fetch(&client).await, Err(TransportError::AddressDenied));
    assert_eq!(fetch(&client).await, Err(TransportError::AddressDenied));
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    // p4.invalid has no public DNS; success proves the validated address pin.
    assert_eq!(fetch(&client).await.unwrap(), pinned_body);
    // Re-resolve even if the prior HTTP/1.1 connection could be reused.
    assert_eq!(fetch(&client).await, Err(TransportError::AddressDenied));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 4);
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn production_constructor_rejects_unsafe_endpoints() {
    assert!(matches!(
        GuardedTransport::new_production("http://p4.invalid/v1/search", Limits::default()),
        Err(TransportError::Endpoint)
    ));
    assert!(matches!(
        GuardedTransport::new_production(
            "https://user:pass@p4.invalid/v1/search",
            Limits::default()
        ),
        Err(TransportError::Endpoint)
    ));
    assert!(matches!(
        GuardedTransport::new_production("https://127.0.0.1/v1/search", Limits::default()),
        Err(TransportError::Endpoint)
    ));
    assert!(
        GuardedTransport::new_production("https://p4.invalid/v1/search", Limits::default()).is_ok()
    );
    let long_path = format!("https://p4.invalid/{}", "x".repeat(16 * 1024));
    assert!(matches!(
        GuardedTransport::new_production(&long_path, Limits::default()),
        Err(TransportError::RequestLimitExceeded)
    ));
    let tiny_request = Limits {
        max_request_bytes: 128,
        ..Limits::default()
    };
    assert!(matches!(
        GuardedTransport::new_production("https://p4.invalid/v1/search", tiny_request),
        Err(TransportError::RequestLimitExceeded)
    ));
}

#[tokio::test]
async fn hostname_tls_validation_preserved() {
    use tokio_rustls::TlsAcceptor;
    use tokio_rustls::rustls::ServerConfig;
    use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

    let certified = rcgen::generate_simple_self_signed(vec!["safe.test".to_owned()]).unwrap();
    let cert_der = certified.cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = synthetic_body(b'S');
    let server_body = body.clone();
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let Ok(mut tls) = acceptor.accept(stream).await else {
                continue;
            };
            read_headers(&mut tls).await.unwrap();
            tls.write_all(&response("200 OK", &[], &server_body))
                .await
                .unwrap();
        }
    });
    let root = reqwest::Certificate::from_der(cert_der.as_ref()).unwrap();
    let positive = transport(
        &format!("https://safe.test:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        Limits::default(),
        Some(root.clone()),
    );
    assert_eq!(fetch(&positive).await.unwrap(), body);
    let mismatch = transport(
        &format!("https://wrong.test:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        Limits::default(),
        Some(root),
    );
    assert_eq!(fetch(&mismatch).await, Err(TransportError::Network));
    server.abort();
}

#[tokio::test]
async fn decoded_stream_limit_and_content_length() {
    let advertised = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n".to_vec();
    let (header_addr, header_seen, header_task) = spawn_http(Reply::HeadersThenStall(
        advertised,
        Duration::from_millis(500),
    ))
    .await;
    let limits = Limits {
        max_decoded_response_bytes: 32,
        read_timeout: Duration::from_millis(200),
        ..Limits::default()
    };
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", header_addr.port()),
        SequenceResolver::new(vec![vec![header_addr]]),
        limits,
        None,
    );
    assert_eq!(
        fetch(&client).await,
        Err(TransportError::ContentLengthExceeded)
    );
    assert_eq!(header_seen.load(Ordering::SeqCst), 1);
    header_task.abort();

    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&vec![*synthetic_body(b'G').last().unwrap(); 1024])
        .unwrap();
    let compressed = gzip.finish().unwrap();
    assert!(compressed.len() < 128);
    let body = response(
        "200 OK",
        &[("Content-Encoding", "gzip".to_owned())],
        &compressed,
    );
    let (gzip_addr, gzip_seen, gzip_task) = spawn_http(Reply::Raw(body)).await;
    let limits = Limits {
        max_decoded_response_bytes: 128,
        ..Limits::default()
    };
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", gzip_addr.port()),
        SequenceResolver::new(vec![vec![gzip_addr]]),
        limits,
        None,
    );
    assert_eq!(
        fetch(&client).await,
        Err(TransportError::DecodedLimitExceeded)
    );
    assert_eq!(gzip_seen.load(Ordering::SeqCst), 1);
    gzip_task.abort();
}

#[tokio::test]
async fn connect_read_total_deadline_and_cancel() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let stalled_tls = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(700)).await;
    });
    let limits = Limits {
        connect_timeout: Duration::from_millis(80),
        total_timeout: Duration::from_millis(550),
        ..Limits::default()
    };
    let client = transport(
        &format!("https://p4.invalid:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        limits,
        None,
    );
    let start = Instant::now();
    assert_eq!(fetch(&client).await, Err(TransportError::Timeout));
    assert!(start.elapsed() < Duration::from_millis(400));
    stalled_tls.abort();

    let headers = b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n".to_vec();
    let (addr, _, task) = spawn_http(Reply::HeadersThenStall(
        headers.clone(),
        Duration::from_millis(700),
    ))
    .await;
    let limits = Limits {
        read_timeout: Duration::from_millis(90),
        total_timeout: Duration::from_millis(550),
        ..Limits::default()
    };
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        limits,
        None,
    );
    let start = Instant::now();
    assert_eq!(fetch(&client).await, Err(TransportError::Timeout));
    assert!(start.elapsed() < Duration::from_millis(400));
    task.abort();

    let (addr, _, task) = spawn_http(Reply::Trickle(
        headers,
        Duration::from_millis(40),
        20,
        *synthetic_body(b'B').last().unwrap(),
    ))
    .await;
    let limits = Limits {
        read_timeout: Duration::from_millis(120),
        total_timeout: Duration::from_millis(180),
        ..Limits::default()
    };
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        limits,
        None,
    );
    assert_eq!(fetch(&client).await, Err(TransportError::TotalTimeout));
    task.abort();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (header_tx, header_rx) = tokio::sync::oneshot::channel::<()>();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n")
            .await
            .unwrap();
        header_tx.send(()).unwrap();
        let mut one = [0_u8; 1];
        tokio::time::timeout(Duration::from_millis(400), stream.read(&mut one))
            .await
            .unwrap()
            .unwrap()
    });
    let limits = Limits {
        read_timeout: Duration::from_millis(500),
        ..Limits::default()
    };
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        limits,
        None,
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let request = tokio::spawn(async move {
        client
            .fetch(async move {
                let _ = rx.await;
            })
            .await
    });
    header_rx.await.unwrap();
    tx.send(()).unwrap();
    assert_eq!(request.await.unwrap(), Err(TransportError::Cancelled));
    assert_eq!(
        peer.await.unwrap(),
        0,
        "cancellation must close the TCP body stream"
    );
}

#[tokio::test]
async fn no_automatic_retry_or_disk_spool() {
    let (addr, seen, task) = spawn_http(Reply::DropConnection).await;
    let client = transport(
        &format!("http://p4.invalid:{}/v1/search", addr.port()),
        SequenceResolver::new(vec![vec![addr]]),
        Limits::default(),
        None,
    );
    assert_eq!(fetch(&client).await, Err(TransportError::Network));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    task.abort();
}
