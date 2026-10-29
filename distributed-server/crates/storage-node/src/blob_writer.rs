//! Direct durable blob writes.
//!
//! SRS: `FR-UP-012 / 1`, `FR-UP-013 / 1`, `FR-UP-022 / 1`, `NFR-REL-008 / 2`.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::metadata::{ContentHash, CONTENT_HASH_LEN};

/// Read chunk used while streaming the payload.
const READ_CHUNK_LEN: usize = 64 * 1024;
/// Error message used when the streamed size exceeds `u64`.
const SIZE_OVERFLOW_MESSAGE: &str = "blob size overflow";

/// A blob written to its final location and synced to stable storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// Final location of the written blob.
    pub path: PathBuf,
    /// Number of bytes streamed into the blob.
    pub size_bytes: u64,
    /// SHA-256 computed from the streamed bytes.
    pub content_hash: ContentHash,
}

/// Failure modes when writing a blob directly to its final location.
#[derive(Debug, thiserror::Error)]
pub enum BlobWriteError {
    /// The final blob path already existed and was left untouched.
    #[error("the blob already exists")]
    AlreadyExists(#[source] io::Error),
    /// The parent blob directory could not be created.
    #[error("the blob directory could not be created")]
    CreateDirectory(#[source] io::Error),
    /// The final blob file could not be created.
    #[error("the blob could not be created")]
    Create(#[source] io::Error),
    /// The source payload could not be read.
    #[error("the source could not be read")]
    Read(#[source] io::Error),
    /// The payload could not be written to the final blob file.
    #[error("the blob could not be written")]
    Write(#[source] io::Error),
    /// The blob file or its parent directory could not be synced.
    #[error("the blob could not be synced")]
    Sync(#[source] io::Error),
    /// The streamed byte count differs from the declared size.
    #[error("the blob size does not match: expected {expected}, actual {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    /// The streamed SHA-256 differs from the declared hash.
    #[error("the blob hash does not match the declared hash")]
    HashMismatch,
    /// The rejected partial blob could not be removed.
    #[error("the rejected blob could not be removed")]
    Cleanup(#[source] io::Error),
}

/// Streams `reader` directly into `path`, verifies its size and SHA-256, fsyncs the file and its parent directory, and removes the partial file on any rejection.
pub fn write_blob_direct<R: Read>(
    path: &Path,
    reader: R,
    expected_hash: ContentHash,
    expected_size: u64,
) -> Result<StoredBlob, BlobWriteError> {
    if let Some(parent) = non_empty_parent(path) {
        std::fs::create_dir_all(parent).map_err(BlobWriteError::CreateDirectory)?;
    }

    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(BlobWriteError::AlreadyExists(error));
        }
        Err(error) => return Err(BlobWriteError::Create(error)),
    };

    let (actual_size, actual_hash) = match stream_blob(&mut file, reader) {
        Ok(streamed) => streamed,
        Err(error) => {
            drop(file);
            return Err(reject(path, error));
        }
    };

    if actual_size != expected_size {
        drop(file);
        return Err(reject(
            path,
            BlobWriteError::SizeMismatch {
                expected: expected_size,
                actual: actual_size,
            },
        ));
    }

    if actual_hash != expected_hash {
        drop(file);
        return Err(reject(path, BlobWriteError::HashMismatch));
    }

    if let Err(error) = file.sync_all() {
        drop(file);
        return Err(reject(path, BlobWriteError::Sync(error)));
    }
    drop(file);

    if let Some(parent) = non_empty_parent(path) {
        if let Err(error) = File::open(parent).and_then(|directory| directory.sync_all()) {
            return Err(reject(path, BlobWriteError::Sync(error)));
        }
    }

    Ok(StoredBlob {
        path: path.to_path_buf(),
        size_bytes: actual_size,
        content_hash: actual_hash,
    })
}

fn stream_blob<R: Read>(
    file: &mut File,
    mut reader: R,
) -> Result<(u64, ContentHash), BlobWriteError> {
    let mut hasher = Sha256::new();
    let mut actual_size = 0u64;
    let mut buffer = [0u8; READ_CHUNK_LEN];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => return Err(BlobWriteError::Read(error)),
        };
        file.write_all(&buffer[..read])
            .map_err(BlobWriteError::Write)?;
        hasher.update(&buffer[..read]);
        let Some(size) = actual_size.checked_add(read as u64) else {
            return Err(BlobWriteError::Write(io::Error::other(
                SIZE_OVERFLOW_MESSAGE,
            )));
        };
        actual_size = size;
    }
    let digest: [u8; CONTENT_HASH_LEN] = hasher.finalize().into();
    Ok((actual_size, ContentHash::from_bytes(digest)))
}

fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

fn reject(path: &Path, error: BlobWriteError) -> BlobWriteError {
    match std::fs::remove_file(path) {
        Ok(()) => error,
        Err(remove) if remove.kind() == io::ErrorKind::NotFound => error,
        Err(remove) => BlobWriteError::Cleanup(remove),
    }
}
