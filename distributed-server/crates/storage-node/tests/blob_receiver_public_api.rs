//! Public `BlobReplicaReceiver` behaviour.
//!
//! SRS: `INT-NET-115 / 1` (confirm success only after receiving exactly the
//! transfer compressed size, decompressing to exactly the original size,
//! checking SHA-256, writing directly, and making the content durable),
//! `INT-NET-128 / 1` (one complete Zstandard frame with exact transfer
//! consumption and a bounded decompressed size), `FR-UP-012 / 1` (reject a
//! blob whose SHA-256 differs from the declared hash before commit),
//! `FR-UP-013 / 1` (delete the incomplete data of a rejected upload),
//! `FR-UP-022 / 1` (write directly into the final blob location without an
//! intermediate file for a later rename or copy), and `NFR-REL-008 / 2` (the
//! blob content is durable through `fsync` before it is accounted for).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use storage_node::BlobMetadata;
use storage_node::BlobPutBegin;
use storage_node::BlobReplicaReceiver;
use storage_node::ContentHash;
use storage_node::ReplicaError;
use storage_node::StorageLayout;
use storage_node::UploadId;
use storage_node::UPLOAD_ID_LEN;
use tempfile::TempDir;

/// Login owning the replicated upload.
const LOGIN: &str = "alice";
/// Original source name recorded in the upload metadata.
const SOURCE_NAME: &str = "photo.jpg";
/// Fixed capture/creation timestamp in Unix milliseconds.
const TIMESTAMP_MS: i64 = 1_700_000_000_000;
/// Zstandard compression level used to build the transfer stream.
const ZSTD_COMPRESSION_LEVEL: i32 = 3;
/// Maximum file size accepted by the receiver in these tests.
const MAX_FILE_BYTES: u64 = 1_048_576;
/// File-limit boundary used to reject oversized metadata.
const SMALL_FILE_LIMIT: u64 = 1024;
/// Original payload transferred by the accepted case.
const PAYLOAD: &[u8] = b"HomeCloud blob receiver payload: a complete Zstandard frame is decompressed into the declared original size, checked against the SHA-256 of the original bytes, and written directly to the durable blob path.";
/// Marker that must never be printed by the receiver's `Debug`.
const DEBUG_MARKER: &str = "DO-NOT-LEAK-RECEIVER-PAYLOAD";
/// Fixed upload identifier used by every case.
const UPLOAD_ID: UploadId = UploadId::from_bytes([0x42; UPLOAD_ID_LEN]);

fn metadata_for(payload: &[u8]) -> BlobMetadata {
    BlobMetadata {
        source_name: SOURCE_NAME.to_owned(),
        timestamp_ms: TIMESTAMP_MS,
        original_size_bytes: payload.len() as u64,
        compressed_size_bytes: payload.len() as u64,
        sha256: ContentHash::digest(payload),
    }
}

fn compressed(payload: &[u8]) -> Vec<u8> {
    zstd::stream::encode_all(payload, ZSTD_COMPRESSION_LEVEL)
        .expect("the test payload compresses into one Zstandard frame")
}

fn put_begin(metadata: BlobMetadata, transfer_compressed_size: u64) -> BlobPutBegin {
    BlobPutBegin {
        login: LOGIN.to_owned(),
        upload_id: UPLOAD_ID,
        metadata,
        transfer_compressed_size,
    }
}

fn blob_path(layout: &StorageLayout) -> PathBuf {
    layout
        .blob_path(LOGIN, *UPLOAD_ID.as_bytes())
        .expect("the fixed login and upload id form a blob path")
}

fn entry_names(directory: &Path) -> Vec<OsString> {
    let mut names: Vec<OsString> = fs::read_dir(directory)
        .expect("the blob parent directory is readable")
        .map(|entry| entry.expect("each directory entry is readable").file_name())
        .collect();
    names.sort();
    names
}

fn receiver(layout: &StorageLayout, begin: BlobPutBegin) -> BlobReplicaReceiver {
    BlobReplicaReceiver::new(begin, layout.clone(), MAX_FILE_BYTES)
        .expect("the valid metadata fits within the file limit")
}

#[test]
fn receives_a_complete_zstd_transfer_and_writes_the_durable_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    let path = blob_path(&layout);
    let transfer = compressed(PAYLOAD);
    let mut receiver = receiver(
        &layout,
        put_begin(metadata_for(PAYLOAD), transfer.len() as u64),
    );

    let first_split = transfer.len() / 3;
    let second_split = first_split * 2;
    receiver
        .push(&transfer[..first_split])
        .expect("the first transfer chunk is accepted");
    receiver
        .push(&transfer[first_split..second_split])
        .expect("the middle transfer chunk is accepted");
    receiver
        .push(&transfer[second_split..])
        .expect("the final transfer chunk is accepted");

    let stored = receiver.finish().expect("the complete transfer is stored");

    assert_eq!(stored.path, path);
    assert_eq!(stored.size_bytes, PAYLOAD.len() as u64);
    assert_eq!(stored.content_hash, ContentHash::digest(PAYLOAD));
    assert_eq!(
        fs::read(&path).expect("the durable blob is readable"),
        PAYLOAD
    );

    let parent = path.parent().expect("the blob path has a parent directory");
    assert_eq!(
        entry_names(parent),
        vec![path
            .file_name()
            .expect("the blob path has a file name")
            .to_os_string()],
        "the final directory holds only the blob, so no temp file remains"
    );
}

#[test]
fn rejects_truncated_and_oversized_transfer_streams() {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    let transfer = compressed(PAYLOAD);
    let transfer_size = transfer.len() as u64;

    let mut truncated = receiver(&layout, put_begin(metadata_for(PAYLOAD), transfer_size));
    truncated
        .push(&transfer[..transfer.len() - 1])
        .expect("the partial transfer chunk is accepted");
    let error = truncated
        .finish()
        .expect_err("a stream shorter than the declared transfer is rejected");
    assert!(matches!(error, ReplicaError::Truncated));

    let mut oversized = receiver(&layout, put_begin(metadata_for(PAYLOAD), transfer_size));
    oversized
        .push(&transfer)
        .expect("the exact transfer stream is accepted");
    let error = oversized
        .push(&[0])
        .expect_err("a byte beyond the declared transfer is rejected");
    assert!(matches!(error, ReplicaError::Overflow));
}

#[test]
fn rejects_decompression_failure_and_decompressed_size_overflow() {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    let invalid = b"not a complete Zstandard frame";
    let mut corrupt = receiver(
        &layout,
        put_begin(metadata_for(PAYLOAD), invalid.len() as u64),
    );
    corrupt
        .push(invalid)
        .expect("the invalid bytes accumulate as the transfer");
    let error = corrupt
        .finish()
        .expect_err("invalid Zstandard bytes are rejected");
    assert!(matches!(error, ReplicaError::Decompress));

    let transfer = compressed(PAYLOAD);
    let mut metadata = metadata_for(PAYLOAD);
    metadata.original_size_bytes = PAYLOAD.len() as u64 - 1;
    let mut overflowing = receiver(&layout, put_begin(metadata, transfer.len() as u64));
    overflowing
        .push(&transfer)
        .expect("the complete transfer stream is accepted");
    let error = overflowing
        .finish()
        .expect_err("decompressed bytes beyond the declared original size are rejected");
    assert!(matches!(error, ReplicaError::SizeMismatch));
}

#[test]
fn rejects_a_hash_mismatch_and_leaves_no_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    let path = blob_path(&layout);
    let transfer = compressed(PAYLOAD);
    let mut metadata = metadata_for(PAYLOAD);
    metadata.sha256 = ContentHash::digest(b"different original bytes");
    let mut receiver = receiver(&layout, put_begin(metadata, transfer.len() as u64));
    receiver
        .push(&transfer)
        .expect("the complete transfer stream is accepted");

    let error = receiver
        .finish()
        .expect_err("a hash that differs from the original bytes is rejected");
    assert!(matches!(error, ReplicaError::HashMismatch));
    assert!(!path.exists(), "the rejected blob is removed");

    let parent = path.parent().expect("the blob path has a parent directory");
    if parent.exists() {
        assert!(
            entry_names(parent).is_empty(),
            "no partial or temporary file remains beside the rejected blob"
        );
    }
}

#[test]
fn rejects_metadata_above_the_file_limit() {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    let transfer = compressed(PAYLOAD);
    let transfer_size = transfer.len() as u64;
    assert!(
        transfer_size <= SMALL_FILE_LIMIT,
        "the transfer itself stays within the test limit"
    );

    let mut oversized_original = metadata_for(PAYLOAD);
    oversized_original.original_size_bytes = SMALL_FILE_LIMIT + 1;
    let result = BlobReplicaReceiver::new(
        put_begin(oversized_original, transfer_size),
        layout.clone(),
        SMALL_FILE_LIMIT,
    );
    assert!(
        matches!(result, Err(ReplicaError::Overflow)),
        "an original size above the file limit is rejected at construction"
    );

    let mut oversized_compressed = metadata_for(PAYLOAD);
    oversized_compressed.compressed_size_bytes = SMALL_FILE_LIMIT + 1;
    let result = BlobReplicaReceiver::new(
        put_begin(oversized_compressed, transfer_size),
        layout,
        SMALL_FILE_LIMIT,
    );
    assert!(
        matches!(result, Err(ReplicaError::Overflow)),
        "a compressed size above the file limit is rejected at construction"
    );
}

#[test]
fn debug_does_not_print_payload() {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    let payload = DEBUG_MARKER.repeat(16).into_bytes();
    let transfer = compressed(&payload);
    let mut receiver = receiver(
        &layout,
        put_begin(metadata_for(&payload), transfer.len() as u64),
    );
    receiver
        .push(&transfer)
        .expect("the complete transfer stream is accepted");

    let debug = format!("{receiver:?}");
    assert!(
        !debug.contains(DEBUG_MARKER),
        "compact Debug output does not include the original payload"
    );
    assert!(
        !debug.contains(&format!("{:?}", &payload[..])),
        "compact Debug output does not include the buffered original bytes"
    );
    assert!(
        !debug.contains(&format!("{:?}", &transfer[..])),
        "compact Debug output does not include the buffered transfer bytes"
    );
}
