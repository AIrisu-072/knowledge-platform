use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::{self, Read, Write},
    process::{Command, exit},
    thread,
    time::Duration,
};

fn main() {
    let mut request = Vec::new();
    if io::stdin()
        .take(16_777_217)
        .read_to_end(&mut request)
        .is_err()
    {
        exit(90);
    }
    let raw = match std::env::var("SEARCH_INPUT_FD") {
        Ok(fd) if fd == "3" => fs::read("/proc/self/fd/3").unwrap_or_default(),
        _ => Vec::new(),
    };
    if raw.is_empty() || request.is_empty() {
        exit(91);
    }
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    if unsafe { fcntl(3, 3) } & 0o3 != 0 {
        exit(101);
    }
    if [
        "DATABASE_URL",
        "AWS_SECRET_ACCESS_KEY",
        "SEARCH_STORAGE_CREDENTIAL",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some())
    {
        exit(99);
    }
    if std::env::var_os("TMPDIR")
        .and_then(|path| fs::metadata(path).ok())
        .is_none_or(|metadata| metadata.permissions().mode() & 0o777 != 0o700)
    {
        exit(100);
    }
    let action = std::str::from_utf8(&raw).unwrap_or("");
    if action == "inherited-fd"
        && fs::read_link("/proc/self/fd/200").is_ok_and(|path| path.ends_with("secret-marker"))
    {
        exit(92);
    }
    if action == "no-marker" || action == "protocol-no-marker" {
        unsafe { std::env::remove_var("SEARCH_SANDBOX_REQUIRED") };
    }
    if document_sandbox_runner::seal_worker_sandbox().is_err() {
        if action == "protocol-no-marker" {
            eprintln!("SEARCH_SANDBOX_UNAVAILABLE");
            exit(78);
        }
        exit(93);
    }

    if action.starts_with("protocol-") {
        let _ = search_extraction_core::decode_request(&request).expect("bounded request");
        let response = match action {
            "protocol-report" => search_extraction_core::WorkerResponse::Report(
                search_extraction_core::WorkerReport {
                    coverage: search_extraction_core::BodyCoverage::Supported,
                    fragments: Vec::new(),
                    reader_use: Vec::new(),
                    scope_items: 0,
                    known_omissions: Vec::new(),
                    traversal_complete: true,
                },
            ),
            "protocol-resource" => search_extraction_core::WorkerResponse::Failure(
                search_extraction_core::ReaderFailure::Unsupported(
                    search_extraction_core::CoverageReason::ResourceLimit,
                ),
            ),
            "protocol-encrypted" => search_extraction_core::WorkerResponse::Failure(
                search_extraction_core::ReaderFailure::Unsupported(
                    search_extraction_core::CoverageReason::Encrypted,
                ),
            ),
            "protocol-exit-79" => exit(79),
            "protocol-permanent" => search_extraction_core::WorkerResponse::Failure(
                search_extraction_core::ReaderFailure::Permanent(
                    search_extraction_core::PermanentFailureCode::CorruptDocument,
                ),
            ),
            "protocol-retryable" => search_extraction_core::WorkerResponse::Failure(
                search_extraction_core::ReaderFailure::Retryable(
                    search_extraction_core::RetryableFailureCode::WorkerUnavailable,
                ),
            ),
            "protocol-partial" => search_extraction_core::WorkerResponse::Report(
                search_extraction_core::WorkerReport {
                    coverage: search_extraction_core::BodyCoverage::Supported,
                    fragments: Vec::new(),
                    reader_use: Vec::new(),
                    scope_items: 1,
                    known_omissions: Vec::new(),
                    traversal_complete: false,
                },
            ),
            "protocol-truncated" => {
                let bytes = search_extraction_core::encode_response(
                    &search_extraction_core::WorkerResponse::Report(
                        search_extraction_core::WorkerReport {
                            coverage: search_extraction_core::BodyCoverage::Supported,
                            fragments: Vec::new(),
                            reader_use: Vec::new(),
                            scope_items: 0,
                            known_omissions: Vec::new(),
                            traversal_complete: true,
                        },
                    ),
                )
                .expect("valid response");
                io::stdout().write_all(&bytes[..bytes.len() - 1]).unwrap();
                return;
            }
            _ => exit(98),
        };
        io::stdout()
            .write_all(&search_extraction_core::encode_response(&response).unwrap())
            .unwrap();
        return;
    }

    match action {
        "pid" => print!("{}", std::process::id()),
        "outside-read" => {
            if fs::read("/etc/passwd").is_ok() {
                exit(94);
            }
            print!("sealed");
        }
        "spawn" => {
            if Command::new("/bin/true").status().is_ok() {
                exit(95);
            }
            print!("sealed");
        }
        "network" => {
            if std::net::UdpSocket::bind("127.0.0.1:0").is_ok() {
                exit(96);
            }
            print!("sealed");
        }
        "inherited-fd" => print!("sealed"),
        "timeout" => thread::sleep(Duration::from_secs(30)),
        "stdout-overflow" => {
            let _ = io::stdout().write_all(&vec![b'x'; 16_777_217]);
        }
        "stderr-overflow" => {
            let _ = io::stderr().write_all(&vec![b'x'; 1_048_577]);
        }
        "scratch-overflow" => {
            let scratch = std::env::var_os("TMPDIR").expect("private scratch");
            for i in 0..540 {
                let file = fs::File::create(std::path::Path::new(&scratch).join(format!("{i}")))
                    .expect("scratch entry");
                file.set_len(2_000_000).expect("sparse scratch file");
            }
            print!("sealed");
        }
        "stderr-leak" => {
            eprint!("sensitive-body-marker");
            exit(97);
        }
        "kill" => {
            unsafe extern "C" {
                fn getpid() -> i32;
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // SIGKILL is 9 on supported Linux targets.
            unsafe { kill(getpid(), 9) };
        }
        "panic" => panic!("synthetic worker panic"),
        _ => exit(98),
    }
}
