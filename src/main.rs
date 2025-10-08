use crate::{
    api::{countries, localities, pmtiles},
    config::Config,
    initialization::{
        ensure_all_localities_present, ensure_database_is_present, ensure_tools_are_present,
    },
    services::{country::CountryService, database::DatabaseService, extraction::ExtractionService},
};
use anyhow::Result;
use arti_client::{TorClient, TorClientConfig};
use axum::{
    routing::{get, Router},
    Json,
};
use futures::StreamExt;
use hyper::{body::Incoming, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server;
use reqwest::StatusCode;
use safelog::{sensitive, DisplayRedacted as _};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{config::OnionServiceConfigBuilder, StreamRequest};
use tor_proto::client::stream::IncomingStreamRequest;
use tower::Service;
use tower_http::cors::CorsLayer;

mod api;
mod config;
mod initialization;
mod models;
mod services;
mod utils;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db_service: Arc<DatabaseService>,
    pub extraction_service: Arc<ExtractionService>,
    pub country_service: Arc<CountryService>,
}

#[tokio::main]
async fn main() {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    let config = match Config::from_env() {
        Ok(config) => Arc::new(config),
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) =
        ensure_tools_are_present(&[&config.pmtiles_cmd, &config.bzip2_cmd, &config.find_cmd]).await
    {
        eprintln!("Failed to ensure tools are present: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = ensure_database_is_present(&config).await {
        eprintln!("Failed to ensure database is present: {}", e);
        std::process::exit(1);
    }

    let db_service = match DatabaseService::new(
        &config.database_url(),
        &config.database_path().to_string_lossy(),
        &config.whosonfirst_db_url,
        &config.bzip2_cmd,
    )
    .await
    {
        Ok(service) => Arc::new(service),
        Err(e) => {
            eprintln!("Failed to initialize database service: {}", e);
            std::process::exit(1);
        }
    };

    let country_service = match CountryService::new(&config.country_codes_path()).await {
        Ok(service) => Arc::new(service),
        Err(e) => {
            eprintln!("Failed to initialize country service: {}", e);
            std::process::exit(1);
        }
    };

    let extraction_service = Arc::new(ExtractionService::new(config.clone(), db_service.clone()));

    if let Err(e) =
        ensure_all_localities_present(&extraction_service, &country_service, &config, &db_service)
            .await
    {
        eprintln!("Failed to ensure all localities are present: {}", e);
        std::process::exit(1);
    }

    let app_state = AppState {
        config: config.clone(),
        db_service: db_service.clone(),
        extraction_service: extraction_service.clone(),
        country_service: country_service.clone(),
    };

    let app = Router::new()
        .route("/countries", get(countries::get_countries))
        .route(
            "/countries/{country_code}/localities",
            get(localities::search_localities),
        )
        .route(
            "/countries/{country_code}/localities/{id}/pmtiles",
            get(pmtiles::serve_pmtiles),
        )
        .route(
            "/health",
            get({
                (
                    StatusCode::OK,
                    Json(serde_json::json!({ "status": "healthy" })),
                )
            }),
        )
        .layer(CorsLayer::permissive())
        .with_state(app_state.clone());

    // Check if we should run as a Tor hidden service
    if std::env::var("TOR_HIDDEN_SERVICE").unwrap_or_else(|_| "false".to_string()) == "true" {
        run_as_tor_hidden_service(app, app_state).await;
    } else {
        run_as_regular_server(app, config).await;
    }
}

async fn run_as_regular_server(app: Router, config: Arc<Config>) {
    let listener = TcpListener::bind(&format!("0.0.0.0:{}", config.server_port))
        .await
        .unwrap();

    println!("Server listening on http://0.0.0.0:{}", config.server_port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn run_as_tor_hidden_service(app: Router, _app_state: AppState) {
    // The client config includes things like where to store persistent Tor network state.
    let config = TorClientConfig::default();

    // We now let the Arti client start and bootstrap a connection to the network.
    let client = TorClient::create_bootstrapped(config).await.unwrap();

    let svc_cfg = OnionServiceConfigBuilder::default()
        .nickname("localitysrv".parse().unwrap())
        .build()
        .unwrap();

    let (service, request_stream) = client.launch_onion_service(svc_cfg).unwrap();
    println!("{}", service.onion_address().unwrap().display_unredacted());

    // Wait until the service is believed to be fully reachable.
    eprintln!("waiting for service to become fully reachable");
    while let Some(status) = service.status_events().next().await {
        if status.state().is_fully_reachable() {
            break;
        }
    }

    let stream_requests = tor_hsservice::handle_rend_requests(request_stream);
    tokio::pin!(stream_requests);
    eprintln!("ready to serve connections");

    while let Some(stream_request) = stream_requests.next().await {
        let app = app.clone();

        tokio::spawn(async move {
            let request = stream_request.request().clone();
            if let Err(err) = handle_stream_request(stream_request, app).await {
                eprintln!("error serving connection {:?}: {}", sensitive(request), err);
            };
        });
    }

    drop(service);
    eprintln!("onion service exited cleanly");
}

async fn handle_stream_request(stream_request: StreamRequest, app: Router) -> Result<()> {
    match stream_request.request() {
        IncomingStreamRequest::Begin(begin) if begin.port() == 80 => {
            let onion_service_stream = stream_request.accept(Connected::new_empty()).await?;
            let io = TokioIo::new(onion_service_stream);

            let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
                app.clone().call(request)
            });

            server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection(io, hyper_service)
                .await
                .map_err(|x| anyhow::anyhow!(x))?;
        }
        _ => {
            stream_request.shutdown_circuit()?;
        }
    }

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("Signal received, starting graceful shutdown");
}
