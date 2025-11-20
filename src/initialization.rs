use crate::cli::Args;
use crate::config::LocalitySrvConfig;
use crate::services::{
    country::CountryService, database::DatabaseService, extraction::ExtractionService,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use tracing::{info, warn};

async fn download_and_decompress_database(
    config: &LocalitySrvConfig,
    compressed_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Downloading WhosOnFirst database...");
    crate::utils::file::download_file_with_progress(
        &config.whosonfirst_db_url,
        Path::new(compressed_path),
    )
    .await?;
    info!("Database download completed!");

    info!("Decompressing database...");
    let output =
        crate::utils::cmd::run_command(&config.bzip2_cmd, &["-dv", compressed_path], None).await?;

    if !output.stderr.is_empty() {
        warn!("Decompression output: {}", output.stderr);
    }

    info!("Database decompressed successfully!");
    Ok(())
}

async fn extract_missing_localities(
    extraction_service: &ExtractionService,
    results: Vec<(&String, &String, u32, u32, bool)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let missing_country_codes: Vec<String> = results
        .into_iter()
        .filter(|(_, _, _, _, is_complete)| !is_complete)
        .map(|(country_code, _, _, _, _)| (*country_code).clone())
        .collect();

    if !missing_country_codes.is_empty() {
        extraction_service
            .extract_localities(&missing_country_codes)
            .await?;
        info!("Extraction completed.");
    }
    Ok(())
}

pub async fn ensure_tools_are_present(tools: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::cmd::ensure_tools_are_present as check_tools;

    check_tools(tools).await?;
    Ok(())
}

pub async fn ensure_database_is_present(
    config: &LocalitySrvConfig,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    let database_path = &config.database_path;
    let compressed_path = format!("{}.bz2", database_path.display());

    if database_path.exists() {
        info!("Database already present.");
        return Ok(());
    }

    if Path::new(&compressed_path).exists() {
        info!("Compressed database found, decompressing...");

        let output =
            crate::utils::cmd::run_command(&config.bzip2_cmd, &["-dv", &compressed_path], None)
                .await?;

        if !output.stderr.is_empty() {
            warn!("Decompression output: {}", output.stderr);
        }

        info!("Database decompressed successfully!");
        return Ok(());
    }

    info!("WhosOnFirst database not found.");

    if args.should_download_database() {
        info!("Auto-downloading WhosOnFirst database...");
        download_and_decompress_database(config, &compressed_path).await?;
        return Ok(());
    } else if args.is_interactive_mode() {
        print!("Do you want to download the WhosOnFirst database? This may take a while. (y/n) ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase() == "y" {
            download_and_decompress_database(config, &compressed_path).await?;
            return Ok(());
        }
    }
    info!("Database download skipped.");
    Err("Database is missing and download is disabled".into())
}

pub async fn ensure_all_localities_present(
    extraction_service: &ExtractionService,
    country_service: &CountryService,
    config: &LocalitySrvConfig,
    db_service: &DatabaseService,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Checking localities extraction status...");

    let countries_to_check = country_service.get_countries_to_process(&config.target_countries);

    if countries_to_check.is_empty() {
        info!("No countries to process");
        return Ok(());
    }

    info!("Counting pmtiles files...");
    let file_count_map = extraction_service
        .batch_get_pmtiles_file_count(&countries_to_check)
        .await?;

    info!("Querying database for locality counts...");
    let mut db_count_map = HashMap::new();

    for country_code in &countries_to_check {
        let db_count = db_service.get_country_locality_count(country_code).await?;
        db_count_map.insert(country_code.clone(), db_count);
    }

    let mut results = Vec::new();
    let mut all_complete = true;

    for country_code in &countries_to_check {
        let country_name = country_service
            .get_country_name(country_code)
            .unwrap_or(country_code);
        let db_count = db_count_map.get(country_code).unwrap_or(&0);
        let file_count = file_count_map.get(country_code).unwrap_or(&0);
        let is_complete = db_count == file_count;

        if !is_complete {
            all_complete = false;
        }

        results.push((
            country_code,
            country_name,
            *db_count,
            *file_count,
            is_complete,
        ));
    }

    if all_complete {
        info!("✓ All localities have been extracted!");
        return Ok(());
    }

    info!("Country Code | Country Name                  | DB Count | File Count | Status");
    info!("-------------|-------------------------------|----------|------------|--------");

    for (country_code, country_name, db_count, file_count, is_complete) in &results {
        let status = if *is_complete {
            "✓ Complete"
        } else {
            "✗ Incomplete"
        };
        let truncated_name = if country_name.len() > 29 {
            format!("{}...", &country_name[..26])
        } else {
            country_name.to_string()
        };
        info!(
            "{:12} | {:29} | {:8} | {:10} | {}",
            country_code, truncated_name, db_count, file_count, status
        );
    }

    warn!("✗ Some localities are missing. Extraction is incomplete.");

    if args.should_extract_localities() {
        info!("Auto-extracting missing localities...");
        extract_missing_localities(extraction_service, results.clone()).await?;
        return Ok(());
    } else if args.is_interactive_mode() {
        // Interactive mode - prompt the user
        print!("Do you want to extract the missing localities? (y/n) ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase() == "y" {
            extract_missing_localities(extraction_service, results.clone()).await?;
            return Ok(());
        }
    }
    info!("Extraction skipped.");
    Ok(())
}

/// Initialize and start the Codex node
pub async fn initialize_codex_node(
    config: &LocalitySrvConfig,
) -> Result<crate::node::manager::CodexNodeManager, Box<dyn std::error::Error>> {
    info!("Initializing Codex node...");

    // Create Codex configuration from the main config
    let codex_config = config.codex.clone();

    // Create node manager
    let node_manager = crate::node::manager::CodexNodeManager::new(codex_config);

    // Start the node
    node_manager.start().await?;

    info!("Codex node started successfully");
    Ok(node_manager)
}

/// Ensure Codex data directory exists
pub async fn ensure_codex_data_directory(
    config: &LocalitySrvConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = &config.data_dir;

    if !data_dir.exists() {
        info!(
            "Creating Codex data directory with secure permissions: {:?}",
            data_dir
        );
        std::fs::create_dir_all(data_dir)?;

        // Set secure permissions (0700 - read/write/execute for owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(data_dir)?.permissions();
            perms.set_mode(0o700);
            std::fs::set_permissions(data_dir, perms)?;
        }
    } else {
        // Check and fix permissions if directory already exists
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(data_dir)?;
            let current_mode = metadata.permissions().mode();

            // Check if permissions are too permissive (not 0700)
            if current_mode & 0o077 != 0 {
                info!(
                    "Fixing insecure permissions on Codex data directory: {:?}",
                    data_dir
                );
                let mut perms = metadata.permissions();
                perms.set_mode(0o700);
                std::fs::set_permissions(data_dir, perms)?;
            }
        }
    }

    Ok(())
}

/// Check if localities are ready for upload (exist and not already uploaded)
pub async fn check_upload_readiness(
    db_service: &DatabaseService,
    extraction_service: &ExtractionService,
    country_codes: &[String],
) -> Result<HashMap<String, UploadReadiness>, Box<dyn std::error::Error>> {
    let mut readiness_map = HashMap::new();

    for country_code in country_codes {
        let db_count = db_service.get_country_locality_count(country_code).await?;
        let file_count = extraction_service
            .get_pmtiles_file_count(country_code)
            .await?;

        // Check how many are already uploaded - simplified for now
        let uploaded_count = 0u32;

        let readiness = UploadReadiness {
            total_localities: db_count,
            extracted_files: file_count,
            uploaded_files: uploaded_count,
            ready_for_upload: file_count > uploaded_count,
        };

        readiness_map.insert(country_code.clone(), readiness);
    }

    Ok(readiness_map)
}

#[derive(Debug, Clone)]
pub struct UploadReadiness {
    pub total_localities: u32,
    pub extracted_files: u32,
    pub uploaded_files: u32,
    pub ready_for_upload: bool,
}

/// Print upload readiness status
pub fn print_upload_readiness(readiness_map: &HashMap<String, UploadReadiness>) {
    info!("Upload Readiness Status:");
    info!("Country Code | Total | Extracted | Uploaded | Ready");
    info!("-------------|-------|-----------|----------|-------");

    for (country_code, readiness) in readiness_map {
        let ready = if readiness.ready_for_upload {
            "✓ Yes"
        } else {
            "✗ No"
        };

        info!(
            "{:12} | {:5} | {:9} | {:8} | {}",
            country_code,
            readiness.total_localities,
            readiness.extracted_files,
            readiness.uploaded_files,
            ready
        );
    }
}
