use crate::{
    api::{countries, localities, pmtiles},
    config::Config,
    initialization::{
        ensure_all_localities_present, ensure_database_is_present, ensure_tools_are_present,
    },
    services::tor::TorServiceManager,
    services::{country::CountryService, database::DatabaseService, extraction::ExtractionService},
};
use axum::{
    routing::{get, Router},
    Json,
};
use clap::Parser;
use reqwest::StatusCode;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::error;

mod api;
mod cli;
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
    tracing_subscriber::fmt().init();

    let args = cli::Args::parse();

    let config = match Config::from_env() {
        Ok(config) => Arc::new(config),
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) =
        ensure_tools_are_present(&[&config.pmtiles_cmd, &config.bzip2_cmd, &config.find_cmd]).await
    {
        error!("Failed to ensure tools are present: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = ensure_database_is_present(&config, &args).await {
        error!("Failed to ensure database is present: {}", e);
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
            error!("Failed to initialize database service: {}", e);
            std::process::exit(1);
        }
    };

    let country_service = match CountryService::new(&config.country_codes_path()).await {
        Ok(service) => Arc::new(service),
        Err(e) => {
            error!("Failed to initialize country service: {}", e);
            std::process::exit(1);
        }
    };

    let extraction_service = Arc::new(ExtractionService::new(config.clone(), db_service.clone()));

    if let Err(e) = ensure_all_localities_present(
        &extraction_service,
        &country_service,
        &config,
        &db_service,
        &args,
    )
    .await
    {
        error!("Failed to ensure all localities are present: {}", e);
        std::process::exit(1);
    }

    tracing::info!("Initialization complete, starting services...");

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

    let app_state_for_tor = app_state.clone();

    let tor_manager = TorServiceManager::new(app_for_tor, app_state_for_tor, shutdown_signal_tor);

    let tor_handle = tokio::spawn(async move {
        tor_manager.run_with_retry(onion_address_tx).await;
    });

    let onion_address = match onion_address_rx.await {
        Ok(address) => address,
        Err(_) => {
            error!("Failed to get onion address from Tor hidden service");
            return;
        }
    };

    {
        let mut config = app_state.config.lock().await;
        config.onion_address = Some(onion_address.clone());
    }

    let regular_handle = tokio::spawn(async move {
        run_axum_server(app_for_regular, config.clone(), shutdown_signal_regular).await;
    });

    tokio::select! {
        _ = tor_handle => {}
        _ = regular_handle => {}
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("\nShutdown signal received, shutting down...");
            shutdown_signal.notify_waiters();

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}

async fn run_axum_server(
    app: Router,
    config: Arc<Config>,
    shutdown_signal: std::sync::Arc<tokio::sync::Notify>,
) {
    tracing::info!("Starting Axum server...");

    let address = "127.0.0.1";
    let port = config.server_port;

    let listener = match tokio::net::TcpListener::bind(&format!("{}:{}", address, port)).await {
        Ok(listener) => {
            tracing::info!("TCP listener bound to {}:{}", address, port);
            listener
        }
        Err(e) => {
            error!(
                "TCP listener: Failed to bind to {}:{}: {}",
                address, port, e
            );
            return;
        }
    };

    let shutdown = shutdown_signal.clone();

    match axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tracing::info!("✓ Axum server successfully started");
            shutdown.notified().await;
            tracing::info!("Axum server shutting down...");
        })
        .await
    {
        Ok(_) => {
            tracing::info!("Axum server stopped successfully");
        }
        Err(e) => {
            error!("Axum server error: {}", e);
        }
    }
}
