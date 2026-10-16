use std::process::ExitCode;
use std::time::Duration;

use search_api_http::send::{ServeOptions, serve};
use search_server::composition::compose;
use search_server::config::{Command, ProcessEnvironment, ServerConfig};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("search-server: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = match (args.next().as_deref(), args.next()) {
        (Some("serve"), None) => Command::Serve,
        (Some("migrate"), None) => Command::Migrate,
        _ => return Err("usage: search-server serve|migrate".into()),
    };
    let config =
        ServerConfig::from_env(&ProcessEnvironment, command).map_err(|error| error.to_string())?;
    match command {
        Command::Migrate => {
            // The Search runtime and Graph migrations, after `document-server migrate`.
            let pool = PgPoolOptions::new()
                .max_connections(2)
                .connect(config.database_url())
                .await
                .map_err(|_| "Search database unavailable".to_string())?;
            search_runtime::migrate(&pool)
                .await
                .map_err(|error| format!("Search migrations: {error}"))?;
            search_graph::migrate(&pool)
                .await
                .map_err(|error| format!("Graph migrations: {error}"))?;
            pool.close().await;
            println!("search-server: migration complete");
            Ok(())
        }
        Command::Serve => {
            let serve_config = config.serve.as_ref().ok_or("serve configuration missing")?;
            if serve_config.non_loopback_warning {
                eprintln!(
                    "search-server: WARNING listening on a non-loopback address; the PoC identity is not production authentication"
                );
            }
            let pool = PgPoolOptions::new()
                .max_connections(serve_config.pool_size)
                .connect(config.database_url())
                .await
                .map_err(|_| "Search database unavailable".to_string())?;
            let runtime = compose(serve_config, pool).await?;
            let listener = tokio::net::TcpListener::bind(serve_config.bind)
                .await
                .map_err(|_| format!("cannot bind {}", serve_config.bind))?;
            eprintln!(
                "search-server: {} listening on {}",
                serve_config.profile.principal(),
                serve_config.bind
            );
            serve(
                listener,
                runtime.router(),
                ServeOptions {
                    send_deadline: Duration::from_secs(60),
                    send_buffer_bytes: None,
                },
                None,
                shutdown(),
            )
            .await
            .map_err(|_| "server stopped".to_string())
        }
    }
}

/// Stops on Ctrl-C or SIGTERM (what a container stop sends).
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(term) => term,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
