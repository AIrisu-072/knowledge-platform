use std::{
    env,
    fs::{self, File},
    hint::black_box,
    io::Write,
    net::{TcpListener, UdpSocket},
    process::{self, Command},
    thread,
    time::Duration,
};

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
        "sleep" => {
            let millis: u64 = args.next().expect("sleep millis").parse().expect("u64");
            thread::sleep(Duration::from_millis(millis));
            allowed("sleep");
        }
        "spawn-child" => match Command::new("true").spawn() {
            Ok(mut child) => {
                let _ = child.wait();
                allowed("spawn-child");
            }
            Err(error) => denied("spawn-child", error),
        },
        "spawn-child-sleep" => match Command::new("sleep").arg("30").spawn() {
            Ok(child) => {
                println!("child-pid={}", child.id());
                std::io::stdout().flush().expect("flush child pid");
                thread::sleep(Duration::from_secs(30));
                allowed("spawn-child-sleep");
            }
            Err(error) => denied("spawn-child-sleep", error),
        },
        other => {
            eprintln!("unknown action: {other}");
            process::exit(64);
        }
    }
}
