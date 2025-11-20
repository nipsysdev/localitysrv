use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PendingUpload {
    pub country_code: String,
    pub locality_id: u32,
    pub file_path: PathBuf,
}

impl PendingUpload {
    pub fn new(country_code: String, locality_id: u32, file_path: PathBuf) -> Self {
        Self {
            country_code,
            locality_id,
            file_path,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompletedUpload {
    pub country_code: String,
    pub locality_id: u32,
    pub cid: String,
    pub file_size: u64,
}

impl CompletedUpload {
    pub fn new(country_code: String, locality_id: u32, cid: String, file_size: u64) -> Self {
        Self {
            country_code,
            locality_id,
            cid,
            file_size,
        }
    }
}

#[derive(Debug)]
pub struct UploadQueue {
    pending_uploads: VecDeque<PendingUpload>,
    batch_size: usize,
    max_queue_size: usize,
}

impl UploadQueue {
    pub fn new(batch_size: usize, max_queue_size: usize) -> Self {
        Self {
            pending_uploads: VecDeque::new(),
            batch_size,
            max_queue_size,
        }
    }

    pub fn add_upload(&mut self, upload: PendingUpload) -> Result<(), QueueError> {
        if self.pending_uploads.len() >= self.max_queue_size {
            return Err(QueueError::QueueFull);
        }
        self.pending_uploads.push_back(upload);
        Ok(())
    }

    pub fn take_batch(&mut self) -> Vec<PendingUpload> {
        let batch_size = std::cmp::min(self.batch_size, self.pending_uploads.len());
        (0..batch_size)
            .map(|_| self.pending_uploads.pop_front().unwrap())
            .collect()
    }

    pub fn is_full(&self) -> bool {
        self.pending_uploads.len() >= self.batch_size
    }

    pub fn is_empty(&self) -> bool {
        self.pending_uploads.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Upload queue is full")]
    QueueFull,
}

#[derive(Debug, Clone)]
pub struct UploadStats {
    pub total_uploaded: u64,
    pub total_failed: u64,
    pub total_bytes_uploaded: u64,
}

impl UploadStats {
    pub fn new() -> Self {
        Self {
            total_uploaded: 0,
            total_failed: 0,
            total_bytes_uploaded: 0,
        }
    }

    pub fn increment_uploaded(&mut self, bytes: u64) {
        self.total_uploaded += 1;
        self.total_bytes_uploaded += bytes;
    }

    pub fn increment_failed(&mut self) {
        self.total_failed += 1;
    }
}

impl Default for UploadStats {
    fn default() -> Self {
        Self::new()
    }
}
