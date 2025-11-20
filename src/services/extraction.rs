use crate::config::LocalitySrvConfig;
use crate::models::locality::Locality;
use crate::utils::cmd::{run_command, CmdError};
use crate::utils::file::{ensure_dir_exists, FileError};
use futures::future::join_all;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum ExtractionError {
    #[error("Failed to get planet PMTiles URL: {0}")]
    PlanetUrlFailed(String),
    #[error("Extraction failed: {0}")]
    ExtractionFailed(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Cmd error: {0}")]
    CmdError(#[from] CmdError),
    #[error("File error: {0}")]
    FileError(#[from] FileError),
}

#[derive(Clone)]
pub struct ExtractionService {
    config: Arc<LocalitySrvConfig>,
    db_service: Arc<super::database::DatabaseService>,
}

impl ExtractionService {
    pub fn new(
        config: Arc<LocalitySrvConfig>,
        db_service: Arc<super::database::DatabaseService>,
    ) -> Self {
        Self { config, db_service }
    }

    pub async fn get_planet_pmtiles_source(&self) -> Result<String, ExtractionError> {
        // Check if a local planet pmtiles path is configured
        if let Some(ref local_path) = self.config.planet_pmtiles_path {
            let path = Path::new(local_path);
            if path.exists() {
                info!("Using local planet pmtiles file: {}", local_path);
                return Ok(local_path.clone());
            } else {
                return Err(ExtractionError::PlanetUrlFailed(format!(
                    "Local planet pmtiles file not found: {}",
                    local_path
                )));
            }
        }

        // No HTTP fetching in decentralized mode - require local file
        Err(ExtractionError::PlanetUrlFailed(
            "No local planet pmtiles file configured. Set PLANET_PMTILES_PATH environment variable.".to_string()
        ))
    }

    pub async fn extract_locality(
        &self,
        locality: &Locality,
        planet_pmtiles_url: &str,
        country_dir: &Path,
    ) -> Result<(), ExtractionError> {
        let output_path = country_dir.join(format!("{}.pmtiles", locality.id));

        if output_path.exists() {
            info!("Skipping existing file: {}", output_path.display());
            return Ok(());
        }

        let bbox = format!(
            "{},{},{},{}",
            locality.min_longitude,
            locality.min_latitude,
            locality.max_longitude,
            locality.max_latitude
        );

        let args = &[
            "extract",
            planet_pmtiles_url,
            output_path.to_str().unwrap(),
            &format!("--bbox={}", bbox),
        ];

        info!("Extracting locality {} with bbox: {}", locality.id, bbox);
        info!("Command: {} {}", &self.config.pmtiles_cmd, args.join(" "));

        let output = run_command(&self.config.pmtiles_cmd, args, None).await?;

        if !output.stdout.is_empty() {
            info!("Extraction output for {}: {}", locality.id, output.stdout);
        }

        if !output.stderr.is_empty() {
            error!("Extraction error for {}: {}", locality.id, output.stderr);
        }

        if output_path.exists() {
            info!("Successfully created file: {}", output_path.display());
        } else {
            error!("Failed to create file: {}", output_path.display());
            return Err(ExtractionError::ExtractionFailed(format!(
                "Failed to create PMTiles file for locality {}",
                locality.id
            )));
        }

        Ok(())
    }

    pub async fn extract_localities(
        &self,
        country_codes: &[String],
    ) -> Result<(), ExtractionError> {
        let planet_url = self.get_planet_pmtiles_source().await?;

        for country_code in country_codes {
            info!("Processing country: {}", country_code);

            let country_dir = self.config.localities_dir.join(country_code);
            ensure_dir_exists(&country_dir)?;

            let localities = self
                .db_service
                .get_country_localities(country_code)
                .await
                .map_err(|e| ExtractionError::DatabaseError(e.to_string()))?;

            if localities.is_empty() {
                info!("No localities found for country: {}", country_code);
                continue;
            }

            info!(
                "Found {} localities for country: {}",
                localities.len(),
                country_code
            );

            let mut existing_count = 0;
            for locality in &localities {
                let output_path = country_dir.join(format!("{}.pmtiles", locality.id));
                if output_path.exists() {
                    existing_count += 1;
                }
            }

            let total_count = localities.len();
            let remaining_count = total_count - existing_count;

            if remaining_count == 0 {
                info!(
                    "All {} localities already exist for country: {}",
                    total_count, country_code
                );
                continue;
            }

            info!(
                "Progress: {}/{} localities already exist, {} remaining to extract",
                existing_count, total_count, remaining_count
            );

            let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_extractions));
            let mut tasks = Vec::new();
            let completed_count = Arc::new(std::sync::atomic::AtomicUsize::new(existing_count));

            for locality in localities {
                let planet_url = planet_url.clone();
                let country_dir = country_dir.clone();
                let semaphore = semaphore.clone();
                let extraction_service = self.clone();
                let completed_count = completed_count.clone();

                let task = tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    let result = extraction_service
                        .extract_locality(&locality, &planet_url, &country_dir)
                        .await;

                    // Update progress counter
                    if result.is_ok() {
                        let current =
                            completed_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        // Use carriage return to overwrite the line
                        info!(
                            "Progress: {}/{} localities extracted for {}",
                            current + 1,
                            total_count,
                            locality.country
                        );
                    }

                    result
                });

                tasks.push(task);
            }

            let results = join_all(tasks).await;

            let mut has_errors = false;
            for result in results {
                match result {
                    Ok(Ok(())) => {} // Success
                    Ok(Err(e)) => {
                        error!("Extraction task failed: {}", e);
                        has_errors = true;
                    }
                    Err(e) => {
                        error!("Extraction task panicked: {:?}", e);
                        has_errors = true;
                    }
                }
            }

            if has_errors {
                return Err(ExtractionError::ExtractionFailed(format!(
                    "Some extraction tasks failed for country: {}",
                    country_code
                )));
            }
        }

        Ok(())
    }

    pub async fn get_pmtiles_file_count(&self, country_code: &str) -> Result<u32, ExtractionError> {
        let country_dir = self.config.localities_dir.join(country_code);

        if !country_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(country_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("pmtiles") {
                count += 1;
            }
        }

        Ok(count)
    }

    pub async fn batch_get_pmtiles_file_count(
        &self,
        country_codes: &[String],
    ) -> Result<HashMap<String, u32>, ExtractionError> {
        let mut counts = HashMap::new();

        for country_code in country_codes {
            let count = self.get_pmtiles_file_count(country_code).await?;
            counts.insert(country_code.clone(), count);
        }

        Ok(counts)
    }
}
