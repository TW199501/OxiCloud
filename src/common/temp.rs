//! Shared helper for creating upload spool temp files.
//!
//! Upload paths spool the request body to a temp file before deduplication.
//! By default `tempfile` uses the OS temp dir (`std::env::temp_dir()`, i.e.
//! `$TMPDIR` / `/tmp`), which in many container setups is **tmpfs (RAM)**.
//! Writing a multi-hundred-MB upload there fills page-cache that counts
//! against the cgroup memory limit and can OOMKill the process. Pointing the
//! spool at a real-disk directory (`OXICLOUD_UPLOAD_TMPDIR`) keeps the upload
//! footprint proportional to the streaming buffer, not the file size.

use std::path::Path;
use tempfile::NamedTempFile;

/// Create a [`NamedTempFile`], honoring an optional configured spool directory.
///
/// When `dir` is `Some`, the temp file is created there (the directory is
/// created if missing); otherwise the OS default temp dir is used.
pub fn new_spool_temp_file(dir: Option<&Path>) -> std::io::Result<NamedTempFile> {
    match dir {
        Some(d) => {
            std::fs::create_dir_all(d)?;
            NamedTempFile::new_in(d)
        }
        None => NamedTempFile::new(),
    }
}
