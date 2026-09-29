use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;

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
