use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use std::path::PathBuf;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
};

pub async fn serve_pmtiles(
    State(app_state): State<AppState>,
    Path((country_code, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let config = app_state.config.lock().await;
    let file_path = PathBuf::from(&config.assets_dir)
        .join("localities")
        .join(country_code)
        .join(format!("{}.pmtiles", id));

    // Check if file exists and get its metadata
    let metadata = match tokio::fs::metadata(&file_path).await {
        Ok(metadata) => metadata,
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };
    let file_size = metadata.len();

    // Handle range requests (HTTP 206 Partial Content)
    if let Some(range_header) = headers.get("Range") {
        if let Ok(range_str) = range_header.to_str() {
            // Parse range header: "bytes=start-end" or "bytes=start-"
            if let Some(caps) = regex::Regex::new(r"bytes=(\d+)-(\d*)")
                .unwrap()
                .captures(range_str)
            {
                let start: u64 = caps[1].parse().unwrap_or(0);
                let end = if caps[2].is_empty() {
                    file_size - 1
                } else {
                    caps[2].parse().unwrap_or(file_size - 1)
                };

                // Validate range
                if start < file_size && end < file_size && start <= end {
                    let content_length = end - start + 1;

                    // Open file and seek to start position
                    let mut file = match File::open(&file_path).await {
                        Ok(file) => file,
                        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
                    };

                    match file.seek(std::io::SeekFrom::Start(start)).await {
                        Ok(_) => {}
                        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
                    }

                    // Read the specific range
                    let mut buffer = vec![0u8; content_length.try_into().unwrap_or(0)];
                    match file.read_exact(&mut buffer).await {
                        Ok(_) => {}
                        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
                    }

                    // Return partial content response
                    return Ok(Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("Content-Type", "application/octet-stream")
                        .header("Content-Length", content_length.to_string())
                        .header(
                            "Content-Range",
                            format!("bytes {}-{}/{}", start, end, file_size),
                        )
                        .header("Accept-Ranges", "bytes")
                        .body(Body::from(buffer))
                        .unwrap());
                }
            }
        }
    }

    // Full file response (HTTP 200)
    let file = match File::open(&file_path).await {
        Ok(file) => file,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", file_size.to_string())
        .header("Accept-Ranges", "bytes")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}.pmtiles\"", id),
        )
        .body(body)
        .unwrap())
}
