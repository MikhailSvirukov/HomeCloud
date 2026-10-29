//! Integration tests exercising only the public `storage_node` blob-integrity API.
//!
//! SRS: `FR-CL-021 / 2` (a local committed blob whose computed SHA-256 differs
//! from the committed hash is marked corrupted locally).

use std::path::{Path, PathBuf};

use storage_node::{
    verify_local_blob, BlobIntegrity, ContentHash, IntegrityError, StorageLayout, UploadId,
    UploadRecord, CORRUPT_MARKER_SUFFIX, UPLOAD_ID_LEN,
};
use tempfile::TempDir;

/// Upload id shared by every record built in these tests.
const FIXED_UPLOAD_ID: [u8; UPLOAD_ID_LEN] = [0x2a; UPLOAD_ID_LEN];
const LOGIN: &str = "alice";
const ORIGINAL_NAME: &str = "photo.jpg";
const CAPTURED_AT_MS: u64 = 1_700_000_000_000;
const ACCEPTED_AT_MS: u64 = 1_700_000_001_000;

fn record(payload: &[u8]) -> UploadRecord {
    UploadRecord {
        upload_id: UploadId::from_bytes(FIXED_UPLOAD_ID),
        login: LOGIN.to_owned(),
        original_name: ORIGINAL_NAME.to_owned(),
        size_bytes: payload.len() as u64,
        compressed_size_bytes: payload.len() as u64 / 2,
        content_hash: ContentHash::digest(payload),
        captured_at_ms: CAPTURED_AT_MS,
        accepted_at_ms: ACCEPTED_AT_MS,
    }
}

fn empty_layout() -> (TempDir, StorageLayout) {
    let temp = TempDir::new().expect("temp directory is created");
    let layout = StorageLayout::new(temp.path());
    layout
        .ensure_dirs()
        .expect("layout directories are created");
    (temp, layout)
}

fn blob_path(layout: &StorageLayout, record: &UploadRecord) -> PathBuf {
    layout
        .blob_path(&record.login, *record.upload_id.as_bytes())
        .expect("record login and upload id form a valid blob path")
}

fn write_blob(layout: &StorageLayout, record: &UploadRecord, payload: &[u8]) {
    let path = blob_path(layout, record);
    std::fs::create_dir_all(path.parent().expect("blob path has an account directory"))
        .expect("account directory is created");
    std::fs::write(&path, payload).expect("blob payload is written");
}

fn marker_path(blob: &Path) -> PathBuf {
    let mut marker = blob.as_os_str().to_owned();
    marker.push(CORRUPT_MARKER_SUFFIX);
    PathBuf::from(marker)
}

fn flip_first_byte(payload: &[u8]) -> Vec<u8> {
    let mut corrupted = payload.to_vec();
    corrupted[0] ^= 0xFF;
    corrupted
}

#[test]
fn healthy_blob_is_reported_and_a_stale_marker_is_removed() {
    let (_temp, layout) = empty_layout();
    let payload = b"committed blob payload";
    let record = record(payload);
    write_blob(&layout, &record, payload);
    let marker = marker_path(&blob_path(&layout, &record));
    std::fs::write(&marker, []).expect("stale corruption marker is written");
    assert!(marker.exists(), "precondition: a stale marker exists");

    let integrity = verify_local_blob(&layout, &record).expect("blob is readable");

    assert!(
        matches!(integrity, BlobIntegrity::Healthy),
        "a blob matching the committed hash is healthy"
    );
    assert!(
        !marker.exists(),
        "a matching check removes the stale marker"
    );
}

#[test]
fn modified_blob_is_marked_corrupted() {
    let (_temp, layout) = empty_layout();
    let payload = b"committed blob payload";
    let record = record(payload);
    write_blob(&layout, &record, payload);
    let blob = blob_path(&layout, &record);
    let marker = marker_path(&blob);

    let pristine = verify_local_blob(&layout, &record).expect("blob is readable");
    assert!(
        matches!(pristine, BlobIntegrity::Healthy),
        "precondition: the written blob matches the committed hash"
    );

    std::fs::write(&blob, flip_first_byte(payload)).expect("blob bytes are overwritten");

    let corrupted = verify_local_blob(&layout, &record).expect("modified blob is readable");
    assert!(
        matches!(corrupted, BlobIntegrity::Corrupted),
        "a hash mismatch marks the local copy corrupted"
    );
    assert!(marker.exists(), "a hash mismatch creates the marker");

    let rechecked = verify_local_blob(&layout, &record).expect("marked blob is readable");
    assert!(
        matches!(rechecked, BlobIntegrity::Corrupted),
        "the marked copy stays corrupted while its bytes keep mismatching"
    );
    assert!(marker.exists(), "a repeated mismatch keeps the marker");
}

#[test]
fn restored_blob_clears_the_marker() {
    let (_temp, layout) = empty_layout();
    let payload = b"committed blob payload";
    let record = record(payload);
    write_blob(&layout, &record, payload);
    let blob = blob_path(&layout, &record);
    let marker = marker_path(&blob);

    std::fs::write(&blob, flip_first_byte(payload)).expect("blob bytes are overwritten");
    let corrupted = verify_local_blob(&layout, &record).expect("modified blob is readable");
    assert!(
        matches!(corrupted, BlobIntegrity::Corrupted),
        "precondition: the mismatching copy is marked corrupted"
    );
    assert!(marker.exists(), "precondition: a marker exists");

    std::fs::write(&blob, payload).expect("original blob bytes are restored");

    let restored = verify_local_blob(&layout, &record).expect("restored blob is readable");
    assert!(
        matches!(restored, BlobIntegrity::Healthy),
        "restored bytes matching the committed hash are healthy"
    );
    assert!(
        !marker.exists(),
        "a later matching check removes the marker"
    );
}

#[test]
fn missing_blob_reports_missing() {
    let (_temp, layout) = empty_layout();
    let record = record(b"never written");

    let result = verify_local_blob(&layout, &record);

    assert!(
        matches!(result, Err(IntegrityError::Missing(_))),
        "a blob absent from the layout is reported as missing"
    );
}
