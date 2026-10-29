//! Integration tests for the public upload protocol error codes.
//!
//! Covers `INT-NET-064 / 1`: upload failures carry the protocol-table codes
//! `0x0001_0001` for an unknown operation, `0x0001_0002` for invalid
//! arguments, `0x0002_0001` for rejected authentication, `0x0005_0001` for a
//! file-size limit, `0x0005_0002` for a size/metadata/hash mismatch,
//! `0x0005_0003` for an invalid frame sequence, `0x0005_0004` for an upload-id
//! conflict, `0x0007_0002` for exhausted storage capacity, and `0x0008_0001`
//! for an internal error. Supporting `INT-NET-065 / 1`: permanent request
//! failures must stay distinguishable from transient ones. Supporting
//! `FR-UP-012 / 1` and `FR-UP-013 / 1`: a rejected blob write maps to the
//! size/metadata/hash code after the partial blob is cleaned up. Supporting
//! `FR-UP-023 / 1`: an existing blob at the final path maps to the upload-id
//! conflict code instead of being treated as a fresh upload.

use std::io;

use storage_node::{
    BlobWriteError, FrameSequenceError, IntegrityError, LayoutError, UploadErrorCode,
    UploadExchangeError,
};

fn assert_maps_to(error: impl Into<UploadErrorCode>, expected: UploadErrorCode) {
    assert_eq!(error.into(), expected);
}

fn io_failure() -> io::Error {
    io::Error::other("simulated I/O failure")
}

#[test]
fn wire_codes_match_the_upload_protocol_table() {
    assert_eq!(u32::from(UploadErrorCode::UnknownOperation), 0x0001_0001);
    assert_eq!(u32::from(UploadErrorCode::InvalidArguments), 0x0001_0002);
    assert_eq!(
        u32::from(UploadErrorCode::AuthenticationRejected),
        0x0002_0001
    );
    assert_eq!(u32::from(UploadErrorCode::FileSizeExceeded), 0x0005_0001);
    assert_eq!(
        u32::from(UploadErrorCode::SizeMetadataHashMismatch),
        0x0005_0002
    );
    assert_eq!(
        u32::from(UploadErrorCode::InvalidFrameSequence),
        0x0005_0003
    );
    assert_eq!(u32::from(UploadErrorCode::UploadIdConflict), 0x0005_0004);
    assert_eq!(
        u32::from(UploadErrorCode::StorageCapacityExhausted),
        0x0007_0002
    );
    assert_eq!(u32::from(UploadErrorCode::Internal), 0x0008_0001);
}

#[test]
fn maps_upload_exchange_failures() {
    assert_maps_to(
        UploadExchangeError::InvalidOperation,
        UploadErrorCode::UnknownOperation,
    );
    assert_maps_to(
        UploadExchangeError::InvalidLogin,
        UploadErrorCode::AuthenticationRejected,
    );
    assert_maps_to(
        UploadExchangeError::InvalidPassword,
        UploadErrorCode::AuthenticationRejected,
    );
    assert_maps_to(
        UploadExchangeError::InvalidSourceName,
        UploadErrorCode::InvalidArguments,
    );
    assert_maps_to(
        UploadExchangeError::InvalidSizes,
        UploadErrorCode::FileSizeExceeded,
    );
    for error in [
        UploadExchangeError::NotAnArray,
        UploadExchangeError::WrongLength,
        UploadExchangeError::Decode,
    ] {
        assert_maps_to(error, UploadErrorCode::InvalidArguments);
    }
    assert_maps_to(UploadExchangeError::Encode, UploadErrorCode::Internal);
}

#[test]
fn maps_blob_write_failures() {
    assert_maps_to(
        BlobWriteError::AlreadyExists(io_failure()),
        UploadErrorCode::UploadIdConflict,
    );
    assert_maps_to(
        BlobWriteError::SizeMismatch {
            expected: 4,
            actual: 3,
        },
        UploadErrorCode::SizeMetadataHashMismatch,
    );
    assert_maps_to(
        BlobWriteError::HashMismatch,
        UploadErrorCode::SizeMetadataHashMismatch,
    );
    for error in [
        BlobWriteError::CreateDirectory(io_failure()),
        BlobWriteError::Create(io_failure()),
        BlobWriteError::Read(io_failure()),
        BlobWriteError::Write(io_failure()),
        BlobWriteError::Sync(io_failure()),
        BlobWriteError::Cleanup(io_failure()),
    ] {
        assert_maps_to(error, UploadErrorCode::Internal);
    }
}

#[test]
fn maps_integrity_and_sequence_failures() {
    for error in [
        IntegrityError::Layout(LayoutError::InvalidLogin),
        IntegrityError::Missing(io_failure()),
        IntegrityError::Read(io_failure()),
        IntegrityError::Marker(io_failure()),
    ] {
        assert_maps_to(error, UploadErrorCode::Internal);
    }
    for error in [
        FrameSequenceError::OutOfOrder {
            expected: 1,
            actual: 2,
        },
        FrameSequenceError::Exhausted,
    ] {
        assert_maps_to(error, UploadErrorCode::InvalidFrameSequence);
    }
}

#[test]
fn maps_disk_full_failures_to_storage_capacity_exhausted() {
    let write = BlobWriteError::Write(std::io::Error::new(std::io::ErrorKind::StorageFull, "full"));
    assert_eq!(
        UploadErrorCode::from(write),
        UploadErrorCode::StorageCapacityExhausted
    );

    let create =
        BlobWriteError::Create(std::io::Error::new(std::io::ErrorKind::StorageFull, "full"));
    assert_eq!(
        UploadErrorCode::from(create),
        UploadErrorCode::StorageCapacityExhausted
    );
}
