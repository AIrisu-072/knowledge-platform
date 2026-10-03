//! P6-G07: a real child process can die after a committed claim.

use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};
use uuid::Uuid;

#[cfg(target_os = "linux")]
mod owned_runtime {
    use super::*;
    use g07_owned::{
        OwnedDatabase, OwnedPgScope, append_child_record, bounded, bytes_sha256, child_pool,
        observe_process, observed_monotonic_ns, private_directory, read_private, write_new_private,
    };
    use outbox_delivery::{DeliveryPolicy, OutboxStore, postgres::PostgresOutboxStore};
    use sqlx::PgPool;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        os::unix::process::ExitStatusExt,
        process::{Child, Command, ExitStatus, Stdio},
        sync::mpsc::{self, Receiver},
        thread,
        time::Instant,
    };

    #[derive(Debug)]
    pub(super) enum CaseError {
        Fixture(FixtureError),
        Assertion(&'static str),
        Unwired,
    }

    impl From<FixtureError> for CaseError {
        fn from(error: FixtureError) -> Self {
            Self::Fixture(error)
        }
    }

    fn require(condition: bool, cause: &'static str) -> Result<(), CaseError> {
        if condition {
            Ok(())
        } else {
            Err(CaseError::Assertion(cause))
        }
    }

    #[derive(Clone, Copy)]
    struct CaseClock {
        start: Instant,
        deadline: Deadline,
    }

    impl CaseClock {
        fn new(allowance: Duration) -> Self {
            Self {
                start: Instant::now(),
                deadline: Deadline(allowance),
            }
        }
        fn elapsed(self) -> Duration {
            self.start.elapsed()
        }
        fn phase(self, allowance: Duration) -> Deadline {
            Deadline::phase(self.deadline, self.elapsed(), allowance)
        }
        fn remaining(self, phase: Deadline, local: Duration) -> Result<Duration, CaseError> {
            phase
                .remaining(self.elapsed(), local)
                .ok_or(CaseError::Fixture(FixtureError::Timeout("owned_phase")))
        }
        fn end(self, phase: Deadline) -> Result<Instant, FixtureError> {
            self.start
                .checked_add(phase.0)
                .ok_or(FixtureError::Timeout("clock_overflow"))
        }
    }

    struct DrainResult {
        captured: CapturedStream,
        read_error: bool,
    }

    fn drain_stream(mut stream: impl Read + Send + 'static) -> Receiver<DrainResult> {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut captured = CapturedStream::default();
            let mut bytes = [0_u8; 4096];
            let mut read_error = false;
            loop {
                match stream.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => captured.retain(&bytes[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        read_error = true;
                        break;
                    }
                }
            }
            let _ = sender.send(DrainResult {
                captured,
                read_error,
            });
        });
        receiver
    }

    struct ChildGuard {
        handle: Option<Child>,
        config: ChildConfiguration,
        identity: Option<ProcessIdentity>,
        parent: ProcessIdentity,
        scope: OwnedPgScope,
        clock: CaseClock,
        life: Deadline,
        reap_deadline: Option<Deadline>,
        registered: bool,
        status: Option<ExitStatus>,
        kill_failed: bool,
        cleanup_unknown: bool,
        stdout: Option<Receiver<DrainResult>>,
        stderr: Option<Receiver<DrainResult>>,
        pending_stdout: Option<DrainResult>,
        pending_stderr: Option<DrainResult>,
        output: Option<(DrainResult, DrainResult)>,
        output_persisted: bool,
        details: Vec<Value>,
    }

    impl ChildGuard {
        fn registry_record(&self, event: &str) -> Result<Value, FixtureError> {
            let identity = self
                .identity
                .as_ref()
                .ok_or(FixtureError::Unknown("child_spawn_identity"))?;
            Ok(
                json!({"schema_version":1,"run_id":self.scope.run_id().to_string(),
            "test_launch_id":self.config.test_launch_id.to_string(),"event":event,
            "pid":identity.pid,"start_ticks":identity.start_ticks,"uid":identity.uid,
            "executable":identity.executable.to_str().ok_or(FixtureError::IdentityMismatch)?,
            "executable_sha256":identity.executable_sha256,"executable_dev":identity.executable_dev,
            "executable_inode":identity.executable_inode,"mode":self.config.mode,
            "owner":self.config.owner.to_string(),"handshake":self.config.handshake.to_string(),"limit":self.config.limit,
            "parent_pid":self.parent.pid,"parent_start_ticks":self.parent.start_ticks,
            "observed_monotonic_ns":observed_monotonic_ns()?}),
            )
        }

        fn register(&mut self) -> Result<(), CaseError> {
            let pid = self
                .handle
                .as_ref()
                .ok_or(FixtureError::Unknown("spawn_handle"))?
                .id();
            let identity = observe_process(pid, &self.parent.executable)?;
            require(
                identity.executable_sha256 == self.parent.executable_sha256
                    && identity.executable_dev == self.parent.executable_dev
                    && identity.executable_inode == self.parent.executable_inode,
                "self_spawn_executable_changed",
            )?;
            self.identity = Some(identity);
            // This is the first subsequent operation after capturing spawn identity.
            append_child_record(&self.scope.root(), &self.registry_record("spawn")?)?;
            self.registered = true;
            let child = self
                .handle
                .as_mut()
                .ok_or(FixtureError::Unknown("spawn_handle"))?;
            self.stdout = Some(drain_stream(
                child
                    .stdout
                    .take()
                    .ok_or(FixtureError::Unknown("stdout_pipe"))?,
            ));
            self.stderr = Some(drain_stream(
                child
                    .stderr
                    .take()
                    .ok_or(FixtureError::Unknown("stderr_pipe"))?,
            ));
            Ok(())
        }

        fn observe_exit(&mut self) -> Result<Option<ExitStatus>, CaseError> {
            if let Some(status) = self.status {
                return Ok(Some(status));
            }
            let Some(child) = self.handle.as_mut() else {
                return Err(FixtureError::Unknown("lost_child_handle").into());
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.status = Some(status);
                    self.handle.take(); // Only after the owned handle has positively reaped.
                    let mut record = match self.registry_record("exit") {
                        Ok(record) => record,
                        Err(_) => {
                            self.cleanup_unknown = true;
                            return Err(FixtureError::Unknown("exit_registry_identity").into());
                        }
                    };
                    record["exit_code"] = json!(status.code());
                    record["exit_signal"] = json!(status.signal());
                    record["reaped_by_parent"] = json!(true);
                    record["status"] = json!(if self.kill_failed {
                        "unknown"
                    } else {
                        "confirmed"
                    });
                    if let Err(error) = append_child_record(&self.scope.root(), &record) {
                        self.cleanup_unknown = true;
                        return Err(error.into());
                    }
                    Ok(Some(status))
                }
                Ok(None) => Ok(None),
                Err(error) => {
                    self.cleanup_unknown = true;
                    self.details
                        .push(json!({"event":"try_wait_error","error":error.to_string()}));
                    Err(FixtureError::Unknown("child_try_wait").into())
                }
            }
        }

        fn collect_output(&mut self, deadline: Deadline) -> Result<(), CaseError> {
            require(self.status.is_some(), "output_before_confirmed_exit")?;
            if self.output.is_some() {
                return require(self.output_persisted, "owned_output_not_persisted");
            }
            let phase = Deadline::phase(deadline, self.clock.elapsed(), Duration::from_secs(2));
            if self.pending_stdout.is_none() {
                self.pending_stdout = Some(
                    self.stdout
                        .as_ref()
                        .ok_or(FixtureError::Unknown("stdout_drainer"))?
                        .recv_timeout(self.clock.remaining(phase, Duration::from_secs(2))?)
                        .map_err(|_| FixtureError::Unknown("stdout_completion"))?,
                );
            }
            if self.pending_stderr.is_none() {
                self.pending_stderr = Some(
                    self.stderr
                        .as_ref()
                        .ok_or(FixtureError::Unknown("stderr_drainer"))?
                        .recv_timeout(self.clock.remaining(phase, Duration::from_secs(2))?)
                        .map_err(|_| FixtureError::Unknown("stderr_completion"))?,
                );
            }
            self.stdout.take();
            self.stderr.take();
            let stdout = self
                .pending_stdout
                .take()
                .ok_or(FixtureError::Unknown("stdout_capture"))?;
            let stderr = self
                .pending_stderr
                .take()
                .ok_or(FixtureError::Unknown("stderr_capture"))?;
            self.output = Some((stdout, stderr));
            let (stdout, stderr) = self
                .output
                .as_ref()
                .ok_or(FixtureError::Unknown("owned_output"))?;
            let identity = self
                .identity
                .as_ref()
                .ok_or(FixtureError::Unknown("child_identity"))?;
            let stem = format!(
                "child-{}-{}-{}",
                self.config.test_launch_id, identity.pid, identity.start_ticks
            );
            let stdout_path = self
                .scope
                .root()
                .join(format!("receipts/{stem}.stdout.log"));
            let stderr_path = self
                .scope
                .root()
                .join(format!("receipts/{stem}.stderr.log"));
            write_new_private(&stdout_path, &stdout.captured.bytes)?;
            write_new_private(&stderr_path, &stderr.captured.bytes)?;
            self.output_persisted = true;
            self.details.push(json!({"event":"output","stdout_path":stdout_path,"stderr_path":stderr_path,
            "stdout_sha256":bytes_sha256(&stdout.captured.bytes),"stderr_sha256":bytes_sha256(&stderr.captured.bytes),
            "stdout_bytes":stdout.captured.bytes.len(),"stderr_bytes":stderr.captured.bytes.len(),
            "stdout_overflow":stdout.captured.overflow,"stderr_overflow":stderr.captured.overflow,
            "stdout_read_error":stdout.read_error,"stderr_read_error":stderr.read_error}));
            if stdout.read_error || stderr.read_error {
                return Err(FixtureError::Unknown("child_output_read").into());
            }
            require(
                !stdout.captured.overflow && !stderr.captured.overflow,
                "child_output_overflow",
            )
        }

        fn has_unwired_marker(&self) -> bool {
            let Some((stdout, _)) = &self.output else {
                return false;
            };
            let markers = String::from_utf8_lossy(&stdout.captured.bytes)
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("P6_G07_UNWIRED=")
                        .and_then(|value| serde_json::from_str::<Value>(value).ok())
                })
                .collect::<Vec<_>>();
            markers.len() == 1
                && markers[0]
                    == json!({"test_launch_id":self.config.test_launch_id.to_string(),
            "mode":self.config.mode,"owner":self.config.owner.to_string(),"handshake":self.config.handshake.to_string(),
            "cause":"G07 child process path not wired"})
        }

        fn check_alive(&mut self) -> Result<(), CaseError> {
            if let Some(status) = self.observe_exit()? {
                self.collect_output(self.clock.deadline)?;
                if status.code() == Some(124) {
                    return Err(FixtureError::Timeout("child_watchdog").into());
                }
                if status.code() == Some(101)
                    && status.signal().is_none()
                    && self.has_unwired_marker()
                {
                    return Err(CaseError::Unwired);
                }
                return Err(CaseError::Assertion("claim_child_exited_unexpectedly"));
            }
            self.clock.remaining(self.life, Duration::from_millis(20))?;
            Ok(())
        }

        fn kill_and_reap(&mut self, cohort: Deadline, deliberate: bool) -> Result<(), CaseError> {
            self.clock.remaining(cohort, Duration::from_secs(3))?;
            if self.status.is_none() {
                if self.observe_exit()?.is_none() {
                    self.reap_deadline = Some(Deadline::phase(
                        cohort,
                        self.clock.elapsed(),
                        Duration::from_secs(3),
                    ));
                    let handle = OwnedChildHandle(u64::from(
                        self.handle
                            .as_ref()
                            .ok_or(FixtureError::Unknown("kill_handle"))?
                            .id(),
                    ));
                    let result = finish_owned_child(self, handle);
                    self.details
                        .push(json!({"event":"kill_reap","deliberate":deliberate,
                    "result":format!("{result:?}"),"kill_failed":self.kill_failed,
                    "observed_monotonic_ns":observed_monotonic_ns()?}));
                    if result != ChildCleanup::Reaped {
                        return Err(FixtureError::Unknown("child_kill_reap").into());
                    }
                } else if deliberate {
                    return Err(CaseError::Assertion("claim_exited_before_deliberate_kill"));
                }
            } else if deliberate {
                return Err(CaseError::Assertion("claim_already_exited_before_kill"));
            }
            self.collect_output(cohort)?;
            if deliberate {
                require(
                    self.status.is_some_and(|status| status.signal() == Some(9)),
                    "deliberate_kill_did_not_observe_sigkill",
                )?;
            }
            Ok(())
        }

        fn wait_reaper(&mut self) -> Result<u64, CaseError> {
            loop {
                if let Some(status) = self.observe_exit()? {
                    self.collect_output(self.clock.deadline)?;
                    if status.code() == Some(124) {
                        return Err(FixtureError::Timeout("child_watchdog").into());
                    }
                    require(status.success(), "reaper_exit_failure")?;
                    let stdout = String::from_utf8_lossy(
                        &self
                            .output
                            .as_ref()
                            .ok_or(FixtureError::Unknown("reaper_output"))?
                            .0
                            .captured
                            .bytes,
                    );
                    let markers = stdout
                        .lines()
                        .filter_map(|line| {
                            line.strip_prefix("P6_G07_REAP_RESULT=")
                                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                        })
                        .collect::<Vec<_>>();
                    require(markers.len() == 1, "reaper_result_missing_or_duplicate")?;
                    let result = &markers[0];
                    require(
                        result["test_launch_id"] == json!(self.config.test_launch_id.to_string())
                            && result["owner"] == json!(self.config.owner.to_string())
                            && result["handshake"] == json!(self.config.handshake.to_string())
                            && result["mode"] == json!("reap"),
                        "reaper_result_identity",
                    )?;
                    return result["count"]
                        .as_u64()
                        .filter(|count| *count <= 1)
                        .ok_or(CaseError::Assertion("reaper_result_count"));
                }
                let remaining = self.clock.remaining(self.life, Duration::from_millis(10))?;
                thread::sleep(remaining);
            }
        }

        fn confirmed_reaped(&self) -> bool {
            self.registered
                && self.identity.is_some()
                && self.status.is_some()
                && self.handle.is_none()
                && !self.kill_failed
                && !self.cleanup_unknown
        }

        fn receipt(&self) -> Value {
            let identity = self.identity.as_ref();
            let output_complete = self.output_persisted
                && self
                    .output
                    .as_ref()
                    .is_some_and(|(out, err)| !out.read_error && !err.read_error);
            json!({"pid":identity.map(|identity| identity.pid),"start_ticks":identity.map(|identity| identity.start_ticks),
            "uid":identity.map(|identity| identity.uid),"executable":identity.and_then(|identity| identity.executable.to_str()),
            "executable_sha256":identity.map(|identity| identity.executable_sha256.as_str()),
            "executable_dev":identity.map(|identity| identity.executable_dev),"executable_inode":identity.map(|identity| identity.executable_inode),
            "mode":self.config.mode,"owner":self.config.owner.to_string(),"handshake":self.config.handshake.to_string(),
            "registered":self.registered,"reaped_by_parent":self.status.is_some() && self.handle.is_none(),
            "status":if self.confirmed_reaped() { "confirmed" } else { "unknown" },
            "exit_code":self.status.and_then(|status| status.code()),"exit_signal":self.status.and_then(|status| status.signal()),
            "output_complete":output_complete,
            "stdout_overflow":self.output.as_ref().is_some_and(|(out, _)| out.captured.overflow),
            "stderr_overflow":self.output.as_ref().is_some_and(|(_, err)| err.captured.overflow)})
        }
    }

    impl OwnedChildControl for ChildGuard {
        fn kill(&mut self, handle: OwnedChildHandle) -> KillObservation {
            let Some(child) = self.handle.as_mut() else {
                return KillObservation::Unknown;
            };
            if u64::from(child.id()) != handle.0 || !self.registered || self.cleanup_unknown {
                return KillObservation::Unknown;
            }
            match child.kill() {
                Ok(()) => KillObservation::Sent,
                Err(error) => {
                    self.kill_failed = true;
                    self.details
                        .push(json!({"event":"kill_error","error":error.to_string()}));
                    KillObservation::Failed
                }
            }
        }
        fn try_reap(&mut self, handle: OwnedChildHandle) -> ReapObservation {
            if self
                .identity
                .as_ref()
                .is_none_or(|identity| u64::from(identity.pid) != handle.0)
            {
                return ReapObservation::Unknown;
            }
            let Some(deadline) = self.reap_deadline else {
                return ReapObservation::Unknown;
            };
            loop {
                match self.observe_exit() {
                    Ok(Some(_)) => return ReapObservation::Reaped,
                    Err(_) => return ReapObservation::Unknown,
                    Ok(None) => match self.clock.remaining(deadline, Duration::from_millis(10)) {
                        Ok(remaining) => thread::sleep(remaining),
                        Err(_) => return ReapObservation::Running,
                    },
                }
            }
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.cleanup_unknown || !self.registered {
                return;
            }
            if self
                .clock
                .deadline
                .remaining(self.clock.elapsed(), Duration::from_millis(1))
                .is_none()
            {
                return;
            }
            let Some(child) = self.handle.as_mut() else {
                return;
            };
            // Last-resort cleanup of this stored owned handle only. No blocking wait,
            // no inferred success, and no unbounded drainer join or registry scan.
            let until = Instant::now() + Duration::from_millis(100);
            if child.kill().is_err() {
                return;
            }
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        self.handle.take();
                        break;
                    }
                    Ok(None) if Instant::now() < until => thread::sleep(Duration::from_millis(5)),
                    _ => break,
                }
            }
        }
    }

    struct OwnedCase {
        case: &'static str,
        launch: Uuid,
        clock: CaseClock,
        parent: ProcessIdentity,
        database: OwnedDatabase,
        children: Vec<ChildGuard>,
        max_simultaneous: usize,
        observations: Vec<Value>,
        forced_expiry: Vec<&'static str>,
    }

    impl OwnedCase {
        fn new(case: &'static str) -> Result<Self, CaseError> {
            require(
                matches!(
                    case,
                    "last_claim_kill9_restart_reaps_unknown_once"
                        | "four_processes_disjoint_claims_and_recover_expired"
                ),
                "unadmitted_g07_case",
            )?;
            let clock = CaseClock::new(Duration::from_secs(90));
            let scope = OwnedPgScope::from_environment()?;
            validate_runtime_environment(&scope, false)?;
            let launch = environment_uuid("P6_G07_TEST_LAUNCH_ID")?;
            let executable = std::env::current_exe().map_err(|_| FixtureError::IdentityMismatch)?;
            executable.to_str().ok_or(FixtureError::IdentityMismatch)?;
            let parent = observe_process(std::process::id(), &executable)?;
            Ok(Self {
                case,
                launch,
                clock,
                parent,
                database: OwnedDatabase::new(scope),
                children: Vec::new(),
                max_simultaneous: 0,
                observations: Vec::new(),
                forced_expiry: Vec::new(),
            })
        }
        fn pool(&self) -> Result<&PgPool, CaseError> {
            self.database
                .pool
                .as_ref()
                .ok_or(FixtureError::Unknown("owned_pool").into())
        }
        fn spawn(
            &mut self,
            mode: &'static str,
            owner: Uuid,
            limit: u32,
            barrier: Option<&str>,
        ) -> Result<usize, CaseError> {
            require(
                !owner.is_nil()
                    && match self.case {
                        "last_claim_kill9_restart_reaps_unknown_once" => {
                            limit == 1
                                && ((mode == "claim" && owner == Uuid::from_u128(500))
                                    || (mode == "reap" && matches!(owner.as_u128(), 600 | 601)))
                        }
                        "four_processes_disjoint_claims_and_recover_expired" => {
                            mode == "claim"
                                && limit == 2
                                && matches!(owner.as_u128(), 1_000..=1_003 | 2_000)
                        }
                        _ => false,
                    },
                "child_case_mode_owner_limit",
            )?;
            let (total, simultaneous) = if self.case.starts_with("last_") {
                (3, 2)
            } else {
                (5, 4)
            };
            let active = self
                .children
                .iter()
                .filter(|child| child.handle.is_some())
                .count();
            require(
                self.children.len() < total && active < simultaneous,
                "child_inventory_bound",
            )?;
            self.clock
                .remaining(self.clock.deadline, G07_CHILD_LIFETIME)?;
            let identity = self
                .database
                .identity
                .as_ref()
                .ok_or(FixtureError::Unknown("child_database_identity"))?;
            let scope = self.database.scope.clone();
            let config = ChildConfiguration {
                scope_path: scope.path(),
                database: identity.name.clone(),
                database_oid: identity.oid,
                database_owner: identity.owner.clone(),
                mode,
                owner,
                limit,
                handshake: Uuid::now_v7(),
                test_launch_id: self.launch,
                barrier: barrier.map(str::to_owned),
                home: scope.root().join("home"),
                tmp: scope.root().join("tmp"),
            };
            let environment = child_environment(&config, &[]);
            require(environment.clear_inherited, "child_env_clear_required")?;
            let life = Deadline::phase(
                self.clock.deadline,
                self.clock.elapsed(),
                G07_CHILD_LIFETIME,
            );
            let mut command = Command::new(&self.parent.executable);
            command
                .args([
                    "--exact",
                    "child_worker_fixture",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env_clear()
                .envs(environment.entries)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let child = command
                .spawn()
                .map_err(|_| FixtureError::Unknown("child_spawn"))?;
            // Retain the handle even if immediate identity/registry observation fails.
            let index = self.children.len();
            self.children.push(ChildGuard {
                handle: Some(child),
                config,
                identity: None,
                parent: self.parent.clone(),
                scope,
                clock: self.clock,
                life,
                reap_deadline: None,
                registered: false,
                status: None,
                kill_failed: false,
                cleanup_unknown: false,
                stdout: None,
                stderr: None,
                pending_stdout: None,
                pending_stderr: None,
                output: None,
                output_persisted: false,
                details: Vec::new(),
            });
            self.children[index].register()?;
            self.max_simultaneous = self.max_simultaneous.max(active + 1);
            Ok(index)
        }
        fn expected_receipt(&self) -> Option<ExpectedReceipt> {
            Some(ExpectedReceipt {
                run_id: self.database.scope.run_id(),
                test_launch_id: self.launch,
                case: self.case,
                database: self.database.identity.clone()?,
                children: self
                    .children
                    .iter()
                    .map(|child| {
                        Some(ExpectedChildReceipt {
                            identity: child.identity.clone()?,
                            mode: child.config.mode,
                            owner: child.config.owner,
                            handshake: child.config.handshake,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            })
        }
    }

    async fn insert_event(
        case: &OwnedCase,
        id: Uuid,
        attempts: i32,
        limit: Option<i32>,
    ) -> Result<(), CaseError> {
        let result = bounded(
            case.clock.end(case.clock.deadline)?,
            Duration::from_secs(5),
            "insert_synthetic_event",
            sqlx::query(
                "INSERT INTO outbox_events \
         (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at, \
          attempt_count,attempt_limit) \
         VALUES ($1,'DocumentRegistered','Document',$2,$3, \
                 '2000-01-01T00:00:00Z','2000-01-01T00:00:00Z',$4,$5)",
            )
            .bind(id)
            .bind(Uuid::from_u128(50))
            .bind(json!({"preserve": [1, null], "event": id.to_string()}))
            .bind(attempts)
            .bind(limit)
            .execute(case.pool()?),
        )
        .await?;
        require(result.rows_affected() == 1, "synthetic_insert_count")
    }

    async fn wait_for_claimed(
        case: &mut OwnedCase,
        workers: &[usize],
        owners: &[Uuid],
        count: i64,
    ) -> Result<(), CaseError> {
        let phase = case.clock.phase(Duration::from_secs(15));
        let pool = case.pool()?.clone();
        loop {
            for index in workers {
                case.children
                    .get_mut(*index)
                    .ok_or(FixtureError::Unknown("owned_child_index"))?
                    .check_alive()?;
            }
            let (active, observed_at): (i64, String) = bounded(case.clock.end(phase)?, Duration::from_secs(5), "observe_committed_claims", sqlx::query_as(
                "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
                 SELECT (SELECT count(*) FROM outbox_events CROSS JOIN tick \
                 WHERE lease_owner = ANY($1) AND lease_token IS NOT NULL \
                   AND lease_expires_at > tick.t AND delivered_at IS NULL AND dead_lettered_at IS NULL), \
                 tick.t::text FROM tick")
            .bind(owners)
            .fetch_one(&pool)).await?;
            for index in workers {
                case.children[*index].check_alive()?;
            }
            if active == count {
                case.observations.push(json!({"event":"committed_live_claims","owners":owners,"count":active,"database_clock":observed_at,
                "observed_monotonic_ns":observed_monotonic_ns()?}));
                return Ok(());
            }
            tokio::time::sleep(case.clock.remaining(phase, Duration::from_millis(20))?).await;
        }
    }

    async fn row(case: &mut OwnedCase, id: Uuid, phase: &str) -> Result<Value, CaseError> {
        let result: Value = bounded(case.clock.end(case.clock.deadline)?, Duration::from_secs(5), "observe_event_row", sqlx::query_scalar(
        "SELECT jsonb_build_object('row',to_jsonb(o),'database_clock',clock_timestamp()::text) FROM outbox_events AS o WHERE event_id=$1")
        .bind(id)
        .fetch_one(case.pool()?)).await?;
        case.observations.push(json!({"event":phase,"observation":result,"observed_monotonic_ns":observed_monotonic_ns()?}));
        Ok(result["row"].clone())
    }

    fn store(pool: &PgPool) -> PostgresOutboxStore {
        PostgresOutboxStore::new(pool.clone(), DeliveryPolicy::default())
    }

    fn environment_uuid(name: &str) -> Result<Uuid, FixtureError> {
        let text = std::env::var(name).map_err(|_| FixtureError::ConfigurationRejected)?;
        let id = Uuid::parse_str(&text).map_err(|_| FixtureError::ConfigurationRejected)?;
        if id.is_nil() || text != id.to_string() {
            return Err(FixtureError::ConfigurationRejected);
        }
        Ok(id)
    }

    fn validate_runtime_environment(scope: &OwnedPgScope, child: bool) -> Result<(), FixtureError> {
        let base = [
            "HOME",
            "TMPDIR",
            "LC_ALL",
            "TZ",
            "RUST_BACKTRACE",
            "P6_G07_SCOPE_PATH",
            "P6_G07_TEST_LAUNCH_ID",
        ];
        let extra = [
            "P6_G07_DATABASE_NAME",
            "P6_G07_DATABASE_OID",
            "P6_G07_DATABASE_OWNER",
            "P6_G07_MODE",
            "P6_G07_OWNER",
            "P6_G07_LIMIT",
            "P6_G07_CHILD_HANDSHAKE",
            "P6_G07_CHILD_BARRIER",
        ];
        for (name, _) in std::env::vars_os() {
            let name = name.to_str().ok_or(FixtureError::ConfigurationRejected)?;
            if !base.contains(&name) && !(child && extra.contains(&name)) {
                return Err(FixtureError::ConfigurationRejected);
            }
        }
        for (name, expected) in [
            ("HOME", scope.root().join("home").into_os_string()),
            ("TMPDIR", scope.root().join("tmp").into_os_string()),
            ("P6_G07_SCOPE_PATH", scope.path().into_os_string()),
            ("LC_ALL", OsString::from("C")),
            ("TZ", OsString::from("UTC")),
            ("RUST_BACKTRACE", OsString::from("0")),
        ] {
            if std::env::var_os(name) != Some(expected) {
                return Err(FixtureError::ConfigurationRejected);
            }
        }
        private_directory(&scope.root().join("home"))?;
        private_directory(&scope.root().join("tmp"))?;
        environment_uuid("P6_G07_TEST_LAUNCH_ID")?;
        Ok(())
    }

    fn child_configuration(scope: &OwnedPgScope) -> Result<ChildConfiguration, FixtureError> {
        validate_runtime_environment(scope, true)?;
        let text = |name| std::env::var(name).map_err(|_| FixtureError::ConfigurationRejected);
        let mode = match text("P6_G07_MODE")?.as_str() {
            "claim" => "claim",
            "reap" => "reap",
            _ => return Err(FixtureError::ConfigurationRejected),
        };
        let limit = text("P6_G07_LIMIT")?
            .parse::<u32>()
            .map_err(|_| FixtureError::ConfigurationRejected)?;
        let database = text("P6_G07_DATABASE_NAME")?;
        let database_oid = text("P6_G07_DATABASE_OID")?
            .parse::<u32>()
            .map_err(|_| FixtureError::ConfigurationRejected)?;
        let database_owner = text("P6_G07_DATABASE_OWNER")?;
        if !matches!(limit, 1 | 2) || (mode == "reap" && limit != 1) {
            return Err(FixtureError::ConfigurationRejected);
        }
        let identity = DatabaseIdentity {
            cluster_system_identifier: scope.system_identifier().to_owned(),
            name: database.clone(),
            oid: database_oid,
            owner: database_owner.clone(),
        };
        checked_drop_statement(&identity, &identity)?;
        scope.options(&database)?;
        let barrier = std::env::var_os("P6_G07_CHILD_BARRIER")
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| FixtureError::ConfigurationRejected)
            })
            .transpose()?;
        if let Some(barrier) = &barrier {
            let address: std::net::SocketAddr = barrier
                .parse()
                .map_err(|_| FixtureError::ConfigurationRejected)?;
            if address.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                || address.port() == 0
                || address.to_string() != *barrier
            {
                return Err(FixtureError::ConfigurationRejected);
            }
        }
        Ok(ChildConfiguration {
            scope_path: scope.path(),
            database,
            database_oid,
            database_owner,
            mode,
            owner: environment_uuid("P6_G07_OWNER")?,
            limit,
            handshake: environment_uuid("P6_G07_CHILD_HANDSHAKE")?,
            test_launch_id: environment_uuid("P6_G07_TEST_LAUNCH_ID")?,
            barrier,
            home: scope.root().join("home"),
            tmp: scope.root().join("tmp"),
        })
    }

    fn await_parent_registration(
        scope: &OwnedPgScope,
        config: &ChildConfiguration,
        clock: CaseClock,
    ) -> Result<(), CaseError> {
        let phase = clock.phase(Duration::from_secs(3));
        let executable = std::env::current_exe().map_err(|_| FixtureError::IdentityMismatch)?;
        let expected_executable = executable.to_str().ok_or(FixtureError::IdentityMismatch)?;
        let identity = observe_process(std::process::id(), &executable)?;
        loop {
            let bytes = read_private(&scope.root().join("receipts/children.jsonl"), 64 * 1024)?;
            if !bytes.is_empty() && bytes.last() == Some(&b'\n') {
                let document = String::from_utf8(bytes)
                    .map_err(|_| FixtureError::Unknown("child_registry_encoding"))?;
                let mut matching = Vec::new();
                for line in document.lines() {
                    let record: Value = serde_json::from_str(line)
                        .map_err(|_| FixtureError::Unknown("child_registry_parse"))?;
                    let base = [
                        "schema_version",
                        "run_id",
                        "test_launch_id",
                        "event",
                        "pid",
                        "start_ticks",
                        "uid",
                        "executable",
                        "executable_sha256",
                        "executable_dev",
                        "executable_inode",
                        "mode",
                        "owner",
                        "handshake",
                        "limit",
                        "parent_pid",
                        "parent_start_ticks",
                        "observed_monotonic_ns",
                    ];
                    let exit = ["exit_code", "exit_signal", "reaped_by_parent", "status"];
                    let object = record
                        .as_object()
                        .ok_or(FixtureError::Unknown("child_registry_object"))?;
                    let exiting = record["event"] == json!("exit");
                    if record["schema_version"] != json!(1)
                        || !matches!(record["event"].as_str(), Some("spawn" | "exit"))
                        || object.len() != base.len() + if exiting { exit.len() } else { 0 }
                        || object.keys().any(|key| {
                            !base.contains(&key.as_str())
                                && !(exiting && exit.contains(&key.as_str()))
                        })
                        || record["run_id"] != json!(scope.run_id().to_string())
                        || record["uid"] != json!(1000)
                        || [
                            "pid",
                            "start_ticks",
                            "executable_dev",
                            "executable_inode",
                            "parent_pid",
                            "parent_start_ticks",
                            "observed_monotonic_ns",
                        ]
                        .iter()
                        .any(|field| record[*field].as_u64().is_none_or(|number| number == 0))
                        || !matches!(record["mode"].as_str(), Some("claim" | "reap"))
                        || !matches!(record["limit"].as_u64(), Some(1 | 2))
                        || ["test_launch_id", "owner", "handshake"]
                            .iter()
                            .any(|field| {
                                record[*field]
                                    .as_str()
                                    .and_then(|value| Uuid::parse_str(value).ok())
                                    .is_none_or(|id| id.is_nil())
                            })
                        || record["executable"]
                            .as_str()
                            .is_none_or(|path| !std::path::Path::new(path).is_absolute())
                        || record["executable_sha256"].as_str().is_none_or(|hash| {
                            hash.len() != 64
                                || !hash.bytes().all(|byte| {
                                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                                })
                        })
                    {
                        return Err(FixtureError::Unknown("child_registry_schema").into());
                    }
                    if record["event"] == json!("spawn")
                        && record["pid"] == json!(identity.pid)
                        && record["test_launch_id"] == json!(config.test_launch_id.to_string())
                    {
                        matching.push(record);
                    }
                }
                if !matching.is_empty() {
                    require(matching.len() == 1, "duplicate_child_registration")?;
                    let record = &matching[0];
                    require(
                        record["run_id"] == json!(scope.run_id().to_string())
                            && record["start_ticks"] == json!(identity.start_ticks)
                            && record["uid"] == json!(identity.uid)
                            && record["executable"].as_str() == Some(expected_executable)
                            && record["executable_sha256"] == json!(identity.executable_sha256)
                            && record["executable_dev"] == json!(identity.executable_dev)
                            && record["executable_inode"] == json!(identity.executable_inode)
                            && record["mode"] == json!(config.mode)
                            && record["owner"] == json!(config.owner.to_string())
                            && record["handshake"] == json!(config.handshake.to_string())
                            && record["limit"] == json!(config.limit),
                        "parent_spawn_registration_identity",
                    )?;
                    return Ok(());
                }
            }
            thread::sleep(clock.remaining(phase, Duration::from_millis(10))?);
        }
    }

    fn child_barrier(config: &ChildConfiguration, clock: CaseClock) -> Result<(), CaseError> {
        let Some(address) = &config.barrier else {
            return Ok(());
        };
        let phase = clock.phase(G07_BARRIER_PHASE);
        let address = address
            .parse::<std::net::SocketAddr>()
            .map_err(|_| FixtureError::ConfigurationRejected)?;
        let mut gate =
            TcpStream::connect_timeout(&address, clock.remaining(phase, Duration::from_secs(2))?)
                .map_err(|_| FixtureError::Unknown("child_barrier_connect"))?;
        barrier_write(
            &mut gate,
            format!("{}\n", config.handshake).as_bytes(),
            clock,
            phase,
        )?;
        let mut ack = [0_u8];
        barrier_read(&mut gate, &mut ack, clock, phase)?;
        require(ack == [1], "child_barrier_ack")
    }

    fn barrier_read(
        stream: &mut TcpStream,
        bytes: &mut [u8],
        clock: CaseClock,
        cohort: Deadline,
    ) -> Result<(), CaseError> {
        let phase = Deadline::phase(cohort, clock.elapsed(), Duration::from_secs(2));
        let mut offset = 0;
        while offset < bytes.len() {
            stream
                .set_read_timeout(Some(clock.remaining(phase, Duration::from_secs(2))?))
                .map_err(|_| FixtureError::Unknown("barrier_read_timeout"))?;
            match stream.read(&mut bytes[offset..]) {
                Ok(0) => return Err(FixtureError::Unknown("barrier_read_eof").into()),
                Ok(count) => offset += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(FixtureError::Timeout("barrier_read").into());
                }
                Err(_) => return Err(FixtureError::Unknown("barrier_read").into()),
            }
        }
        Ok(())
    }

    fn barrier_write(
        stream: &mut TcpStream,
        bytes: &[u8],
        clock: CaseClock,
        cohort: Deadline,
    ) -> Result<(), CaseError> {
        let phase = Deadline::phase(cohort, clock.elapsed(), Duration::from_secs(2));
        let mut offset = 0;
        while offset < bytes.len() {
            stream
                .set_write_timeout(Some(clock.remaining(phase, Duration::from_secs(2))?))
                .map_err(|_| FixtureError::Unknown("barrier_write_timeout"))?;
            match stream.write(&bytes[offset..]) {
                Ok(0) => return Err(FixtureError::Unknown("barrier_write_zero").into()),
                Ok(count) => offset += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(FixtureError::Timeout("barrier_write").into());
                }
                Err(_) => return Err(FixtureError::Unknown("barrier_write").into()),
            }
        }
        Ok(())
    }

    // The independently armed watchdog precedes any configuration/barrier/DB work.
    // Only this approved test executable is spawned; it spawns no descendants.
    pub(super) async fn child_worker_entry() {
        let clock = CaseClock::new(G07_CHILD_LIFETIME);
        thread::spawn(|| {
            thread::sleep(G07_CHILD_LIFETIME);
            eprintln!("G07 independent child watchdog expired");
            std::process::exit(124);
        });
        let scope =
            OwnedPgScope::from_environment().expect("G07 child scope must be explicitly owned");
        let config = child_configuration(&scope).expect("G07 child configuration must fail closed");
        await_parent_registration(&scope, &config, clock)
            .expect("G07 child registration must be independently observed");
        let identity = DatabaseIdentity {
            cluster_system_identifier: scope.system_identifier().to_owned(),
            name: config.database.clone(),
            oid: config.database_oid,
            owner: config.database_owner.clone(),
        };
        let pool = child_pool(
            &scope,
            &identity,
            clock.end(clock.deadline).expect("child deadline"),
        )
        .await
        .expect("G07 child owned DB identity");
        child_barrier(&config, clock).expect("G07 child bounded barrier");
        // Deliberately no policy/claim/reap body until the chronological real G07 RED.
        let remaining = clock
            .remaining(clock.deadline, Duration::from_secs(3))
            .expect("child close budget");
        tokio::time::timeout(remaining, pool.close())
            .await
            .expect("G07 unwired child pool close");
        print!(
            "{}",
            format_child_marker(
                "P6_G07_UNWIRED",
                &json!({"test_launch_id":config.test_launch_id.to_string(), "mode":config.mode,
        "owner":config.owner.to_string(),"handshake":config.handshake.to_string(),"cause":"G07 child process path not wired"})
            )
        );
        std::io::stdout().flush().expect("G07 child marker flush");
        panic!("G07 child process path not wired");
    }

    fn release_barrier(
        listener: &TcpListener,
        case: &mut OwnedCase,
        workers: &[usize],
        phase: Deadline,
    ) -> Result<(), CaseError> {
        listener
            .set_nonblocking(true)
            .map_err(|_| FixtureError::Unknown("barrier_nonblocking"))?;
        let expected = workers
            .iter()
            .map(|index| case.children[*index].config.handshake.to_string())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut gates = Vec::new();
        for _ in workers {
            let (mut gate, peer) = loop {
                for index in workers {
                    case.children[*index].check_alive()?;
                }
                match listener.accept() {
                    Ok(accepted) => break accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(case.clock.remaining(phase, Duration::from_millis(10))?)
                    }
                    Err(_) => return Err(FixtureError::Unknown("parent_barrier_accept").into()),
                }
            };
            require(
                peer.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                "unexpected_barrier_peer",
            )?;
            let mut ready = [0_u8; 37];
            barrier_read(&mut gate, &mut ready, case.clock, phase)?;
            let token = std::str::from_utf8(&ready[..36])
                .map_err(|_| FixtureError::Unknown("barrier_token_encoding"))?;
            require(
                ready[36] == b'\n' && expected.contains(token) && seen.insert(token.to_owned()),
                "unexpected_or_duplicate_barrier_handshake",
            )?;
            gates.push(gate);
        }
        require(seen == expected, "barrier_inventory")?;
        for mut gate in gates {
            barrier_write(&mut gate, &[1], case.clock, phase)?;
        }
        Ok(())
    }

    async fn last_claim_case(case: &mut OwnedCase) -> Result<(), CaseError> {
        let id = Uuid::from_u128(1);
        insert_event(case, id, 7, Some(8)).await?;
        let original = row(case, id, "original_event").await?;
        let owner = Uuid::from_u128(500);
        let worker = case.spawn("claim", owner, 1, None)?;
        wait_for_claimed(case, &[worker], &[owner], 1).await?;
        let claimed = row(case, id, "confirmed_final_claim").await?;
        require(
            claimed["attempt_count"] == json!(8) && claimed["attempt_limit"] == json!(8),
            "final_claim_attempt_limit",
        )?;
        for key in [
            "event_id",
            "event_type",
            "aggregate_type",
            "aggregate_id",
            "payload",
            "occurred_at",
            "available_at",
        ] {
            require(
                claimed[key] == original[key],
                "claim_changed_original_event",
            )?;
        }
        case.children[worker].check_alive()?;
        case.children[worker].kill_and_reap(case.clock.phase(G07_COHORT_REAP), true)?;
        case.forced_expiry.push("FORCED_BY_TEST_SQL");
        let expiry: Vec<Value> = bounded(case.clock.end(case.clock.deadline)?, Duration::from_secs(5), "force_final_claim_expiry", sqlx::query_scalar(
        "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE event_id=$1 AND lease_owner=$2 \
         RETURNING jsonb_build_object('event_id',event_id,'lease_owner',lease_owner,'lease_expires_at',lease_expires_at,'database_clock',clock_timestamp()::text)")
        .bind(id).bind(owner).fetch_all(case.pool()?)).await?;
        require(expiry.len() == 1, "forced_final_expiry_count")?;
        case.observations.push(json!({"event":"forced_expiry","label":"FORCED_BY_TEST_SQL","affected_rows":expiry.len(),"rows":expiry}));
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| FixtureError::Unknown("parent_barrier_bind"))?;
        let address = listener
            .local_addr()
            .map_err(|_| FixtureError::Unknown("parent_barrier_address"))?
            .to_string();
        let barrier_phase = case.clock.phase(G07_BARRIER_PHASE);
        let mut reapers = Vec::new();
        for n in 0_u128..2 {
            reapers.push(case.spawn("reap", Uuid::from_u128(600 + n), 1, Some(&address))?);
        }
        case.observations.push(json!({"event":"reaper_barrier","endpoint":address,"handshakes":reapers.iter().map(|index| case.children[*index].config.handshake.to_string()).collect::<Vec<_>>()}));
        release_barrier(&listener, case, &reapers, barrier_phase)?;
        let mut counts = Vec::new();
        for reaper in reapers {
            counts.push(case.children[reaper].wait_reaper()?);
        }
        counts.sort_unstable();
        require(counts == [0, 1], "two_reapers_exactly_one_terminal_update")?;
        case.observations
            .push(json!({"event":"reaper_counts","counts":counts}));
        let after = row(case, id, "terminal_reaped_event").await?;
        for key in [
            "event_id",
            "event_type",
            "aggregate_type",
            "aggregate_id",
            "payload",
            "occurred_at",
            "available_at",
            "attempt_count",
            "attempt_limit",
            "last_attempt_at",
        ] {
            require(
                after[key] == claimed[key],
                "reaper_changed_preserved_column",
            )?;
        }
        require(
            after["payload"] == original["payload"]
                && after["last_error_code"] == json!("delivery_unknown_at_limit")
                && !after["dead_lettered_at"].is_null()
                && after["delivered_at"].is_null()
                && after["lease_token"].is_null()
                && after["lease_owner"].is_null()
                && after["lease_expires_at"].is_null(),
            "terminal_unknown_state",
        )?;
        let parent_store = store(case.pool()?);
        let final_reap = bounded(
            case.clock.end(case.clock.deadline)?,
            Duration::from_secs(5),
            "final_parent_reap",
            parent_store.reap_exhausted(1),
        )
        .await?;
        let final_claim = bounded(
            case.clock.end(case.clock.deadline)?,
            Duration::from_secs(5),
            "final_parent_claim",
            parent_store.claim(Uuid::from_u128(700), 1, Duration::from_secs(1)),
        )
        .await?;
        require(
            final_reap == 0 && final_claim.is_empty(),
            "terminal_event_must_not_be_claimed_or_reaped_again",
        )?;
        case.observations.push(json!({"event":"final_parent_checks","reap_count":final_reap,"claim_count":final_claim.len()}));
        Ok(())
    }

    async fn claim_snapshot(
        case: &mut OwnedCase,
        ids: &[Uuid],
        phase: &str,
    ) -> Result<Vec<Value>, CaseError> {
        let snapshot: Value = bounded(case.clock.end(case.clock.deadline)?, Duration::from_secs(5), "claim_snapshot", sqlx::query_scalar(
        "WITH tick AS MATERIALIZED (SELECT clock_timestamp() AS t) \
         SELECT jsonb_build_object('database_clock',tick.t::text,'rows', \
         jsonb_agg(to_jsonb(o) || jsonb_build_object('lease_live',o.lease_expires_at > tick.t) ORDER BY o.event_id)) \
         FROM outbox_events o CROSS JOIN tick WHERE event_id=ANY($1) GROUP BY tick.t")
        .bind(ids).fetch_one(case.pool()?)).await?;
        let rows = snapshot["rows"]
            .as_array()
            .ok_or(FixtureError::Unknown("snapshot_rows_decode"))?
            .clone();
        case.observations.push(
        json!({"event":phase,"snapshot":snapshot,"observed_monotonic_ns":observed_monotonic_ns()?}),
    );
        Ok(rows)
    }

    async fn four_claim_case(case: &mut OwnedCase) -> Result<(), CaseError> {
        let ids = (100_u128..108).map(Uuid::from_u128).collect::<Vec<_>>();
        for id in &ids {
            insert_event(case, *id, 0, None).await?;
        }
        let owners = (0_u128..4)
            .map(|n| Uuid::from_u128(1_000 + n))
            .collect::<Vec<_>>();
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| FixtureError::Unknown("parent_barrier_bind"))?;
        let address = listener
            .local_addr()
            .map_err(|_| FixtureError::Unknown("parent_barrier_address"))?
            .to_string();
        let barrier_phase = case.clock.phase(G07_BARRIER_PHASE);
        let mut workers = Vec::new();
        for owner in &owners {
            workers.push(case.spawn("claim", *owner, 2, Some(&address))?);
        }
        case.observations.push(json!({"event":"claim_barrier","endpoint":address,"handshakes":workers.iter().map(|index| case.children[*index].config.handshake.to_string()).collect::<Vec<_>>()}));
        release_barrier(&listener, case, &workers, barrier_phase)?;
        wait_for_claimed(case, &workers, &owners, 8).await?;
        let rows = claim_snapshot(case, &ids, "simultaneously_live_disjoint_claims").await?;
        for index in &workers {
            case.children[*index].check_alive()?;
        }
        require(
            rows.len() == 8
                && rows.iter().all(|row| {
                    row["lease_live"] == json!(true) && row["attempt_count"] == json!(1)
                }),
            "eight_live_first_claims",
        )?;
        let tokens = rows
            .iter()
            .map(|row| {
                row["lease_token"]
                    .as_str()
                    .and_then(|token| Uuid::parse_str(token).ok())
                    .ok_or(FixtureError::Unknown("token_decode"))
            })
            .collect::<Result<HashSet<_>, _>>()?;
        require(tokens.len() == 8, "live_tokens_must_be_disjoint")?;
        for owner in &owners {
            require(
                rows.iter()
                    .filter(|row| row["lease_owner"] == json!(owner.to_string()))
                    .count()
                    == 2,
                "two_claims_per_live_owner",
            )?;
        }
        case.children[workers[0]].kill_and_reap(case.clock.phase(G07_COHORT_REAP), true)?;
        case.forced_expiry.push("FORCED_BY_TEST_SQL");
        let expiry: Vec<Value> = bounded(case.clock.end(case.clock.deadline)?, Duration::from_secs(5), "force_killed_owner_expiry", sqlx::query_scalar(
        "UPDATE outbox_events SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE lease_owner=$1 AND event_id=ANY($2) \
         RETURNING jsonb_build_object('event_id',event_id,'lease_owner',lease_owner,'lease_expires_at',lease_expires_at,'database_clock',clock_timestamp()::text)")
        .bind(owners[0]).bind(&ids).fetch_all(case.pool()?)).await?;
        require(expiry.len() == 2, "forced_killed_owner_expiry_count")?;
        case.observations.push(json!({"event":"forced_expiry","label":"FORCED_BY_TEST_SQL","affected_rows":expiry.len(),"rows":expiry}));
        let recovered_owner = Uuid::from_u128(2_000);
        let replacement = case.spawn("claim", recovered_owner, 2, None)?;
        wait_for_claimed(case, &[replacement], &[recovered_owner], 2).await?;
        let after = claim_snapshot(case, &ids, "replacement_and_untouched_live_claims").await?;
        for index in workers.iter().skip(1).chain(std::iter::once(&replacement)) {
            case.children[*index].check_alive()?;
        }
        require(after.len() == 8, "recovery_row_count")?;
        let mut recovered_count = 0;
        let mut untouched_count = 0;
        for current in &after {
            let previous = rows
                .iter()
                .find(|previous| previous["event_id"] == current["event_id"])
                .ok_or(FixtureError::Unknown("recovered_event_identity"))?;
            for key in [
                "event_id",
                "event_type",
                "aggregate_type",
                "aggregate_id",
                "payload",
                "occurred_at",
                "available_at",
                "attempt_limit",
            ] {
                require(
                    current[key] == previous[key],
                    "recovery_changed_preserved_event",
                )?;
            }
            require(
                current["lease_live"] == json!(true),
                "recovery_observation_requires_live_leases",
            )?;
            if previous["lease_owner"] == json!(owners[0].to_string()) {
                require(
                    current["lease_owner"] == json!(recovered_owner.to_string())
                        && current["attempt_count"] == json!(2)
                        && !current["lease_token"].is_null()
                        && current["lease_token"] != previous["lease_token"],
                    "replacement_exact_killed_events_new_tokens",
                )?;
                recovered_count += 1;
            } else {
                for key in [
                    "lease_owner",
                    "lease_token",
                    "attempt_count",
                    "lease_expires_at",
                    "last_attempt_at",
                ] {
                    require(
                        current[key] == previous[key],
                        "healthy_six_claims_must_be_untouched",
                    )?;
                }
                untouched_count += 1;
            }
        }
        require(
            recovered_count == 2 && untouched_count == 6,
            "recovery_two_and_healthy_six",
        )?;
        let cohort = case.clock.phase(G07_COHORT_REAP);
        case.children[replacement].kill_and_reap(cohort, true)?;
        for index in workers.iter().skip(1) {
            case.children[*index].kill_and_reap(cohort, true)?;
        }
        Ok(())
    }

    fn note_error(error: &CaseError, unknowns: &mut Vec<String>, timeouts: &mut Vec<String>) {
        match error {
            CaseError::Fixture(FixtureError::Timeout(phase)) => {
                timeouts.push((*phase).to_owned());
                unknowns.push(format!("timeout:{phase}"));
            }
            CaseError::Fixture(FixtureError::Unknown(phase)) => unknowns.push((*phase).to_owned()),
            CaseError::Fixture(FixtureError::IdentityMismatch) => {
                unknowns.push("owned_identity_mismatch".to_owned())
            }
            CaseError::Fixture(FixtureError::ConfigurationRejected) => {
                unknowns.push("owned_configuration_rejected".to_owned())
            }
            CaseError::Assertion(cause) if *cause == "child_output_overflow" => {
                unknowns.push("child_output_overflow".to_owned())
            }
            CaseError::Assertion(_) | CaseError::Unwired => {}
        }
    }

    pub(super) async fn run_owned_case(name: &'static str) -> Result<(), CaseError> {
        let mut case = OwnedCase::new(name)?;
        let deadline = case.clock.end(case.clock.deadline)?;
        let remaining = case
            .clock
            .remaining(case.clock.deadline, Duration::from_secs(90))?;
        let result = match tokio::time::timeout(remaining, async {
            case.database
                .create_and_migrate(case.launch, case.case, deadline)
                .await?;
            match name {
                "last_claim_kill9_restart_reaps_unknown_once" => last_claim_case(&mut case).await,
                "four_processes_disjoint_claims_and_recover_expired" => {
                    four_claim_case(&mut case).await
                }
                _ => Err(CaseError::Assertion("unadmitted_g07_case")),
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Err(CaseError::Fixture(FixtureError::Timeout("case_total"))),
        };
        let mut unknowns = Vec::new();
        let mut timeouts = Vec::new();
        if let Err(error) = &result {
            note_error(error, &mut unknowns, &mut timeouts);
        }
        let cleanup = Deadline::cleanup_reserve(case.clock.deadline, case.clock.elapsed());
        let cohort = Deadline::phase(cleanup, case.clock.elapsed(), G07_COHORT_REAP);
        for child in &mut case.children {
            if let Err(error) = child.kill_and_reap(cohort, false) {
                note_error(&error, &mut unknowns, &mut timeouts);
            }
            // Preserve already captured bounded bytes even if the other pipe was
            // incomplete. These partial paths never assert complete output.
            if let Some(identity) = &child.identity {
                for (stream, captured) in [
                    ("stdout", &child.pending_stdout),
                    ("stderr", &child.pending_stderr),
                ] {
                    if let Some(captured) = captured {
                        let path = child.scope.root().join(format!(
                            "receipts/partial-{}-{}-{}.{}.log",
                            child.config.test_launch_id, identity.pid, identity.start_ticks, stream
                        ));
                        match write_new_private(&path, &captured.captured.bytes) {
                        Ok(()) => child.details.push(json!({"event":"partial_output","stream":stream,"path":path,
                            "bytes":captured.captured.bytes.len(),"sha256":bytes_sha256(&captured.captured.bytes)})),
                        Err(error) => note_error(&error.into(), &mut unknowns, &mut timeouts),
                    }
                    }
                }
            }
        }
        let children_reaped = case.children.iter().all(ChildGuard::confirmed_reaped);
        if !children_reaped {
            unknowns.push("owned_children_cleanup_unconfirmed".to_owned());
        }
        if let Err(error) = case
            .database
            .close(case.clock.end(cleanup)?, children_reaped)
            .await
        {
            note_error(&error.into(), &mut unknowns, &mut timeouts);
        }
        let database_cleanup = case.database.cleanup == "confirmed";
        if !database_cleanup {
            unknowns.push("owned_database_cleanup_unconfirmed".to_owned());
        }
        let children = case
            .children
            .iter()
            .map(ChildGuard::receipt)
            .collect::<Vec<_>>();
        if children
            .iter()
            .any(|child| child["output_complete"] != json!(true))
        {
            unknowns.push("bounded_output_completion_unconfirmed".to_owned());
        }
        let overflow = children.iter().any(|child| {
            child["stdout_overflow"] == json!(true) || child["stderr_overflow"] == json!(true)
        });
        if overflow {
            unknowns.push("child_output_overflow".to_owned());
        }
        unknowns.sort();
        unknowns.dedup();
        timeouts.sort();
        timeouts.dedup();
        let expected = case.expected_receipt();
        if expected.is_none() {
            unknowns.push("independent_expected_context_unavailable".to_owned());
        }
        let (outcome, cause) = if !unknowns.is_empty() {
            ("unknown", json!(format!("{:?}", result.as_ref().err())))
        } else {
            match &result {
                Ok(()) => ("green", Value::Null),
                Err(CaseError::Unwired) => {
                    ("expected_red", json!("G07 child process path not wired"))
                }
                Err(error) => ("failure", json!(format!("{error:?}"))),
            }
        };
        let mut receipt = json!({"schema_version":1,"run_id":case.database.scope.run_id().to_string(),"test_launch_id":case.launch.to_string(),
        "case":case.case,"outcome":outcome,"cause":cause,"expected_exit_code":if outcome == "green" { 0 } else { 101 },
        "unknowns":unknowns,"timeouts":timeouts,"output_overflow":overflow,"registered_children":case.children.iter().filter(|child| child.registered).count(),
        "observed_children":case.children.iter().filter(|child| child.identity.is_some()).count(),"owned_children_reaped":children_reaped,
        "database_cleanup_confirmed":database_cleanup,"database":case.database.receipt(),"children":children,"forced_expiry_labels":case.forced_expiry});
        let confirmed = expected
            .as_ref()
            .is_some_and(|expected| terminal_receipt_is_confirmed(&receipt, expected));
        if matches!(outcome, "green" | "expected_red") && !confirmed {
            receipt["outcome"] = json!("unknown");
            receipt["cause"] = json!("terminal_owned_evidence_rejected");
            receipt["unknowns"]
                .as_array_mut()
                .ok_or(FixtureError::Unknown("receipt_unknowns"))?
                .push(json!("terminal_owned_evidence_rejected"));
            receipt["expected_exit_code"] = json!(101);
        }
        let details = json!({"schema_version":1,"run_id":case.database.scope.run_id().to_string(),"test_launch_id":case.launch.to_string(),"case":case.case,
        "creation_observation":case.database.scope.root().join(format!("receipts/observations-{}.json", case.launch)),
        "control_receipt":case.database.scope.root().join("receipts/cluster-control.json"),"parent_identity":{
            "pid":case.parent.pid,"start_ticks":case.parent.start_ticks,"uid":case.parent.uid,"executable":case.parent.executable,
            "executable_sha256":case.parent.executable_sha256,"executable_dev":case.parent.executable_dev,"executable_inode":case.parent.executable_inode},
        "elapsed_ms":case.clock.elapsed().as_millis(),"max_simultaneous":case.max_simultaneous,"child_count":case.children.len(),
        "fixture_observations":case.database.observations,"assertion_observations":case.observations,
        "children":case.children.iter().map(|child| json!({"identity":child.receipt(),"details":child.details})).collect::<Vec<_>>()});
        write_new_private(
            &case
                .database
                .scope
                .root()
                .join(format!("receipts/details-{}.json", case.launch)),
            &serde_json::to_vec(&details).map_err(|_| FixtureError::Unknown("details_encode"))?,
        )?;
        write_new_private(
            &case
                .database
                .scope
                .root()
                .join(format!("receipts/test-{}.json", case.launch)),
            &serde_json::to_vec(&receipt).map_err(|_| FixtureError::Unknown("terminal_encode"))?,
        )?;
        if receipt["outcome"] == json!("green") && confirmed {
            Ok(())
        } else if receipt["outcome"] == json!("expected_red") && confirmed {
            Err(CaseError::Unwired)
        } else {
            Err(CaseError::Fixture(FixtureError::Unknown(
                "terminal_case_outcome",
            )))
        }
    }
}

#[tokio::test]
async fn last_claim_kill9_restart_reaps_unknown_once() {
    #[cfg(target_os = "linux")]
    owned_runtime::run_owned_case("last_claim_kill9_restart_reaps_unknown_once")
        .await
        .expect("G07 owned last-claim case");
    #[cfg(not(target_os = "linux"))]
    panic!(
        "G07 runtime is unqualified on this platform: the reviewed owned Linux fixture is required"
    );
}

#[tokio::test]
async fn four_processes_disjoint_claims_and_recover_expired() {
    #[cfg(target_os = "linux")]
    owned_runtime::run_owned_case("four_processes_disjoint_claims_and_recover_expired")
        .await
        .expect("G07 owned four-process case");
    #[cfg(not(target_os = "linux"))]
    panic!(
        "G07 runtime is unqualified on this platform: the reviewed owned Linux fixture is required"
    );
}

#[tokio::test]
#[ignore = "child process fixture"]
async fn child_worker_fixture() {
    #[cfg(target_os = "linux")]
    owned_runtime::child_worker_entry().await;
    #[cfg(not(target_os = "linux"))]
    panic!(
        "G07 child runtime is unqualified on this platform: the reviewed owned Linux fixture is required"
    );
}

// Pure G07 preparation only. None of these contracts reads ambient state,
// touches a socket/database/file, or spawns/signals/waits for a real process.
// The child entrypoint above intentionally remains unwired for later G07 RED.
use g07_owned::{
    DatabaseIdentity, DatabaseResolution, DdlOperation, DdlResponse, FixtureError, OwnedScopeFacts,
    PathFact, PathKind, ProcessIdentity, checked_drop_statement, classify_ddl_response,
    parse_process_start_ticks, validate_owned_scope,
};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
};

const G07_STREAM_CAP: usize = 64 * 1024;
const G07_CHILD_LIFETIME: Duration = Duration::from_secs(45);
const G07_BARRIER_PHASE: Duration = Duration::from_secs(15);
const G07_CLEANUP_PHASE: Duration = Duration::from_secs(15);
const G07_COHORT_REAP: Duration = Duration::from_secs(10);

#[test]
fn g07_linux_process_identity_parses_final_parenthesis() {
    // Linux stat fields after the final ')' start with state (field 3).
    // Field 22 (starttime) is therefore index 19, regardless of comm text.
    let tail = "S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 991 20";
    assert_eq!(
        parse_process_start_ticks(&format!("41 (worker (owned) name) {tail}"), 41),
        Ok(991)
    );
    for invalid in [
        format!("42 (worker) {tail}"),
        "41 (worker) S 1 2".to_string(),
        format!("41 (worker) {}", tail.replace("991", "0")),
        format!("41 (worker) {}", tail.replace("991", "unknown")),
        "41 worker S 1 2".to_string(),
    ] {
        assert_eq!(
            parse_process_start_ticks(&invalid, 41),
            Err(FixtureError::IdentityMismatch),
            "unverified stat must not give process authority: {invalid}"
        );
    }
}

#[derive(Clone)]
struct ExpectedChildReceipt {
    identity: ProcessIdentity,
    mode: &'static str,
    owner: Uuid,
    handshake: Uuid,
}

#[derive(Clone)]
struct ExpectedReceipt {
    run_id: Uuid,
    test_launch_id: Uuid,
    case: &'static str,
    database: DatabaseIdentity,
    children: Vec<ExpectedChildReceipt>,
}

// Authority comes from independently recorded real observations supplied as
// expected context, never the receipt's own claims. RED and GREEN classification
// each have a parent-recorded chronological pure RED; both GREEN inventories
// were subsequently exercised by the parent-recorded nine-test pure GREEN.
fn terminal_receipt_is_confirmed(receipt: &Value, expected: &ExpectedReceipt) -> bool {
    let red = receipt["outcome"] == json!("expected_red");
    let modes: &[&str] = match (receipt["outcome"].as_str(), expected.case) {
        (Some("expected_red"), "last_claim_kill9_restart_reaps_unknown_once") => &["claim"],
        (Some("green"), "last_claim_kill9_restart_reaps_unknown_once") => {
            &["claim", "reap", "reap"]
        }
        (Some("green"), "four_processes_disjoint_claims_and_recover_expired") => &["claim"; 5],
        _ => return false,
    };
    let cause = if red {
        json!("G07 child process path not wired")
    } else {
        Value::Null
    };
    let labels = if red {
        json!([])
    } else {
        json!(["FORCED_BY_TEST_SQL"])
    };
    let run_prefix = format!("p6_g07_{}_", &expected.run_id.simple().to_string()[..16]);
    let mut pairs = HashSet::new();
    let fields = [
        "schema_version",
        "run_id",
        "test_launch_id",
        "case",
        "outcome",
        "cause",
        "expected_exit_code",
        "unknowns",
        "timeouts",
        "output_overflow",
        "registered_children",
        "observed_children",
        "owned_children_reaped",
        "database_cleanup_confirmed",
        "database",
        "children",
        "forced_expiry_labels",
    ];
    let Some(object) = receipt.as_object() else {
        return false;
    };
    if object.len() != fields.len()
        || object.keys().any(|key| !fields.contains(&key.as_str()))
        || expected.run_id.is_nil()
        || expected.test_launch_id.is_nil()
        || expected.children.len() != modes.len()
        || !expected.children.iter().zip(modes).all(|(child, mode)| {
            child.mode == *mode && pairs.insert((child.identity.pid, child.identity.start_ticks))
        })
        || !expected.database.name.starts_with(&run_prefix)
        || receipt["schema_version"] != json!(1)
        || receipt["run_id"] != json!(expected.run_id.to_string())
        || receipt["test_launch_id"] != json!(expected.test_launch_id.to_string())
        || receipt["case"] != json!(expected.case)
        || receipt["cause"] != cause
        || receipt["expected_exit_code"] != json!(if red { 101 } else { 0 })
        || receipt["unknowns"] != json!([])
        || receipt["timeouts"] != json!([])
        || receipt["forced_expiry_labels"] != labels
        || receipt["output_overflow"] != json!(false)
        || receipt["owned_children_reaped"] != json!(true)
        || receipt["database_cleanup_confirmed"] != json!(true)
        || receipt["registered_children"] != json!(modes.len())
        || receipt["observed_children"] != json!(modes.len())
        || checked_drop_statement(&expected.database, &expected.database).is_err()
    {
        return false;
    }
    let database = &receipt["database"];
    let Some(database_object) = database.as_object() else {
        return false;
    };
    let database_fields = [
        "cluster_system_identifier",
        "name",
        "oid",
        "owner",
        "create",
        "cleanup",
    ];
    if database_object.len() != database_fields.len()
        || database_object
            .keys()
            .any(|key| !database_fields.contains(&key.as_str()))
        || database["cluster_system_identifier"]
            != json!(expected.database.cluster_system_identifier)
        || database["name"] != json!(expected.database.name)
        || database["oid"] != json!(expected.database.oid)
        || database["owner"] != json!(expected.database.owner)
        || database["create"] != json!("confirmed")
        || database["cleanup"] != json!("confirmed")
    {
        return false;
    }
    let Some(children) = receipt["children"].as_array() else {
        return false;
    };
    if children.len() != expected.children.len() {
        return false;
    }
    let child_fields = [
        "pid",
        "start_ticks",
        "uid",
        "executable",
        "executable_sha256",
        "executable_dev",
        "executable_inode",
        "mode",
        "owner",
        "handshake",
        "registered",
        "reaped_by_parent",
        "status",
        "exit_code",
        "exit_signal",
        "output_complete",
        "stdout_overflow",
        "stderr_overflow",
    ];
    children
        .iter()
        .zip(&expected.children)
        .all(|(child, expected)| {
            let identity = &expected.identity;
            let Some(object) = child.as_object() else {
                return false;
            };
            let Some(expected_executable) = identity.executable.to_str() else {
                return false;
            };
            let Some(observed_executable) = child["executable"].as_str() else {
                return false;
            };
            identity.pid > 0
                && identity.start_ticks > 0
                && identity.uid == 1000
                && identity.executable.is_absolute()
                && identity.executable_dev > 0
                && identity.executable_inode > 0
                && identity.executable_sha256.len() == 64
                && identity
                    .executable_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && !expected.owner.is_nil()
                && !expected.handshake.is_nil()
                && object.len() == child_fields.len()
                && object
                    .keys()
                    .all(|key| child_fields.contains(&key.as_str()))
                && child["pid"] == json!(identity.pid)
                && child["start_ticks"] == json!(identity.start_ticks)
                && child["uid"] == json!(identity.uid)
                && observed_executable == expected_executable
                && child["executable_sha256"] == json!(identity.executable_sha256)
                && child["executable_dev"] == json!(identity.executable_dev)
                && child["executable_inode"] == json!(identity.executable_inode)
                && child["mode"] == json!(expected.mode)
                && child["owner"] == json!(expected.owner.to_string())
                && child["handshake"] == json!(expected.handshake.to_string())
                && child["registered"] == json!(true)
                && child["reaped_by_parent"] == json!(true)
                && child["status"] == json!("confirmed")
                && child["exit_code"]
                    == if red {
                        json!(101)
                    } else if expected.mode == "reap" {
                        json!(0)
                    } else {
                        Value::Null
                    }
                && child["exit_signal"]
                    == if !red && expected.mode == "claim" {
                        json!(9)
                    } else {
                        Value::Null
                    }
                && child["output_complete"] == json!(true)
                && child["stdout_overflow"] == json!(false)
                && child["stderr_overflow"] == json!(false)
        })
}

#[test]
fn g07_terminal_receipt_requires_exact_owned_cleanup() {
    let database = synthetic_database_identity();
    let expected = ExpectedReceipt {
        run_id: Uuid::from_u128(7),
        test_launch_id: Uuid::from_u128(8),
        case: "last_claim_kill9_restart_reaps_unknown_once",
        database: database.clone(),
        children: vec![ExpectedChildReceipt {
            identity: ProcessIdentity {
                pid: 41,
                start_ticks: 991,
                uid: 1000,
                executable: PathBuf::from("/synthetic/g07/process_recovery"),
                executable_sha256: "a".repeat(64),
                executable_dev: 7,
                executable_inode: 80,
            },
            mode: "claim",
            owner: Uuid::from_u128(500),
            handshake: Uuid::from_u128(900),
        }],
    };
    let receipt = json!({
        "schema_version": 1,
        "run_id": "00000000-0000-0000-0000-000000000007",
        "test_launch_id": "00000000-0000-0000-0000-000000000008",
        "case": "last_claim_kill9_restart_reaps_unknown_once",
        "outcome": "expected_red",
        "cause": "G07 child process path not wired",
        "expected_exit_code": 101,
        "unknowns": [],
        "timeouts": [],
        "output_overflow": false,
        "registered_children": 1,
        "observed_children": 1,
        "owned_children_reaped": true,
        "database_cleanup_confirmed": true,
        "database": {"cluster_system_identifier":database.cluster_system_identifier,
            "name":database.name,"oid":database.oid,"owner":database.owner,
            "create":"confirmed", "cleanup":"confirmed"},
        "children": [{"pid":41,"start_ticks":991,"uid":1000,
            "executable":"/synthetic/g07/process_recovery",
            "executable_sha256":"a".repeat(64),"executable_dev":7,"executable_inode":80,
            "mode":"claim","owner":Uuid::from_u128(500).to_string(),
            "handshake":Uuid::from_u128(900).to_string(),
            "registered":true,"reaped_by_parent":true,"status":"confirmed",
            "exit_code":101,"exit_signal":null,"output_complete":true,
            "stdout_overflow":false,"stderr_overflow":false}],
        "forced_expiry_labels": []
    });
    assert!(terminal_receipt_is_confirmed(&receipt, &expected));
    for (field, value) in [
        ("unknowns", json!(["lost_create_response"])),
        ("timeouts", json!(["child_watchdog"])),
        ("output_overflow", json!(true)),
        ("owned_children_reaped", json!(false)),
        ("database_cleanup_confirmed", json!(false)),
        ("observed_children", json!(0)),
        ("registered_children", json!(2)),
        ("case", json!("unadmitted_case")),
        ("cause", json!("unrelated failure")),
        ("forced_expiry_labels", json!(["FORCED_BY_TEST_SQL"])),
    ] {
        let mut invalid = receipt.clone();
        invalid[field] = value;
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "reject {field}"
        );
    }
    for field in ["registered", "reaped_by_parent", "output_complete"] {
        let mut invalid = receipt.clone();
        invalid["children"][0][field] = json!(false);
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "reject child {field}"
        );
    }
    let mut invalid = receipt.clone();
    invalid["database"]["cleanup"] = json!("unknown");
    assert!(!terminal_receipt_is_confirmed(&invalid, &expected));
    for (field, value) in [
        ("run_id", json!(Uuid::from_u128(9).to_string())),
        ("test_launch_id", json!(Uuid::from_u128(9).to_string())),
        ("schema_version", json!(2)),
    ] {
        let mut invalid = receipt.clone();
        invalid[field] = value;
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "reject {field}"
        );
    }
    for (field, value) in [
        (
            "name",
            json!("p6_g07_0000000000000007_00000000000000000000000000000009"),
        ),
        ("oid", json!(database.oid + 1)),
        ("owner", json!("other")),
        ("cluster_system_identifier", json!("7500000000000000008")),
    ] {
        let mut invalid = receipt.clone();
        invalid["database"][field] = value;
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "reject database {field}"
        );
    }
    for (field, value) in [
        ("pid", json!(42)),
        ("start_ticks", json!(992)),
        ("uid", json!(1001)),
        ("executable", json!("/synthetic/unrelated")),
        ("executable_sha256", json!("b".repeat(64))),
        ("executable_dev", json!(8)),
        ("executable_inode", json!(81)),
        ("mode", json!("reap")),
        ("owner", json!(Uuid::from_u128(501).to_string())),
        ("handshake", json!(Uuid::from_u128(901).to_string())),
    ] {
        let mut invalid = receipt.clone();
        invalid["children"][0][field] = value;
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "reject child {field}"
        );
    }
    let mut invalid = receipt.clone();
    invalid.as_object_mut().unwrap().remove("test_launch_id");
    assert!(!terminal_receipt_is_confirmed(&invalid, &expected));
    let mut invalid = receipt.clone();
    invalid["database"].as_object_mut().unwrap().remove("oid");
    assert!(!terminal_receipt_is_confirmed(&invalid, &expected));
    let mut invalid = receipt.clone();
    invalid["children"][0]
        .as_object_mut()
        .unwrap()
        .remove("start_ticks");
    assert!(!terminal_receipt_is_confirmed(&invalid, &expected));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut expected = expected;
        expected.children[0].identity.executable =
            PathBuf::from(OsString::from_vec(b"/synthetic/g07/\xff".to_vec()));
        let mut invalid = receipt;
        invalid["children"][0]["executable"] = Value::Null;
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "unrepresentable expected path and null JSON must not compare as owned identity"
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut expected = expected;
        let mut wide = r"C:\synthetic\g07\".encode_utf16().collect::<Vec<_>>();
        wide.push(0xd800); // Unpaired surrogate is not representable as UTF-8.
        expected.children[0].identity.executable = PathBuf::from(OsString::from_wide(&wide));
        let mut invalid = receipt;
        invalid["children"][0]["executable"] = Value::Null;
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &expected),
            "unrepresentable expected path and null JSON must not compare as owned identity"
        );
    }
}

#[test]
fn g07_green_terminal_receipt_requires_exact_inventory() {
    for (case, modes) in [
        (
            "last_claim_kill9_restart_reaps_unknown_once",
            vec!["claim", "reap", "reap"],
        ),
        (
            "four_processes_disjoint_claims_and_recover_expired",
            vec!["claim"; 5],
        ),
    ] {
        let expected = ExpectedReceipt {
            run_id: Uuid::from_u128(7),
            test_launch_id: Uuid::from_u128(8),
            case,
            database: synthetic_database_identity(),
            children: modes
                .iter()
                .enumerate()
                .map(|(index, mode)| ExpectedChildReceipt {
                    identity: ProcessIdentity {
                        pid: 41 + index as u32,
                        // Several valid children can share a coarse start tick.
                        // Authority belongs to the PID/start pair, not ticks alone.
                        start_ticks: 991,
                        uid: 1000,
                        executable: PathBuf::from("/synthetic/g07/process_recovery"),
                        executable_sha256: "a".repeat(64),
                        executable_dev: 7,
                        executable_inode: 80,
                    },
                    mode,
                    owner: Uuid::from_u128(500 + index as u128),
                    handshake: Uuid::from_u128(900 + index as u128),
                })
                .collect(),
        };
        let children = expected.children.iter().map(|child| {
            let identity = &child.identity;
            json!({"pid":identity.pid,"start_ticks":identity.start_ticks,"uid":identity.uid,
                "executable":identity.executable.to_str().unwrap(),
                "executable_sha256":identity.executable_sha256,
                "executable_dev":identity.executable_dev,"executable_inode":identity.executable_inode,
                "mode":child.mode,"owner":child.owner.to_string(),"handshake":child.handshake.to_string(),
                "registered":true,"reaped_by_parent":true,"status":"confirmed",
                "exit_code":if child.mode == "reap" { json!(0) } else { Value::Null },
                "exit_signal":if child.mode == "claim" { json!(9) } else { Value::Null },
                "output_complete":true,"stdout_overflow":false,"stderr_overflow":false})
        }).collect::<Vec<_>>();
        let database = &expected.database;
        let receipt = json!({
            "schema_version":1,"run_id":expected.run_id.to_string(),
            "test_launch_id":expected.test_launch_id.to_string(),"case":case,
            "outcome":"green","cause":null,"expected_exit_code":0,
            "unknowns":[],"timeouts":[],"output_overflow":false,
            "registered_children":children.len(),"observed_children":children.len(),
            "owned_children_reaped":true,"database_cleanup_confirmed":true,
            "database":{"cluster_system_identifier":database.cluster_system_identifier,
                "name":database.name,"oid":database.oid,"owner":database.owner,
                "create":"confirmed","cleanup":"confirmed"},
            "children":children,"forced_expiry_labels":["FORCED_BY_TEST_SQL"]
        });
        assert!(
            terminal_receipt_is_confirmed(&receipt, &expected),
            "exact frozen GREEN inventory must be admitted: {case}"
        );
        for (field, value) in [
            ("outcome", json!("expected_red")),
            ("cause", json!("unrelated failure")),
            ("expected_exit_code", json!(101)),
            ("unknowns", json!(["lost_drop_response"])),
            ("timeouts", json!(["child_watchdog"])),
            ("output_overflow", json!(true)),
            ("owned_children_reaped", json!(false)),
            ("database_cleanup_confirmed", json!(false)),
            ("registered_children", json!(expected.children.len() + 1)),
            ("observed_children", json!(expected.children.len() - 1)),
            ("forced_expiry_labels", json!([])),
            ("forced_expiry_labels", json!(["NATURAL_TIME"])),
            (
                "forced_expiry_labels",
                json!(["FORCED_BY_TEST_SQL", "NATURAL_TIME"]),
            ),
            (
                "forced_expiry_labels",
                json!(["FORCED_BY_TEST_SQL", "FORCED_BY_TEST_SQL"]),
            ),
        ] {
            let mut invalid = receipt.clone();
            invalid[field] = value;
            assert!(
                !terminal_receipt_is_confirmed(&invalid, &expected),
                "reject GREEN {case}/{field}"
            );
        }
        for (field, value) in [
            ("mode", json!("reap")),
            ("exit_code", json!(0)),
            ("exit_signal", Value::Null),
            ("registered", json!(false)),
            ("reaped_by_parent", json!(false)),
            ("status", json!("unknown")),
            ("output_complete", json!(false)),
            ("stdout_overflow", json!(true)),
            ("stderr_overflow", json!(true)),
            ("owner", json!(Uuid::from_u128(999).to_string())),
            ("handshake", json!(Uuid::from_u128(999).to_string())),
        ] {
            let mut invalid = receipt.clone();
            invalid["children"][0][field] = value;
            assert!(
                !terminal_receipt_is_confirmed(&invalid, &expected),
                "reject GREEN child {case}/{field}"
            );
        }
        let mut invalid = receipt.clone();
        invalid["children"].as_array_mut().unwrap().pop();
        assert!(!terminal_receipt_is_confirmed(&invalid, &expected));
        let mut invalid = receipt.clone();
        invalid["database"]["oid"] = json!(database.oid + 1);
        assert!(!terminal_receipt_is_confirmed(&invalid, &expected));
        let mut wrong_context = expected.clone();
        wrong_context.test_launch_id = Uuid::from_u128(9);
        assert!(!terminal_receipt_is_confirmed(&receipt, &wrong_context));
        let mut wrong_context = expected.clone();
        wrong_context.children[0].identity.start_ticks += 1;
        assert!(!terminal_receipt_is_confirmed(&receipt, &wrong_context));

        let mut wrong_inventory = expected.clone();
        wrong_inventory.children[1].mode = if case.starts_with("last_") {
            "claim"
        } else {
            "reap"
        };
        let mut invalid = receipt.clone();
        invalid["children"][1]["mode"] = json!(wrong_inventory.children[1].mode);
        invalid["children"][1]["exit_code"] = if wrong_inventory.children[1].mode == "reap" {
            json!(0)
        } else {
            Value::Null
        };
        invalid["children"][1]["exit_signal"] = if wrong_inventory.children[1].mode == "claim" {
            json!(9)
        } else {
            Value::Null
        };
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &wrong_inventory),
            "self-consistent wrong case inventory cannot authorize GREEN"
        );

        let mut duplicate_context = expected.clone();
        duplicate_context.children[1].identity = expected.children[0].identity.clone();
        let mut duplicate = receipt.clone();
        duplicate["children"][1]["pid"] = duplicate["children"][0]["pid"].clone();
        assert!(
            !terminal_receipt_is_confirmed(&duplicate, &duplicate_context),
            "duplicate PID/start pair cannot authorize GREEN"
        );
        let mut wrong_run = expected.clone();
        wrong_run.database.name =
            "p6_g07_0000000000000007_00000000000000000000000000000007".to_string();
        let mut invalid = receipt.clone();
        invalid["database"]["name"] = json!(wrong_run.database.name);
        assert!(
            !terminal_receipt_is_confirmed(&invalid, &wrong_run),
            "database must belong to the independently expected run prefix"
        );
    }
}

fn synthetic_scope() -> (Value, OwnedScopeFacts) {
    let root = "/workspace/scratch/13897606dfde/p6pg.Test0007";
    let paths = [
        ("run_root", root.to_string()),
        ("data_dir", format!("{root}/data")),
        ("socket_dir", format!("{root}/socket")),
        ("home_dir", format!("{root}/home")),
        ("tmp_dir", format!("{root}/tmp")),
        ("config_file", format!("{root}/data/postgresql.conf")),
        ("hba_file", format!("{root}/data/pg_hba.conf")),
        (
            "postgres_executable",
            format!("{root}/install/bin/postgres"),
        ),
    ];
    let executable_sha256 = "a".repeat(64);
    let server = ProcessIdentity {
        pid: 41,
        start_ticks: 500,
        uid: 1000,
        executable: PathBuf::from(&paths[7].1),
        executable_sha256: executable_sha256.clone(),
        executable_dev: 7,
        executable_inode: 80,
    };
    let mut manifest = json!({
        "schema_version": 1,
        "run_id": "00000000-0000-0000-0000-000000000007",
        "postgres_executable_sha256": executable_sha256,
        "server_pid": server.pid,
        "server_start_ticks": server.start_ticks,
        "server_uid": server.uid,
        "server_executable_dev": server.executable_dev,
        "server_executable_inode": server.executable_inode,
        "os_user": "agent",
        "database_user": "agent",
        "admin_database": "postgres",
        "port": 5432,
        "server_version_num": 180006,
        "offline_system_identifier": "7500000000000000007",
        "listen_addresses": ""
    });
    for (field, path) in &paths {
        manifest[*field] = json!(path);
    }
    let facts = OwnedScopeFacts {
        expected_run_root: PathBuf::from(root),
        current_uid: 1000,
        current_os_user: "agent".to_string(),
        paths: paths
            .iter()
            .map(|(field, path)| PathFact {
                field,
                canonical: PathBuf::from(path),
                has_symlink_component: false,
                uid: 1000,
                mode: match *field {
                    "config_file" | "hba_file" => 0o600,
                    "postgres_executable" => 0o755,
                    _ => 0o700,
                },
                kind: if matches!(*field, "config_file" | "hba_file" | "postgres_executable") {
                    PathKind::RegularFile
                } else {
                    PathKind::Directory
                },
            })
            .collect(),
        server,
        offline_system_identifier: "7500000000000000007".to_string(),
    };
    (manifest, facts)
}

#[test]
fn g07_fixture_configuration_fails_closed() {
    let (valid, facts) = synthetic_scope();
    let path = OsStr::new("/workspace/scratch/13897606dfde/p6pg.Test0007/scope.json");
    let encoded = valid.to_string();
    assert_eq!(
        validate_owned_scope(None, &encoded, &facts, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    assert_eq!(
        validate_owned_scope(
            Some(OsStr::new("/outside/scope.json")),
            &encoded,
            &facts,
            &[]
        ),
        Err(FixtureError::ConfigurationRejected)
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let non_unicode = OsString::from_vec(vec![0xff]);
        assert_eq!(
            validate_owned_scope(Some(&non_unicode), &encoded, &facts, &[]),
            Err(FixtureError::ConfigurationRejected)
        );
    }
    for malformed in ["", "not-json", "[]", "{\"schema_version\":1}"] {
        assert_eq!(
            validate_owned_scope(Some(path), malformed, &facts, &[]),
            Err(FixtureError::ConfigurationRejected),
            "malformed manifest: {malformed}"
        );
    }
    // URLs/host lists/credentials have no permitted encoding in the structured
    // scope. Reject the field itself, including otherwise valid JSON.
    for (field, value) in [
        ("admin_url", json!("postgres://agent@127.0.0.1/postgres")),
        (
            "hosts",
            json!(["/synthetic/g07-run/socket", "remote.example"]),
        ),
        ("host", json!("localhost")),
        ("password", json!("synthetic-forbidden")),
        ("passfile", json!("/synthetic/g07-run/home/.pgpass")),
        ("service", json!("ambient")),
        ("options", json!("-c search_path=public")),
        ("sslcert", json!("/synthetic/g07-run/client.crt")),
        ("deadline_ms", json!(0)),
    ] {
        let mut hostile = valid.clone();
        hostile[field] = value;
        assert_eq!(
            validate_owned_scope(Some(path), &hostile.to_string(), &facts, &[]),
            Err(FixtureError::ConfigurationRejected),
            "unlisted field {field}"
        );
    }
    for (field, value) in [
        ("run_root", json!("/synthetic/other-run")),
        ("socket_dir", json!("/outside/socket")),
        ("listen_addresses", json!("127.0.0.1")),
        ("database_user", json!("postgres")),
        ("server_pid", json!(42)),
        ("server_start_ticks", json!(501)),
        ("server_uid", json!(1001)),
        ("server_executable_dev", json!(8)),
        ("server_executable_inode", json!(81)),
        ("postgres_executable_sha256", json!("b".repeat(64))),
        ("offline_system_identifier", json!("7500000000000000008")),
        ("server_version_num", json!(180005)),
        ("port", json!(5433)),
    ] {
        let mut hostile = valid.clone();
        hostile[field] = value;
        assert_eq!(
            validate_owned_scope(Some(path), &hostile.to_string(), &facts, &[]),
            Err(FixtureError::ConfigurationRejected),
            "mismatched {field}"
        );
    }
    let mut symlink = facts.clone();
    symlink.paths[2].has_symlink_component = true;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &symlink, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut outside = facts.clone();
    outside.paths[2].canonical = PathBuf::from("/outside/socket");
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &outside, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut wrong_owner = facts.clone();
    wrong_owner.paths[2].uid = 1001;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &wrong_owner, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut public_socket = facts.clone();
    public_socket.paths[2].mode = 0o755;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &public_socket, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut public_root = facts.clone();
    public_root.paths[0].mode = 0o755;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &public_root, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut root_file = facts.clone();
    root_file.paths[0].kind = PathKind::RegularFile;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &root_file, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut executable_directory = facts.clone();
    executable_directory.paths[7].kind = PathKind::Directory;
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &executable_directory, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut missing_path = facts.clone();
    missing_path.paths.pop();
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &missing_path, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    let mut duplicate_path = facts.clone();
    duplicate_path.paths.push(duplicate_path.paths[0].clone());
    assert_eq!(
        validate_owned_scope(Some(path), &encoded, &duplicate_path, &[]),
        Err(FixtureError::ConfigurationRejected)
    );
    // SQLx constructors can consume PG* even without pgpass. The eventual
    // connection adapter must reject ambient PG* before constructing options.
    for variable in [
        "PGHOST",
        "PGHOSTADDR",
        "PGPASSWORD",
        "PGPASSFILE",
        "PGSERVICE",
        "PGOPTIONS",
        "PGSSLKEY",
        "PGUNRECOGNIZED",
    ] {
        let ambient = [(OsString::from(variable), OsString::from("hostile"))];
        assert_eq!(
            validate_owned_scope(Some(path), &encoded, &facts, &ambient),
            Err(FixtureError::ConfigurationRejected),
            "ambient {variable}"
        );
    }
    assert!(
        validate_owned_scope(Some(path), &encoded, &facts, &[]).is_ok(),
        "exact owned configuration must be accepted"
    );
    // No capability is present in this reducer: rejected input cannot connect,
    // perform DDL or invoke Docker. Runtime ordering still needs source review.
}

fn synthetic_database_identity() -> DatabaseIdentity {
    DatabaseIdentity {
        cluster_system_identifier: "7500000000000000007".to_string(),
        name: "p6_g07_0000000000000000_00000000000000000000000000000007".to_string(),
        oid: 16_401,
        owner: "agent".to_string(),
    }
}

#[test]
fn g07_cleanup_requires_exact_owned_identity() {
    let expected = synthetic_database_identity();
    let mut mismatches = Vec::new();
    let mut wrong = expected.clone();
    wrong.cluster_system_identifier.push('8');
    mismatches.push(wrong);
    let mut wrong = expected.clone();
    wrong.oid += 1;
    mismatches.push(wrong);
    let mut wrong = expected.clone();
    wrong.owner = "postgres".to_string();
    mismatches.push(wrong);
    let mut wrong = expected.clone();
    wrong.name.push('8');
    mismatches.push(wrong);
    for observed in mismatches {
        assert_eq!(
            checked_drop_statement(&expected, &observed),
            Err(FixtureError::IdentityMismatch),
            "no DROP for {observed:?}"
        );
    }
    for name in ["postgres", "bad; DROP DATABASE postgres", "p6_g07_*"] {
        let mut invalid = expected.clone();
        invalid.name = name.to_string();
        assert_eq!(
            checked_drop_statement(&invalid, &invalid),
            Err(FixtureError::IdentityMismatch)
        );
    }
    let sql = checked_drop_statement(&expected, &expected)
        .expect("exact owned identity must produce one DROP");
    assert_eq!(sql, format!("DROP DATABASE {}", expected.name));
    assert!(!sql.contains("FORCE"));
    assert!(!sql.contains("IF EXISTS"));
    assert!(!sql.contains("pg_terminate_backend"));
}

#[test]
fn g07_unknown_database_outcome_is_not_success() {
    let identity = synthetic_database_identity();
    for operation in [DdlOperation::Create, DdlOperation::Drop] {
        for recorded in [None, Some(&identity)] {
            let decision = classify_ddl_response(operation, DdlResponse::Unknown, recorded);
            assert_eq!(
                decision.resolution,
                DatabaseResolution::Unknown,
                "lost {operation:?} response is Unknown"
            );
            assert!(!decision.retry, "uncertainty does not permit repeated DDL");
            assert_eq!(
                decision.cleanup_target, None,
                "uncertainty never authorizes DROP"
            );
        }
    }
    let created = classify_ddl_response(
        DdlOperation::Create,
        DdlResponse::Confirmed,
        Some(&identity),
    );
    assert_eq!(
        created.resolution,
        DatabaseResolution::Ready(identity.clone())
    );
    assert!(!created.retry);
    assert_eq!(created.cleanup_target, Some(identity.clone()));
    let no_identity = classify_ddl_response(DdlOperation::Create, DdlResponse::Confirmed, None);
    assert_eq!(
        no_identity.resolution,
        DatabaseResolution::Unknown,
        "generated name is not ownership evidence"
    );
    assert_eq!(no_identity.cleanup_target, None);
    let dropped =
        classify_ddl_response(DdlOperation::Drop, DdlResponse::Confirmed, Some(&identity));
    assert_eq!(dropped.resolution, DatabaseResolution::Dropped);
    assert!(!dropped.retry);
    assert_eq!(dropped.cleanup_target, None);
    let unverified_drop = classify_ddl_response(DdlOperation::Drop, DdlResponse::Confirmed, None);
    assert_eq!(unverified_drop.resolution, DatabaseResolution::Unknown);
    assert_eq!(unverified_drop.cleanup_target, None);
}

#[derive(Debug, Clone, Copy)]
struct Deadline(Duration);

impl Deadline {
    // The cleanup phase shares the original work+reserve/outer bounds.
    // Finishing early gets only 15s; arriving late cannot restart that reserve.
    fn cleanup_reserve(work: Self, now: Duration) -> Self {
        let original_end = work
            .0
            .checked_add(G07_CLEANUP_PHASE)
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_secs(120));
        Self::phase(Self(original_end), now, G07_CLEANUP_PHASE)
    }
    fn phase(parent: Self, now: Duration, allowance: Duration) -> Self {
        Self(now.checked_add(allowance).unwrap_or(now).min(parent.0))
    }

    fn remaining(self, now: Duration, local: Duration) -> Option<Duration> {
        let remaining = self.0.checked_sub(now)?.min(local);
        (!remaining.is_zero()).then_some(remaining)
    }
}

#[test]
fn g07_deadlines_do_not_restart_between_phases() {
    let case = Deadline(Duration::from_secs(90));
    let child = Deadline::phase(case, Duration::ZERO, G07_CHILD_LIFETIME);
    let barrier = Deadline::phase(child, Duration::ZERO, G07_BARRIER_PHASE);
    for operation in ["accept", "read", "write"] {
        assert_eq!(
            barrier.remaining(Duration::from_millis(14_750), Duration::from_secs(2)),
            Some(Duration::from_millis(250)),
            "{operation} shares one cohort barrier deadline"
        );
        assert_eq!(
            barrier.remaining(Duration::from_secs(15), Duration::from_secs(2)),
            None,
            "{operation} cannot restart expired barrier"
        );
    }
    let expired_phase = Deadline::phase(child, Duration::from_secs(45), G07_BARRIER_PHASE);
    assert_eq!(
        expired_phase.remaining(Duration::from_secs(45), Duration::from_secs(2)),
        None
    );
    assert_eq!(
        child.remaining(Duration::from_secs(44), Duration::from_secs(3)),
        Some(Duration::from_secs(1)),
        "wait cannot outlive child lifetime"
    );
    let cleanup = Deadline::phase(case, Duration::from_secs(82), G07_CLEANUP_PHASE);
    let cohort = Deadline::phase(cleanup, Duration::from_secs(83), G07_COHORT_REAP);
    for operation in ["kill/wait", "output", "pool-close", "drop"] {
        assert_eq!(
            cohort.remaining(Duration::from_secs(89), Duration::from_secs(3)),
            Some(Duration::from_secs(1)),
            "{operation} shares remaining cleanup/case budget"
        );
        assert_eq!(
            cohort.remaining(Duration::from_secs(90), Duration::from_secs(3)),
            None
        );
    }
    let overflow = Deadline::phase(case, Duration::MAX, Duration::from_secs(1));
    assert_eq!(
        overflow.remaining(Duration::MAX, Duration::from_secs(1)),
        None,
        "clock overflow fails closed"
    );
    let early = Deadline::cleanup_reserve(case, Duration::from_secs(20));
    assert_eq!(
        early.remaining(Duration::from_secs(20), Duration::from_secs(90)),
        Some(Duration::from_secs(15)),
        "early finish gets only the 15-second cleanup phase"
    );
    assert_eq!(
        early.remaining(Duration::from_secs(35), Duration::from_secs(3)),
        None,
        "early cleanup phase must not consume the full original reserve"
    );
    let reserve = Deadline::cleanup_reserve(case, Duration::from_secs(90));
    assert_eq!(
        reserve.remaining(Duration::from_secs(90), Duration::from_secs(3)),
        Some(Duration::from_secs(3)),
        "completed 90-second work must leave the separate anchored cleanup reserve"
    );
    let late = Deadline::cleanup_reserve(case, Duration::from_secs(99));
    assert_eq!(
        late.remaining(Duration::from_secs(99), Duration::from_secs(15)),
        Some(Duration::from_secs(6)),
        "late cleanup must not restart the original 15-second reserve"
    );
    let expired = Deadline::cleanup_reserve(case, Duration::from_secs(106));
    assert_eq!(
        expired.remaining(Duration::from_secs(106), Duration::from_secs(15)),
        None,
        "expired original reserve stays expired inside the outer 120-second bound"
    );
}

struct ChildConfiguration {
    scope_path: PathBuf,
    database: String,
    database_oid: u32,
    database_owner: String,
    mode: &'static str,
    owner: Uuid,
    limit: u32,
    handshake: Uuid,
    test_launch_id: Uuid,
    barrier: Option<String>,
    home: PathBuf,
    tmp: PathBuf,
}

struct ChildEnvironment {
    clear_inherited: bool,
    entries: BTreeMap<OsString, OsString>,
}

fn child_environment(
    config: &ChildConfiguration,
    _ambient: &[(OsString, OsString)],
) -> ChildEnvironment {
    let mut entries = [
        (
            "P6_G07_SCOPE_PATH",
            config.scope_path.as_os_str().to_owned(),
        ),
        ("P6_G07_DATABASE_NAME", OsString::from(&config.database)),
        (
            "P6_G07_DATABASE_OID",
            OsString::from(config.database_oid.to_string()),
        ),
        (
            "P6_G07_DATABASE_OWNER",
            OsString::from(&config.database_owner),
        ),
        ("P6_G07_MODE", OsString::from(config.mode)),
        ("P6_G07_OWNER", OsString::from(config.owner.to_string())),
        ("P6_G07_LIMIT", OsString::from(config.limit.to_string())),
        (
            "P6_G07_CHILD_HANDSHAKE",
            OsString::from(config.handshake.to_string()),
        ),
        (
            "P6_G07_TEST_LAUNCH_ID",
            OsString::from(config.test_launch_id.to_string()),
        ),
        ("HOME", config.home.as_os_str().to_owned()),
        ("TMPDIR", config.tmp.as_os_str().to_owned()),
        ("LC_ALL", OsString::from("C")),
        ("TZ", OsString::from("UTC")),
        ("RUST_BACKTRACE", OsString::from("0")),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), value))
    .collect::<BTreeMap<_, _>>();
    if let Some(barrier) = &config.barrier {
        entries.insert(
            OsString::from("P6_G07_CHILD_BARRIER"),
            OsString::from(barrier),
        );
    }
    ChildEnvironment {
        clear_inherited: true,
        entries,
    }
}

#[test]
fn g07_child_environment_is_allowlisted() {
    let mut config = ChildConfiguration {
        scope_path: PathBuf::from("/workspace/scratch/13897606dfde/p6pg.Test0007/scope.json"),
        database: synthetic_database_identity().name,
        database_oid: 16_401,
        database_owner: "agent".to_string(),
        mode: "claim",
        owner: Uuid::from_u128(500),
        limit: 2,
        handshake: Uuid::from_u128(900),
        test_launch_id: Uuid::from_u128(901),
        barrier: None,
        home: PathBuf::from("/workspace/scratch/13897606dfde/p6pg.Test0007/home"),
        tmp: PathBuf::from("/workspace/scratch/13897606dfde/p6pg.Test0007/tmp"),
    };
    let ambient = [
        "DATABASE_URL",
        "PGHOST",
        "PGPASSWORD",
        "DOCKER_HOST",
        "AWS_ACCESS_KEY_ID",
        "CARGO_REGISTRY_TOKEN",
        "P6_CHILD_URL",
        "P6_CHILD_BARRIER",
        "P6_G07_CHILD_BARRIER",
        "PATH",
        "LD_PRELOAD",
    ]
    .map(|name| (OsString::from(name), OsString::from("stale-hostile")));
    let environment = child_environment(&config, &ambient);
    assert!(
        environment.clear_inherited,
        "Command must clear inherited environment"
    );
    let expected = [
        (
            "P6_G07_SCOPE_PATH",
            config.scope_path.to_str().unwrap().to_string(),
        ),
        ("P6_G07_DATABASE_NAME", config.database.clone()),
        ("P6_G07_DATABASE_OID", config.database_oid.to_string()),
        ("P6_G07_DATABASE_OWNER", config.database_owner.clone()),
        ("P6_G07_MODE", config.mode.to_string()),
        ("P6_G07_OWNER", config.owner.to_string()),
        ("P6_G07_LIMIT", config.limit.to_string()),
        ("P6_G07_CHILD_HANDSHAKE", config.handshake.to_string()),
        ("P6_G07_TEST_LAUNCH_ID", config.test_launch_id.to_string()),
        ("HOME", config.home.to_str().unwrap().to_string()),
        ("TMPDIR", config.tmp.to_str().unwrap().to_string()),
        ("LC_ALL", "C".to_string()),
        ("TZ", "UTC".to_string()),
        ("RUST_BACKTRACE", "0".to_string()),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(
        environment.entries, expected,
        "only explicit owned values may reach child"
    );
    config.barrier = Some("127.0.0.1:34123".to_string());
    let barrier_environment = child_environment(&config, &ambient);
    let mut barrier_expected = expected;
    barrier_expected.insert(
        OsString::from("P6_G07_CHILD_BARRIER"),
        OsString::from("127.0.0.1:34123"),
    );
    assert!(barrier_environment.clear_inherited);
    assert_eq!(
        barrier_environment.entries, barrier_expected,
        "only supplied barrier/token can appear"
    );
}

fn format_child_marker(marker: &str, payload: &Value) -> String {
    format!("\n{marker}={payload}\n")
}

#[derive(Default)]
struct CapturedStream {
    bytes: Vec<u8>,
    overflow: bool,
}

impl CapturedStream {
    fn retain(&mut self, chunk: &[u8]) {
        let available = G07_STREAM_CAP.saturating_sub(self.bytes.len());
        let retain = chunk.len().min(available);
        self.bytes.extend_from_slice(&chunk[..retain]);
        self.overflow |= retain < chunk.len();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnedChildHandle(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillObservation {
    Sent,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReapObservation {
    Reaped,
    Running,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildCleanup {
    Reaped,
    Failed,
    Unknown,
}

trait OwnedChildControl {
    fn kill(&mut self, handle: OwnedChildHandle) -> KillObservation;
    fn try_reap(&mut self, handle: OwnedChildHandle) -> ReapObservation;
}

fn finish_owned_child(
    control: &mut impl OwnedChildControl,
    handle: OwnedChildHandle,
) -> ChildCleanup {
    let kill = control.kill(handle);
    let reap = control.try_reap(handle);
    match (kill, reap) {
        (KillObservation::Failed, _) => ChildCleanup::Failed,
        (KillObservation::Sent, ReapObservation::Reaped) => ChildCleanup::Reaped,
        _ => ChildCleanup::Unknown,
    }
}

struct ControlProbe {
    kill_result: KillObservation,
    reap_result: ReapObservation,
    calls: Vec<(&'static str, OwnedChildHandle)>,
}

impl OwnedChildControl for ControlProbe {
    fn kill(&mut self, handle: OwnedChildHandle) -> KillObservation {
        self.calls.push(("kill", handle));
        self.kill_result
    }
    fn try_reap(&mut self, handle: OwnedChildHandle) -> ReapObservation {
        self.calls.push(("try_reap", handle));
        self.reap_result
    }
}

#[test]
fn g07_child_output_is_bounded_and_kill_targets_owned_handle() {
    let mut failures = Vec::new();
    let mut stdout = CapturedStream::default();
    let mut stderr = CapturedStream::default();
    stdout.retain(&vec![b'o'; G07_STREAM_CAP]);
    assert!(!stdout.overflow, "exact cap is allowed");
    stdout.retain(b"discard while continuing to drain");
    if stdout.bytes.len() != G07_STREAM_CAP {
        failures.push("stdout retention exceeds cap".to_string());
    }
    if !stdout.overflow {
        failures.push("stdout overflow did not mark failure".to_string());
    }
    stdout.retain(b"more bytes after overflow");
    if stdout.bytes != vec![b'o'; G07_STREAM_CAP] {
        failures.push("post-overflow bytes were retained".to_string());
    }
    stderr.retain(&vec![b'e'; G07_STREAM_CAP + 1]);
    if stderr.bytes != vec![b'e'; G07_STREAM_CAP] {
        failures.push("stderr retention exceeds cap".to_string());
    }
    if !stderr.overflow {
        failures.push("stderr overflow did not mark failure".to_string());
    }
    let owned = OwnedChildHandle(7);
    for (kill, reap, expected) in [
        (
            KillObservation::Sent,
            ReapObservation::Reaped,
            ChildCleanup::Reaped,
        ),
        (
            KillObservation::Failed,
            ReapObservation::Reaped,
            ChildCleanup::Failed,
        ),
        (
            KillObservation::Unknown,
            ReapObservation::Reaped,
            ChildCleanup::Unknown,
        ),
        (
            KillObservation::Sent,
            ReapObservation::Running,
            ChildCleanup::Unknown,
        ),
        (
            KillObservation::Sent,
            ReapObservation::Unknown,
            ChildCleanup::Unknown,
        ),
    ] {
        let mut probe = ControlProbe {
            kill_result: kill,
            reap_result: reap,
            calls: Vec::new(),
        };
        let observed = finish_owned_child(&mut probe, owned);
        if observed != expected {
            failures.push(format!(
                "kill={kill:?}, reap={reap:?}: expected {expected:?}, observed {observed:?}"
            ));
        }
        if probe.calls != [("kill", owned), ("try_reap", owned)] {
            failures.push(format!(
                "owned-handle calls missing/wrong: {:?}",
                probe.calls
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "bounded output / owned-handle contract: {failures:?}"
    );

    let payload = json!({"test_launch_id":Uuid::from_u128(41).to_string(),"mode":"claim",
        "owner":Uuid::from_u128(500).to_string(),"handshake":Uuid::from_u128(42).to_string(),
        "cause":"G07 child process path not wired"});
    let output = format!(
        "test child_worker_fixture ... {}",
        format_child_marker("P6_G07_UNWIRED", &payload)
    );
    let expected = format!("P6_G07_UNWIRED={payload}");
    assert_eq!(
        output
            .lines()
            .filter(|line| line.starts_with("P6_G07_UNWIRED="))
            .collect::<Vec<_>>(),
        [expected.as_str()],
        "G07 child marker must be a standalone line after serial libtest prefix"
    );
}

// G07-only pure reducers, implemented after the recorded six-contract RED.
// These have no connection to postgres(), Docker, or its networked destructor.
// The G07-specific runtime adapters below never call the legacy fixture.
pub mod g07_owned {
    use std::{
        ffi::{OsStr, OsString},
        path::{Path, PathBuf},
    };

    use serde_json::Value;
    use uuid::Uuid;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct OwnedPgScope {
        manifest: Value,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PathFact {
        pub field: &'static str,
        pub canonical: PathBuf,
        pub has_symlink_component: bool,
        pub uid: u32,
        pub mode: u32,
        pub kind: PathKind,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PathKind {
        Directory,
        RegularFile,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ProcessIdentity {
        pub pid: u32,
        pub start_ticks: u64,
        pub uid: u32,
        pub executable: PathBuf,
        pub executable_sha256: String,
        pub executable_dev: u64,
        pub executable_inode: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct OwnedScopeFacts {
        pub expected_run_root: PathBuf,
        pub current_uid: u32,
        pub current_os_user: String,
        pub paths: Vec<PathFact>,
        pub server: ProcessIdentity,
        pub offline_system_identifier: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FixtureError {
        ConfigurationRejected,
        IdentityMismatch,
        Unknown(&'static str),
        Timeout(&'static str),
    }

    // Linux stat field 22 is index 19 after the final comm-closing ')'.
    // The exact positive/negative contract has a parent-recorded pure RED.
    pub fn parse_process_start_ticks(
        document: &str,
        expected_pid: u32,
    ) -> Result<u64, FixtureError> {
        let reject = || FixtureError::IdentityMismatch;
        let comm_start = document.find(" (").ok_or_else(reject)?;
        if expected_pid == 0 || document[..comm_start].parse::<u32>().ok() != Some(expected_pid) {
            return Err(reject());
        }
        let comm_end = document.rfind(") ").ok_or_else(reject)?;
        if comm_end <= comm_start + 1 {
            return Err(reject());
        }
        let start_ticks = document[comm_end + 2..]
            .split_whitespace()
            .nth(19)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|ticks| *ticks > 0)
            .ok_or_else(reject)?;
        Ok(start_ticks)
    }

    // The real observer must supply canonical/symlink/uid/executable/process
    // facts to this same reducer before any connection or DDL. A JSON marker
    // alone cannot provide ownership evidence. No environment is read here.
    pub fn validate_owned_scope(
        scope_path: Option<&OsStr>,
        document: &str,
        facts: &OwnedScopeFacts,
        ambient: &[(OsString, OsString)],
    ) -> Result<OwnedPgScope, FixtureError> {
        let reject = || FixtureError::ConfigurationRejected;
        let scope_path = scope_path
            .and_then(OsStr::to_str)
            .map(Path::new)
            .ok_or_else(reject)?;
        let root = &facts.expected_run_root;
        if !root.is_absolute()
            || scope_path != root.join("scope.json")
            || facts.current_uid != 1000
            || facts.current_os_user != "agent"
            || ambient
                .iter()
                .any(|(name, _)| name.to_string_lossy().starts_with("PG"))
        {
            return Err(reject());
        }
        let manifest: Value = serde_json::from_str(document).map_err(|_| reject())?;
        let object = manifest.as_object().ok_or_else(reject)?;
        let expected_paths = [
            ("run_root", root.clone(), PathKind::Directory),
            ("data_dir", root.join("data"), PathKind::Directory),
            ("socket_dir", root.join("socket"), PathKind::Directory),
            ("home_dir", root.join("home"), PathKind::Directory),
            ("tmp_dir", root.join("tmp"), PathKind::Directory),
            (
                "config_file",
                root.join("data/postgresql.conf"),
                PathKind::RegularFile,
            ),
            (
                "hba_file",
                root.join("data/pg_hba.conf"),
                PathKind::RegularFile,
            ),
            (
                "postgres_executable",
                root.join("install/bin/postgres"),
                PathKind::RegularFile,
            ),
        ];
        let other_fields = [
            "schema_version",
            "run_id",
            "postgres_executable_sha256",
            "server_pid",
            "server_start_ticks",
            "server_uid",
            "server_executable_dev",
            "server_executable_inode",
            "os_user",
            "database_user",
            "admin_database",
            "port",
            "server_version_num",
            "offline_system_identifier",
            "listen_addresses",
        ];
        if object.len() != expected_paths.len() + other_fields.len()
            || object.keys().any(|field| {
                !other_fields.contains(&field.as_str())
                    && !expected_paths
                        .iter()
                        .any(|(known, _, _)| field.as_str() == *known)
            })
            || facts.paths.len() != expected_paths.len()
        {
            return Err(reject());
        }
        for (field, expected_path, kind) in &expected_paths {
            let mut matching = facts.paths.iter().filter(|fact| fact.field == *field);
            let fact = matching.next().ok_or_else(reject)?;
            let permissions = fact.mode & 0o7777;
            let mode_allowed = match *field {
                "postgres_executable" => permissions & 0o100 != 0 && permissions & 0o7022 == 0,
                "config_file" | "hba_file" => permissions == 0o600,
                _ => permissions == 0o700,
            };
            if matching.next().is_some()
                || object.get(*field).and_then(Value::as_str) != expected_path.to_str()
                || fact.canonical != *expected_path
                || fact.has_symlink_component
                || fact.uid != facts.current_uid
                || fact.kind != *kind
                || !mode_allowed
            {
                return Err(reject());
            }
        }
        let server = &facts.server;
        let hash = &server.executable_sha256;
        let system_identifier = &facts.offline_system_identifier;
        let numbers = [
            ("schema_version", 1),
            ("port", 5432),
            ("server_version_num", 180006),
            ("server_pid", u64::from(server.pid)),
            ("server_start_ticks", server.start_ticks),
            ("server_uid", u64::from(server.uid)),
            ("server_executable_dev", server.executable_dev),
            ("server_executable_inode", server.executable_inode),
        ];
        let strings = [
            ("os_user", "agent"),
            ("database_user", "agent"),
            ("admin_database", "postgres"),
            ("listen_addresses", ""),
            ("postgres_executable_sha256", hash.as_str()),
            ("offline_system_identifier", system_identifier.as_str()),
        ];
        if server.pid == 0
            || server.start_ticks == 0
            || server.uid != facts.current_uid
            || server.executable_dev == 0
            || server.executable_inode == 0
            || server.executable != root.join("install/bin/postgres")
            || hash.len() != 64
            || !hash.bytes().all(lower_hex)
            || !positive_decimal(system_identifier)
            || numbers.iter().any(|(field, expected)| {
                object.get(*field).and_then(Value::as_u64) != Some(*expected)
            })
            || strings.iter().any(|(field, expected)| {
                object.get(*field).and_then(Value::as_str) != Some(*expected)
            })
            || object
                .get("run_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_none_or(|id| id.is_nil())
        {
            return Err(reject());
        }
        Ok(OwnedPgScope { manifest })
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DatabaseIdentity {
        pub cluster_system_identifier: String,
        pub name: String,
        pub oid: u32,
        pub owner: String,
    }

    pub fn checked_drop_statement(
        expected: &DatabaseIdentity,
        observed: &DatabaseIdentity,
    ) -> Result<String, FixtureError> {
        if expected != observed || !valid_database_identity(expected) {
            return Err(FixtureError::IdentityMismatch);
        }
        Ok(format!("DROP DATABASE {}", expected.name))
    }

    fn lower_hex(byte: u8) -> bool {
        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
    }

    fn positive_decimal(value: &str) -> bool {
        !value.starts_with('0')
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|number| number > 0)
    }

    fn valid_database_identity(identity: &DatabaseIdentity) -> bool {
        identity.oid > 0
            && identity.owner == "agent"
            && positive_decimal(&identity.cluster_system_identifier)
            && identity
                .name
                .strip_prefix("p6_g07_")
                .and_then(|name| name.split_once('_'))
                .is_some_and(|(run, database)| {
                    run.len() == 16
                        && database.len() == 32
                        && run.bytes().all(lower_hex)
                        && database.bytes().all(lower_hex)
                })
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DdlOperation {
        Create,
        Drop,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DdlResponse {
        Confirmed,
        Unknown,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DatabaseResolution {
        Ready(DatabaseIdentity),
        Dropped,
        Unknown,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DdlDecision {
        pub resolution: DatabaseResolution,
        pub retry: bool,
        pub cleanup_target: Option<DatabaseIdentity>,
    }

    pub fn classify_ddl_response(
        operation: DdlOperation,
        response: DdlResponse,
        confirmed_identity: Option<&DatabaseIdentity>,
    ) -> DdlDecision {
        let identity = confirmed_identity.filter(|identity| valid_database_identity(identity));
        let (resolution, cleanup_target) = match (response, identity, operation) {
            (DdlResponse::Confirmed, Some(identity), DdlOperation::Create) => (
                DatabaseResolution::Ready(identity.clone()),
                Some(identity.clone()),
            ),
            (DdlResponse::Confirmed, Some(_), DdlOperation::Drop) => {
                (DatabaseResolution::Dropped, None)
            }
            _ => (DatabaseResolution::Unknown, None),
        };
        DdlDecision {
            resolution,
            retry: false,
            cleanup_target,
        }
    }

    // Keep the Linux-only adapters out of other callers' platform builds.
    #[cfg(target_os = "linux")]
    pub use runtime::*;

    #[cfg(target_os = "linux")]
    mod runtime {
        use super::*;
        use serde_json::json;
        use sha2::{Digest, Sha256};
        use sqlx::{
            Connection, PgConnection, PgPool, Row,
            postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
        };
        use std::{
            fs::{self, File, OpenOptions},
            future::Future,
            io::{Read, Write},
            os::unix::fs::{MetadataExt, OpenOptionsExt},
            time::{Duration, Instant},
        };

        // Linux O_NOFOLLOW, without adding libc or an unsafe syscall dependency.
        const NOFOLLOW: i32 = 0o400000;
        const DOCUMENT_CAP: u64 = 64 * 1024;
        const EXECUTABLE_CAP: u64 = 256 * 1024 * 1024;

        pub async fn bounded<T, E>(
            deadline: Instant,
            allowance: Duration,
            phase: &'static str,
            future: impl Future<Output = Result<T, E>>,
        ) -> Result<T, FixtureError> {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .map(|remaining| remaining.min(allowance))
                .filter(|remaining| !remaining.is_zero())
                .ok_or(FixtureError::Timeout(phase))?;
            tokio::time::timeout(remaining, future)
                .await
                .map_err(|_| FixtureError::Timeout(phase))?
                .map_err(|_| FixtureError::Unknown(phase))
        }

        fn no_symlink_components(path: &Path) -> Result<(), FixtureError> {
            if !path.is_absolute() {
                return Err(FixtureError::ConfigurationRejected);
            }
            let mut prefix = PathBuf::new();
            for component in path.components() {
                if matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                ) {
                    return Err(FixtureError::ConfigurationRejected);
                }
                prefix.push(component);
                let metadata = fs::symlink_metadata(&prefix)
                    .map_err(|_| FixtureError::ConfigurationRejected)?;
                if metadata.file_type().is_symlink() {
                    return Err(FixtureError::ConfigurationRejected);
                }
            }
            if fs::canonicalize(path).map_err(|_| FixtureError::ConfigurationRejected)? != path {
                return Err(FixtureError::ConfigurationRejected);
            }
            Ok(())
        }

        pub fn private_directory(path: &Path) -> Result<(), FixtureError> {
            no_symlink_components(path)?;
            let metadata =
                fs::symlink_metadata(path).map_err(|_| FixtureError::ConfigurationRejected)?;
            if !metadata.is_dir()
                || metadata.uid() != 1000
                || metadata.gid() != 1000
                || metadata.mode() & 0o7777 != 0o700
            {
                return Err(FixtureError::ConfigurationRejected);
            }
            Ok(())
        }

        fn private_file(path: &Path, write: bool, append: bool) -> Result<File, FixtureError> {
            no_symlink_components(path)?;
            let file = OpenOptions::new()
                .read(!write)
                .write(write)
                .append(append)
                .custom_flags(NOFOLLOW)
                .open(path)
                .map_err(|_| FixtureError::ConfigurationRejected)?;
            let metadata = file
                .metadata()
                .map_err(|_| FixtureError::ConfigurationRejected)?;
            if !metadata.is_file()
                || metadata.uid() != 1000
                || metadata.gid() != 1000
                || metadata.nlink() != 1
                || metadata.mode() & 0o7777 != 0o600
            {
                return Err(FixtureError::ConfigurationRejected);
            }
            Ok(file)
        }

        pub fn read_private(path: &Path, cap: u64) -> Result<Vec<u8>, FixtureError> {
            let file = private_file(path, false, false)?;
            if file
                .metadata()
                .map_err(|_| FixtureError::ConfigurationRejected)?
                .len()
                > cap
            {
                return Err(FixtureError::ConfigurationRejected);
            }
            let mut bytes = Vec::new();
            file.take(cap + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| FixtureError::Unknown("read_owned_file"))?;
            if bytes.len() as u64 > cap {
                return Err(FixtureError::ConfigurationRejected);
            }
            Ok(bytes)
        }

        pub fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), FixtureError> {
            private_directory(path.parent().ok_or(FixtureError::ConfigurationRejected)?)?;
            if bytes.len() > 1024 * 1024 {
                return Err(FixtureError::ConfigurationRejected);
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(NOFOLLOW)
                .open(path)
                .map_err(|_| FixtureError::Unknown("create_owned_receipt"))?;
            let metadata = file
                .metadata()
                .map_err(|_| FixtureError::Unknown("receipt_identity"))?;
            if !metadata.is_file()
                || metadata.uid() != 1000
                || metadata.gid() != 1000
                || metadata.nlink() != 1
                || metadata.mode() & 0o7777 != 0o600
            {
                return Err(FixtureError::IdentityMismatch);
            }
            file.write_all(bytes)
                .and_then(|()| file.sync_data())
                .map_err(|_| FixtureError::Unknown("write_owned_receipt"))
        }

        pub fn append_child_record(root: &Path, record: &Value) -> Result<(), FixtureError> {
            private_directory(root)?;
            private_directory(&root.join("receipts"))?;
            let mut bytes =
                serde_json::to_vec(record).map_err(|_| FixtureError::Unknown("registry_encode"))?;
            bytes.push(b'\n');
            if bytes.len() > 4096 {
                return Err(FixtureError::Unknown("registry_record_size"));
            }
            let mut file = private_file(&root.join("receipts/children.jsonl"), true, true)?;
            let count = file
                .write(&bytes)
                .map_err(|_| FixtureError::Unknown("registry_append"))?;
            if count != bytes.len() {
                return Err(FixtureError::Unknown("registry_short_write"));
            }
            file.sync_data()
                .map_err(|_| FixtureError::Unknown("registry_flush"))
        }

        pub fn bytes_sha256(bytes: &[u8]) -> String {
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }

        pub fn executable_identity(path: &Path) -> Result<(String, u64, u64), FixtureError> {
            no_symlink_components(path)?;
            let mut file = OpenOptions::new()
                .read(true)
                .custom_flags(NOFOLLOW)
                .open(path)
                .map_err(|_| FixtureError::IdentityMismatch)?;
            let metadata = file
                .metadata()
                .map_err(|_| FixtureError::IdentityMismatch)?;
            if !metadata.is_file()
                || metadata.uid() != 1000
                || metadata.gid() != 1000
                || metadata.mode() & 0o100 == 0
                || metadata.mode() & 0o7022 != 0
                || metadata.len() > EXECUTABLE_CAP
            {
                return Err(FixtureError::IdentityMismatch);
            }
            let mut hash = Sha256::new();
            let mut bytes = [0_u8; 64 * 1024];
            let mut observed = 0_u64;
            loop {
                let count = file
                    .read(&mut bytes)
                    .map_err(|_| FixtureError::IdentityMismatch)?;
                if count == 0 {
                    break;
                }
                observed += count as u64;
                if observed > EXECUTABLE_CAP {
                    return Err(FixtureError::IdentityMismatch);
                }
                hash.update(&bytes[..count]);
            }
            let after = file
                .metadata()
                .map_err(|_| FixtureError::IdentityMismatch)?;
            if observed != metadata.len()
                || metadata.len() != after.len()
                || metadata.dev() != after.dev()
                || metadata.ino() != after.ino()
                || metadata.mtime() != after.mtime()
                || metadata.mtime_nsec() != after.mtime_nsec()
            {
                return Err(FixtureError::IdentityMismatch);
            }
            Ok((
                hash.finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                metadata.dev(),
                metadata.ino(),
            ))
        }

        fn kernel_text(path: &Path) -> Result<String, FixtureError> {
            let file = File::open(path).map_err(|_| FixtureError::IdentityMismatch)?;
            let mut bytes = Vec::new();
            file.take(DOCUMENT_CAP + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| FixtureError::IdentityMismatch)?;
            if bytes.len() as u64 > DOCUMENT_CAP {
                return Err(FixtureError::IdentityMismatch);
            }
            String::from_utf8(bytes).map_err(|_| FixtureError::IdentityMismatch)
        }

        fn observed_uid(status: &str) -> Result<u32, FixtureError> {
            for field in ["Uid:", "Gid:"] {
                let mut lines = status.lines().filter(|line| line.starts_with(field));
                let line = lines.next().ok_or(FixtureError::IdentityMismatch)?;
                let ids = line
                    .split_whitespace()
                    .skip(1)
                    .map(str::parse::<u32>)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| FixtureError::IdentityMismatch)?;
                if lines.next().is_some() || ids != [1000; 4] {
                    return Err(FixtureError::IdentityMismatch);
                }
            }
            Ok(1000)
        }

        pub fn observe_process(
            pid: u32,
            executable: &Path,
        ) -> Result<ProcessIdentity, FixtureError> {
            let proc = PathBuf::from(format!("/proc/{pid}"));
            let stat = kernel_text(&proc.join("stat"))?;
            let start_ticks = parse_process_start_ticks(&stat, pid)?;
            let state = stat
                .rfind(") ")
                .and_then(|end| stat[end + 2..].split_whitespace().next())
                .ok_or(FixtureError::IdentityMismatch)?;
            if !matches!(state, "R" | "S" | "D" | "T" | "t" | "I") {
                return Err(FixtureError::IdentityMismatch);
            }
            let uid = observed_uid(&kernel_text(&proc.join("status"))?)?;
            if fs::read_link(proc.join("exe")).map_err(|_| FixtureError::IdentityMismatch)?
                != executable
            {
                return Err(FixtureError::IdentityMismatch);
            }
            let (executable_sha256, executable_dev, executable_inode) =
                executable_identity(executable)?;
            let live_exe =
                fs::metadata(proc.join("exe")).map_err(|_| FixtureError::IdentityMismatch)?;
            if live_exe.dev() != executable_dev
                || live_exe.ino() != executable_inode
                || parse_process_start_ticks(&kernel_text(&proc.join("stat"))?, pid)? != start_ticks
            {
                return Err(FixtureError::IdentityMismatch);
            }
            Ok(ProcessIdentity {
                pid,
                start_ticks,
                uid,
                executable: executable.to_owned(),
                executable_sha256,
                executable_dev,
                executable_inode,
            })
        }

        pub fn observed_monotonic_ns() -> Result<u64, FixtureError> {
            let document = kernel_text(Path::new("/proc/uptime"))?;
            let uptime = document
                .split_whitespace()
                .next()
                .ok_or(FixtureError::IdentityMismatch)?;
            let (seconds, fraction) = uptime
                .split_once('.')
                .ok_or(FixtureError::IdentityMismatch)?;
            if fraction.len() > 9
                || fraction.is_empty()
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(FixtureError::IdentityMismatch);
            }
            let seconds = seconds
                .parse::<u64>()
                .map_err(|_| FixtureError::IdentityMismatch)?;
            let fraction = format!("{fraction:0<9}")
                .parse::<u64>()
                .map_err(|_| FixtureError::IdentityMismatch)?;
            seconds
                .checked_mul(1_000_000_000)
                .and_then(|seconds| seconds.checked_add(fraction))
                .ok_or(FixtureError::IdentityMismatch)
        }

        impl OwnedPgScope {
            pub fn root(&self) -> PathBuf {
                PathBuf::from(self.manifest["run_root"].as_str().expect("validated root"))
            }
            pub fn path(&self) -> PathBuf {
                self.root().join("scope.json")
            }
            pub fn run_id(&self) -> Uuid {
                Uuid::parse_str(self.manifest["run_id"].as_str().expect("validated run"))
                    .expect("validated UUID")
            }
            pub fn system_identifier(&self) -> &str {
                self.manifest["offline_system_identifier"]
                    .as_str()
                    .expect("validated system ID")
            }

            pub fn from_environment() -> Result<Self, FixtureError> {
                let path = std::env::var_os("P6_G07_SCOPE_PATH")
                    .ok_or(FixtureError::ConfigurationRejected)?;
                Self::from_manifest(Path::new(&path))
            }

            pub fn from_manifest(path: &Path) -> Result<Self, FixtureError> {
                let root = path.parent().ok_or(FixtureError::ConfigurationRejected)?;
                let name = root
                    .file_name()
                    .and_then(OsStr::to_str)
                    .and_then(|name| name.strip_prefix("p6pg."))
                    .ok_or(FixtureError::ConfigurationRejected)?;
                if root.parent() != Some(Path::new("/workspace/scratch/13897606dfde"))
                    || name.len() != 8
                    || !name.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    || path != root.join("scope.json")
                {
                    return Err(FixtureError::ConfigurationRejected);
                }
                private_directory(root)?;
                private_directory(&root.join("receipts"))?;
                let document = String::from_utf8(read_private(path, DOCUMENT_CAP)?)
                    .map_err(|_| FixtureError::ConfigurationRejected)?;
                let manifest: Value = serde_json::from_str(&document)
                    .map_err(|_| FixtureError::ConfigurationRejected)?;
                let current_uid = observed_uid(&kernel_text(Path::new("/proc/self/status"))?)?;
                let passwd = kernel_text(Path::new("/etc/passwd"))?;
                let users = passwd
                    .lines()
                    .map(|line| line.split(':').collect::<Vec<_>>())
                    .filter(|fields| fields.len() == 7 && fields[2] == "1000")
                    .collect::<Vec<_>>();
                if users.len() != 1 || users[0][0] != "agent" || users[0][3] != "1000" {
                    return Err(FixtureError::IdentityMismatch);
                }
                let fields = [
                    ("run_root", root.to_owned()),
                    ("data_dir", root.join("data")),
                    ("socket_dir", root.join("socket")),
                    ("home_dir", root.join("home")),
                    ("tmp_dir", root.join("tmp")),
                    ("config_file", root.join("data/postgresql.conf")),
                    ("hba_file", root.join("data/pg_hba.conf")),
                    ("postgres_executable", root.join("install/bin/postgres")),
                ];
                let mut paths = Vec::new();
                for (field, owned_path) in &fields {
                    no_symlink_components(owned_path)?;
                    let metadata = fs::symlink_metadata(owned_path)
                        .map_err(|_| FixtureError::ConfigurationRejected)?;
                    if metadata.gid() != 1000 {
                        return Err(FixtureError::ConfigurationRejected);
                    }
                    let kind = if metadata.is_dir() {
                        PathKind::Directory
                    } else if metadata.is_file() {
                        PathKind::RegularFile
                    } else {
                        return Err(FixtureError::ConfigurationRejected);
                    };
                    paths.push(PathFact {
                        field,
                        canonical: fs::canonicalize(owned_path)
                            .map_err(|_| FixtureError::ConfigurationRejected)?,
                        has_symlink_component: false,
                        uid: metadata.uid(),
                        mode: metadata.mode(),
                        kind,
                    });
                }
                let pid = manifest["server_pid"]
                    .as_u64()
                    .and_then(|pid| u32::try_from(pid).ok())
                    .ok_or(FixtureError::ConfigurationRejected)?;
                let server = observe_process(pid, &root.join("install/bin/postgres"))?;
                let offline_system_identifier = read_control_receipt(root, &manifest)?;
                let facts = OwnedScopeFacts {
                    expected_run_root: root.to_owned(),
                    current_uid,
                    current_os_user: "agent".to_string(),
                    paths,
                    server,
                    offline_system_identifier,
                };
                let scope = validate_owned_scope(
                    Some(path.as_os_str()),
                    &document,
                    &facts,
                    &std::env::vars_os().collect::<Vec<_>>(),
                )?;
                scope.verify_postmaster_and_hba()?;
                Ok(scope)
            }

            pub fn revalidate(&self) -> Result<(), FixtureError> {
                if Self::from_manifest(&self.path())? != *self {
                    return Err(FixtureError::IdentityMismatch);
                }
                Ok(())
            }

            fn verify_postmaster_and_hba(&self) -> Result<(), FixtureError> {
                let root = self.root();
                let pid = String::from_utf8(read_private(
                    &root.join("data/postmaster.pid"),
                    DOCUMENT_CAP,
                )?)
                .map_err(|_| FixtureError::IdentityMismatch)?;
                let lines = pid.lines().collect::<Vec<_>>();
                if lines.len() < 8
                    || lines[0].parse::<u64>().ok() != self.manifest["server_pid"].as_u64()
                    || lines[1]
                        != self.manifest["data_dir"]
                            .as_str()
                            .ok_or(FixtureError::IdentityMismatch)?
                    || lines[3] != "5432"
                    || lines[4]
                        != self.manifest["socket_dir"]
                            .as_str()
                            .ok_or(FixtureError::IdentityMismatch)?
                    || !lines[5].is_empty()
                    || lines[7].trim() != "ready"
                {
                    return Err(FixtureError::IdentityMismatch);
                }
                let hba =
                    String::from_utf8(read_private(&root.join("data/pg_hba.conf"), DOCUMENT_CAP)?)
                        .map_err(|_| FixtureError::IdentityMismatch)?;
                let actual = hba
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                    .collect::<std::collections::HashSet<_>>();
                let expected = [
                    "local all all peer",
                    "host all all 127.0.0.1/32 reject",
                    "host all all ::1/128 reject",
                    "local replication all peer",
                    "host replication all 127.0.0.1/32 reject",
                    "host replication all ::1/128 reject",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect::<std::collections::HashSet<_>>();
                if actual != expected {
                    return Err(FixtureError::IdentityMismatch);
                }
                Ok(())
            }

            pub fn options(&self, database: &str) -> Result<PgConnectOptions, FixtureError> {
                if std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("PG")) {
                    return Err(FixtureError::ConfigurationRejected);
                }
                if database != "postgres" {
                    let identity = DatabaseIdentity {
                        cluster_system_identifier: self.system_identifier().to_owned(),
                        name: database.to_owned(),
                        oid: 1,
                        owner: "agent".to_owned(),
                    };
                    let prefix = format!("p6_g07_{}_", &self.run_id().simple().to_string()[..16]);
                    if !valid_database_identity(&identity) || !database.starts_with(&prefix) {
                        return Err(FixtureError::ConfigurationRejected);
                    }
                }
                let socket = self.root().join("socket");
                // This constructor alone skips pgpass. Every PG* name was rejected
                // before it; all endpoint/identity fields are explicitly replaced.
                Ok(PgConnectOptions::new_without_pgpass()
                    .host(socket.to_str().ok_or(FixtureError::ConfigurationRejected)?)
                    .socket(&socket)
                    .port(5432)
                    .username("agent")
                    .database(database)
                    .ssl_mode(PgSslMode::Disable)
                    .statement_cache_capacity(0))
            }

            async fn admin(&self, deadline: Instant) -> Result<PgConnection, FixtureError> {
                self.revalidate()?;
                let mut admin = bounded(
                    deadline,
                    Duration::from_secs(3),
                    "admin_connect",
                    PgConnection::connect_with(&self.options("postgres")?),
                )
                .await?;
                if let Err(error) = self.verify_live(&mut admin, "postgres", deadline).await {
                    let _ = bounded(
                        deadline,
                        Duration::from_secs(3),
                        "rejected_admin_close",
                        admin.close(),
                    )
                    .await;
                    return Err(error);
                }
                Ok(admin)
            }

            pub async fn verify_live(
                &self,
                connection: &mut PgConnection,
                database: &str,
                deadline: Instant,
            ) -> Result<(), FixtureError> {
                let row = bounded(deadline, Duration::from_secs(5), "live_cluster_identity", sqlx::query(
                "SELECT current_setting('server_version_num') AS version, current_user::text AS role, \
                 current_database() AS database, current_setting('data_directory') AS data, \
                 current_setting('config_file') AS config, current_setting('hba_file') AS hba, \
                 current_setting('listen_addresses') AS listen, current_setting('unix_socket_directories') AS socket, \
                 current_setting('port') AS port, current_setting('max_connections') AS max_connections, \
                 current_setting('shared_buffers') AS shared_buffers, current_setting('work_mem') AS work_mem, \
                 current_setting('fsync') AS fsync, current_setting('full_page_writes') AS full_page_writes, \
                 current_setting('synchronous_commit') AS synchronous_commit, \
                 (pg_control_system()).system_identifier::text AS system_identifier")
                .fetch_one(&mut *connection)).await?;
                let expected = [
                    ("version", "180006"),
                    ("role", "agent"),
                    ("database", database),
                    ("port", "5432"),
                    ("listen", ""),
                    ("max_connections", "16"),
                    ("shared_buffers", "32MB"),
                    ("work_mem", "4MB"),
                    ("fsync", "on"),
                    ("full_page_writes", "on"),
                    ("synchronous_commit", "on"),
                    ("system_identifier", self.system_identifier()),
                    (
                        "data",
                        self.manifest["data_dir"]
                            .as_str()
                            .ok_or(FixtureError::IdentityMismatch)?,
                    ),
                    (
                        "config",
                        self.manifest["config_file"]
                            .as_str()
                            .ok_or(FixtureError::IdentityMismatch)?,
                    ),
                    (
                        "hba",
                        self.manifest["hba_file"]
                            .as_str()
                            .ok_or(FixtureError::IdentityMismatch)?,
                    ),
                    (
                        "socket",
                        self.manifest["socket_dir"]
                            .as_str()
                            .ok_or(FixtureError::IdentityMismatch)?,
                    ),
                ];
                for (field, expected) in expected {
                    if row
                        .try_get::<String, _>(field)
                        .map_err(|_| FixtureError::Unknown("identity_decode"))?
                        != expected
                    {
                        return Err(FixtureError::IdentityMismatch);
                    }
                }
                self.revalidate()
            }
        }

        fn read_control_receipt(root: &Path, manifest: &Value) -> Result<String, FixtureError> {
            let document = read_private(&root.join("receipts/cluster-control.json"), DOCUMENT_CAP)?;
            let receipt: Value = serde_json::from_slice(&document)
                .map_err(|_| FixtureError::ConfigurationRejected)?;
            let fields = [
                "schema_version",
                "run_id",
                "run_root",
                "data_dir",
                "offline_system_identifier",
                "owner_uid",
                "observed_before_server_spawn",
                "executable",
                "executable_sha256",
                "executable_dev",
                "executable_inode",
                "argv",
                "actual_exit_code",
                "stdout_path",
                "stdout_sha256",
            ];
            let object = receipt
                .as_object()
                .ok_or(FixtureError::ConfigurationRejected)?;
            if object.len() != fields.len()
                || object.keys().any(|key| !fields.contains(&key.as_str()))
                || receipt["schema_version"] != json!(1)
                || receipt["run_id"] != manifest["run_id"]
                || receipt["run_root"] != manifest["run_root"]
                || receipt["data_dir"] != manifest["data_dir"]
                || receipt["owner_uid"] != json!(1000)
                || receipt["observed_before_server_spawn"] != json!(true)
            {
                return Err(FixtureError::IdentityMismatch);
            }
            let pg = &receipt;
            let executable = root.join("install/bin/pg_controldata");
            let (hash, dev, inode) = executable_identity(&executable)?;
            if pg["executable"].as_str() != executable.to_str()
                || pg["executable_sha256"] != json!(hash)
                || pg["executable_dev"] != json!(dev)
                || pg["executable_inode"] != json!(inode)
                || pg["argv"]
                    != json!([
                        executable
                            .to_str()
                            .ok_or(FixtureError::ConfigurationRejected)?,
                        "-D",
                        root.join("data")
                            .to_str()
                            .ok_or(FixtureError::ConfigurationRejected)?
                    ])
                || pg["actual_exit_code"] != json!(0)
            {
                return Err(FixtureError::IdentityMismatch);
            }
            let stdout_path = PathBuf::from(
                pg["stdout_path"]
                    .as_str()
                    .ok_or(FixtureError::ConfigurationRejected)?,
            );
            if stdout_path != root.join("logs/offline-control.stdout.log") {
                return Err(FixtureError::ConfigurationRejected);
            }
            private_directory(&root.join("logs"))?;
            let stdout = read_private(&stdout_path, DOCUMENT_CAP)?;
            if pg["stdout_sha256"] != json!(bytes_sha256(&stdout)) {
                return Err(FixtureError::IdentityMismatch);
            }
            let stdout = String::from_utf8(stdout).map_err(|_| FixtureError::IdentityMismatch)?;
            let identifiers = stdout
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("Database system identifier:")
                        .map(str::trim)
                })
                .collect::<Vec<_>>();
            if identifiers.len() != 1
                || !positive_decimal(identifiers[0])
                || receipt["offline_system_identifier"] != json!(identifiers[0])
            {
                return Err(FixtureError::IdentityMismatch);
            }
            Ok(identifiers[0].to_owned())
        }

        pub struct OwnedDatabase {
            pub scope: OwnedPgScope,
            pub name: String,
            pub identity: Option<DatabaseIdentity>,
            pub pool: Option<PgPool>,
            pub create: &'static str,
            pub cleanup: &'static str,
            pub observations: Vec<Value>,
        }

        impl OwnedDatabase {
            pub fn new(scope: OwnedPgScope) -> Self {
                let name = format!(
                    "p6_g07_{}_{}",
                    &scope.run_id().simple().to_string()[..16],
                    Uuid::now_v7().simple()
                );
                Self {
                    scope,
                    name,
                    identity: None,
                    pool: None,
                    create: "not_attempted",
                    cleanup: "not_attempted",
                    observations: Vec::new(),
                }
            }

            pub async fn create_and_migrate(
                &mut self,
                launch: Uuid,
                case: &str,
                deadline: Instant,
            ) -> Result<(), FixtureError> {
                let mut admin = self.scope.admin(deadline).await?;
                self.scope.revalidate()?;
                let statement = format!(
                    "CREATE DATABASE {} TEMPLATE template0 ENCODING 'UTF8'",
                    self.name
                );
                self.create = "unknown";
                let response = bounded(
                    deadline,
                    Duration::from_secs(10),
                    "create_database",
                    sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(&mut admin),
                )
                .await;
                if let Err(error) = response {
                    let decision =
                        classify_ddl_response(DdlOperation::Create, DdlResponse::Unknown, None);
                    debug_assert_eq!(decision.resolution, DatabaseResolution::Unknown);
                    let _ = bounded(
                        deadline,
                        Duration::from_secs(3),
                        "unknown_create_admin_close",
                        admin.close(),
                    )
                    .await;
                    return Err(error);
                }
                let (identity, observed_at) =
                    catalog_identity(&mut admin, &self.scope, &self.name, deadline).await?;
                let decision = classify_ddl_response(
                    DdlOperation::Create,
                    DdlResponse::Confirmed,
                    Some(&identity),
                );
                let DatabaseResolution::Ready(identity) = decision.resolution else {
                    return Err(FixtureError::Unknown("create_identity"));
                };
                self.identity = Some(identity.clone());
                self.create = "confirmed";
                let observation = json!({"schema_version":1,"run_id":self.scope.run_id().to_string(),
                "test_launch_id":launch.to_string(),"case":case,"create":"confirmed",
                "database":{"cluster_system_identifier":identity.cluster_system_identifier,"name":identity.name,"oid":identity.oid,"owner":identity.owner}});
                write_new_private(
                    &self
                        .scope
                        .root()
                        .join(format!("receipts/observations-{launch}.json")),
                    &serde_json::to_vec(&observation)
                        .map_err(|_| FixtureError::Unknown("creation_receipt_encode"))?,
                )?;
                self.observations.push(observation);
                self.observations
                    .push(json!({"event":"confirmed_create_catalog","database_clock":observed_at}));
                bounded(
                    deadline,
                    Duration::from_secs(3),
                    "created_admin_close",
                    admin.close(),
                )
                .await?;
                let pool = bounded(
                    deadline,
                    Duration::from_secs(3),
                    "database_connect",
                    PgPoolOptions::new()
                        .max_connections(4)
                        .acquire_timeout(Duration::from_secs(3))
                        .connect_with(self.scope.options(&self.name)?),
                )
                .await?;
                self.pool = Some(pool);
                verify_pool_identity(
                    &self.scope,
                    self.pool
                        .as_ref()
                        .ok_or(FixtureError::Unknown("owned_pool"))?,
                    self.identity
                        .as_ref()
                        .ok_or(FixtureError::Unknown("owned_identity"))?,
                    deadline,
                )
                .await?;
                bounded(
                    deadline,
                    Duration::from_secs(30),
                    "domain_migrations_0001_0009",
                    document_repository_postgres::migrate(
                        self.pool
                            .as_ref()
                            .ok_or(FixtureError::Unknown("owned_pool"))?,
                    ),
                )
                .await?;
                Ok(())
            }

            pub async fn close(
                &mut self,
                deadline: Instant,
                children_reaped: bool,
            ) -> Result<(), FixtureError> {
                self.cleanup = "unknown";
                if !children_reaped {
                    return Err(FixtureError::Unknown("children_not_reaped"));
                }
                if let Some(pool) = &self.pool {
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .map(|remaining| remaining.min(Duration::from_secs(3)))
                        .filter(|remaining| !remaining.is_zero())
                        .ok_or(FixtureError::Timeout("pool_close"))?;
                    tokio::time::timeout(remaining, pool.close())
                        .await
                        .map_err(|_| FixtureError::Timeout("pool_close"))?;
                }
                self.pool.take();
                let Some(expected) = self.identity.as_ref() else {
                    if self.create == "not_attempted" {
                        self.cleanup = "not_needed";
                        return Ok(());
                    }
                    return Err(FixtureError::Unknown("database_identity_unconfirmed"));
                };
                if self.create != "confirmed" {
                    return Err(FixtureError::Unknown("create_unconfirmed"));
                }
                let mut admin = self.scope.admin(deadline).await?;
                let (observed, observed_at) =
                    catalog_identity(&mut admin, &self.scope, &self.name, deadline).await?;
                self.observations.push(
                    json!({"event":"pre_drop_catalog","database_clock":observed_at,
                "name":observed.name,"oid":observed.oid,"owner":observed.owner}),
                );
                let statement = checked_drop_statement(expected, &observed)?;
                self.scope.revalidate()?;
                let response = bounded(
                    deadline,
                    Duration::from_secs(5),
                    "drop_database",
                    sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(&mut admin),
                )
                .await;
                if let Err(error) = response {
                    let decision = classify_ddl_response(
                        DdlOperation::Drop,
                        DdlResponse::Unknown,
                        Some(expected),
                    );
                    debug_assert_eq!(decision.resolution, DatabaseResolution::Unknown);
                    let _ = bounded(
                        deadline,
                        Duration::from_secs(3),
                        "unknown_drop_admin_close",
                        admin.close(),
                    )
                    .await;
                    return Err(error);
                }
                let exists: bool = bounded(
                deadline,
                Duration::from_secs(5),
                "drop_absence_observation",
                sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1 OR oid::bigint=$2)",
                )
                .bind(&expected.name)
                .bind(i64::from(expected.oid))
                .fetch_one(&mut admin),
            )
            .await?;
                if exists {
                    return Err(FixtureError::IdentityMismatch);
                }
                bounded(
                    deadline,
                    Duration::from_secs(3),
                    "drop_admin_close",
                    admin.close(),
                )
                .await?;
                let decision = classify_ddl_response(
                    DdlOperation::Drop,
                    DdlResponse::Confirmed,
                    Some(expected),
                );
                if decision.resolution != DatabaseResolution::Dropped {
                    return Err(FixtureError::Unknown("drop_resolution"));
                }
                self.cleanup = "confirmed";
                Ok(())
            }

            pub fn receipt(&self) -> Value {
                json!({"cluster_system_identifier":self.scope.system_identifier(),"name":self.name,
                "oid":self.identity.as_ref().map(|identity| identity.oid),"owner":self.identity.as_ref().map(|identity| identity.owner.as_str()),
                "create":self.create,"cleanup":self.cleanup})
            }
        }

        impl Drop for OwnedDatabase {
            fn drop(&mut self) {
                // No networked destructor or implicit DDL. If explicit close could
                // not finish, retain resources for the launcher-owned cluster stop.
                if let Some(pool) = self.pool.take() {
                    std::mem::forget(pool);
                }
                if self.create != "not_attempted" && self.cleanup != "confirmed" {
                    eprintln!(
                        "G07 owned database retained: {} ({}/{})",
                        self.name, self.create, self.cleanup
                    );
                }
            }
        }

        pub async fn catalog_identity(
            admin: &mut PgConnection,
            scope: &OwnedPgScope,
            name: &str,
            deadline: Instant,
        ) -> Result<(DatabaseIdentity, String), FixtureError> {
            let row = bounded(deadline, Duration::from_secs(5), "database_catalog_identity",
            sqlx::query("SELECT oid::bigint AS oid, pg_get_userbyid(datdba)::text AS owner, clock_timestamp()::text AS observed_at FROM pg_database WHERE datname=$1")
                .bind(name).fetch_optional(&mut *admin)).await?.ok_or(FixtureError::IdentityMismatch)?;
            let identity = DatabaseIdentity {
                cluster_system_identifier: scope.system_identifier().to_owned(),
                name: name.to_owned(),
                oid: u32::try_from(
                    row.try_get::<i64, _>("oid")
                        .map_err(|_| FixtureError::Unknown("database_oid_decode"))?,
                )
                .map_err(|_| FixtureError::IdentityMismatch)?,
                owner: row
                    .try_get("owner")
                    .map_err(|_| FixtureError::Unknown("database_owner_decode"))?,
            };
            if !valid_database_identity(&identity) {
                return Err(FixtureError::IdentityMismatch);
            }
            let observed_at = row
                .try_get("observed_at")
                .map_err(|_| FixtureError::Unknown("catalog_clock_decode"))?;
            Ok((identity, observed_at))
        }

        async fn verify_pool_identity(
            scope: &OwnedPgScope,
            pool: &PgPool,
            expected: &DatabaseIdentity,
            deadline: Instant,
        ) -> Result<(), FixtureError> {
            let mut connection = bounded(
                deadline,
                Duration::from_secs(3),
                "pool_identity_acquire",
                pool.acquire(),
            )
            .await?;
            scope
                .verify_live(&mut connection, &expected.name, deadline)
                .await?;
            let (actual, _) =
                catalog_identity(&mut connection, scope, &expected.name, deadline).await?;
            checked_drop_statement(expected, &actual)?;
            Ok(())
        }

        pub async fn child_pool(
            scope: &OwnedPgScope,
            expected: &DatabaseIdentity,
            deadline: Instant,
        ) -> Result<PgPool, FixtureError> {
            let mut admin = scope.admin(deadline).await?;
            let (actual, _) = catalog_identity(&mut admin, scope, &expected.name, deadline).await?;
            checked_drop_statement(expected, &actual)?; // identity check only; no DROP is executed
            bounded(
                deadline,
                Duration::from_secs(3),
                "child_admin_close",
                admin.close(),
            )
            .await?;
            let pool = bounded(
                deadline,
                Duration::from_secs(3),
                "child_database_connect",
                PgPoolOptions::new()
                    .max_connections(1)
                    .acquire_timeout(Duration::from_secs(3))
                    .connect_with(scope.options(&expected.name)?),
            )
            .await?;
            verify_pool_identity(scope, &pool, expected, deadline).await?;
            Ok(pool)
        }
    }
}
