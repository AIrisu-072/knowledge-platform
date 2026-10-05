//! 配送メトリクスの有限ラベルと、明示的に参照するトレース文脈。

use crate::{DeliveryDecision, DeliveryError, ErrorCode};
use std::{fmt, sync::Arc, time::Duration};
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMetricKind {
    Pending,
    InFlight,
    DeadLetter,
    OldestAge,
    Claimed,
    Acknowledged,
    Retried,
    Reaped,
    StaleFence,
    HandlerDuration,
    /// 生成側のコミット時刻は現行スキーマにないため、現在は未計測。
    CommitToAckLag,
    StoreError,
    ConsumerError,
}
impl DeliveryMetricKind {
    pub const ALL: [Self; 13] = [
        Self::Pending,
        Self::InFlight,
        Self::DeadLetter,
        Self::OldestAge,
        Self::Claimed,
        Self::Acknowledged,
        Self::Retried,
        Self::Reaped,
        Self::StaleFence,
        Self::HandlerDuration,
        Self::CommitToAckLag,
        Self::StoreError,
        Self::ConsumerError,
    ];
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryRoute {
    Generic,
    Search,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryOutcome {
    Observed,
    Applied,
    KnownNoop,
    Retryable,
    Terminal,
    Exhausted,
    LeaseExpired,
    Lost,
    Unknown,
    PolicyMismatch,
    LegacyExhausted,
    InvalidConfig,
}
impl DeliveryOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Applied => "applied",
            Self::KnownNoop => "known_noop",
            Self::Retryable => "retryable",
            Self::Terminal => "terminal",
            Self::Exhausted => "exhausted",
            Self::LeaseExpired => "lease_expired",
            Self::Lost => "lost",
            Self::Unknown => "unknown",
            Self::PolicyMismatch => "policy_mismatch",
            Self::LegacyExhausted => "legacy_exhausted",
            Self::InvalidConfig => "invalid_config",
        }
    }
}
#[derive(Clone, Debug)]
pub struct DeliveryMetric {
    kind: DeliveryMetricKind,
    value: f64,
    route: DeliveryRoute,
    code: Option<ErrorCode>,
    outcome: DeliveryOutcome,
}
impl DeliveryMetric {
    pub fn kind(&self) -> DeliveryMetricKind {
        self.kind
    }
    /// 件数、または秒。未観測値をゼロで補完しない。
    pub fn value(&self) -> f64 {
        self.value
    }
    pub fn labels(&self) -> Vec<(&'static str, &'static str)> {
        let mut labels = vec![
            (
                "route",
                match self.route {
                    DeliveryRoute::Generic => "generic",
                    DeliveryRoute::Search => "search",
                },
            ),
            ("outcome", self.outcome.as_str()),
        ];
        if let Some(code) = self.code {
            labels.push(("error_code", code.as_str()));
        }
        labels
    }
}
#[derive(Clone, Copy, Debug)]
pub struct QueueSnapshot {
    pub pending: u64,
    pub in_flight: u64,
    pub dead_letter: u64,
    pub expired: u64,
    pub exhausted: u64,
    pub oldest_age: Option<Duration>,
}

/// コールバックは短時間で完了し、ブロックや panic を起こさないこと。
/// 識別子は明示的なスパン関連付けだけに使い、メトリクスへ転用しない。
pub trait DeliveryObserver: Send + Sync {
    fn record(&self, metric: DeliveryMetric);
    fn start_span(&self, _span: &DeliverySpan) {}
    fn finish_span(&self, _span: &DeliverySpan, _outcome: DeliveryOutcome) {}
}

#[derive(Clone, Eq, PartialEq)]
pub struct ValidatedTrace {
    traceparent: String,
    tracestate: Option<String>,
}
impl ValidatedTrace {
    pub fn traceparent(&self) -> &str {
        &self.traceparent
    }
    pub fn tracestate(&self) -> Option<&str> {
        self.tracestate.as_deref()
    }
}
impl fmt::Debug for ValidatedTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidatedTrace { redacted }")
    }
}

/// W3C Trace Context §3.2–3.3。入力長はコピー前に上限を検査する。
/// 不正な state は parent を無効化しない: https://www.w3.org/TR/trace-context/#tracestate-header
pub fn validate_trace_context(parent: Option<&str>, state: Option<&str>) -> Option<ValidatedTrace> {
    let parent = parent?;
    let bytes = parent.as_bytes();
    let hex = |s: &[u8]| {
        s.iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
    };
    if !(55..=512).contains(&bytes.len())
        || !parent.is_ascii()
        || bytes[2] != b'-'
        || bytes[35] != b'-'
        || bytes[52] != b'-'
        || !hex(&bytes[..2])
        || &bytes[..2] == b"ff"
        || !hex(&bytes[3..35])
        || bytes[3..35].iter().all(|c| *c == b'0')
        || !hex(&bytes[36..52])
        || bytes[36..52].iter().all(|c| *c == b'0')
        || !hex(&bytes[53..55])
        || (&bytes[..2] == b"00" && bytes.len() != 55)
        || (bytes.len() > 55
            && (bytes[55] != b'-' || !bytes[56..].iter().all(|c| (0x21..=0x7e).contains(c))))
    {
        return None;
    }
    Some(ValidatedTrace {
        traceparent: parent.to_owned(),
        tracestate: state.filter(|s| valid_state(s)).map(str::to_owned),
    })
}
fn valid_state(state: &str) -> bool {
    if state.len() > 512 || !state.is_ascii() {
        return false;
    }
    let mut keys = Vec::new();
    for (index, member) in state.split(',').enumerate() {
        if index >= 32 {
            return false;
        }
        let member = member.trim_matches([' ', '\t']);
        if member.is_empty() {
            continue;
        }
        let Some((key, value)) = member.split_once('=') else {
            return false;
        };
        let key_char = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-*/".contains(&c);
        let valid_key = if let Some((tenant, system)) = key.split_once('@') {
            (1..=241).contains(&tenant.len())
                && tenant.as_bytes()[0].is_ascii_alphanumeric()
                && tenant.bytes().all(key_char)
                && (1..=14).contains(&system.len())
                && system.as_bytes()[0].is_ascii_lowercase()
                && system.bytes().all(key_char)
        } else {
            (1..=256).contains(&key.len())
                && key.as_bytes()[0].is_ascii_lowercase()
                && key.bytes().all(key_char)
        };
        if !valid_key
            || keys.contains(&key)
            || !(1..=256).contains(&value.len())
            || !value
                .bytes()
                .all(|c| (0x20..=0x7e).contains(&c) && c != b',' && c != b'=')
            || value.ends_with(' ')
        {
            return false;
        }
        keys.push(key);
    }
    true
}

#[derive(Clone)]
pub struct DeliverySpan {
    event_id: Uuid,
    attempt: i32,
    parent: Option<ValidatedTrace>,
    context: ValidatedTrace,
}
impl DeliverySpan {
    pub(crate) fn new(event_id: Uuid, attempt: i32, parent: Option<ValidatedTrace>) -> Self {
        let trace_id = Uuid::now_v7().simple().to_string();
        let span_id = Uuid::now_v7().simple().to_string();
        let (trace_id, flags) = parent.as_ref().map_or((trace_id.as_str(), "00"), |p| {
            (
                &p.traceparent[3..35],
                if u8::from_str_radix(&p.traceparent[53..55], 16).unwrap_or(0) & 1 == 1 {
                    "01"
                } else {
                    "00"
                },
            )
        });
        let context = ValidatedTrace {
            traceparent: format!("00-{trace_id}-{}-{flags}", &span_id[16..]),
            tracestate: parent.as_ref().and_then(|p| p.tracestate.clone()),
        };
        Self {
            event_id,
            attempt,
            parent,
            context,
        }
    }
    pub fn event_id(&self) -> Uuid {
        self.event_id
    }
    pub fn attempt(&self) -> i32 {
        self.attempt
    }
    pub fn parent(&self) -> Option<&ValidatedTrace> {
        self.parent.as_ref()
    }
    pub fn context(&self) -> &ValidatedTrace {
        &self.context
    }
}
impl fmt::Debug for DeliverySpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeliverySpan")
            .field("attempt", &self.attempt)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) struct Observation {
    pub observer: Option<Arc<dyn DeliveryObserver>>,
    pub route: DeliveryRoute,
}
impl Observation {
    pub fn record(
        &self,
        kind: DeliveryMetricKind,
        value: f64,
        outcome: DeliveryOutcome,
        code: Option<ErrorCode>,
    ) {
        if let Some(observer) = &self.observer {
            observer.record(DeliveryMetric {
                kind,
                value,
                route: self.route,
                code,
                outcome,
            });
        }
    }
    pub fn error(&self, error: &DeliveryError) {
        let outcome = match error {
            DeliveryError::PolicyMismatch => DeliveryOutcome::PolicyMismatch,
            DeliveryError::LegacyExhausted { .. } => DeliveryOutcome::LegacyExhausted,
            DeliveryError::InvalidConfig => DeliveryOutcome::InvalidConfig,
            DeliveryError::StoreUnknown => DeliveryOutcome::Unknown,
        };
        self.record(
            DeliveryMetricKind::StoreError,
            1.0,
            outcome,
            Some(ErrorCode::DeliveryUnknown),
        );
    }
    pub fn stale(&self) {
        self.record(
            DeliveryMetricKind::StaleFence,
            1.0,
            DeliveryOutcome::Lost,
            None,
        );
    }
    pub fn snapshot(&self, snapshot: QueueSnapshot) {
        for (kind, value, outcome) in [
            (
                DeliveryMetricKind::Pending,
                snapshot.pending,
                DeliveryOutcome::Observed,
            ),
            (
                DeliveryMetricKind::InFlight,
                snapshot.in_flight,
                DeliveryOutcome::Observed,
            ),
            (
                DeliveryMetricKind::DeadLetter,
                snapshot.dead_letter,
                DeliveryOutcome::Observed,
            ),
            (
                DeliveryMetricKind::Pending,
                snapshot.expired,
                DeliveryOutcome::LeaseExpired,
            ),
            (
                DeliveryMetricKind::Pending,
                snapshot.exhausted,
                DeliveryOutcome::Exhausted,
            ),
        ] {
            self.record(kind, value as f64, outcome, None);
        }
        if let Some(age) = snapshot.oldest_age {
            self.record(
                DeliveryMetricKind::OldestAge,
                age.as_secs_f64(),
                DeliveryOutcome::Observed,
                None,
            );
        }
    }
}

pub(crate) struct HandlerTiming {
    observation: Observation,
    started: Instant,
    outcome: DeliveryOutcome,
    code: Option<ErrorCode>,
}
impl HandlerTiming {
    pub fn new(observation: Observation) -> Self {
        Self {
            observation,
            started: Instant::now(),
            outcome: DeliveryOutcome::Unknown,
            code: Some(ErrorCode::DeliveryUnknown),
        }
    }
    pub fn complete(mut self, decision: DeliveryDecision) {
        (self.outcome, self.code) = match decision {
            DeliveryDecision::Applied => (DeliveryOutcome::Applied, None),
            DeliveryDecision::KnownNoop => (DeliveryOutcome::KnownNoop, None),
            DeliveryDecision::Retryable(code) => (DeliveryOutcome::Retryable, Some(code)),
            DeliveryDecision::Terminal(code) => (DeliveryOutcome::Terminal, Some(code)),
        };
    }
}
impl Drop for HandlerTiming {
    fn drop(&mut self) {
        self.observation.record(
            DeliveryMetricKind::HandlerDuration,
            self.started.elapsed().as_secs_f64(),
            self.outcome,
            self.code,
        );
        if self.code.is_some() {
            self.observation.record(
                DeliveryMetricKind::ConsumerError,
                1.0,
                self.outcome,
                self.code,
            );
        }
    }
}
pub(crate) struct SpanTiming {
    observation: Observation,
    pub span: DeliverySpan,
    pub outcome: DeliveryOutcome,
}
impl SpanTiming {
    pub fn new(observation: Observation, span: DeliverySpan) -> Self {
        if let Some(observer) = &observation.observer {
            observer.start_span(&span);
        }
        Self {
            observation,
            span,
            outcome: DeliveryOutcome::Unknown,
        }
    }
}
impl Drop for SpanTiming {
    fn drop(&mut self) {
        if let Some(observer) = &self.observation.observer {
            observer.finish_span(&self.span, self.outcome);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    #[test]
    fn valid_trace_is_accepted_and_debug_redacts_identifiers_and_state() {
        let trace = validate_trace_context(Some(PARENT), Some("vendor=state-sentinel")).unwrap();
        assert!(!format!("{trace:?}").contains("4bf92f"));
        assert!(!format!("{trace:?}").contains("state-sentinel"));
    }
    #[test]
    fn tracestate_grammar_and_bounds_preserve_valid_parent() {
        let excessive_members = (0..33)
            .map(|i| format!("k{i}=v"))
            .collect::<Vec<_>>()
            .join(",");
        let oversized_value = format!("vendor={}", "x".repeat(257));
        for state in [
            "a=one,a=two",
            "Upper=v",
            "1simple=v",
            "a=",
            "a=b=c",
            "a=bad\nvalue",
            "a=bad\tvalue",
            "tenant@123=v",
            excessive_members.as_str(),
            oversized_value.as_str(),
        ] {
            let parsed = validate_trace_context(Some(PARENT), Some(state)).unwrap();
            assert_eq!(parsed.traceparent(), PARENT);
            assert_eq!(parsed.tracestate(), None);
        }
        for state in [
            "",
            "vendor=opaque",
            "1tenant@system=opaque",
            "a=one, b=two",
            ",a=one,,",
        ] {
            assert_eq!(
                validate_trace_context(Some(PARENT), Some(state))
                    .unwrap()
                    .tracestate(),
                Some(state)
            );
        }
    }
    #[test]
    fn malformed_parents_are_rejected_without_panicking() {
        for parent in [
            "",
            "é",
            "00-short",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        ] {
            assert!(validate_trace_context(Some(parent), None).is_none());
        }
    }
}
