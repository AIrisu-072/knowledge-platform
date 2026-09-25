use std::{
    fs,
    path::{Path, PathBuf},
    process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use document_semantic_inspection_sandbox_preflight::{
    SandboxDisposition, SandboxLaunch, SandboxPolicy, run_sandboxed,
};

fn probe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_dsi-sandbox-probe"))
}

fn fresh_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "dsi-sandbox-{label}-{}-{nonce}",
        process::id()
    ));
    fs::create_dir_all(&path).expect("create temp test directory");
    path
}

fn test_policy(read: Vec<PathBuf>, write: Vec<PathBuf>) -> SandboxPolicy {
    SandboxPolicy::test_profile(read, write)
}

#[test]
fn network_syscalls_are_denied_for_tcp_and_udp() {
    let root = fresh_dir("network");
    let policy = test_policy(Vec::new(), vec![root]);

    for action in ["tcp-socket", "udp-socket"] {
        let outcome = run_sandboxed(
            probe(),
            &policy,
            &SandboxLaunch::new(action),
        )
        .expect("sandbox launch");
        assert!(
            matches!(outcome.disposition(), SandboxDisposition::Denied),
            "{action} unexpectedly escaped sandbox: {outcome:?}"
        );
    }
}

#[test]
fn filesystem_is_confined_to_explicit_read_and_write_surface() {
    let root = fresh_dir("filesystem");
    let allowed_read = root.join("allowed-read.txt");
    let allowed_write_dir = root.join("private");
    let outside = fresh_dir("outside");
    let outside_read = outside.join("outside-read.txt");
    let outside_write = outside.join("outside-write.txt");

    fs::write(&allowed_read, b"allowed").expect("allowed read fixture");
    fs::write(&outside_read, b"outside").expect("outside read fixture");
    fs::create_dir_all(&allowed_write_dir).expect("allowed write dir");

    let policy = test_policy(vec![allowed_read.clone()], vec![allowed_write_dir.clone()]);

    let allowed_read_result = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("read").arg(allowed_read.display().to_string()),
    )
    .expect("allowed read launch");
    assert_eq!(allowed_read_result.disposition(), &SandboxDisposition::Allowed);

    let allowed_write_result = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("write").arg(
            allowed_write_dir.join("result.txt").display().to_string(),
        ),
    )
    .expect("allowed write launch");
    assert_eq!(allowed_write_result.disposition(), &SandboxDisposition::Allowed);

    for launch in [
        SandboxLaunch::new("read").arg(outside_read.display().to_string()),
        SandboxLaunch::new("write").arg(outside_write.display().to_string()),
    ] {
        let outcome = run_sandboxed(probe(), &policy, &launch).expect("sandbox launch");
        assert_eq!(outcome.disposition(), &SandboxDisposition::Denied);
    }
}

#[test]
fn credential_like_environment_is_not_inherited() {
    let root = fresh_dir("environment");
    let policy = test_policy(Vec::new(), vec![root]);
    let secrets = [
        ("DATABASE_URL", "postgres://synthetic:not-a-secret@example.invalid/db"),
        ("AWS_SECRET_ACCESS_KEY", "synthetic-test-secret"),
        ("DSI_TEST_STORAGE_CREDENTIAL", "synthetic-storage-secret"),
    ];

    for (key, value) in secrets {
        let outcome = run_sandboxed(
            probe(),
            &policy,
            &SandboxLaunch::new("env")
                .arg(key)
                .ambient_env(key, value),
        )
        .expect("sandbox launch");
        assert_eq!(
            outcome.disposition(),
            &SandboxDisposition::Denied,
            "{key} was inherited: {outcome:?}"
        );
    }
}

#[test]
fn cpu_memory_and_output_file_limits_are_enforced() {
    let root = fresh_dir("resources");
    let mut policy = test_policy(Vec::new(), vec![root.clone()]);
    policy.cpu_seconds = 1;
    policy.address_space_bytes = 256 * 1024 * 1024;
    policy.file_size_bytes = 64 * 1024;
    policy.wall_timeout = Duration::from_secs(3);

    let cpu = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("cpu-spin"),
    )
    .expect("cpu launch");
    assert!(cpu.disposition().is_resource_termination(), "{cpu:?}");

    let memory = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("alloc").arg((512usize * 1024 * 1024).to_string()),
    )
    .expect("memory launch");
    assert!(memory.disposition().is_denied_or_signaled(), "{memory:?}");

    let output = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("file-write")
            .arg(root.join("oversized.bin").display().to_string())
            .arg((1024usize * 1024).to_string()),
    )
    .expect("file launch");
    assert!(output.disposition().is_denied_or_signaled(), "{output:?}");
}

#[test]
fn wall_timeout_kills_the_sandboxed_process() {
    let root = fresh_dir("timeout");
    let mut policy = test_policy(Vec::new(), vec![root]);
    policy.wall_timeout = Duration::from_millis(250);
    policy.cpu_seconds = 8;

    let outcome = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("sleep").arg("5000"),
    )
    .expect("timeout launch");
    assert_eq!(outcome.disposition(), &SandboxDisposition::TimedOut);
}

#[test]
fn production_profile_denies_child_process_creation() {
    let root = fresh_dir("children-denied");
    let policy = test_policy(Vec::new(), vec![root]);

    let outcome = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("spawn-child"),
    )
    .expect("child launch");
    assert_eq!(outcome.disposition(), &SandboxDisposition::Denied);
}

#[test]
fn supervision_test_kills_the_entire_process_group() {
    let root = fresh_dir("process-tree");
    let mut policy = test_policy(Vec::new(), vec![root]);
    policy.wall_timeout = Duration::from_millis(400);

    let outcome = run_sandboxed(
        probe(),
        &policy,
        &SandboxLaunch::new("spawn-child-sleep").allow_children_for_supervision_test(),
    )
    .expect("process-tree launch");
    assert_eq!(outcome.disposition(), &SandboxDisposition::TimedOut);

    let child_pid = outcome
        .stdout()
        .lines()
        .find_map(|line| line.strip_prefix("child-pid="))
        .expect("child pid must be captured")
        .parse::<u32>()
        .expect("numeric child pid");

    let proc_path = PathBuf::from(format!("/proc/{child_pid}"));
    for _ in 0..50 {
        if !proc_path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("grandchild survived sandbox process-group termination: {child_pid}");
}
