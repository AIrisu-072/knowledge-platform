//! Only pre-redacted application observations and safe runtime categories are enabled.
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt};

pub fn subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(|metadata| {
            matches!(metadata.target(), "document_api_http" | "document_server")
                && *metadata.level() <= tracing::Level::INFO
        }))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(writer),
        )
}

pub fn install() -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    tracing::subscriber::set_global_default(subscriber(std::io::stderr))
}

pub(crate) fn readiness_unavailable(category: crate::health::ReadinessFailure) {
    tracing::warn!(target:"document_server",error_category=?category,"readiness unavailable");
}

#[cfg(test)]
mod tests {
    use document_api_http::trace::{HttpObservation, HttpObservationSink, TracingObservationSink};
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };
    #[derive(Clone)]
    struct Writer(Arc<Mutex<Vec<u8>>>);
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn subscriber_records_correlated_safe_events_and_excludes_other_targets() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let output = buffer.clone();
        let subscriber = super::subscriber(move || Writer(output.clone()));
        tracing::subscriber::with_default(subscriber, || {
            TracingObservationSink.record(HttpObservation {
                trace_id: "11111111111111111111111111111111".into(),
                route_template: "/v1/documents/{document_id}".into(),
                method: "GET".into(),
                status: 200,
                duration_micros: 5,
                invocation_kind: Some("agent"),
                error_code: None,
            });
            super::readiness_unavailable(crate::health::ReadinessFailure::Storage);
            tracing::error!(target:"sqlx",database_url="synthetic-secret",path="/synthetic/private","must not log");
        });
        let logged = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert!(logged.contains("11111111111111111111111111111111"));
        assert!(logged.contains("/v1/documents/{document_id}"));
        assert!(logged.contains("Storage"));
        assert!(!logged.contains("synthetic-secret"));
        assert!(!logged.contains("/synthetic/private"));
    }
}
