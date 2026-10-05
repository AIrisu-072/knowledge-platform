//! P5-07: the body-owned disclosure lease and the socket-completion server.
//!
//! The final gate produces the bounded JSON once; from then on the bytes are
//! owned by a `LeasedBody` whose `SendLease` stays open until the connection
//! carrying it has finished: success (all bytes written and the connection
//! closed), write error or disconnect, the send deadline, server cancel or
//! drop. Neither the handler returning nor the body reaching EOF counts as
//! completion. Each connection serves exactly one HTTP/1.1 response without
//! keep-alive, so the connection future's completion is the socket send
//! completion of that response. No bytes are kept after closing.

use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body::{Frame, SizeHint};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tower::ServiceExt;

use crate::problem::{NO_SNIFF, PRIVATE_NO_STORE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Completed,
    ConnectionError,
    Deadline,
    Cancelled,
    Dropped,
}

/// Payload-free lifecycle events for tests and telemetry.
pub trait SendObserver: Send + Sync {
    fn opened(&self, evaluation_closed: bool);
    fn closed(&self, reason: CloseReason);
}

struct LeaseState {
    open: bool,
    reason: Option<CloseReason>,
}

/// The transient disclosure lease of one response body.
pub struct SendLease {
    state: Mutex<LeaseState>,
    observer: Option<Arc<dyn SendObserver>>,
}

impl fmt::Debug for SendLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendLease(<transient>)")
    }
}

impl SendLease {
    /// Closes exactly once; later calls are ignored.
    fn close(&self, reason: CloseReason) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.open {
            return false;
        }
        state.open = false;
        state.reason = Some(reason);
        drop(state);
        if let Some(observer) = &self.observer {
            observer.closed(reason);
        }
        true
    }

    pub fn is_open(&self) -> bool {
        self.state.lock().map(|state| state.open).unwrap_or(false)
    }

    pub fn reason(&self) -> Option<CloseReason> {
        self.state.lock().ok().and_then(|state| state.reason)
    }
}

struct Registry {
    leases: Mutex<Vec<Arc<SendLease>>>,
    observer: Option<Arc<dyn SendObserver>>,
}

impl Drop for Registry {
    fn drop(&mut self) {
        if let Ok(leases) = self.leases.lock() {
            for lease in leases.iter() {
                lease.close(CloseReason::Dropped);
            }
        }
    }
}

/// The leases of one connection, attached to each of its requests.
#[derive(Clone)]
pub struct ConnectionLeases(Arc<Registry>);

impl fmt::Debug for ConnectionLeases {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConnectionLeases(<connection>)")
    }
}

impl ConnectionLeases {
    pub fn new(observer: Option<Arc<dyn SendObserver>>) -> Self {
        Self(Arc::new(Registry {
            leases: Mutex::new(Vec::new()),
            observer,
        }))
    }

    pub fn open(&self, evaluation_closed: bool) -> Arc<SendLease> {
        let lease = Arc::new(SendLease {
            state: Mutex::new(LeaseState {
                open: true,
                reason: None,
            }),
            observer: self.0.observer.clone(),
        });
        if let Some(observer) = &self.0.observer {
            observer.opened(evaluation_closed);
        }
        if let Ok(mut leases) = self.0.leases.lock() {
            leases.push(lease.clone());
        }
        lease
    }

    pub fn close_all(&self, reason: CloseReason) {
        if let Ok(leases) = self.0.leases.lock() {
            for lease in leases.iter() {
                lease.close(reason);
            }
        }
    }
}

/// A response body that holds its lease while the transport writes it.
pub struct LeasedBody {
    bytes: Option<Bytes>,
    lease: Arc<SendLease>,
}

impl http_body::Body for LeasedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // A closed lease never yields its bytes again.
        if !self.lease.is_open() {
            self.bytes = None;
            return Poll::Ready(None);
        }
        Poll::Ready(self.bytes.take().map(|bytes| Ok(Frame::data(bytes))))
    }

    fn is_end_stream(&self) -> bool {
        self.bytes.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.bytes.as_ref().map_or(0, |bytes| bytes.len() as u64))
    }
}

/// A 200 JSON response whose body owns `lease` until the send completes.
pub fn leased_response(bytes: Vec<u8>, lease: Arc<SendLease>) -> Response {
    let body = LeasedBody {
        bytes: Some(Bytes::from(bytes)),
        lease,
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, PRIVATE_NO_STORE)
        .header(header::X_CONTENT_TYPE_OPTIONS, NO_SNIFF)
        .header(header::CONNECTION, "close")
        .body(Body::new(body))
        .unwrap_or_else(|_| {
            let mut fallback = Response::new(Body::empty());
            *fallback.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
            fallback
        })
}

/// Transport bounds of the socket server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeOptions {
    /// One connection (request and complete response send) at most this long.
    pub send_deadline: Duration,
    /// A bounded kernel send buffer, so completion follows the client.
    pub send_buffer_bytes: Option<u32>,
}

fn bound_send_buffer(
    stream: tokio::net::TcpStream,
    bytes: Option<u32>,
) -> std::io::Result<tokio::net::TcpStream> {
    let Some(bytes) = bytes else {
        return Ok(stream);
    };
    let stream = stream.into_std()?;
    // The duplicate descriptor shares the socket; dropping it keeps `stream`.
    tokio::net::TcpSocket::from_std_stream(stream.try_clone()?).set_send_buffer_size(bytes)?;
    tokio::net::TcpStream::from_std(stream)
}

/// Serves `router` on real TCP until `shutdown`; every connection carries
/// one response and closes its leases when the connection has finished.
pub async fn serve(
    listener: TcpListener,
    router: Router,
    options: ServeOptions,
    observer: Option<Arc<dyn SendObserver>>,
    shutdown: impl Future<Output = ()>,
) -> std::io::Result<()> {
    let mut connections: JoinSet<()> = JoinSet::new();
    let mut active: Vec<ConnectionLeases> = Vec::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            // Finished connections are reaped as they end, not at shutdown.
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    // Descriptor exhaustion must not spin the accept loop.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let Ok(stream) = bound_send_buffer(stream, options.send_buffer_bytes) else {
                    continue;
                };
                let leases = ConnectionLeases::new(observer.clone());
                active.retain(|known| Arc::strong_count(&known.0) > 1);
                active.push(leases.clone());
                let router = router.clone();
                connections.spawn(async move {
                    let per_request = leases.clone();
                    let service = hyper::service::service_fn(move |mut request: Request<Incoming>| {
                        request.extensions_mut().insert(per_request.clone());
                        let router = router.clone();
                        async move { router.oneshot(request.map(Body::new)).await }
                    });
                    let connection = http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(stream), service);
                    let reason = match tokio::time::timeout(options.send_deadline, connection).await {
                        Ok(Ok(())) => CloseReason::Completed,
                        Ok(Err(_)) => CloseReason::ConnectionError,
                        Err(_) => CloseReason::Deadline,
                    };
                    leases.close_all(reason);
                });
            }
        }
    }
    // Cancel: every in-flight response closes its lease, then the tasks end.
    for leases in &active {
        leases.close_all(CloseReason::Cancelled);
    }
    connections.shutdown().await;
    Ok(())
}
