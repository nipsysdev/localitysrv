use crate::config::Config;
use crate::models::locality::Locality;
use crate::utils::cmd::{run_command, CmdError};
use crate::utils::file::{ensure_dir_exists, FileError};
use futures::future::join_all;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;

#[derive(Error, Debug)]
pub enum ExtractionError {
    #[error("Failed to get planet PMTiles URL: {0}")]
    PlanetUrlFailed(String),
    #[error("Extraction failed: {0}")]
    ExtractionFailed(String),
    #[error("File operation failed: {0}")]
    FileOperationFailed(String),
    #[error("Command execution failed: {0}")]
    CommandFailed(String),
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
    config: Arc<Config>,
    db_service: Arc<super::database::DatabaseService>,
}

impl ExtractionService {
    pub fn new(config: Arc<Config>, db_service: Arc<super::database::DatabaseService>) -> Self {
        Self { config, db_service }
    }

    pub async fn get_latest_planet_pmtiles_url(&self) -> Result<String, ExtractionError> {
        println!("Fetching latest planet pmtiles URL...");

        let response = reqwest::get(&self.config.protomaps_builds_url)
            .await
            .map_err(|e| {
                ExtractionError::PlanetUrlFailed(format!("Failed to fetch builds: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ExtractionError::PlanetUrlFailed(format!(
                "Failed to fetch builds: {}",
                response.status()
            )));
        }

        let builds: Vec<serde_json::Value> = response.json().await.map_err(|e| {
            ExtractionError::PlanetUrlFailed(format!("Failed to parse builds: {}", e))
        })?;

        if builds.is_empty() {
            return Err(ExtractionError::PlanetUrlFailed(
                "No builds found".to_string(),
            ));
        }

        let latest_build = builds
            .iter()
            .max_by_key(|build| build.get("uploaded").and_then(|v| v.as_str()).unwrap_or(""))
            .ok_or_else(|| ExtractionError::PlanetUrlFailed("No valid builds found".to_string()))?;

        let key = latest_build
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtractionError::PlanetUrlFailed("Build has no key".to_string()))?;

        let url = format!("https://build.protomaps.com/{}", key);
        println!("Latest planet pmtiles URL: {}", url);

        Ok(url)
    }

    pub async fn extract_locality(
        &self,
        locality: &Locality,
        planet_pmtiles_url: &str,
        country_dir: &Path,
    ) -> Result<(), ExtractionError> {
        let output_path = country_dir.join(format!("{}.pmtiles", locality.id));

        if output_path.exists() {
            println!("Skipping existing file: {}", output_path.display());
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

        println!("Extracting locality {} with bbox: {}", locality.id, bbox);
        println!("Command: {} {}", &self.config.pmtiles_cmd, args.join(" "));

        let output = run_command(&self.config.pmtiles_cmd, args, None).await?;

        if !output.stdout.is_empty() {
            println!("Extraction output for {}: {}", locality.id, output.stdout);
        }

        if !output.stderr.is_empty() {
            eprintln!("Extraction error for {}: {}", locality.id, output.stderr);
        }

        if output_path.exists() {
            println!("Successfully created file: {}", output_path.display());
        } else {
            eprintln!("Failed to create file: {}", output_path.display());
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
        let planet_url = self.get_latest_planet_pmtiles_url().await?;

        for country_code in country_codes {
            println!("Processing country: {}", country_code);

            let country_dir = self.config.localities_dir().join(country_code);
            ensure_dir_exists(&country_dir)?;

            let localities = self
                .db_service
                .get_country_localities(country_code)
                .await
                .map_err(|e| ExtractionError::DatabaseError(e.to_string()))?;

            if localities.is_empty() {
                println!("No localities found for country: {}", country_code);
                continue;
            }

            println!(
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
                println!(
                    "All {} localities already exist for country: {}",
                    total_count, country_code
                );
                continue;
            }

            println!(
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
                        eprint!(
                            "\n\rProgress: {}/{} localities extracted for {}\n\n",
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
                        eprintln!("Extraction task failed: {}", e);
                        has_errors = true;
                    }
                    Err(e) => {
                        eprintln!("Extraction task panicked: {:?}", e);
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

    pub async fn ensure_all_localities_present(&self) -> Result<(), ExtractionError> {
        let countries = self.config.target_countries.clone();

        if countries.is_empty() {
            return Err(ExtractionError::ExtractionFailed(
                "No target countries specified".to_string(),
            ));
        }

        self.extract_localities(&countries).await
    }

    async fn get_pmtiles_file_count(&self, country_code: &str) -> Result<u32, ExtractionError> {
        let country_dir = self.config.localities_dir().join(country_code);

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
