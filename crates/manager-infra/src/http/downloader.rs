use async_trait::async_trait;
use futures_util::StreamExt;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::runtime::DownloadPort;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct ReqwestDownloader {
    client: reqwest::Client,
}

impl ReqwestDownloader {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("StardewModManager/0.1.0")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

impl Default for ReqwestDownloader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DownloadPort for ReqwestDownloader {
    async fn download_file(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf> {
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AppError::system(
                    "DOWNLOAD_DIR_ERROR",
                    format!(
                        "Failed to create download directory '{}': {}",
                        parent.display(),
                        e
                    ),
                )
            })?;
        }

        let temp_dest = destination.with_extension(format!(
            "{}.tmp",
            destination
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("download")
        ));

        let response = self.client.get(url).send().await.map_err(|e| {
            AppError::network(
                "DOWNLOAD_REQUEST_FAILED",
                format!("Failed to connect to '{}': {}", url, e),
            )
        })?;

        if !response.status().is_success() {
            return Err(AppError::network(
                "DOWNLOAD_HTTP_ERROR",
                format!(
                    "HTTP error {} while downloading '{}'",
                    response.status(),
                    url
                ),
            ));
        }

        let mut file = std::fs::File::create(&temp_dest).map_err(|e| {
            AppError::system(
                "DOWNLOAD_FILE_CREATE_FAILED",
                format!(
                    "Failed to create destination file '{}': {}",
                    temp_dest.display(),
                    e
                ),
            )
        })?;

        let mut hasher = Sha256::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| {
                AppError::network(
                    "DOWNLOAD_STREAM_ERROR",
                    format!("Network error while streaming '{}': {}", url, e),
                )
            })?;
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|e| {
                AppError::storage(
                    "DOWNLOAD_WRITE_FAILED",
                    format!("Failed to write chunk to '{}': {}", temp_dest.display(), e),
                )
            })?;
        }

        file.flush().map_err(|e| {
            AppError::storage("DOWNLOAD_FLUSH_FAILED", format!("Failed to flush: {}", e))
        })?;
        drop(file);

        let actual_hash = format!("{:x}", hasher.finalize());
        if let Some(expected) = expected_sha256 {
            if !actual_hash.eq_ignore_ascii_case(expected) {
                let _ = std::fs::remove_file(&temp_dest);
                return Err(AppError::validation(
                    "DOWNLOAD_HASH_MISMATCH",
                    format!(
                        "Downloaded file checksum mismatch: expected {}, got {}",
                        expected, actual_hash
                    ),
                ));
            }
        }

        std::fs::rename(&temp_dest, destination).map_err(|e| {
            AppError::storage(
                "DOWNLOAD_RENAME_FAILED",
                format!(
                    "Failed to move downloaded file into '{}': {}",
                    destination.display(),
                    e
                ),
            )
        })?;

        Ok(destination.to_path_buf())
    }
}
