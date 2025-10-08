use crate::{
    api::{countries, localities, pmtiles},
    config::Config,
    initialization::{
        ensure_all_localities_present, ensure_database_is_present, ensure_tools_are_present,
    },
    services::{country::CountryService, database::DatabaseService, extraction::ExtractionService},
};
use axum::{
    routing::{get, Router},
    Json,
};
use reqwest::StatusCode;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
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
        .with_state(app_state);

    let listener = TcpListener::bind(&format!("0.0.0.0:{}", config.server_port))
        .await
        .unwrap();

    println!("Server listening on http://0.0.0.0:{}", config.server_port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
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
