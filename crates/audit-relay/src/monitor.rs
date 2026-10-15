//! What a running `audit-relay run` shows outside its process (design §12):
//! the circuit-breaker state it reports to the Document database for
//! `health`, and bounded progress lines on stderr.
//!
//! Both carry fixed codes and counts only: the circuit state, the gate code,
//! the outage counts and the number of handler outcomes per kind. Never a
//! payload, subject, actor, resource, event id, event type or reason.
//!
//! - Report: each tick samples the breaker; on a change, and at least every
//!   heartbeat, `audit_relay.report_runtime` upserts this process's row
//!   (keyed by a random instance id that appears nowhere else); a clean stop
//!   deletes it. `audit_relay.status()` counts a process as running while it
//!   reported within the last 60 seconds. Reporting is best effort: a failed
//!   report is retried on the next tick and never stops delivery.
//! - Progress: an `event=circuit` line when the circuit state or the gate
//!   changes (sampled once per tick; the breaker re-evaluates the gate at
//!   most once per cooldown), an `event=progress` line at most once per
//!   progress interval and only when something was handled, and an
//!   `event=final` line on stop when counts remain. An idle relay prints
//!   nothing after its first gate result. Counts are handler outcomes since
//!   the previous line: `delivered` (stored), `duplicate`, `held` (relay-side
//!   holds), `outage` (Store outages) and `quarantined`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior};
use uuid::Uuid;

use crate::breaker::{Breaker, BreakerSnapshot, Gate, Mode};

/// Monitor timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorConfig {
    /// Sampling period of the breaker.
    pub tick: Duration,
    /// Minimum spacing of `event=progress` lines.
    pub progress_interval: Duration,
    /// Maximum spacing of runtime reports while nothing changes (well below
    /// the 60 seconds after which `health` no longer counts the process).
    pub heartbeat: Duration,
    /// Bound on one runtime report.
    pub report_timeout: Duration,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(1),
            progress_interval: Duration::from_secs(10),
            heartbeat: Duration::from_secs(10),
            report_timeout: Duration::from_secs(2),
        }
    }
}

/// One handler outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The Store stored the event.
    Delivered,
    /// The Store already had it (`duplicate*`).
    Duplicate,
    /// Held on the relay side (catalog skew, projection, source).
    Held,
    /// Held as a Store outage.
    Outage,
    /// Quarantined (Store or relay verdict).
    Quarantined,
}

/// Handler outcome counts since the previous line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProgressCounts {
    pub delivered: u64,
    pub duplicate: u64,
    pub held: u64,
    pub outage: u64,
    pub quarantined: u64,
}

impl ProgressCounts {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn add(&mut self, other: Self) {
        self.delivered += other.delivered;
        self.duplicate += other.duplicate;
        self.held += other.held;
        self.outage += other.outage;
        self.quarantined += other.quarantined;
    }
}

/// Shared outcome counters (handler → monitor).
#[derive(Debug, Default)]
pub struct Progress {
    delivered: AtomicU64,
    duplicate: AtomicU64,
    held: AtomicU64,
    outage: AtomicU64,
    quarantined: AtomicU64,
}

impl Progress {
    pub fn record(&self, outcome: Outcome) {
        let counter = match outcome {
            Outcome::Delivered => &self.delivered,
            Outcome::Duplicate => &self.duplicate,
            Outcome::Held => &self.held,
            Outcome::Outage => &self.outage,
            Outcome::Quarantined => &self.quarantined,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// The counts since the previous call.
    pub fn take(&self) -> ProgressCounts {
        ProgressCounts {
            delivered: self.delivered.swap(0, Ordering::Relaxed),
            duplicate: self.duplicate.swap(0, Ordering::Relaxed),
            held: self.held.swap(0, Ordering::Relaxed),
            outage: self.outage.swap(0, Ordering::Relaxed),
            quarantined: self.quarantined.swap(0, Ordering::Relaxed),
        }
    }
}

/// Decides which progress lines to print (pure: the caller samples the
/// breaker and the counters and supplies the time).
#[derive(Debug, Clone)]
pub struct ProgressLog {
    interval: Duration,
    state: (Mode, Gate),
    last_line: Option<Instant>,
    pending: ProgressCounts,
}

impl ProgressLog {
    /// Starts from the breaker's current state (not printed).
    pub fn new(interval: Duration, initial: BreakerSnapshot) -> Self {
        Self {
            interval,
            state: (initial.mode, initial.gate),
            last_line: None,
            pending: ProgressCounts::default(),
        }
    }

    /// One sample: the line to print, if any.
    pub fn observe(
        &mut self,
        now: Instant,
        snapshot: BreakerSnapshot,
        counts: ProgressCounts,
    ) -> Option<String> {
        self.pending.add(counts);
        let state = (snapshot.mode, snapshot.gate);
        if state != self.state {
            let was = self.state;
            self.state = state;
            return Some(self.line(now, "circuit", snapshot, Some(was)));
        }
        let due = self
            .last_line
            .is_none_or(|last| now.duration_since(last) >= self.interval);
        (!self.pending.is_empty() && due).then(|| self.line(now, "progress", snapshot, None))
    }

    /// The last line on stop, when counts remain.
    pub fn finish(
        &mut self,
        now: Instant,
        snapshot: BreakerSnapshot,
        counts: ProgressCounts,
    ) -> Option<String> {
        self.pending.add(counts);
        (!self.pending.is_empty()).then(|| self.line(now, "final", snapshot, None))
    }

    fn line(
        &mut self,
        now: Instant,
        event: &str,
        snapshot: BreakerSnapshot,
        was: Option<(Mode, Gate)>,
    ) -> String {
        let counts = std::mem::take(&mut self.pending);
        self.last_line = Some(now);
        let was = was.map_or(String::new(), |(mode, gate)| {
            format!(" was_circuit={} was_gate={}", mode.as_str(), gate.as_str())
        });
        format!(
            "audit-relay: event={event} circuit={} gate={}{was} outage_streak={} delivered={} \
             duplicate={} held={} outage={} quarantined={}",
            snapshot.mode.as_str(),
            snapshot.gate.as_str(),
            snapshot.outage_streak,
            counts.delivered,
            counts.duplicate,
            counts.held,
            counts.outage,
            counts.quarantined,
        )
    }
}

/// Reports this process's breaker to `audit_relay.report_runtime` (worker
/// login).
#[derive(Debug, Clone)]
pub struct RuntimeReporter {
    source: PgPool,
    instance: Uuid,
}

impl RuntimeReporter {
    pub fn new(source: PgPool) -> Self {
        Self {
            source,
            instance: Uuid::now_v7(),
        }
    }

    async fn call(&self, snapshot: &BreakerSnapshot, stopped: bool) -> Result<(), sqlx::Error> {
        let count = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        sqlx::query("SELECT audit_relay.report_runtime($1, $2, $3, $4, $5, $6)")
            .bind(self.instance)
            .bind(snapshot.mode.as_str())
            .bind(snapshot.gate.as_str())
            .bind(count(snapshot.outage_streak))
            .bind(count(snapshot.outages).max(count(snapshot.outage_streak)))
            .bind(stopped)
            .execute(&self.source)
            .await
            .map(|_| ())
    }

    /// Upserts this process's row.
    pub async fn report(&self, snapshot: &BreakerSnapshot) -> Result<(), sqlx::Error> {
        self.call(snapshot, false).await
    }

    /// Deletes this process's row (clean stop).
    pub async fn stop(&self, snapshot: &BreakerSnapshot) -> Result<(), sqlx::Error> {
        self.call(snapshot, true).await
    }
}

/// Where progress lines go (stderr in production).
pub type LineSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Samples the breaker and the outcome counters of a running relay.
pub struct Monitor {
    breaker: Arc<Breaker>,
    progress: Arc<Progress>,
    reporter: Option<RuntimeReporter>,
    config: MonitorConfig,
    sink: LineSink,
}

impl Monitor {
    pub fn new(breaker: Arc<Breaker>, progress: Arc<Progress>, config: MonitorConfig) -> Self {
        Self {
            breaker,
            progress,
            reporter: None,
            config,
            sink: Arc::new(|line: &str| eprintln!("{line}")),
        }
    }

    pub fn with_reporter(mut self, reporter: RuntimeReporter) -> Self {
        self.reporter = Some(reporter);
        self
    }

    pub fn with_sink(mut self, sink: LineSink) -> Self {
        self.sink = sink;
        self
    }

    async fn report(&self, snapshot: &BreakerSnapshot, stopped: bool) -> bool {
        let Some(reporter) = &self.reporter else {
            return true;
        };
        matches!(
            tokio::time::timeout(self.config.report_timeout, reporter.call(snapshot, stopped))
                .await,
            Ok(Ok(()))
        )
    }

    /// Runs until `stop` turns true (or its sender is dropped), then prints
    /// the remaining counts and removes this process's report.
    pub async fn run(self, mut stop: watch::Receiver<bool>) {
        let mut log = ProgressLog::new(self.config.progress_interval, self.breaker.snapshot());
        let mut reported: Option<(BreakerSnapshot, Instant)> = None;
        let mut ticks = tokio::time::interval(self.config.tick.max(Duration::from_millis(10)));
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        break;
                    }
                }
                _ = ticks.tick() => {
                    let now = Instant::now();
                    let snapshot = self.breaker.snapshot();
                    if let Some(line) = log.observe(now, snapshot, self.progress.take()) {
                        (self.sink)(&line);
                    }
                    let due = reported.is_none_or(|(last, at)| {
                        last != snapshot || now.duration_since(at) >= self.config.heartbeat
                    });
                    if due && self.report(&snapshot, false).await {
                        reported = Some((snapshot, now));
                    }
                }
            }
        }
        let snapshot = self.breaker.snapshot();
        if let Some(line) = log.finish(Instant::now(), snapshot, self.progress.take()) {
            (self.sink)(&line);
        }
        self.report(&snapshot, true).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_core::OutageCode;

    fn snapshot(mode: Mode, gate: Gate) -> BreakerSnapshot {
        BreakerSnapshot {
            mode,
            gate,
            outage_streak: 0,
            outages: 0,
        }
    }

    #[test]
    fn progress_counts_are_taken_once() {
        let progress = Progress::default();
        for outcome in [
            Outcome::Delivered,
            Outcome::Delivered,
            Outcome::Duplicate,
            Outcome::Held,
            Outcome::Outage,
            Outcome::Quarantined,
        ] {
            progress.record(outcome);
        }
        assert_eq!(
            progress.take(),
            ProgressCounts {
                delivered: 2,
                duplicate: 1,
                held: 1,
                outage: 1,
                quarantined: 1,
            }
        );
        assert!(progress.take().is_empty());
    }

    #[test]
    fn lines_are_rate_limited_and_an_idle_relay_is_quiet() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let interval = Duration::from_secs(10);
        let mut log = ProgressLog::new(interval, snapshot(Mode::HalfOpen, Gate::Unknown));
        let idle = ProgressCounts::default();
        // The first gate result is a transition.
        let line = log
            .observe(at(0), snapshot(Mode::HalfOpen, Gate::Ok), idle)
            .expect("transition");
        assert_eq!(
            line,
            "audit-relay: event=circuit circuit=half_open gate=ok was_circuit=half_open \
             was_gate=unknown outage_streak=0 delivered=0 duplicate=0 held=0 outage=0 \
             quarantined=0"
        );
        // Idle: nothing, however long.
        for ms in [1_000, 20_000, 600_000] {
            assert_eq!(
                log.observe(at(ms), snapshot(Mode::HalfOpen, Gate::Ok), idle),
                None
            );
        }
        // Work is reported at most once per interval, with the counts since
        // the previous line.
        let one = ProgressCounts {
            delivered: 1,
            ..ProgressCounts::default()
        };
        let line = log
            .observe(at(601_000), snapshot(Mode::Closed, Gate::Ok), one)
            .expect("transition to closed");
        assert!(line.contains("event=circuit circuit=closed") && line.contains("delivered=1"));
        assert_eq!(
            log.observe(at(602_000), snapshot(Mode::Closed, Gate::Ok), one),
            None
        );
        assert_eq!(
            log.observe(at(605_000), snapshot(Mode::Closed, Gate::Ok), one),
            None
        );
        let line = log
            .observe(at(611_000), snapshot(Mode::Closed, Gate::Ok), one)
            .expect("interval elapsed");
        assert!(line.starts_with("audit-relay: event=progress circuit=closed gate=ok"));
        assert!(
            line.ends_with("delivered=3 duplicate=0 held=0 outage=0 quarantined=0"),
            "{line}"
        );
        // An outage changes the gate: printed at once.
        let mut open = snapshot(Mode::Open, Gate::Outage(OutageCode::Connection));
        open.outage_streak = 2;
        let line = log
            .observe(
                at(611_500),
                open,
                ProgressCounts {
                    outage: 1,
                    ..ProgressCounts::default()
                },
            )
            .expect("transition");
        assert!(
            line.contains("event=circuit circuit=open gate=store_connection was_circuit=closed")
                && line.contains("outage_streak=2")
                && line.contains("outage=1"),
            "{line}"
        );
        assert_eq!(log.finish(at(612_000), open, idle), None, "nothing left");
        let line = log
            .finish(at(612_000), open, one)
            .expect("remaining counts");
        assert!(line.starts_with("audit-relay: event=final circuit=open"));
    }

    #[test]
    fn lines_use_fixed_keys_and_codes_only() {
        let mut log = ProgressLog::new(Duration::ZERO, snapshot(Mode::Closed, Gate::Ok));
        let line = log
            .observe(
                Instant::now(),
                snapshot(Mode::Closed, Gate::Ok),
                ProgressCounts {
                    delivered: 4,
                    duplicate: 3,
                    held: 2,
                    outage: 1,
                    quarantined: 5,
                },
            )
            .expect("line");
        let fields: Vec<&str> = line
            .strip_prefix("audit-relay: ")
            .expect("prefix")
            .split(' ')
            .collect();
        for field in &fields {
            let (key, value) = field.split_once('=').expect("key=value");
            assert!(
                [
                    "event",
                    "circuit",
                    "gate",
                    "was_circuit",
                    "was_gate",
                    "outage_streak",
                    "delivered",
                    "duplicate",
                    "held",
                    "outage",
                    "quarantined"
                ]
                .contains(&key),
                "{key}"
            );
            assert!(audit_core::kinds::is_code(value), "{value}");
        }
    }
}
