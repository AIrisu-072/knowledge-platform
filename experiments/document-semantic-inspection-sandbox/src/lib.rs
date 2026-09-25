//! Document Semantic Inspection v0 Task 1 sandbox preflight.
//!
//! This experiment qualifies the Linux primitives required by the frozen
//! production trust boundary before any sandbox dependency is promoted into
//! production crates.

#![cfg(target_os = "linux")]

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use landlock::{
    Access, AccessFs, CompatLevel, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, ABI,
    path_beneath_rules,
};
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule, TargetArch};
use thiserror::Error;

pub const TASK1_RED_CONTRACT_VERSION: &str = "dsi-sandbox-preflight-red-v1";
const DENIED_EXIT: i32 = 77;
const ENV_PREFIX: &str = "DSI_SANDBOX_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxDisposition {
    Allowed,
    Denied,
    TimedOut,
    Signaled(i32),
    Failed(i32),
}

impl SandboxDisposition {
    pub fn is_resource_termination(&self) -> bool {
        matches!(self, Self::TimedOut | Self::Signaled(_))
    }

    pub fn is_denied_or_signaled(&self) -> bool {
        matches!(self, Self::Denied | Self::TimedOut | Self::Signaled(_))
    }
}

#[derive(Debug, Clone)]
pub struct SandboxOutcome {
    disposition: SandboxDisposition,
    stdout: String,
    stderr: String,
}

impl SandboxOutcome {
    pub fn disposition(&self) -> &SandboxDisposition {
        &self.disposition
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }
}

#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub cpu_seconds: u64,
    pub address_space_bytes: u64,
    pub file_size_bytes: u64,
    pub wall_timeout: Duration,
}

impl SandboxPolicy {
    pub fn test_profile(read_paths: Vec<PathBuf>, write_paths: Vec<PathBuf>) -> Self {
        Self {
            read_paths,
            write_paths,
            cpu_seconds: 8,
            address_space_bytes: 2 * 1024 * 1024 * 1024,
            file_size_bytes: 2048 * 512,
            wall_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SandboxLaunch {
    action: OsString,
    args: Vec<OsString>,
    ambient_env: Vec<(OsString, OsString)>,
    allow_children: bool,
}

impl SandboxLaunch {
    pub fn new(action: impl Into<OsString>) -> Self {
        Self {
            action: action.into(),
            args: Vec::new(),
            ambient_env: Vec::new(),
            allow_children: false,
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn ambient_env(
        mut self,
        key: impl Into<OsString>,
        value: impl Into<OsString>,
    ) -> Self {
        self.ambient_env.push((key.into(), value.into()));
        self
    }

    pub fn allow_children_for_supervision_test(mut self) -> Self {
        self.allow_children = true;
        self
    }
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox spawn failed: {0}")]
    Spawn(#[source] io::Error),
    #[error("sandbox wait failed: {0}")]
    Wait(#[source] io::Error),
    #[error("sandbox configuration is invalid: {0}")]
    InvalidConfig(String),
    #[error("sandbox enforcement failed: {0}")]
    Enforcement(String),
}

pub fn run_sandboxed(
    probe: &Path,
    policy: &SandboxPolicy,
    launch: &SandboxLaunch,
) -> Result<SandboxOutcome, SandboxError> {
    let mut command = Command::new(probe);
    command
        .arg(&launch.action)
        .args(&launch.args)
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    // These values deliberately model a hostile ambient environment. The child
    // bootstrap must remove them before the probe action runs.
    for (key, value) in &launch.ambient_env {
        command.env(key, value);
    }

    encode_policy(&mut command, policy, launch.allow_children);

    let mut child = command.spawn().map_err(SandboxError::Spawn)?;
    let pid = child.id() as i32;
    let deadline = Instant::now() + policy.wall_timeout;
    let mut timed_out = false;

    loop {
        if child.try_wait().map_err(SandboxError::Wait)?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            // The child is its own process-group leader. Kill the entire group
            // so a malicious grandchild cannot survive the controller timeout.
            let rc = unsafe { libc::kill(-pid, libc::SIGKILL) };
            if rc != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(SandboxError::Wait(error));
                }
            }
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    let output = child.wait_with_output().map_err(SandboxError::Wait)?;
    let disposition = if timed_out {
        SandboxDisposition::TimedOut
    } else if output.status.success() {
        SandboxDisposition::Allowed
    } else if let Some(code) = output.status.code() {
        if code == DENIED_EXIT {
            SandboxDisposition::Denied
        } else {
            SandboxDisposition::Failed(code)
        }
    } else {
        use std::os::unix::process::ExitStatusExt;
        SandboxDisposition::Signaled(output.status.signal().unwrap_or_default())
    };

    Ok(SandboxOutcome {
        disposition,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn encode_policy(command: &mut Command, policy: &SandboxPolicy, allow_children: bool) {
    command
        .env(format!("{ENV_PREFIX}CPU_SECONDS"), policy.cpu_seconds.to_string())
        .env(
            format!("{ENV_PREFIX}ADDRESS_SPACE_BYTES"),
            policy.address_space_bytes.to_string(),
        )
        .env(
            format!("{ENV_PREFIX}FILE_SIZE_BYTES"),
            policy.file_size_bytes.to_string(),
        )
        .env(
            format!("{ENV_PREFIX}ALLOW_CHILDREN"),
            if allow_children { "1" } else { "0" },
        )
        .env(
            format!("{ENV_PREFIX}READ_COUNT"),
            policy.read_paths.len().to_string(),
        )
        .env(
            format!("{ENV_PREFIX}WRITE_COUNT"),
            policy.write_paths.len().to_string(),
        );

    for (index, path) in policy.read_paths.iter().enumerate() {
        command.env(format!("{ENV_PREFIX}READ_{index}"), path);
    }
    for (index, path) in policy.write_paths.iter().enumerate() {
        command.env(format!("{ENV_PREFIX}WRITE_{index}"), path);
    }
}

/// Apply the preflight sandbox to the current probe process.
///
/// This must be called immediately on process start, before any untrusted
/// parsing/action logic. It is public only so the synthetic probe can exercise
/// the exact child-side bootstrap.
pub fn bootstrap_current_process_from_env() -> Result<(), SandboxError> {
    let encoded = EncodedPolicy::from_env()?;
    clear_environment();
    apply_rlimits(&encoded)?;
    apply_landlock(&encoded)?;
    apply_seccomp(encoded.allow_children)?;
    Ok(())
}

#[derive(Debug)]
struct EncodedPolicy {
    read_paths: Vec<PathBuf>,
    write_paths: Vec<PathBuf>,
    cpu_seconds: u64,
    address_space_bytes: u64,
    file_size_bytes: u64,
    allow_children: bool,
}

impl EncodedPolicy {
    fn from_env() -> Result<Self, SandboxError> {
        Ok(Self {
            read_paths: read_indexed_paths("READ")?,
            write_paths: read_indexed_paths("WRITE")?,
            cpu_seconds: read_u64("CPU_SECONDS")?,
            address_space_bytes: read_u64("ADDRESS_SPACE_BYTES")?,
            file_size_bytes: read_u64("FILE_SIZE_BYTES")?,
            allow_children: match read_required("ALLOW_CHILDREN")?.to_string_lossy().as_ref() {
                "0" => false,
                "1" => true,
                other => {
                    return Err(SandboxError::InvalidConfig(format!(
                        "invalid ALLOW_CHILDREN value {other:?}"
                    )));
                }
            },
        })
    }
}

fn read_required(name: &str) -> Result<OsString, SandboxError> {
    std::env::var_os(format!("{ENV_PREFIX}{name}")).ok_or_else(|| {
        SandboxError::InvalidConfig(format!("missing {ENV_PREFIX}{name}"))
    })
}

fn read_u64(name: &str) -> Result<u64, SandboxError> {
    read_required(name)?
        .to_string_lossy()
        .parse()
        .map_err(|_| SandboxError::InvalidConfig(format!("invalid integer {name}")))
}

fn read_indexed_paths(kind: &str) -> Result<Vec<PathBuf>, SandboxError> {
    let count: usize = read_required(&format!("{kind}_COUNT"))?
        .to_string_lossy()
        .parse()
        .map_err(|_| SandboxError::InvalidConfig(format!("invalid {kind}_COUNT")))?;
    (0..count)
        .map(|index| {
            read_required(&format!("{kind}_{index}"))
                .map(PathBuf::from)
        })
        .collect()
}

fn clear_environment() {
    let keys: Vec<OsString> = std::env::vars_os().map(|(key, _)| key).collect();
    for key in keys {
        // SAFETY: the probe bootstrap runs before any threads are created.
        unsafe { std::env::remove_var(key) };
    }
}

fn apply_rlimits(policy: &EncodedPolicy) -> Result<(), SandboxError> {
    set_rlimit(libc::RLIMIT_CPU, policy.cpu_seconds)?;
    set_rlimit(libc::RLIMIT_AS, policy.address_space_bytes)?;
    set_rlimit(libc::RLIMIT_FSIZE, policy.file_size_bytes)?;
    Ok(())
}

fn set_rlimit(resource: libc::__rlimit_resource_t, value: u64) -> Result<(), SandboxError> {
    let value = libc::rlim_t::try_from(value)
        .map_err(|_| SandboxError::InvalidConfig("rlimit overflow".into()))?;
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    let rc = unsafe { libc::setrlimit(resource, &limit) };
    if rc == 0 {
        Ok(())
    } else {
        Err(SandboxError::Enforcement(format!(
            "setrlimit({resource}) failed: {}",
            io::Error::last_os_error()
        )))
    }
}

fn apply_landlock(policy: &EncodedPolicy) -> Result<(), SandboxError> {
    let abi = ABI::V9;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);

    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .map_err(|error| SandboxError::Enforcement(format!("landlock handle_access: {error}")))?
        .create()
        .map_err(|error| SandboxError::Enforcement(format!("landlock create: {error}")))?
        .add_rules(path_beneath_rules(
            policy.read_paths.iter().map(PathBuf::as_path),
            read_access,
        ))
        .map_err(|error| SandboxError::Enforcement(format!("landlock read rules: {error}")))?
        .add_rules(path_beneath_rules(
            policy.write_paths.iter().map(PathBuf::as_path),
            write_access,
        ))
        .map_err(|error| SandboxError::Enforcement(format!("landlock write rules: {error}")))?
        .set_compatibility(CompatLevel::HardRequirement)
        .restrict_self()
        .map_err(|error| SandboxError::Enforcement(format!("landlock restrict_self: {error}")))?;

    if status.ruleset != RulesetStatus::FullyEnforced {
        return Err(SandboxError::Enforcement(format!(
            "landlock was not fully enforced: {:?}",
            status.ruleset
        )));
    }
    Ok(())
}

fn apply_seccomp(allow_children: bool) -> Result<(), SandboxError> {
    let mut denied: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    let mut deny = |syscall: i64| {
        denied.insert(syscall, Vec::new());
    };

    for syscall in [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_recvfrom,
        libc::SYS_recvmsg,
        libc::SYS_shutdown,
    ] {
        deny(syscall);
    }

    if !allow_children {
        for syscall in [
            libc::SYS_clone,
            libc::SYS_clone3,
            libc::SYS_fork,
            libc::SYS_vfork,
        ] {
            deny(syscall);
        }
    }

    let arch = target_arch()?;
    let filter = SeccompFilter::new(
        denied,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )
    .map_err(|error| SandboxError::Enforcement(format!("seccomp compile: {error}")))?;
    let program: BpfProgram = filter
        .try_into()
        .map_err(|error| SandboxError::Enforcement(format!("seccomp BPF: {error}")))?;
    seccompiler::apply_filter(&program)
        .map_err(|error| SandboxError::Enforcement(format!("seccomp apply: {error}")))
}

fn target_arch() -> Result<TargetArch, SandboxError> {
    #[cfg(target_arch = "x86_64")]
    {
        Ok(TargetArch::x86_64)
    }
    #[cfg(target_arch = "aarch64")]
    {
        Ok(TargetArch::aarch64)
    }
    #[cfg(target_arch = "riscv64")]
    {
        Ok(TargetArch::riscv64)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
    {
        Err(SandboxError::Enforcement(
            "unsupported seccomp architecture".into(),
        ))
    }
}
