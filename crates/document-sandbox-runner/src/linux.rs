use crate::{SandboxRunError, SandboxRunErrorKind};
use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
    RulesetStatus, path_beneath_rules,
};
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule, TargetArch};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn unavailable() -> SandboxRunError {
    SandboxRunError::new(
        SandboxRunErrorKind::Unavailable,
        "mandatory sandbox control unavailable",
    )
}

pub(super) fn seal() -> Result<(), SandboxRunError> {
    let dsi = env::var_os("DSI_SANDBOX_REQUIRED").as_deref() == Some(std::ffi::OsStr::new("1"));
    let search =
        env::var_os("SEARCH_SANDBOX_REQUIRED").as_deref() == Some(std::ffi::OsStr::new("1"));
    if dsi == search {
        return Err(unavailable());
    }
    let scratch = env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(unavailable)?;
    let scratch = fs::canonicalize(scratch).map_err(|_| unavailable())?;
    if !scratch.is_dir() {
        return Err(unavailable());
    }
    let thread_count = fs::read_dir("/proc/self/task")
        .map_err(|_| unavailable())?
        .try_fold(0usize, |count, entry| entry.map(|_| count + 1))
        .map_err(|_| unavailable())?;
    if thread_count != 1 {
        return Err(unavailable());
    }
    apply_landlock(&scratch)?;
    apply_seccomp()?;
    Ok(())
}

fn apply_landlock(scratch: &std::path::Path) -> Result<(), SandboxRunError> {
    let abi = ABI::V3;
    let status = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .map_err(|_| unavailable())?
        .create()
        .map_err(|_| unavailable())?
        .add_rules(path_beneath_rules([scratch], AccessFs::from_all(abi)))
        .map_err(|_| unavailable())?
        .set_compatibility(CompatLevel::HardRequirement)
        .restrict_self()
        .map_err(|_| unavailable())?;
    if status.ruleset != RulesetStatus::FullyEnforced {
        return Err(unavailable());
    }
    Ok(())
}

fn apply_seccomp() -> Result<(), SandboxRunError> {
    let mut denied: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
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
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_execve,
        libc::SYS_execveat,
    ] {
        denied.insert(syscall, Vec::new());
    }
    #[cfg(target_arch = "x86_64")]
    for syscall in [libc::SYS_fork, libc::SYS_vfork] {
        denied.insert(syscall, Vec::new());
    }
    let filter = SeccompFilter::new(
        denied,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        target_arch()?,
    )
    .map_err(|_| unavailable())?;
    let program: BpfProgram = filter.try_into().map_err(|_| unavailable())?;
    seccompiler::apply_filter(&program).map_err(|_| unavailable())
}

fn target_arch() -> Result<TargetArch, SandboxRunError> {
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
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64"
    )))]
    {
        Err(unavailable())
    }
}
