//! Restoring a marked-corrupt or missing local blob from a verified source.
//!
//! SRS: `FR-CL-025 / 1`, supporting `FR-CL-021 / 2`, `NFR-REL-008 / 2`.

use std::io::{self, Read};
use std::path::Path;

use crate::blob_writer::{write_blob_direct, BlobWriteError, StoredBlob};
use crate::integrity::corrupt_marker_path;
use crate::metadata::ContentHash;

/// Failure modes when restoring a local blob.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// The local blob exists without a corruption marker.
    #[error("the local blob exists without a corruption marker")]
    ExistingBlob,
    /// The previous blob or marker could not be removed.
    #[error("the previous blob or marker could not be removed")]
    Cleanup(#[source] io::Error),
    /// The corruption marker could not be updated.
    #[error("the corruption marker could not be updated")]
    Marker(#[source] io::Error),
    /// The restored blob could not be written.
    #[error("the restored blob could not be written")]
    Write(#[from] BlobWriteError),
}

/// Replaces a missing or marked-corrupt blob with verified, durable bytes from `reader`; an existing blob without a marker is refused.
pub fn restore_blob_direct<R: Read>(
    path: &Path,
    reader: R,
    expected_hash: ContentHash,
    expected_size: u64,
) -> Result<StoredBlob, RestoreError> {
    let marker = corrupt_marker_path(path);
    if path.exists() && !marker.exists() {
        return Err(RestoreError::ExistingBlob);
    }
    if path.exists() {
        std::fs::remove_file(path).map_err(RestoreError::Cleanup)?;
    }

    let stored = match write_blob_direct(path, reader, expected_hash, expected_size) {
        Ok(stored) => stored,
        Err(error) => {
            std::fs::write(&marker, []).map_err(RestoreError::Marker)?;
            return Err(RestoreError::Write(error));
        }
    };

    match std::fs::remove_file(&marker) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(RestoreError::Cleanup(error)),
    }
    Ok(stored)
}
