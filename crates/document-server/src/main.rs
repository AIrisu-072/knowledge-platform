use document_server::{
    bootstrap::bootstrap_poc,
    composition::{compose_runtime, connect_database},
    config::{Command, ProcessEnvironment, RuntimeConfig},
};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(category) => {
            eprintln!("document-server: {category}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), String> {
    document_server::observability::install()
        .map_err(|_| "runtime diagnostics initialization failed".to_owned())?;
    let mut args = std::env::args().skip(1);
    let command = match (args.next().as_deref(), args.next()) {
        (Some("serve"), None) => Command::Serve,
        (Some("migrate"), None) => Command::Migrate,
        (Some("bootstrap-poc"), None) => Command::BootstrapPoc,
        _ => return Err("usage: document-server serve|migrate|bootstrap-poc".into()),
    };
    let config =
        RuntimeConfig::from_env(&ProcessEnvironment, command).map_err(|error| error.to_string())?;
    match command {
        Command::Migrate => {
            let pool = connect_database(&config)
                .await
                .map_err(|error| error.to_string())?;
            let result = document_repository_postgres::migrate(&pool).await;
            pool.close().await;
            result.map_err(|_| "explicit migration failed".to_owned())?;
            println!("document-server: migration complete");
        }
        Command::BootstrapPoc => {
            let pool = connect_database(&config)
                .await
                .map_err(|error| error.to_string())?;
            document_repository_postgres::check_schema_compatibility(&pool)
                .await
                .map_err(|_| "document schema is incompatible".to_owned())?;
            let result = bootstrap_poc(&pool, config.profile().ok_or("PoC profile missing")?).await;
            pool.close().await;
            println!(
                "document-server: {:?}",
                result.map_err(|error| error.to_string())?
            );
        }
        Command::Serve => {
            let runtime = compose_runtime(&config)
                .await
                .map_err(|error| error.to_string())?;
            let serve = config.serve().ok_or("serve configuration missing")?;
            if serve.non_loopback_warning() {
                eprintln!(
                    "WARNING: PoC non-loopback override enabled. Every reachable caller obtains the fixed process identity. This is not production authentication."
                );
            }
            let listener = tokio::net::TcpListener::bind(serve.bind())
                .await
                .map_err(|_| "HTTP listener is unavailable".to_owned())?;
            eprintln!("document-server: PoC listener ready");
            runtime
                .serve(listener, shutdown_signal())
                .await
                .map_err(|error| error.to_string())?;
            eprintln!("document-server: graceful drain complete");
        }
    }
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            return;
        };
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
