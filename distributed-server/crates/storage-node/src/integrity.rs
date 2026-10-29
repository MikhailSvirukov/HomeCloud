//! Local blob integrity verification.
//!
//! SRS: `FR-CL-021 / 2`.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::metadata::{ContentHash, UploadRecord, CONTENT_HASH_LEN};
use crate::storage_layout::{LayoutError, StorageLayout};

/// Suffix of the sidecar marker file created for a corrupted local blob.
pub const CORRUPT_MARKER_SUFFIX: &str = ".corrupt";
/// Read chunk used for streaming the digest.
const READ_CHUNK_LEN: usize = 64 * 1024;

/// State of a local committed blob after an integrity scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobIntegrity {
    /// The local bytes match the committed SHA-256.
    Healthy,
    /// The local bytes differ from the committed SHA-256.
    Corrupted,
}

/// Failure modes when verifying a local committed blob.
#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    /// The committed login does not form a valid blob path.
    #[error("the committed login does not form a valid blob path")]
    Layout(#[from] LayoutError),
    /// The local blob file does not exist.
    #[error("the local blob is missing")]
    Missing(#[source] io::Error),
    /// The local blob could not be read.
    #[error("the local blob could not be read")]
    Read(#[source] io::Error),
    /// The corruption marker could not be updated.
    #[error("the corruption marker could not be updated")]
    Marker(#[source] io::Error),
}

/// Streams the local blob at the committed path, compares its SHA-256 with the committed hash, and marks a mismatch with a sidecar `<blob>.corrupt` file; a matching blob removes any stale marker.
pub fn verify_local_blob(
    layout: &StorageLayout,
    record: &UploadRecord,
) -> Result<BlobIntegrity, IntegrityError> {
    let blob = layout.blob_path(&record.login, *record.upload_id.as_bytes())?;
    let mut file = match File::open(&blob) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(IntegrityError::Missing(error));
        }
        Err(error) => return Err(IntegrityError::Read(error)),
    };

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; READ_CHUNK_LEN];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => return Err(IntegrityError::Read(error)),
        };
        hasher.update(&buffer[..read]);
    }
    let digest: [u8; CONTENT_HASH_LEN] = hasher.finalize().into();
    let computed = ContentHash::from_bytes(digest);

    let marker = marker_path(&blob);
    if computed == record.content_hash {
        match std::fs::remove_file(&marker) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(IntegrityError::Marker(error)),
        }
        return Ok(BlobIntegrity::Healthy);
    }

    std::fs::write(&marker, []).map_err(IntegrityError::Marker)?;
    Ok(BlobIntegrity::Corrupted)
}

fn marker_path(blob: &Path) -> PathBuf {
    let mut marker = blob.as_os_str().to_owned();
    marker.push(CORRUPT_MARKER_SUFFIX);
    PathBuf::from(marker)
}
