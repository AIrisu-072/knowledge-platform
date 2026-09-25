use std::{
    io::{Read, stderr, stdin, stdout},
    process::exit,
};

use document_semantic_inspection_worker::{
    WorkerFailure, WorkerFailureCode, open_inherited_input, run_worker_shell, write_failure,
};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;

fn main() {
    let input_fd = match std::env::var("DSI_INPUT_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
    {
        Some(fd) => fd,
        None => {
            let failure = WorkerFailure::new(
                WorkerFailureCode::ExtractorUnavailable,
                "DSI_INPUT_FD must identify the inherited read-only input descriptor",
            );
            let mut stderr = stderr().lock();
            write_failure(&mut stderr, &failure);
            exit(69);
        }
    };

    let mut input = match open_inherited_input(input_fd) {
        Ok(input) => input,
        Err(failure) => {
            let mut stderr = stderr().lock();
            write_failure(&mut stderr, &failure);
            exit(69);
        }
    };

    let mut request_bytes = Vec::with_capacity(MAX_REQUEST_BYTES.min(8 * 1024));
    if let Err(error) = stdin()
        .lock()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut request_bytes)
    {
        let failure = WorkerFailure::new(
            WorkerFailureCode::MalformedRequest,
            format!("failed to read worker request: {error}"),
        );
        let mut stderr = stderr().lock();
        write_failure(&mut stderr, &failure);
        exit(65);
    }

    let mut stdout = stdout().lock();
    let mut stderr = stderr().lock();
    let code = run_worker_shell(
        &request_bytes,
        &mut input,
        &mut stdout,
        &mut stderr,
        MAX_REQUEST_BYTES,
        MAX_INPUT_BYTES,
    );
    exit(code);
}
