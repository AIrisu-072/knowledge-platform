//! G08: 実際に確定した配送結果だけを有限ラベルで観測する。

use outbox_delivery::{
    ClaimedEvent, DeliveryConfig, DeliveryDecision, DeliveryEnvelope, DeliveryError,
    DeliveryFuture, ErrorCode, FenceResult, HandlerFuture, OutboxStore,
    observe::{
        DeliveryMetric, DeliveryMetricKind as Kind, DeliveryObserver, DeliveryOutcome,
        DeliveryRoute, DeliverySpan, QueueSnapshot, ValidatedTrace, validate_trace_context,
    },
    runner::{DeliveryContext, DeliveryHandler, DeliveryRunner, NoopAdmission, NoopPermit},
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Default)]
struct Recorder(
    Mutex<Vec<DeliveryMetric>>,
    Mutex<Vec<DeliverySpan>>,
    Mutex<Vec<DeliveryOutcome>>,
);
impl DeliveryObserver for Recorder {
    fn record(&self, metric: DeliveryMetric) {
        self.0.lock().unwrap().push(metric);
    }
    fn start_span(&self, span: &DeliverySpan) {
        self.1.lock().unwrap().push(span.clone());
    }
    fn finish_span(&self, _: &DeliverySpan, outcome: DeliveryOutcome) {
        self.2.lock().unwrap().push(outcome);
    }
}

struct Store {
    event: Mutex<Option<ClaimedEvent>>,
    settle: FenceResult,
    unknown: bool,
    snapshot: bool,
    snapshot_pending: bool,
    snapshot_calls: std::sync::atomic::AtomicUsize,
    trace: Option<ValidatedTrace>,
    trace_pending: bool,
    renew_count: std::sync::atomic::AtomicUsize,
    lose_on_renew: Option<usize>,
}
impl Store {
    fn new(attempt: i32, settle: FenceResult, unknown: bool) -> Self {
        Self {
            event: Mutex::new(Some(ClaimedEvent {
                envelope: DeliveryEnvelope {
                    event_id: Uuid::from_u128(1),
                    event_type: "payload-sentinel".into(),
                    aggregate_type: "document-sentinel".into(),
                    aggregate_id: Uuid::from_u128(2),
                    payload: json!({"actor": "actor-sentinel"}),
                    occurred_at: OffsetDateTime::UNIX_EPOCH,
                },
                attempt,
                attempt_limit: 8,
                lease_token: Uuid::from_u128(3),
                lease_owner: Uuid::from_u128(4),
                lease_expires_at: OffsetDateTime::now_utc() + time::Duration::minutes(2),
            })),
            settle,
            unknown,
            snapshot: false,
            snapshot_pending: false,
            snapshot_calls: std::sync::atomic::AtomicUsize::new(0),
            trace: None,
            trace_pending: false,
            renew_count: std::sync::atomic::AtomicUsize::new(0),
            lose_on_renew: None,
        }
    }
}
impl OutboxStore for Store {
    fn trace_context(&self, _: Uuid, _: Uuid) -> DeliveryFuture<'_, Option<ValidatedTrace>> {
        Box::pin(async {
            if self.trace_pending {
                std::future::pending().await
            } else {
                Ok(self.trace.clone())
            }
        })
    }
    fn queue_snapshot(&self) -> DeliveryFuture<'_, Option<QueueSnapshot>> {
        Box::pin(async {
            self.snapshot_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.snapshot_pending {
                return std::future::pending().await;
            }
            Ok(self.snapshot.then_some(QueueSnapshot {
                pending: 7,
                in_flight: 3,
                dead_letter: 2,
                expired: 2,
                exhausted: 1,
                oldest_age: Some(Duration::from_secs(42)),
            }))
        })
    }
    fn verify_policy(&self) -> DeliveryFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn claim(&self, _: Uuid, _: u32, _: Duration) -> DeliveryFuture<'_, Vec<ClaimedEvent>> {
        Box::pin(async { Ok(self.event.lock().unwrap().take().into_iter().collect()) })
    }
    fn renew(&self, _: Uuid, _: Uuid, _: Duration) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async {
            let count = self
                .renew_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            Ok(if self.lose_on_renew == Some(count) {
                FenceResult::Lost
            } else {
                FenceResult::Updated
            })
        })
    }
    fn settle_success(&self, _: Uuid, _: Uuid) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(async {
            if self.unknown {
                Err(DeliveryError::StoreUnknown)
            } else {
                Ok(self.settle)
            }
        })
    }
    fn settle_failure(
        &self,
        id: Uuid,
        token: Uuid,
        _: ErrorCode,
        _: bool,
        _: Duration,
    ) -> DeliveryFuture<'_, FenceResult> {
        self.settle_success(id, token)
    }
    fn reap_exhausted(&self, _: u32) -> DeliveryFuture<'_, u64> {
        Box::pin(async { Ok(2) })
    }
}
struct Handler(DeliveryDecision);
impl DeliveryHandler<NoopPermit> for Handler {
    fn deliver(
        &self,
        _: DeliveryEnvelope,
        _: DeliveryContext,
        _: NoopPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async { self.0 })
    }
}
async fn run(
    decision: DeliveryDecision,
    attempt: i32,
    fence: FenceResult,
    unknown: bool,
) -> (
    Result<outbox_delivery::runner::CycleSummary, DeliveryError>,
    Vec<DeliveryMetric>,
) {
    let recorder = Arc::new(Recorder::default());
    let runner = DeliveryRunner::new(
        Arc::new(Store::new(attempt, fence, unknown)),
        Arc::new(Handler(decision)),
        Arc::new(NoopAdmission),
        DeliveryConfig::default(),
    )
    .unwrap()
    .with_observer(recorder.clone(), DeliveryRoute::Generic);
    let result = runner.run_cycle().await;
    let metrics = recorder.0.lock().unwrap().clone();
    (result, metrics)
}
fn sum(metrics: &[DeliveryMetric], kind: Kind) -> f64 {
    metrics
        .iter()
        .filter(|m| m.kind() == kind)
        .map(DeliveryMetric::value)
        .sum()
}
#[tokio::test]
async fn ack_and_reap_metrics_match_confirmed_results_without_payload_or_fake_lag() {
    let (result, metrics) = run(DeliveryDecision::Applied, 1, FenceResult::Updated, false).await;
    assert_eq!(result.unwrap().settled, 1);
    assert_eq!(sum(&metrics, Kind::Claimed), 1.0);
    assert_eq!(sum(&metrics, Kind::Acknowledged), 1.0);
    assert_eq!(sum(&metrics, Kind::Reaped), 2.0);
    assert!(metrics.iter().any(|m| m.kind() == Kind::HandlerDuration));
    assert!(!metrics.iter().any(|m| m.kind() == Kind::CommitToAckLag));
    assert!(!metrics.iter().any(|m| matches!(
        m.kind(),
        Kind::Pending | Kind::InFlight | Kind::DeadLetter | Kind::OldestAge
    )));
    for metric in metrics {
        assert!(!format!("{metric:?}").contains("sentinel"));
        assert!(metric.labels().iter().all(
            |(k, v)| ["route", "error_code", "outcome"].contains(k) && !v.contains("sentinel")
        ));
    }
}
#[tokio::test]
async fn unknown_or_stale_settlement_never_counts_ack_or_retry() {
    for (fence, unknown) in [(FenceResult::Updated, true), (FenceResult::Lost, false)] {
        let (result, metrics) = run(DeliveryDecision::Applied, 1, fence, unknown).await;
        assert_eq!(sum(&metrics, Kind::Acknowledged), 0.0);
        assert_eq!(sum(&metrics, Kind::Retried), 0.0);
        assert_eq!(
            sum(&metrics, Kind::StaleFence),
            if unknown { 0.0 } else { 1.0 }
        );
        assert_eq!(
            sum(&metrics, Kind::StoreError),
            if unknown { 1.0 } else { 0.0 }
        );
        assert_eq!(result.is_err(), unknown);
    }
}
#[tokio::test]
async fn retry_counter_excludes_terminal_and_attempt_limit() {
    for (decision, attempt, expected) in [
        (
            DeliveryDecision::Retryable(ErrorCode::IndexingFailed),
            1,
            1.0,
        ),
        (
            DeliveryDecision::Retryable(ErrorCode::IndexingFailed),
            8,
            0.0,
        ),
        (
            DeliveryDecision::Terminal(ErrorCode::InvalidEnvelope),
            1,
            0.0,
        ),
    ] {
        let (result, metrics) = run(decision, attempt, FenceResult::Updated, false).await;
        assert!(result.is_ok());
        assert_eq!(sum(&metrics, Kind::Retried), expected);
        assert_eq!(sum(&metrics, Kind::ConsumerError), 1.0);
        assert_eq!(sum(&metrics, Kind::Acknowledged), 0.0);
    }
}

#[tokio::test]
async fn queue_gauges_report_store_snapshot_values() {
    let recorder = Arc::new(Recorder::default());
    let mut store = Store::new(1, FenceResult::Updated, false);
    store.snapshot = true;
    let runner = DeliveryRunner::new(
        Arc::new(store),
        Arc::new(Handler(DeliveryDecision::Applied)),
        Arc::new(NoopAdmission),
        DeliveryConfig::default(),
    )
    .unwrap()
    .with_observer(recorder.clone(), DeliveryRoute::Generic);
    runner.run_cycle().await.unwrap();
    let metrics = recorder.0.lock().unwrap();
    for (kind, value) in [
        (Kind::Pending, 7.0),
        (Kind::InFlight, 3.0),
        (Kind::DeadLetter, 2.0),
        (Kind::OldestAge, 42.0),
    ] {
        assert!(
            metrics
                .iter()
                .any(|m| m.kind() == kind && m.value() == value)
        );
    }
}

struct TraceHandler(Mutex<Vec<Option<ValidatedTrace>>>);
impl DeliveryHandler<NoopPermit> for TraceHandler {
    fn deliver(
        &self,
        _: DeliveryEnvelope,
        context: DeliveryContext,
        _: NoopPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        self.0.lock().unwrap().push(context.trace);
        Box::pin(async { DeliveryDecision::Applied })
    }
}
#[tokio::test]
async fn trace_is_propagated_through_a_new_span_and_legacy_gets_a_root() {
    let parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    for saved in [
        None,
        validate_trace_context(Some(parent), Some("vendor=trace-sentinel")),
    ] {
        let recorder = Arc::new(Recorder::default());
        let handler = Arc::new(TraceHandler(Mutex::new(Vec::new())));
        let mut store = Store::new(1, FenceResult::Updated, false);
        store.trace = saved.clone();
        let runner = DeliveryRunner::new(
            Arc::new(store),
            handler.clone(),
            Arc::new(NoopAdmission),
            DeliveryConfig::default(),
        )
        .unwrap()
        .with_observer(recorder.clone(), DeliveryRoute::Generic);
        runner.run_cycle().await.unwrap();
        let spans = recorder.1.lock().unwrap();
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(span.event_id(), Uuid::from_u128(1));
        assert_eq!(span.parent(), saved.as_ref());
        assert_eq!(span.attempt(), 1);
        assert_eq!(handler.0.lock().unwrap()[0].as_ref(), Some(span.context()));
        assert!(
            validate_trace_context(
                Some(span.context().traceparent()),
                span.context().tracestate()
            )
            .is_some()
        );
        assert_ne!(&span.context().traceparent()[36..52], &parent[36..52]);
        if saved.is_some() {
            assert_eq!(&span.context().traceparent()[3..35], &parent[3..35]);
        } else {
            assert_ne!(&span.context().traceparent()[3..35], &parent[3..35]);
        }
        assert!(!format!("{span:?}").contains("trace-sentinel"));
        assert!(!format!("{span:?}").contains("4bf92f"));
        assert_eq!(*recorder.2.lock().unwrap(), [DeliveryOutcome::Applied]);
    }
}
struct PendingHandler;
impl DeliveryHandler<NoopPermit> for PendingHandler {
    fn deliver(
        &self,
        _: DeliveryEnvelope,
        _: DeliveryContext,
        _: NoopPermit,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(std::future::pending())
    }
}
#[tokio::test]
async fn cancelled_handler_has_duration_and_unknown_outcome_but_no_stale_fence() {
    let recorder = Arc::new(Recorder::default());
    let runner = DeliveryRunner::new(
        Arc::new(Store::new(1, FenceResult::Updated, false)),
        Arc::new(PendingHandler),
        Arc::new(NoopAdmission),
        DeliveryConfig {
            max_processing: Duration::from_millis(30),
            drain_timeout: Duration::from_millis(10),
            ..DeliveryConfig::default()
        },
    )
    .unwrap()
    .with_observer(recorder.clone(), DeliveryRoute::Generic);
    let _ = runner.run_cycle().await;
    let metrics = recorder.0.lock().unwrap();
    assert_eq!(sum(&metrics, Kind::Acknowledged), 0.0);
    assert_eq!(sum(&metrics, Kind::StaleFence), 0.0);
    assert_eq!(sum(&metrics, Kind::ConsumerError), 1.0);
    assert!(sum(&metrics, Kind::HandlerDuration) > 0.0);
    assert_eq!(*recorder.2.lock().unwrap(), [DeliveryOutcome::Unknown]);
}
#[tokio::test]
async fn pending_trace_read_is_bounded_and_disabled_observer_adds_no_io() {
    for observed in [false, true] {
        let recorder = Arc::new(Recorder::default());
        let handler = Arc::new(TraceHandler(Mutex::new(Vec::new())));
        let mut store = Store::new(1, FenceResult::Updated, false);
        store.trace_pending = true;
        let mut runner = DeliveryRunner::new(
            Arc::new(store),
            handler.clone(),
            Arc::new(NoopAdmission),
            DeliveryConfig {
                max_processing: Duration::from_millis(30),
                drain_timeout: Duration::from_millis(10),
                ..DeliveryConfig::default()
            },
        )
        .unwrap();
        if observed {
            runner = runner.with_observer(recorder, DeliveryRoute::Generic);
        }
        let result = tokio::time::timeout(Duration::from_secs(1), runner.run_cycle())
            .await
            .unwrap();
        assert_eq!(result.is_err(), observed);
        assert_eq!(handler.0.lock().unwrap().len(), usize::from(!observed));
    }
}

#[tokio::test]
async fn verified_lost_heartbeat_and_final_preflight_finish_span_as_lost() {
    async fn check<H: DeliveryHandler<NoopPermit> + 'static>(handler: H) {
        let recorder = Arc::new(Recorder::default());
        let mut store = Store::new(1, FenceResult::Updated, false);
        store.lose_on_renew = Some(2);
        let runner = DeliveryRunner::new(
            Arc::new(store),
            Arc::new(handler),
            Arc::new(NoopAdmission),
            DeliveryConfig {
                lease_duration: Duration::from_secs(1),
                renew_interval: Duration::from_millis(10),
                max_processing: Duration::from_millis(100),
                drain_timeout: Duration::from_millis(20),
                ..DeliveryConfig::default()
            },
        )
        .unwrap()
        .with_observer(recorder.clone(), DeliveryRoute::Generic);
        assert_eq!(runner.run_cycle().await.unwrap().lost, 1);
        assert_eq!(sum(&recorder.0.lock().unwrap(), Kind::StaleFence), 1.0);
        assert_eq!(*recorder.2.lock().unwrap(), [DeliveryOutcome::Lost]);
    }
    check(Handler(DeliveryDecision::Applied)).await;
    check(PendingHandler).await;
}

#[tokio::test]
async fn shutdown_interrupts_pending_snapshot_without_claim_or_fake_gauges() {
    let mut store = Store::new(1, FenceResult::Updated, false);
    store.snapshot_pending = true;
    let store = Arc::new(store);
    let recorder = Arc::new(Recorder::default());
    let runner = DeliveryRunner::new(
        store.clone(),
        Arc::new(Handler(DeliveryDecision::Applied)),
        Arc::new(NoopAdmission),
        DeliveryConfig {
            lease_duration: Duration::from_secs(1),
            renew_interval: Duration::from_millis(100),
            max_processing: Duration::from_secs(1),
            drain_timeout: Duration::from_millis(30),
            ..DeliveryConfig::default()
        },
    )
    .unwrap()
    .with_observer(recorder.clone(), DeliveryRoute::Generic);
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move { runner.run_until_shutdown(receiver).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while store
            .snapshot_calls
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    shutdown.send(true).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap(),
        Err(DeliveryError::StoreUnknown)
    );
    assert!(store.event.lock().unwrap().is_some());
    assert!(recorder.0.lock().unwrap().iter().all(|m| !matches!(
        m.kind(),
        Kind::Pending
            | Kind::InFlight
            | Kind::DeadLetter
            | Kind::OldestAge
            | Kind::Acknowledged
            | Kind::StaleFence
    )));
}
