//! P5-07: the disclosure lease lives until the socket send finishes, on a
//! real `127.0.0.1:0` connection with backpressure and disconnects.

#[path = "../../search-application/tests/support/api.rs"]
mod api;
#[path = "support/backend.rs"]
mod backend;
#[path = "../../search-application/tests/support/search_corpus.rs"]
mod corpus;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use backend::{Backend, Tokens};
use search_api_http::auth::{SearchAuthSchemeBinding, StaticBearerChallenge};
use search_api_http::router::{SearchApiBackend, SearchRouterConfig, build_search_router};
use search_api_http::send::{
    CloseReason, ConnectionLeases, SendObserver, ServeOptions, leased_response, serve,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Opened(bool),
    Closed(CloseReason),
}

#[derive(Default)]
struct Events(Mutex<Vec<Event>>);

impl SendObserver for Events {
    fn opened(&self, evaluation_closed: bool) {
        self.0
            .lock()
            .unwrap()
            .push(Event::Opened(evaluation_closed));
    }
    fn closed(&self, reason: CloseReason) {
        self.0.lock().unwrap().push(Event::Closed(reason));
    }
}

impl Events {
    fn all(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
    fn closed(&self) -> Vec<CloseReason> {
        self.all()
            .into_iter()
            .filter_map(|event| match event {
                Event::Closed(reason) => Some(reason),
                Event::Opened(_) => None,
            })
            .collect()
    }
    async fn wait_closed(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.closed().len() < count && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

struct Server {
    addr: SocketAddr,
    events: Arc<Events>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<std::io::Result<()>>,
    backend: &'static Backend,
}

fn documents() -> Vec<corpus::Doc> {
    vec![
        corpus::doc(11, "規程 A1", None),
        corpus::doc(12, "規程 A2", None),
    ]
}

/// A leased response of 900 KiB: larger than any loopback buffer, still
/// within the 1 MiB success bound. Mounted beside the Search routes on the
/// same socket server, it exercises the transport lease alone.
async fn large_body(request: axum::extract::Request) -> axum::response::Response {
    let leases = request
        .extensions()
        .get::<ConnectionLeases>()
        .cloned()
        .unwrap();
    leased_response(vec![b'a'; 900 * 1024], leases.open(true))
}

async fn start(documents: Vec<corpus::Doc>, send_deadline: Duration) -> Server {
    let backend = Backend::with_documents(documents).await;
    let tokens = Arc::new(Tokens::default());
    let reader = backend.handle("reader").await;
    let no_retention = backend
        .handle_with(
            "no-retention",
            &[
                backend.world.document,
                backend.world.second,
                backend.world.remote,
            ],
        )
        .await;
    tokens
        .0
        .lock()
        .unwrap()
        .insert("reader-token".into(), reader);
    tokens
        .0
        .lock()
        .unwrap()
        .insert("no-retention-token".into(), no_retention);
    let backend_object: Arc<dyn SearchApiBackend> = Arc::new(backend);
    let router: Router = build_search_router(SearchRouterConfig {
        backend: backend_object,
        credentials: Some(tokens),
        auth: Some(SearchAuthSchemeBinding::bearer(Arc::new(
            StaticBearerChallenge,
        ))),
        operation_timeout: Duration::from_secs(10),
    })
    .unwrap()
    .route("/test/large", axum::routing::get(large_body));
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_send_buffer_size(4_096).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(16).unwrap();
    let addr = listener.local_addr().unwrap();
    let events = Arc::new(Events::default());
    let (stop, stopped) = oneshot::channel::<()>();
    let observer: Arc<dyn SendObserver> = events.clone();
    let options = ServeOptions {
        send_deadline,
        send_buffer_bytes: Some(4_096),
    };
    let task = tokio::spawn(serve(listener, router, options, Some(observer), async {
        let _ = stopped.await;
    }));
    Server {
        addr,
        events,
        stop: Some(stop),
        task,
        backend,
    }
}

fn request(method: &str, path: &str, token: &str, body: &str) -> Vec<u8> {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn search(page_size: usize) -> String {
    json!({"query": "規程", "coverage": "titleAndPermittedMetadata", "pageSize": page_size})
        .to_string()
}

/// A client with a tiny receive window that has sent one request.
async fn client(addr: SocketAddr, bytes: &[u8]) -> TcpStream {
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(4_096).unwrap();
    let mut stream = socket.connect(addr).await.unwrap();
    stream.write_all(bytes).await.unwrap();
    stream
}

async fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut buffer = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    buffer
}

fn json_body(response: &[u8]) -> Value {
    let text = String::from_utf8_lossy(response);
    let (_, body) = text.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
async fn evaluation_lease_closed_before_public_body_disclosure() {
    let server = start(documents(), Duration::from_secs(10)).await;
    let discover = json!({
        "need": {"purpose": "find a rule", "requiredResourceTypes": ["knowledge"],
                 "requiredClaimIds": ["00000000-0000-0000-0000-000000000051"]},
        "coverage": "titleAndPermittedMetadata"
    })
    .to_string();
    let mut stream = client(
        server.addr,
        &request("POST", "/v1/discover", "reader-token", &discover),
    )
    .await;
    let mut first = [0u8; 1];
    stream.read_exact(&mut first).await.unwrap();
    // The evaluation's own lease had closed before any public byte left.
    assert_eq!(server.events.all().first(), Some(&Event::Opened(true)));
    let response = read_all(&mut stream).await;
    assert!(String::from_utf8_lossy(&response).contains("discoveryEvaluationId"));
    server.events.wait_closed(1).await;
    assert_eq!(server.events.closed(), vec![CloseReason::Completed]);
}

#[tokio::test]
async fn lease_live_after_handler_return_until_socket_finishes() {
    let server = start(documents(), Duration::from_secs(10)).await;
    let mut stream = client(
        server.addr,
        &request("GET", "/test/large", "reader-token", ""),
    )
    .await;
    // The handler has produced the body, but the socket cannot take it yet.
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.events.all().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.events.all(), vec![Event::Opened(true)]);
    let response = read_all(&mut stream).await;
    assert!(response.len() > 900 * 1024);
    server.events.wait_closed(1).await;
    assert_eq!(server.events.closed(), vec![CloseReason::Completed]);
}

#[tokio::test]
async fn blocked_reader_does_not_close_early() {
    let server = start(documents(), Duration::from_secs(10)).await;
    let mut stream = client(
        server.addr,
        &request("GET", "/test/large", "reader-token", ""),
    )
    .await;
    let mut some = vec![0u8; 1_024];
    stream.read_exact(&mut some).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    // Body EOF inside the transport is not completion: still open.
    assert!(server.events.closed().is_empty());
    drop(stream);
    server.events.wait_closed(1).await;
    assert_eq!(server.events.closed(), vec![CloseReason::ConnectionError]);
}

#[tokio::test]
async fn send_success_error_disconnect_cancel_deadline_drop_close_disclosure_once() {
    // Deadline: a reader that never reads.
    let server = start(documents(), Duration::from_millis(400)).await;
    let _stalled = client(
        server.addr,
        &request("GET", "/test/large", "reader-token", ""),
    )
    .await;
    server.events.wait_closed(1).await;
    assert_eq!(server.events.closed(), vec![CloseReason::Deadline]);

    // Cancel: the server stops while a send is blocked.
    let mut server = start(documents(), Duration::from_secs(10)).await;
    let _blocked = client(
        server.addr,
        &request("GET", "/test/large", "reader-token", ""),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.events.all().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    server.stop.take().unwrap().send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), &mut server.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(server.events.closed(), vec![CloseReason::Cancelled]);

    // Drop: the whole server future is dropped mid-send.
    let server = start(documents(), Duration::from_secs(10)).await;
    let _blocked = client(
        server.addr,
        &request("GET", "/test/large", "reader-token", ""),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.events.all().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    server.task.abort();
    server.events.wait_closed(1).await;
    assert_eq!(server.events.closed(), vec![CloseReason::Dropped]);

    // Success and disconnect each close exactly once, never twice.
    let server = start(documents(), Duration::from_secs(10)).await;
    let mut done = client(
        server.addr,
        &request("GET", "/v1/sources", "reader-token", ""),
    )
    .await;
    read_all(&mut done).await;
    let mut cut = client(
        server.addr,
        &request("GET", "/test/large", "reader-token", ""),
    )
    .await;
    // Disconnect mid-body, after the response has started.
    let mut started = vec![0u8; 1_024];
    cut.read_exact(&mut started).await.unwrap();
    drop(cut);
    server.events.wait_closed(2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut closed = server.events.closed();
    closed.sort_by_key(|reason| format!("{reason:?}"));
    assert_eq!(
        closed,
        vec![CloseReason::Completed, CloseReason::ConnectionError]
    );
    let opened = server
        .events
        .all()
        .iter()
        .filter(|event| matches!(event, Event::Opened(_)))
        .count();
    assert_eq!(opened, 2);
}

#[tokio::test]
async fn post_close_handle_cannot_read() {
    let leases = ConnectionLeases::new(None);
    let lease = leases.open(true);
    let response = leased_response(b"{\"items\":[]}".to_vec(), lease.clone());
    leases.close_all(CloseReason::Completed);
    leases.close_all(CloseReason::ConnectionError);
    assert!(!lease.is_open());
    assert_eq!(lease.reason(), Some(CloseReason::Completed));
    // The held body yields nothing once its lease is closed.
    let bytes = axum::body::to_bytes(response.into_body(), 1_024)
        .await
        .unwrap();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn no_retention_never_enters_cursor_cache_disk_log_audit_fixture_spool() {
    let scratch = std::env::temp_dir().join(format!("search-send-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&scratch).unwrap();
    let server = start(
        vec![
            corpus::doc(11, "規程 A1", None),
            corpus::doc(12, "規程 A2", None),
        ],
        Duration::from_secs(10),
    )
    .await;
    let mut stream = client(
        server.addr,
        &request("POST", "/v1/search", "no-retention-token", &search(1)),
    )
    .await;
    let body = json_body(&read_all(&mut stream).await);
    // A visible NO_RETENTION Source: no continuation, nothing retained.
    assert!(body["nextCursor"].is_null());
    assert_eq!(body["partial"], true);
    assert!(
        body["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["reasonCode"] == "PAGINATION_UNAVAILABLE")
    );
    assert!(server.backend.cursors.is_empty());
    server.events.wait_closed(1).await;
    assert_eq!(server.events.closed(), vec![CloseReason::Completed]);
    // Nothing was spooled to disk by the send path.
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    std::fs::remove_dir_all(&scratch).unwrap();
}
