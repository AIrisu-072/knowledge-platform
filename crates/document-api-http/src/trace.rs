use std::sync::Arc;
use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpObservation {
    pub trace_id: String,
    pub route_template: String,
    pub method: String,
    pub status: u16,
    pub duration_micros: u64,
    pub invocation_kind: Option<&'static str>,
    pub error_code: Option<ErrorCode>,
}

pub trait HttpObservationSink: Send + Sync + 'static {
    fn record(&self, observation: HttpObservation);
}

pub struct TracingObservationSink;

impl HttpObservationSink for TracingObservationSink {
    fn record(&self, observation: HttpObservation) {
        tracing::info!(
            target: "document_api_http",
            trace_id = %observation.trace_id,
            route_template = %observation.route_template,
            method = %observation.method,
            status = observation.status,
            duration_micros = observation.duration_micros,
            invocation_kind = ?observation.invocation_kind,
            error_code = ?observation.error_code,
            "document HTTP request"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
}

impl TraceContext {
    pub fn from_traceparent(value: Option<&HeaderValue>) -> Self {
        let incoming = value
            .and_then(|header| header.to_str().ok())
            .and_then(parse_traceparent);
        Self {
            trace_id: incoming.unwrap_or_else(|| uuid::Uuid::now_v7().simple().to_string()),
        }
    }
}

fn parse_traceparent(value: &str) -> Option<String> {
    let mut segments = value.split('-');
    let (Some("00"), Some(trace_id), Some(span_id), Some(flags), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return None;
    };
    if trace_id.len() != 32
        || span_id.len() != 16
        || flags.len() != 2
        || trace_id.bytes().all(|byte| byte == b'0')
        || span_id.bytes().all(|byte| byte == b'0')
        || ![trace_id, span_id, flags]
            .iter()
            .all(|segment| segment.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(trace_id.to_ascii_lowercase())
}

pub async fn attach_trace(mut request: Request, next: Next) -> Response {
    let trace = TraceContext::from_traceparent(request.headers().get("traceparent"));
    request.extensions_mut().insert(trace.clone());
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&trace.trace_id) {
        response.headers_mut().insert("trace-id", value);
    }
    response
}

pub(crate) async fn observe_request(
    State(sink): State<Arc<dyn HttpObservationSink>>,
    request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let trace_id = request
        .extensions()
        .get::<TraceContext>()
        .map(|trace| trace.trace_id.clone())
        .unwrap_or_default();
    let route_template = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "<unmatched>".to_owned());
    let method = request.method().as_str().to_owned();
    let invocation_kind = request
        .extensions()
        .get::<document_application::VerifiedActorContext>()
        .map(|actor| actor.invocation_kind().as_str());
    let response = next.run(request).await;
    let elapsed = started.elapsed().as_micros();
    sink.record(HttpObservation {
        trace_id,
        route_template,
        method,
        status: response.status().as_u16(),
        duration_micros: u64::try_from(elapsed).unwrap_or(u64::MAX),
        invocation_kind,
        error_code: response.extensions().get::<ErrorCode>().copied(),
    });
    response
}
