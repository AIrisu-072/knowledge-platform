use std::{
    env,
    fs::{self, File},
    hint::black_box,
    io::Write,
    net::{TcpListener, ToSocketAddrs, UdpSocket},
    process,
    thread,
    time::Duration,
};

use document_semantic_inspection_sandbox_preflight::bootstrap_current_process_from_env;

const DENIED_EXIT: i32 = 77;

fn allowed(label: &str) -> ! {
    println!("allowed:{label}");
    process::exit(0);
}

fn denied(label: &str, error: impl std::fmt::Display) -> ! {
    println!("denied:{label}:{error}");
    process::exit(DENIED_EXIT);
}

fn main() {
    if let Err(error) = bootstrap_current_process_from_env() {
        eprintln!("sandbox-bootstrap-failed:{error}");
        process::exit(78);
    }

    let mut args = env::args().skip(1);
    let action = args.next().unwrap_or_default();

    match action.as_str() {
        "tcp-socket" => match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => {
                drop(listener);
                allowed("tcp-socket");
            }
            Err(error) => denied("tcp-socket", error),
        },
        "udp-socket" => match UdpSocket::bind("127.0.0.1:0") {
            Ok(socket) => {
                drop(socket);
                allowed("udp-socket");
            }
            Err(error) => denied("udp-socket", error),
        },
        "dns" => match ("dsi-sandbox-probe.invalid", 443).to_socket_addrs() {
            Ok(mut addresses) => match addresses.next() {
                Some(_) => allowed("dns"),
                None => denied("dns", "no address"),
            },
            Err(error) => denied("dns", error),
        },
        "pid" => {
            println!("pid={}", process::id());
            allowed("pid");
        },
        "read" => {
            let path = args.next().expect("read path");
            match fs::read(&path) {
                Ok(_) => allowed("read"),
                Err(error) => denied("read", error),
            }
        }
        "write" => {
            let path = args.next().expect("write path");
            match fs::write(&path, b"dsi-sandbox-probe") {
                Ok(()) => allowed("write"),
                Err(error) => denied("write", error),
            }
        }
        "env" => {
            let key = args.next().expect("environment key");
            match env::var_os(&key) {
                Some(value) => allowed(&format!("env:{}:{}", key, value.to_string_lossy().len())),
                None => denied("env", "absent"),
            }
        }
        "cpu-spin" => loop {
            black_box(1_u64.wrapping_add(1));
        },
        "alloc" => {
            let bytes: usize = args.next().expect("allocation bytes").parse().expect("usize");
            let mut data = Vec::<u8>::new();
            match data.try_reserve_exact(bytes) {
                Ok(()) => {
                    black_box(data.capacity());
                    allowed("alloc");
                }
                Err(error) => denied("alloc", error),
            }
        }
        "file-write" => {
            let path = args.next().expect("file path");
            let bytes: usize = args.next().expect("file bytes").parse().expect("usize");
            let mut file = match File::create(&path) {
                Ok(file) => file,
                Err(error) => denied("file-write-create", error),
            };
            let chunk = [0_u8; 4096];
            let mut written = 0usize;
            while written < bytes {
                let len = (bytes - written).min(chunk.len());
                if let Err(error) = file.write_all(&chunk[..len]) {
                    denied("file-write", error);
                }
                written += len;
            }
            allowed("file-write");
        }
        "fill-dir" => {
            let directory = std::path::PathBuf::from(args.next().expect("directory"));
            let files: usize = args.next().expect("file count").parse().expect("usize");
            let bytes: usize = args.next().expect("bytes per file").parse().expect("usize");
            let chunk = [0_u8; 4096];
            for index in 0..files {
                let mut file = match File::create(directory.join(format!("part-{index}.bin"))) {
                    Ok(file) => file,
                    Err(error) => denied("fill-dir-create", error),
                };
                let mut written = 0usize;
                while written < bytes {
                    let len = (bytes - written).min(chunk.len());
                    if let Err(error) = file.write_all(&chunk[..len]) {
                        denied("fill-dir-write", error);
                    }
                    written += len;
                }
            }
            thread::sleep(Duration::from_secs(5));
            allowed("fill-dir");
        }
        "sleep" => {
            let millis: u64 = args.next().expect("sleep millis").parse().expect("u64");
            thread::sleep(Duration::from_millis(millis));
            allowed("sleep");
        }
        "spawn-child" => {
            let pid = unsafe { libc::fork() };
            if pid < 0 {
                denied("spawn-child", std::io::Error::last_os_error());
            }
            if pid == 0 {
                unsafe { libc::_exit(0) };
            }
            let mut status = 0;
            let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
            if waited < 0 {
                denied("spawn-child-wait", std::io::Error::last_os_error());
            }
            allowed("spawn-child");
        }
        "spawn-child-sleep" => {
            let pid = unsafe { libc::fork() };
            if pid < 0 {
                denied("spawn-child-sleep", std::io::Error::last_os_error());
            }
            if pid == 0 {
                thread::sleep(Duration::from_secs(30));
                unsafe { libc::_exit(0) };
            }
            println!("child-pid={pid}");
            std::io::stdout().flush().expect("flush child pid");
            thread::sleep(Duration::from_secs(30));
            allowed("spawn-child-sleep");
        },
        other => {
            eprintln!("unknown action: {other}");
            process::exit(64);
        }
    }
}
