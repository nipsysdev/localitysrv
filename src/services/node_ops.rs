use crate::models::storage::{CompletedUpload, PendingUpload, UploadQueue, UploadStats};
use crate::node::manager::{CodexNodeManager, NodeManagerError};
use crate::services::database::{DatabaseError, DatabaseService};
use codex_bindings::UploadOptions;
use futures::future::join_all;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Error, Debug)]
pub enum NodeOpsError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] DatabaseError),
    #[error("Node manager error: {0}")]
    NodeManagerError(#[from] NodeManagerError),
    #[error("File error: {0}")]
    FileError(#[from] std::io::Error),
    #[error("Upload queue error: {0}")]
    QueueError(String),
}

pub struct NodeOps {
    db_service: Arc<DatabaseService>, // CID mappings database
    whosonfirst_db_service: Arc<DatabaseService>, // WhosOnFirst database
    node_manager: Arc<CodexNodeManager>,
    upload_queue: Arc<Mutex<UploadQueue>>,
    stats: Arc<Mutex<UploadStats>>,
}

impl NodeOps {
    pub fn new(db_service: Arc<DatabaseService>, node_manager: Arc<CodexNodeManager>) -> Self {
        Self {
            db_service: db_service.clone(),
            whosonfirst_db_service: db_service, // Use same database for both
            node_manager,
            upload_queue: Arc::new(Mutex::new(UploadQueue::new(10, 100))),
            stats: Arc::new(Mutex::new(UploadStats::new())),
        }
    }

    pub fn new_with_databases(
        cid_db_service: Arc<DatabaseService>,
        whosonfirst_db_service: Arc<DatabaseService>,
        node_manager: Arc<CodexNodeManager>,
    ) -> Self {
        Self {
            db_service: cid_db_service,
            whosonfirst_db_service,
            node_manager,
            upload_queue: Arc::new(Mutex::new(UploadQueue::new(10, 100))),
            stats: Arc::new(Mutex::new(UploadStats::new())),
        }
    }

    /// Process all localities by scanning filesystem first
    pub async fn process_all_localities(&self) -> Result<(), NodeOpsError> {
        info!("Starting to process all localities by scanning filesystem for PMTiles files");

        // Scan the assets/localities directory for all PMTiles files
        let localities_dir = std::path::Path::new("assets/localities");
        if !localities_dir.exists() {
            warn!("Localities directory not found: {:?}", localities_dir);
            return Ok(());
        }

        let mut total_files = 0;
        let mut processed_files = 0;

        // Iterate through all country directories
        for country_dir_entry in std::fs::read_dir(localities_dir)? {
            let country_dir = country_dir_entry?;
            let country_path = country_dir.path();

            if !country_path.is_dir() {
                continue;
            }

            let country_code = country_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    NodeOpsError::QueueError("Invalid country directory name".to_string())
                })?;

            info!("Scanning country directory: {}", country_code);

            // Process all PMTiles files in this country directory
            let (country_files, country_processed) = self
                .process_country_directory(&country_path, country_code)
                .await?;
            total_files += country_files;
            processed_files += country_processed;
        }

        // Process any remaining uploads in the queue
        if !self.upload_queue.lock().await.is_empty() {
            info!("Processing remaining uploads in queue...");
            self.process_upload_queue().await?;
        }

        let stats = self.stats.lock().await;
        info!(
            "Filesystem scan completed! Total files found: {}, Total processed: {}, Total uploaded: {}, Total failed: {}, Total bytes: {}",
            total_files, processed_files, stats.total_uploaded, stats.total_failed, stats.total_bytes_uploaded
        );

        Ok(())
    }

    /// Process all PMTiles files in a country directory
    async fn process_country_directory(
        &self,
        country_path: &std::path::Path,
        country_code: &str,
    ) -> Result<(usize, usize), NodeOpsError> {
        let mut total_files = 0;
        let mut processed_files = 0;

        // Iterate through all PMTiles files in the country directory
        for file_entry in std::fs::read_dir(country_path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();

            // Check if it's a PMTiles file
            if !file_path.is_file() || file_path.extension().is_none_or(|ext| ext != "pmtiles") {
                continue;
            }

            total_files += 1;

            // Extract locality ID from filename (without .pmtiles extension)
            let filename = file_path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| NodeOpsError::QueueError("Invalid filename".to_string()))?;

            let locality_id = filename.parse::<u32>().map_err(|_| {
                NodeOpsError::QueueError(format!("Invalid locality ID in filename: {}", filename))
            })?;

            // Check if this locality exists in WhosOnFirst database
            match self
                .whosonfirst_db_service
                .get_locality_by_id(locality_id as i64)
                .await
            {
                Ok(Some(_locality)) => {
                    // Locality exists in database, proceed with upload
                    if self
                        .process_file_for_upload(&file_path, country_code, locality_id)
                        .await?
                    {
                        processed_files += 1;
                    }
                }
                Ok(None) => {
                    warn!(
                        "Locality ID {} found in filesystem but not in database, skipping",
                        locality_id
                    );
                }
                Err(e) => {
                    error!("Database error checking locality {}: {}", locality_id, e);
                }
            }
        }

        info!(
            "Country {}: {} files found, {} processed",
            country_code, total_files, processed_files
        );
        Ok((total_files, processed_files))
    }

    /// Process a single file for upload
    async fn process_file_for_upload(
        &self,
        file_path: &std::path::Path,
        country_code: &str,
        locality_id: u32,
    ) -> Result<bool, NodeOpsError> {
        // Check if already uploaded
        if self
            .db_service
            .has_cid_mapping(country_code, locality_id)
            .await?
        {
            info!("Locality {} already uploaded, skipping", locality_id);
            return Ok(false);
        }

        // Create pending upload
        let pending_upload = PendingUpload::new(
            country_code.to_string(),
            locality_id,
            file_path.to_path_buf(),
        );

        // Add to queue
        {
            let mut queue = self.upload_queue.lock().await;
            if let Err(e) = queue.add_upload(pending_upload) {
                warn!("Failed to add upload to queue: {}", e);
                return Ok(false);
            }
        }

        // Process queue if it's full
        if self.upload_queue.lock().await.is_full() {
            self.process_upload_queue().await?;
        }

        Ok(true)
    }

    /// Process the upload queue
    async fn process_upload_queue(&self) -> Result<(), NodeOpsError> {
        let batch = {
            let mut queue = self.upload_queue.lock().await;
            queue.take_batch()
        };

        if batch.is_empty() {
            return Ok(());
        }

        info!("Processing batch of {} uploads", batch.len());

        // Upload all files in batch concurrently
        let upload_tasks: Vec<_> = batch
            .into_iter()
            .map(|pending| self.upload_single_file(pending))
            .collect();

        let results = join_all(upload_tasks).await;

        // Separate successful and failed uploads
        let mut successful_uploads = Vec::new();
        let mut failed_count = 0;

        for result in results {
            match result {
                Ok(upload) => successful_uploads.push(upload),
                Err(e) => {
                    error!("Upload failed: {}", e);
                    failed_count += 1;
                }
            }
        }

        // Update database with successful uploads
        if !successful_uploads.is_empty() {
            self.batch_update_cid_mappings(&successful_uploads).await?;

            // Update stats
            let mut stats = self.stats.lock().await;
            for upload in &successful_uploads {
                stats.increment_uploaded(upload.file_size);
            }
        }

        // Update failed stats
        {
            let mut stats = self.stats.lock().await;
            for _ in 0..failed_count {
                stats.increment_failed();
            }
        }

        info!(
            "Batch completed: {} successful, {} failed",
            successful_uploads.len(),
            failed_count
        );

        Ok(())
    }

    /// Upload a single file to Codex using the managed node
    async fn upload_single_file(
        &self,
        pending: PendingUpload,
    ) -> Result<CompletedUpload, NodeOpsError> {
        let file_path = &pending.file_path;

        // Verify file exists before attempting upload
        if !file_path.exists() {
            return Err(NodeOpsError::FileError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {:?}", file_path),
            )));
        }

        // Get file size
        let file_size = tokio::fs::metadata(file_path).await?.len();

        info!(
            "Uploading locality {} from country {} ({} bytes) using managed node",
            pending.locality_id, pending.country_code, file_size
        );

        // Create upload options with progress callback
        let locality_id = pending.locality_id;
        let country_code = pending.country_code.clone();

        let upload_options =
            UploadOptions::new()
                .filepath(file_path)
                .on_progress(move |progress| {
                    let percentage = (progress.percentage * 100.0) as u32;
                    info!(
                        "Upload progress for locality {} ({}): {}%",
                        locality_id, country_code, percentage
                    );
                });

        // Use the managed node instead of creating a temporary one
        let upload_result = self
            .node_manager
            .upload_file(upload_options)
            .await
            .map_err(|e| {
                error!("Upload failed for locality {}: {}", pending.locality_id, e);
                e
            })?;

        let completed_upload = CompletedUpload::new(
            pending.country_code.clone(),
            pending.locality_id,
            upload_result.cid.clone(),
            file_size,
        );

        info!(
            "Successfully uploaded locality {} with CID: {} using managed node",
            pending.locality_id, upload_result.cid
        );

        Ok(completed_upload)
    }

    /// Batch update CID mappings in database
    async fn batch_update_cid_mappings(
        &self,
        uploads: &[CompletedUpload],
    ) -> Result<(), NodeOpsError> {
        let mappings: Vec<_> = uploads
            .iter()
            .map(|upload| {
                (
                    upload.country_code.clone(),
                    upload.locality_id,
                    upload.cid.clone(),
                    upload.file_size,
                )
            })
            .collect();

        self.db_service.batch_insert_cid_mappings(&mappings).await?;

        info!("Updated {} CID mappings in database", mappings.len());
        Ok(())
    }

    /// Get all countries that have localities from the database
    async fn get_all_countries(&self) -> Result<Vec<String>, NodeOpsError> {
        // For now, return a list of common countries
        // This can be enhanced later to query from database
        Ok(vec![
            "US".to_string(),
            "CA".to_string(),
            "GB".to_string(),
            "DE".to_string(),
            "FR".to_string(),
            "IT".to_string(),
            "ES".to_string(),
            "JP".to_string(),
            "AU".to_string(),
            "BR".to_string(),
        ])
    }

    /// Get the file path for a locality's PMTiles file
    fn get_locality_file_path(&self, country_code: &str, locality_id: u32) -> std::path::PathBuf {
        // This should match the extraction service's output pattern
        format!("assets/localities/{}/{}.pmtiles", country_code, locality_id).into()
    }

    /// Get current upload statistics
    pub async fn get_stats(&self) -> UploadStats {
        self.stats.lock().await.clone()
    }
}
