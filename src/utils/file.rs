use futures::StreamExt;
use reqwest;
use std::fs;
use std::path::Path;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::info;

#[derive(Error, Debug)]
pub enum FileError {
    #[error("Download failed: {0}")]
    DownloadFailed(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("Tokio IO error: {0}")]
    TokioIoError(#[from] tokio::io::Error),
}

pub async fn download_file_with_progress(url: &str, destination: &Path) -> Result<(), FileError> {
    let response = reqwest::get(url).await?;

    if !response.status().is_success() {
        return Err(FileError::DownloadFailed(format!(
            "HTTP error: {}",
            response.status()
        )));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut file = File::create(destination).await?;
    let mut stream = response.bytes_stream();

    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let percent = (downloaded as f64 / total_size as f64) * 100.0;
            info!(
                "Download progress: {:.2}% ({:.2} MB / {:.2} MB)",
                percent,
                downloaded as f64 / 1_048_576.0,
                total_size as f64 / 1_048_576.0
            );
        }
    }

    info!("");

    Ok(())
}

pub fn ensure_dir_exists(path: &Path) -> Result<(), FileError> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| FileError::IoError(e.to_string()))?;
    }
    Ok(())
}
