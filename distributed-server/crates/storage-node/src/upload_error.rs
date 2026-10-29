//! Protocol result codes for upload failures.
//!
//! SRS: `INT-NET-064 / 1`, supporting `INT-NET-065 / 1`,
//! `FR-UP-012 / 1`, `FR-UP-013 / 1`, `FR-UP-023 / 1`.

use std::io;

use crate::blob_writer::BlobWriteError;
use crate::frame_sequence::FrameSequenceError;
use crate::integrity::IntegrityError;
use crate::upload_exchange::UploadExchangeError;

/// Protocol result code for a failed upload request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum UploadErrorCode {
    /// Unknown client operation.
    UnknownOperation = 0x0001_0001,
    /// Invalid request arguments.
    InvalidArguments = 0x0001_0002,
    /// Authentication rejection.
    AuthenticationRejected = 0x0002_0001,
    /// File size limit exceeded.
    FileSizeExceeded = 0x0005_0001,
    /// Size, metadata, or hash mismatch.
    SizeMetadataHashMismatch = 0x0005_0002,
    /// Invalid frame sequence.
    InvalidFrameSequence = 0x0005_0003,
    /// Upload id conflict.
    UploadIdConflict = 0x0005_0004,
    /// Storage capacity exhausted.
    StorageCapacityExhausted = 0x0007_0002,
    /// Unexpected internal failure.
    Internal = 0x0008_0001,
}

impl From<UploadErrorCode> for u32 {
    fn from(code: UploadErrorCode) -> Self {
        code as Self
    }
}

impl From<UploadExchangeError> for UploadErrorCode {
    fn from(error: UploadExchangeError) -> Self {
        match error {
            UploadExchangeError::InvalidOperation => Self::UnknownOperation,
            UploadExchangeError::InvalidLogin => Self::AuthenticationRejected,
            UploadExchangeError::InvalidSourceName
            | UploadExchangeError::NotAnArray
            | UploadExchangeError::WrongLength
            | UploadExchangeError::Decode => Self::InvalidArguments,
            UploadExchangeError::InvalidSizes => Self::FileSizeExceeded,
            UploadExchangeError::Encode => Self::Internal,
        }
    }
}

impl From<BlobWriteError> for UploadErrorCode {
    fn from(error: BlobWriteError) -> Self {
        match error {
            BlobWriteError::AlreadyExists(_) => Self::UploadIdConflict,
            BlobWriteError::SizeMismatch { .. } | BlobWriteError::HashMismatch => {
                Self::SizeMetadataHashMismatch
            }
            BlobWriteError::Create(error) | BlobWriteError::Write(error)
                if error.kind() == io::ErrorKind::StorageFull =>
            {
                Self::StorageCapacityExhausted
            }
            BlobWriteError::Create(_)
            | BlobWriteError::CreateDirectory(_)
            | BlobWriteError::Read(_)
            | BlobWriteError::Write(_)
            | BlobWriteError::Sync(_)
            | BlobWriteError::Cleanup(_) => Self::Internal,
        }
    }
}

impl From<IntegrityError> for UploadErrorCode {
    fn from(error: IntegrityError) -> Self {
        match error {
            IntegrityError::Layout(_)
            | IntegrityError::Missing(_)
            | IntegrityError::Read(_)
            | IntegrityError::Marker(_) => Self::Internal,
        }
    }
}

impl From<FrameSequenceError> for UploadErrorCode {
    fn from(error: FrameSequenceError) -> Self {
        match error {
            FrameSequenceError::OutOfOrder { .. } | FrameSequenceError::Exhausted => {
                Self::InvalidFrameSequence
            }
        }
    }
}
