//! Bounded blob-put replica receiver.
//!
//! This receiver buffers the declared transfer and decompressed blob in memory,
//! bounded by the configured file and transfer limits. It is not the final
//! streaming buffer-chain path.
//!
//! SRS: `INT-NET-115 / 1`, `INT-NET-128 / 1`, `FR-UP-012 / 1`,
//! `FR-UP-013 / 1`, `FR-UP-022 / 1`, `NFR-REL-008 / 2`.

use core::fmt;
use std::io::Cursor;

use crate::blob_put::BlobPutBegin;
use crate::blob_writer::{write_blob_direct, BlobWriteError, StoredBlob};
use crate::metadata::ContentHash;
use crate::storage_layout::{LayoutError, StorageLayout};

/// Capacity hint for the empty transfer buffer; the declared size is not preallocated.
const INITIAL_TRANSFER_CAPACITY: usize = 64 * 1024;
/// Standard Zstandard frame magic in byte order.
const ZSTD_FRAME_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Result alias for replica receiving.
type Result<T> = core::result::Result<T, ReplicaError>;

/// Failure modes when receiving and storing a replicated blob.
#[derive(Debug, thiserror::Error)]
pub enum ReplicaError {
    /// The transfer stream ended before its declared size.
    #[error("the transfer stream is incomplete")]
    Truncated,
    /// A size or accumulation exceeds its declared or configured bound.
    #[error("the transfer stream exceeds its declared size")]
    Overflow,
    /// The declared transfer size is outside `1..=B(original_size)`.
    #[error("the transfer size is outside the declared bound")]
    TransferBound,
    /// The transfer stream is not one complete Zstandard frame.
    #[error("the transfer stream could not be decompressed")]
    Decompress,
    /// The decompressed byte count differs from the metadata.
    #[error("the decompressed size does not match the metadata")]
    SizeMismatch,
    /// The decompressed SHA-256 differs from the metadata.
    #[error("the decompressed hash does not match the metadata")]
    HashMismatch,
    /// The local blob path is invalid.
    #[error("the local blob path is invalid")]
    Layout(#[from] LayoutError),
    /// The replica could not be written.
    #[error("the replica could not be written")]
    Write(#[from] BlobWriteError),
}

/// Accumulates one transfer-compressed blob and stores it after integrity checks.
pub struct BlobReplicaReceiver {
    begin: BlobPutBegin,
    layout: StorageLayout,
    compressed: Vec<u8>,
    transfer_len: usize,
}

impl fmt::Debug for BlobReplicaReceiver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlobReplicaReceiver")
            .field("transfer_len", &self.transfer_len)
            .field("received", &self.compressed.len())
            .finish()
    }
}

impl BlobReplicaReceiver {
    /// Starts a receiver after checking the declared transfer and local size bounds.
    pub fn new(begin: BlobPutBegin, layout: StorageLayout, max_file_bytes: u64) -> Result<Self> {
        let Some(transfer_bound) =
            crate::blob_put::transfer_size_bound(begin.metadata.original_size_bytes)
        else {
            return Err(ReplicaError::Overflow);
        };
        if begin.transfer_compressed_size == 0 || begin.transfer_compressed_size > transfer_bound {
            return Err(ReplicaError::TransferBound);
        }
        if begin.metadata.original_size_bytes > max_file_bytes
            || begin.metadata.compressed_size_bytes > max_file_bytes
        {
            return Err(ReplicaError::Overflow);
        }
        let transfer_len =
            usize::try_from(begin.transfer_compressed_size).map_err(|_| ReplicaError::Overflow)?;
        let capacity = transfer_len.min(INITIAL_TRANSFER_CAPACITY);

        Ok(Self {
            begin,
            layout,
            compressed: Vec::with_capacity(capacity),
            transfer_len,
        })
    }

    /// Appends a transfer-compressed chunk, rejecting any excess beyond the declared size.
    pub fn push(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let total = self
            .compressed
            .len()
            .checked_add(data.len())
            .ok_or(ReplicaError::Overflow)?;
        if total > self.transfer_len {
            return Err(ReplicaError::Overflow);
        }
        self.compressed.extend_from_slice(data);
        Ok(())
    }

    /// Decompresses, verifies, and durably writes the complete transfer stream.
    pub fn finish(self) -> Result<StoredBlob> {
        if self.compressed.len() != self.transfer_len {
            return Err(ReplicaError::Truncated);
        }

        let original_size = usize::try_from(self.begin.metadata.original_size_bytes)
            .map_err(|_| ReplicaError::Overflow)?;
        let capacity = original_size.checked_add(1).ok_or(ReplicaError::Overflow)?;
        if self.compressed.len() < ZSTD_FRAME_MAGIC.len()
            || self.compressed[..ZSTD_FRAME_MAGIC.len()] != ZSTD_FRAME_MAGIC
        {
            return Err(ReplicaError::Decompress);
        }
        let frame_len = zstd::zstd_safe::find_frame_compressed_size(&self.compressed)
            .map_err(|_| ReplicaError::Decompress)?;
        if frame_len != self.compressed.len() {
            return Err(ReplicaError::Decompress);
        }
        let decompressed = zstd::bulk::decompress(&self.compressed, capacity)
            .map_err(|_| ReplicaError::Decompress)?;

        if decompressed.len() != original_size {
            return Err(ReplicaError::SizeMismatch);
        }
        if ContentHash::digest(&decompressed) != self.begin.metadata.sha256 {
            return Err(ReplicaError::HashMismatch);
        }

        let path = self
            .layout
            .blob_path(&self.begin.login, *self.begin.upload_id.as_bytes())?;
        write_blob_direct(
            &path,
            Cursor::new(decompressed),
            self.begin.metadata.sha256,
            self.begin.metadata.original_size_bytes,
        )
        .map_err(ReplicaError::from)
    }
}
