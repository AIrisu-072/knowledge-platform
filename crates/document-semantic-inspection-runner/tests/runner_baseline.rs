use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::fd::AsRawFd,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};

use document_semantic_inspection_core::decode_worker_response_bounded;

fn synthetic_worker() -> &'static str {
    env!("CARGO_BIN_EXE_dsi-runner-hostile-worker")
}

#[test]
fn synthetic_worker_fixture_starts_and_emits_a_valid_shaped_response() {
    let request = r#"{"protocol_version":"dsi-worker-v0","inspection_profile_version":"dsi-v0","declared_media_type":"text/plain","expected_raw_content_hash":[61,175,105,143,24,37,217,20,236,52,172,159,15,171,246,231,227,50,120,44,179,226,28,95,160,0,214,106,238,91,41,29],"expected_size_bytes":12,"trace_context":{"traceparent":"00-00000000000000000000000000000001-0000000000000001-01","tracestate":"dsi-test=baseline"}}"#;
    let input_path =
        std::env::temp_dir().join(format!("dsi-runner-baseline-{}", std::process::id()));
    fs::write(&input_path, b"input-vector").expect("create synthetic input");
    let input = OpenOptions::new()
        .read(true)
        .open(&input_path)
        .expect("open input read-only");
    let input_fd = input.as_raw_fd();
    let mut command = Command::new(synthetic_worker());
    command
        .env_clear()
        .env("DSI_INPUT_FD", "3")
        .env("DSI_TEST_SKIP_SANDBOX_SEAL", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        command.pre_exec(move || {
            unsafe extern "C" {
                fn dup2(oldfd: i32, newfd: i32) -> i32;
                fn fcntl(fd: i32, cmd: i32, ...) -> i32;
            }
            const F_GETFD: i32 = 1;
            const F_SETFD: i32 = 2;
            const FD_CLOEXEC: i32 = 1;

            if dup2(input_fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let flags = fcntl(3, F_GETFD);
            if flags < 0 || fcntl(3, F_SETFD, flags & !FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn synthetic worker fixture");
    child
        .stdin
        .take()
        .expect("fixture stdin")
        .write_all(request.as_bytes())
        .expect("write fixture request");
    let output = child.wait_with_output().expect("wait for fixture");
    drop(input);
    fs::remove_file(input_path).expect("remove synthetic input");

    assert!(
        output.status.success(),
        "fixture exited with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let response = decode_worker_response_bounded(&output.stdout, 16 * 1024 * 1024)
        .expect("synthetic fixture emits a valid WorkerResponse");
    assert_eq!(response.extractor_provenance.adapter_id, "dsi-runner-test");
    assert_eq!(response.diagnostics[0].code, "synthetic_worker_pid");
}
