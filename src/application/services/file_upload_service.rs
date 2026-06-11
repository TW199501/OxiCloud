use std::sync::Arc;

use crate::application::dtos::file_dto::FileDto;
use crate::application::ports::file_lifecycle::FileLifecycleHook;
use crate::application::ports::file_ports::{FileUploadUseCase, StoredBlob};
use crate::application::ports::storage_ports::{FileReadPort, FileWritePort};
use crate::application::services::storage_usage_service::StorageUsageService;
use crate::common::errors::DomainError;
use crate::infrastructure::repositories::pg::FileBlobReadRepository;
use crate::infrastructure::repositories::pg::FileBlobWriteRepository;
use crate::infrastructure::services::file_content_cache::FileContentCache;
use tracing::{debug, info, warn};

/// Helper function to extract username from folder path string.
/// e.g. "My Folder - user1/subfolder/file.txt" → "user1"
fn extract_username_from_path(path: &str) -> Option<String> {
    if !path.contains("My Folder - ") {
        return None;
    }
    let parts: Vec<&str> = path.split("My Folder - ").collect();
    if parts.len() <= 1 {
        return None;
    }
    let remainder = parts[1].trim();
    let username = remainder.split('/').next().unwrap_or(remainder);
    let username = username.trim();
    if username.is_empty() {
        return None;
    }
    Some(username.to_string())
}

/// Service for file upload operations.
///
/// Content never passes through this service: the interface layer streams
/// the request body straight into the CDC chunk store (no spool file, no
/// full-body buffering) and hands over a [`StoredBlob`] reference. This
/// service registers the metadata row, keeps caches coherent and fires
/// lifecycle hooks. Blob-reference ownership is consumed by the write
/// port, which releases it on failure — callers never compensate.
pub struct FileUploadService {
    /// Write port — registers file rows against ingested blobs.
    file_write: Arc<FileBlobWriteRepository>,
    /// Read port — needed for WebDAV/WOPI update-by-path.
    file_read: Option<Arc<FileBlobReadRepository>>,
    /// Optional storage usage tracking
    storage_usage_service: Option<Arc<StorageUsageService>>,
    /// Content cache — invalidated on file update so stale content is never served.
    content_cache: Option<Arc<FileContentCache>>,
    /// Single lifecycle dispatcher — fires on_file_created / on_file_updated.
    file_lifecycle_hook: Option<Arc<dyn FileLifecycleHook>>,
}

impl FileUploadService {
    /// Constructor with write port only (minimal).
    pub fn new(file_repository: Arc<FileBlobWriteRepository>) -> Self {
        Self {
            file_write: file_repository,
            file_read: None,
            storage_usage_service: None,
            content_cache: None,
            file_lifecycle_hook: None,
        }
    }

    /// Constructor for blob-storage model: write + read ports.
    pub fn new_with_read(
        file_write: Arc<FileBlobWriteRepository>,
        file_read: Arc<FileBlobReadRepository>,
    ) -> Self {
        Self {
            file_write,
            file_read: Some(file_read),
            storage_usage_service: None,
            content_cache: None,
            file_lifecycle_hook: None,
        }
    }

    /// Configures the content cache for invalidation on file updates.
    pub fn with_content_cache(mut self, cache: Arc<FileContentCache>) -> Self {
        self.content_cache = Some(cache);
        self
    }

    /// Registers the lifecycle hook dispatcher (thumbnails, audio metadata, …).
    pub fn with_file_lifecycle_hook(mut self, hook: Arc<dyn FileLifecycleHook>) -> Self {
        self.file_lifecycle_hook = Some(hook);
        self
    }

    /// Configures the storage usage service
    pub fn with_storage_usage_service(
        mut self,
        storage_usage_service: Arc<StorageUsageService>,
    ) -> Self {
        self.storage_usage_service = Some(storage_usage_service);
        self
    }

    // ── private helpers ──────────────────────────────────────────

    /// Optionally update storage usage after a successful upload.
    fn maybe_update_storage_usage(&self, file: &FileDto) {
        if let Some(storage_service) = &self.storage_usage_service {
            let file_path = file.path.clone();
            if let Some(username) = extract_username_from_path(&file_path) {
                let service_clone = Arc::clone(storage_service);
                tokio::spawn(async move {
                    match service_clone
                        .update_user_storage_usage_by_username(&username)
                        .await
                    {
                        Ok(usage) => debug!(
                            "Updated storage usage for user {} to {} bytes",
                            username, usage
                        ),
                        Err(e) => warn!("Failed to update storage usage for {}: {}", username, e),
                    }
                });
            }
        }
    }
}

impl FileUploadUseCase for FileUploadService {
    /// Register a new file row pointing at an already-ingested blob.
    async fn upload_file_streaming(
        &self,
        name: String,
        folder_id: Option<String>,
        content_type: String,
        blob: StoredBlob,
    ) -> Result<FileDto, DomainError> {
        let file = self
            .file_write
            .save_file_with_blob(name.clone(), folder_id, content_type, &blob.hash, blob.size)
            .await?;
        let dto = FileDto::from(file);
        info!(
            "📡 STREAMING UPLOAD: {} ({} bytes, ID: {})",
            name, blob.size, dto.id
        );
        self.maybe_update_storage_usage(&dto);
        if let Some(hook) = &self.file_lifecycle_hook {
            hook.on_file_created(&dto.id, &dto.content_hash, &dto.mime_type, blob.is_new_blob);
        }
        Ok(dto)
    }

    /// Swap the content of the file at `path` to an already-ingested blob,
    /// creating the file when it doesn't exist (WebDAV/NextCloud/WOPI PUT).
    async fn update_file_streaming(
        &self,
        path: &str,
        blob: StoredBlob,
        content_type: &str,
        modified_at: Option<i64>,
    ) -> Result<FileDto, DomainError> {
        // Try to find the existing file first
        if let Some(file_read) = &self.file_read
            && let Some(file) = file_read.find_file_by_path(path).await?
        {
            let file_id = file.id().to_string();
            let (new_hash, updated_at) = self
                .file_write
                .update_file_content_with_blob(&file_id, &blob.hash, blob.size, modified_at)
                .await?;
            // Invalidate content cache — file content has changed.
            if let Some(cc) = &self.content_cache {
                cc.invalidate(&file_id).await;
            }
            // Rebuild the fresh DTO from the entity already in hand plus the
            // values the UPDATE just returned — a re-read would only fetch
            // what we already know, at one extra round-trip per overwrite
            // (WebDAV sync clients overwrite constantly).
            let parts = file.into_parts();
            let updated = crate::domain::entities::file::File::with_timestamps_and_blob_hash(
                parts.id,
                parts.name,
                parts.storage_path,
                blob.size,
                parts.mime_type,
                parts.folder_id,
                parts.created_at,
                updated_at as u64,
                parts.owner_id,
                new_hash,
            )
            .map_err(|e| {
                DomainError::internal_error("FileUpload", format!("rebuild entity: {e}"))
            })?;
            let dto = FileDto::from(updated);
            if let Some(hook) = &self.file_lifecycle_hook {
                hook.on_file_updated(&file_id, &dto.content_hash, content_type);
            }
            return Ok(dto);
        }

        // File doesn't exist — create it via streaming upload
        let path_normalized = path.trim_start_matches('/').trim_end_matches('/');
        let (_, filename) = if let Some(idx) = path_normalized.rfind('/') {
            (&path_normalized[..idx], &path_normalized[idx + 1..])
        } else {
            ("", path_normalized)
        };

        // get_parent_folder_id expects the full file path — it strips the
        // last segment (filename) internally to find the parent folder.
        let parent_id = if path_normalized.contains('/') {
            if let Some(file_read) = &self.file_read {
                file_read.get_parent_folder_id(path_normalized).await.ok()
            } else {
                None
            }
        } else {
            None
        };

        let is_new_blob = blob.is_new_blob;
        let created = self
            .file_write
            .save_file_with_blob(
                filename.to_string(),
                parent_id,
                content_type.to_string(),
                &blob.hash,
                blob.size,
            )
            .await?;
        let dto = FileDto::from(created);
        if let Some(hook) = &self.file_lifecycle_hook {
            hook.on_file_created(&dto.id, &dto.content_hash, content_type, is_new_blob);
        }
        Ok(dto)
    }
}
