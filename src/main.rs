use crate::config::LocalitySrvConfig;
use crate::initialization::{
    check_upload_readiness, ensure_all_localities_present, ensure_codex_data_directory,
    ensure_database_is_present, ensure_tools_are_present, initialize_codex_node,
    print_upload_readiness,
};
use crate::services::{
    country::CountryService, database::DatabaseService, extraction::ExtractionService,
    node_ops::NodeOps,
};
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};

mod cli;
mod config;
mod initialization;
mod models;
mod node;
mod services;
mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();

    let args = cli::Args::parse();

    // Load configuration
    let config = match LocalitySrvConfig::from_env() {
        Ok(config) => Arc::new(config),
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    info!("Starting localitysrv decentralized node...");

    // Ensure required tools are present
    if let Err(e) =
        ensure_tools_are_present(&[&config.pmtiles_cmd, &config.bzip2_cmd, &config.find_cmd]).await
    {
        error!("Failed to ensure tools are present: {}", e);
        std::process::exit(1);
    }

    // Ensure Codex data directory exists
    if let Err(e) = ensure_codex_data_directory(&config).await {
        error!("Failed to ensure Codex data directory: {}", e);
        std::process::exit(1);
    }

    // Ensure database is present
    if let Err(e) = ensure_database_is_present(&config, &args).await {
        error!("Failed to ensure database is present: {}", e);
        std::process::exit(1);
    }

    // Initialize WhosOnFirst database service (read-only)
    let whosonfirst_db_service =
        match DatabaseService::new(&config.database_path.to_string_lossy()).await {
            Ok(service) => Arc::new(service),
            Err(e) => {
                error!("Failed to initialize WhosOnFirst database service: {}", e);
                std::process::exit(1);
            }
        };

    // Initialize CID mappings database service (read-write)
    let cid_db_service =
        match DatabaseService::new(&config.cid_database_path.to_string_lossy()).await {
            Ok(service) => Arc::new(service),
            Err(e) => {
                error!("Failed to initialize CID database service: {}", e);
                std::process::exit(1);
            }
        };

    // Initialize country service
    let country_service = match CountryService::new(&config.country_codes_path()).await {
        Ok(service) => Arc::new(service),
        Err(e) => {
            error!("Failed to initialize country service: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize extraction service (uses WhosOnFirst database)
    let extraction_service = Arc::new(ExtractionService::new(
        config.clone(),
        whosonfirst_db_service.clone(),
    ));

    // Ensure all localities are extracted
    if let Err(e) = ensure_all_localities_present(
        &extraction_service,
        &country_service,
        &config,
        &whosonfirst_db_service,
        &args,
    )
    .await
    {
        error!("Failed to ensure all localities are present: {}", e);
        std::process::exit(1);
    }

    // Initialize Codex node
    let node_manager = match initialize_codex_node(&config).await {
        Ok(manager) => Arc::new(manager),
        Err(e) => {
            error!("Failed to initialize Codex node: {}", e);
            std::process::exit(1);
        }
    };

    info!("Initialization complete, starting upload process...");

    // Check upload readiness
    let readiness_map = check_upload_readiness(
        &whosonfirst_db_service,
        &extraction_service,
        &config.target_countries,
    )
    .await?;

    print_upload_readiness(&readiness_map);

    // Create node operations service (uses CID database for storage, WhosOnFirst for lookups)
    let node_ops = NodeOps::new_with_databases(
        cid_db_service.clone(),
        whosonfirst_db_service.clone(),
        node_manager.clone(),
    );

    // Process all localities for upload
    if let Err(e) = node_ops.process_all_localities().await {
        error!("Failed to process localities: {}", e);
        std::process::exit(1);
    }

    // Get final statistics
    let stats = node_ops.get_stats().await;
    info!(
        "Upload process completed! Total uploaded: {}, Total failed: {}, Total bytes: {}",
        stats.total_uploaded, stats.total_failed, stats.total_bytes_uploaded
    );

    // Get database statistics
    let (total_mappings, unique_countries) = cid_db_service.get_cid_mapping_stats().await?;
    info!(
        "Database contains {} CID mappings across {} countries",
        total_mappings, unique_countries
    );

    info!("All localities uploaded and CID mappings stored!");
    info!("Node is now running and serving files to the network...");
    info!("Press Ctrl+C to stop the node gracefully");

    // Keep the node running until interrupted
    tokio::select! {
        _ = async {
            signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
        } => {
            info!("Received Ctrl+C, shutting down gracefully...");
        }
        _ = async {
            let mut sig_term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");
            sig_term.recv().await;
        } => {
            info!("Received termination signal, shutting down gracefully...");
        }
    }

    // Stop the node gracefully
    if let Err(e) = node_manager.stop().await {
        error!("Failed to stop Codex node: {}", e);
    }

    info!("Node stopped successfully");
    Ok(())
}
