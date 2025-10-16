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
    pub config: Arc<tokio::sync::Mutex<Config>>,
    pub db_service: Arc<DatabaseService>,
    pub extraction_service: Arc<ExtractionService>,
    pub country_service: Arc<CountryService>,
}

#[tokio::main]
async fn main() {
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
        config: Arc::new(tokio::sync::Mutex::new((*config).clone())),
        db_service: db_service.clone(),
        extraction_service: extraction_service.clone(),
        country_service: country_service.clone(),
    };

    let app = Router::new()
        .route("/countries", get(countries::search_countries))
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

    let shutdown_signal = std::sync::Arc::new(tokio::sync::Notify::new());

    let app_for_tor = app.clone();
    let app_for_regular = app.clone();

    let shutdown_signal_tor = shutdown_signal.clone();
    let shutdown_signal_regular = shutdown_signal.clone();

    let (onion_address_tx, onion_address_rx) = tokio::sync::oneshot::channel();

    // Clone the app_state for the Tor service
    let app_state_for_tor = app_state.clone();

    // Spawn the Tor hidden service
    let tor_handle = tokio::spawn(async move {
        run_as_tor_hidden_service(
            app_for_tor,
            app_state_for_tor,
            shutdown_signal_tor,
            onion_address_tx,
        )
        .await;
    });

    // Wait for the Tor hidden service to be ready and get the onion address
    let onion_address = match onion_address_rx.await {
        Ok(address) => address,
        Err(_) => {
            eprintln!("Failed to get onion address from Tor hidden service");
            return;
        }
    };

    // Store the onion address in the config
    {
        let mut config = app_state.config.lock().await;
        config.onion_address = Some(onion_address.clone());
    }

    let regular_handle = tokio::spawn(async move {
        run_tcp_listener(app_for_regular, config.clone(), shutdown_signal_regular).await;
    });

    tokio::select! {
        _ = tor_handle => {
            println!("Tor hidden service shutdown");
        }
        _ = regular_handle => {
            println!("TCP listener shutdown");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutdown signal received, shutting down...");
            shutdown_signal.notify_waiters();

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}

async fn run_tcp_listener(
    app: Router,
    config: Arc<Config>,
    shutdown_signal: std::sync::Arc<tokio::sync::Notify>,
) {
    println!("Starting TCP listener...");
    let address = "127.0.0.1";
    let listener = TcpListener::bind(&format!("{}:{}", address, config.server_port))
        .await
        .unwrap();

    println!(
        "✓ TCP listener binded to http://{}:{}",
        address, config.server_port
    );

    let shutdown = shutdown_signal.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await
        .unwrap();
}

async fn run_as_tor_hidden_service(
    app: Router,
    app_state: AppState,
    shutdown_signal: std::sync::Arc<tokio::sync::Notify>,
    onion_address_tx: tokio::sync::oneshot::Sender<String>,
) {
    println!("Starting Tor hidden service...");

    let config = TorClientConfig::default();

    let client = TorClient::create_bootstrapped(config).await.unwrap();

    let svc_cfg = OnionServiceConfigBuilder::default()
        .nickname("localitysrv".parse().unwrap())
        .build()
        .unwrap();

    let (service, request_stream) = client.launch_onion_service(svc_cfg).unwrap();
    let onion_address = service
        .onion_address()
        .unwrap()
        .display_unredacted()
        .to_string();

    // Store the onion address in the config
    {
        let mut config = app_state.config.lock().await;
        config.onion_address = Some(onion_address.clone());
    }

    println!(
        "Waiting for Tor hidden service to be available at: {}",
        onion_address
    );

    let mut status_events = service.status_events();

    while let Some(status) = status_events.next().await {
        if status.state().is_fully_reachable() {
            println!(
                "✓ Tor hidden service is now fully reachable at http://{}",
                onion_address
            );
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        status_events = service.status_events();
    }

    // Send the onion address back to the main task
    let _ = onion_address_tx.send(onion_address.clone());

    let stream_requests = tor_hsservice::handle_rend_requests(request_stream);
    tokio::pin!(stream_requests);

    let shutdown = shutdown_signal.clone();

    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                println!("Tor hidden service shutting down...");
                drop(service);
                return;
            }
            Some(stream_request) = stream_requests.next() => {
                let app_clone = app.clone();

                tokio::spawn(async move {
                    let request = stream_request.request().clone();
                    if let Err(err) = handle_stream_request(stream_request, app_clone).await {
                        eprintln!("error serving connection {:?}: {}", sensitive(request), err);
                    };
                });
            }
        }
    }
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
