use std::{env, path::PathBuf, time::Duration};

use document_publication_scheduler::DueScheduler;

#[tokio::main]
async fn main() {
    let database_url = required("DOCUMENT_DATABASE_URL");
    let storage_root = PathBuf::from(required("DOCUMENT_STORAGE_ROOT"));
    let worker_executable = PathBuf::from(required("DSI_WORKER_EXECUTABLE"));
    let pdfium_runtime_dir = env::var_os("DSI_PDFIUM_RUNTIME_DIR").map(PathBuf::from);
    let poll_interval = env::var("DOCUMENT_PUBLICATION_POLL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=60).contains(value))
        .unwrap_or(5);
    let scheduler = match DueScheduler::connect(
        &database_url,
        &storage_root,
        &worker_executable,
        pdfium_runtime_dir.as_deref(),
    )
    .await
    {
        Ok(scheduler) => scheduler,
        Err(error) => {
            eprintln!("publication scheduler startup failed: {error}");
            std::process::exit(1);
        }
    };
    scheduler
        .run_until_shutdown(Duration::from_secs(poll_interval))
        .await;
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| {
        eprintln!("publication scheduler requires {name}");
        std::process::exit(2);
    })
}
