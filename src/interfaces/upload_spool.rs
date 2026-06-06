//! Shared streaming upload spool: request body → temp file + incremental hash.
//!
//! Used by both the native WebDAV PUT handler and the NextCloud-compat PUT
//! handler so neither buffers the full request body in memory. Peak heap is
//! ~one HTTP frame regardless of file size; the body is written to a temp
//! file (off tmpfs when [`StorageConfig::upload_temp_dir`] is configured) and
//! BLAKE3-hashed on the fly so the dedup layer can short-circuit on a hit.

use std::path::PathBuf;

use axum::body::Body;
use http_body_util::BodyStream;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

use crate::common::temp::new_spool_temp_file;
use crate::interfaces::errors::AppError;

/// Outcome of spooling a request body to disk.
pub struct SpooledBody {
    /// The temp file holding the body. Kept alive by the caller (dropping it
    /// removes the file unless the dedup layer already consumed/moved it).
    pub temp: NamedTempFile,
    /// Hex-encoded BLAKE3 of the full body — matches `DedupService::hash_file`,
    /// so passing it as `pre_computed_hash` enables the dedup fast path.
    pub hash: String,
    /// Total bytes written.
    pub size: u64,
}

/// Stream an HTTP request body to a temp file, computing its BLAKE3 hash
/// incrementally and enforcing `max_upload` as a hard size limit.
///
/// Peak heap is ~one frame — the body is never fully buffered in RAM.
///
/// `temp_dir` is taken by value (not `&Path`) so the returned future captures
/// no borrowed lifetime — required for the handler future to stay `Send`.
pub async fn spool_body_to_temp(
    body: Body,
    max_upload: usize,
    temp_dir: Option<PathBuf>,
) -> Result<SpooledBody, AppError> {
    let temp = new_spool_temp_file(temp_dir.as_deref())
        .map_err(|e| AppError::internal_error(format!("Failed to create temp file: {e}")))?;
    let temp_path = temp.path().to_path_buf();

    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| AppError::internal_error(format!("Failed to open temp file: {e}")))?;

    let mut hasher = blake3::Hasher::new();
    let mut total_bytes: usize = 0;
    let mut stream = BodyStream::new(body);

    while let Some(frame_result) = stream.next().await {
        let frame = frame_result
            .map_err(|e| AppError::bad_request(format!("Failed to read request body: {e}")))?;
        if let Some(chunk) = frame.data_ref() {
            total_bytes += chunk.len();
            if total_bytes > max_upload {
                // Abort early — stop reading, delete temp file.
                drop(file);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AppError::payload_too_large(format!(
                    "Upload exceeds maximum size of {max_upload} bytes"
                )));
            }
            hasher.update(chunk);
            file.write_all(chunk).await.map_err(|e| {
                AppError::internal_error(format!("Failed to write to temp file: {e}"))
            })?;
        }
    }
    file.flush()
        .await
        .map_err(|e| AppError::internal_error(format!("Failed to flush temp file: {e}")))?;
    drop(file);

    let hash = hasher.finalize().to_hex().to_string();
    Ok(SpooledBody {
        temp,
        hash,
        size: total_bytes as u64,
    })
}
