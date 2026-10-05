use document_server::{
    composition::{compose_runtime_with_identity, connect_database},
    config::{Command, ConfigSource, ProcessEnvironment},
};
use organization_server::{
    DocumentAgentSource, DocumentEvidenceSource, OrganizationConfig, OrganizationProfile,
    OwnedAgentDispatcher, SyntheticIdentityAdapter, bootstrap_document_policy, compose_routes,
    verify_shared_document,
};
use std::{process::ExitCode, sync::Arc};
use uuid::Uuid;
use work_application::WorkRepository;
use work_domain::VerifiedActor;
use work_repository_postgres::PostgresWorkRepository;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("organization-server: {error}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), String> {
    document_server::observability::install().map_err(|_| "diagnostics unavailable")?;
    let mut args = std::env::args().skip(1);
    let action = args
        .next()
        .ok_or("usage: organization-server migrate|bootstrap-poc|seed-work|serve")?;
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    let command = match action.as_str() {
        "serve" => Command::Serve,
        "migrate" => Command::Migrate,
        "bootstrap-poc" | "seed-work" => Command::BootstrapPoc,
        _ => return Err("unknown command".into()),
    };
    let config = OrganizationConfig::from_env(&ProcessEnvironment, command)?;
    if command == Command::BootstrapPoc && config.profile() != OrganizationProfile::Sales {
        return Err("initialization requires sales-01".into());
    }
    let pool = connect_database(config.document())
        .await
        .map_err(|error| error.to_string())?;
    match action.as_str() {
        "migrate" => {
            document_repository_postgres::migrate(&pool)
                .await
                .map_err(|_| "explicit Document migration failed")?;
            work_repository_postgres::migrate(&pool)
                .await
                .map_err(|_| "explicit Work migration failed")?;
            println!("organization-server: explicit migrations complete");
        }
        "bootstrap-poc" => {
            document_repository_postgres::check_schema_compatibility(&pool)
                .await
                .map_err(|_| "Document schema incompatible")?;
            bootstrap_document_policy(&pool, config.profile()).await?;
            println!("organization-server: synthetic Document policy ready");
        }
        "seed-work" => {
            let id = ProcessEnvironment
                .get("KP_ORGANIZATION_DOCUMENT_ID")
                .ok_or("KP_ORGANIZATION_DOCUMENT_ID is required")?;
            let id = Uuid::parse_str(&id).map_err(|_| "shared Document ID is invalid")?;
            verify_shared_document(&pool, id).await?;
            work_repository_postgres::seed_synthetic(&pool, Some(id))
                .await
                .map_err(|_| "Work seed failed; existing workflow is never reset")?;
            println!("organization-server: synthetic Work fixture ready");
        }
        "serve" => {
            work_repository_postgres::check_schema_compatibility(&pool)
                .await
                .map_err(|_| "Work schema incompatible; run explicit migrate first")?;
            let actor = VerifiedActor::from_startup_profile(config.profile().principal())
                .map_err(|_| "synthetic profile unavailable")?;
            let runtime = compose_runtime_with_identity(
                config.document(),
                Arc::new(SyntheticIdentityAdapter::new(config.profile())),
            )
            .await
            .map_err(|error| error.to_string())?;
            let documents = Arc::new(
                document_repository_postgres::PostgresDocumentRepository::new(pool.clone()),
            );
            let repository = Arc::new(PostgresWorkRepository::with_agent_source(
                pool.clone(),
                Arc::new(DocumentEvidenceSource::new(documents.clone())),
                Arc::new(DocumentAgentSource::new(documents)),
            ));
            // A previous process's queued/running work is uncertain, never replayed.
            // Only this startup profile's nonterminal executions are affected.
            repository
                .interrupt_agent_executions(actor)
                .await
                .map_err(|_| "synthetic execution recovery unavailable")?;
            let dispatcher = Arc::new(OwnedAgentDispatcher::new(repository.clone(), actor));
            let work = work_api_http::router_with_agent(repository, actor, dispatcher.clone());
            let joined = compose_routes(work, runtime.router());
            let bind = config
                .document()
                .serve()
                .ok_or("serve configuration unavailable")?
                .bind();
            let listener = tokio::net::TcpListener::bind(bind)
                .await
                .map_err(|_| "HTTP listener unavailable")?;
            eprintln!(
                "organization-server: loopback synthetic profile ready; not production authentication"
            );
            let draining_dispatcher = dispatcher.clone();
            let serve_result = runtime
                .with_router(joined)
                .serve(listener, async move {
                    shutdown_signal().await;
                    // Runtime closes its Document pool only after this signal
                    // settles. Work's separate pool remains open for fencing.
                    let _ = draining_dispatcher.shutdown().await;
                })
                .await;
            // Also drain if serving ended without the normal signal future.
            let drain_result = dispatcher.shutdown().await;
            if let Err(error) = serve_result {
                pool.close().await;
                return Err(error.to_string());
            }
            if drain_result.is_err() {
                pool.close().await;
                return Err("synthetic execution drain could not confirm durable outcomes".into());
            }
        }
        _ => unreachable!(),
    }
    pool.close().await;
    if action == "serve" {
        eprintln!("organization-server: graceful drain complete");
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
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
