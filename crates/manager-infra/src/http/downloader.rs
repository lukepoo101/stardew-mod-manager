use async_trait::async_trait;
use futures_util::StreamExt;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::runtime::DownloadPort;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct ReqwestDownloader {
    client: reqwest::Client,
    /// How long to wait before the one retry of a request that timed out.
    retry_after: Duration,
}

impl ReqwestDownloader {
    pub fn new() -> Self {
        // A connection that cannot be made, or that stops sending, ends
        // the download rather than leaving it waiting forever.
        Self::with_timeouts(
            Duration::from_secs(20),
            Duration::from_secs(60),
            Duration::from_secs(2),
        )
    }

    fn with_timeouts(connect: Duration, read: Duration, retry_after: Duration) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("StardewModManager/0.1.0")
                .connect_timeout(connect)
                .read_timeout(read)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            retry_after,
        }
    }

    /// Sends the request, trying once more when the first try could not
    /// connect or got no answer in time. Nothing has been received at that
    /// point, so a second try cannot mix two downloads.
    async fn send(&self, url: &str) -> reqwest::Result<reqwest::Response> {
        match self.client.get(url).send().await {
            Err(e) if e.is_timeout() || e.is_connect() => {
                tokio::time::sleep(self.retry_after).await;
                self.client.get(url).send().await
            }
            other => other,
        }
    }

    #[allow(clippy::result_large_err)]
    async fn stream_to_temp(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        temp_dest: &Path,
        max_bytes: Option<u64>,
    ) -> AppResult<()> {
        let response = self.send(url).await.map_err(|e| {
            AppError::network(
                "DOWNLOAD_REQUEST_FAILED",
                if e.is_timeout() {
                    "The download did not start: the server took too long to answer. Check your internet connection and try again."
                } else {
                    "The download could not start. Check your internet connection and try again."
                },
            )
            .with_details(format!("{url}: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(AppError::network(
                "DOWNLOAD_HTTP_ERROR",
                if status.is_server_error() {
                    format!("The download site is having trouble (it answered {status}). Try again later.")
                } else if status == reqwest::StatusCode::NOT_FOUND {
                    "The file is no longer at the address it was published at. Check for updates again.".to_string()
                } else {
                    format!("The download site refused the download (it answered {status}).")
                },
            )
            .with_details(format!("HTTP {status} for {url}")));
        }
        let too_large = |limit: u64| {
            AppError::validation(
                "DOWNLOAD_TOO_LARGE",
                format!(
                    "The download was stopped: it is larger than the {} the file was published as, so it is not the file expected. Nothing was kept.",
                    describe_size(limit)
                ),
            )
            .with_details(format!("more than {limit} bytes from {url}"))
        };
        if let (Some(limit), Some(length)) = (max_bytes, response.content_length()) {
            if length > limit {
                return Err(too_large(limit));
            }
        }

        let mut file = std::fs::File::create(temp_dest).map_err(|e| {
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
        let mut received: u64 = 0;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| {
                AppError::network(
                    "DOWNLOAD_STREAM_ERROR",
                    if e.is_timeout() {
                        "The download stopped: nothing arrived for a minute. Check your internet connection and try again."
                    } else {
                        "The download stopped part way. Check your internet connection and try again."
                    },
                )
                .with_details(format!("{url}: {e}"))
            })?;
            received += chunk.len() as u64;
            if max_bytes.is_some_and(|limit| received > limit) {
                return Err(too_large(max_bytes.unwrap_or_default()));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|e| {
                AppError::storage(
                    "DOWNLOAD_WRITE_FAILED",
                    format!("Failed to write chunk to '{}': {}", temp_dest.display(), e),
                )
            })?;
        }

        file.sync_all().map_err(|e| {
            AppError::storage(
                "DOWNLOAD_FLUSH_FAILED",
                format!("Failed to flush '{}': {}", temp_dest.display(), e),
            )
        })?;
        drop(file);

        let actual_hash = manager_core::ids::hash_to_hex(hasher.finalize());
        if let Some(expected) = expected_sha256 {
            if !actual_hash.eq_ignore_ascii_case(expected) {
                return Err(AppError::validation(
                    "DOWNLOAD_HASH_MISMATCH",
                    "The downloaded file is not the one published: its fingerprint does not match, so it was discarded. Try again; if it keeps happening, the file may have been changed.",
                )
                .with_details(format!(
                    "SHA-256 expected {expected}, got {actual_hash}"
                )));
            }
        }

        Ok(())
    }
}

impl Default for ReqwestDownloader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DownloadPort for ReqwestDownloader {
    async fn ensure_downloaded(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf> {
        if destination.is_file() {
            return Ok(destination.to_path_buf());
        }
        self.download_file(url, expected_sha256, destination).await
    }

    async fn download_file(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf> {
        self.download_to(url, expected_sha256, destination, None)
            .await
    }

    async fn download_file_limited(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
        max_bytes: u64,
    ) -> AppResult<PathBuf> {
        self.download_to(url, expected_sha256, destination, Some(max_bytes))
            .await
    }
}

impl ReqwestDownloader {
    #[allow(clippy::result_large_err)]
    async fn download_to(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
        max_bytes: Option<u64>,
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

        // A unique temporary name keeps concurrent downloads sharing a destination
        // from writing over each other.
        let temp_dest = destination.with_file_name(format!(
            "{}.{}.part",
            destination
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("download"),
            uuid::Uuid::new_v4()
        ));

        if let Err(e) = self
            .stream_to_temp(url, expected_sha256, &temp_dest, max_bytes)
            .await
        {
            let _ = std::fs::remove_file(&temp_dest);
            return Err(e);
        }

        if let Err(e) = crate::platform::shared::fs::rename_path(&temp_dest, destination) {
            let _ = std::fs::remove_file(&temp_dest);
            return Err(AppError::storage(
                "DOWNLOAD_RENAME_FAILED",
                format!(
                    "Failed to move downloaded file into '{}': {}",
                    destination.display(),
                    e
                ),
            ));
        }

        Ok(destination.to_path_buf())
    }
}

fn describe_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A server that leaves the first `silent` requests unanswered, then
    /// answers with `body`. Returns its address and how many requests came.
    fn flaky_server(silent: usize, body: &'static [u8]) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/file.zip", listener.local_addr().unwrap());
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                if counter.fetch_add(1, Ordering::SeqCst) < silent {
                    held.push(stream);
                    continue;
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            }
        });
        (address, seen)
    }

    fn quick_downloader() -> ReqwestDownloader {
        ReqwestDownloader::with_timeouts(
            Duration::from_secs(2),
            Duration::from_millis(300),
            Duration::from_millis(10),
        )
    }

    #[tokio::test]
    async fn a_request_with_no_answer_is_tried_once_more() {
        let (url, seen) = flaky_server(1, b"mod archive");
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.zip");
        let path = quick_downloader()
            .download_file(&url, None, &dest)
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"mod archive");
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_second_request_with_no_answer_fails() {
        let (url, seen) = flaky_server(usize::MAX, b"");
        let dir = tempfile::tempdir().unwrap();
        let error = quick_downloader()
            .download_file(&url, None, &dir.path().join("file.zip"))
            .await
            .unwrap_err();
        assert_eq!(error.code, "DOWNLOAD_REQUEST_FAILED");
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }
}
