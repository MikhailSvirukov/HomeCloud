//! Integration tests exercising only the public `storage_node` blob-restore API.
//!
//! SRS: `FR-CL-025 / 1` (a restored blob counts as repaired only after its
//! SHA-256 fully matches and its content is durably recorded), supporting
//! `FR-CL-021 / 2` (a mismatching local committed blob is marked corrupted) and
//! `NFR-REL-008 / 2` (the blob is accounted for only after its content and
//! discovery metadata are made durable).

use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use storage_node::{
    corrupt_marker_path, restore_blob_direct, BlobWriteError, ContentHash, RestoreError,
};
use tempfile::TempDir;

const PAYLOAD: &[u8] = b"restored blob payload";
const CORRUPT_BYTES: &[u8] = b"corrupted bytes";
const DELIVERED_ONCE: &[u8] = b"data";
const BLOB_FILE_NAME: &str = "0123";

/// A reader that yields `DELIVERED_ONCE` and then fails, simulating a source
/// that breaks in the middle of a restore.
struct FailingReader {
    delivered: bool,
}

impl FailingReader {
    fn new() -> Self {
        Self { delivered: false }
    }
}

impl Read for FailingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.delivered {
            return Err(io::Error::other("simulated reader failure"));
        }
        let len = DELIVERED_ONCE.len().min(buffer.len());
        buffer[..len].copy_from_slice(&DELIVERED_ONCE[..len]);
        self.delivered = true;
        Ok(len)
    }
}

fn nested_blob_path(root: &Path) -> PathBuf {
    root.join("accounts").join("abc").join(BLOB_FILE_NAME)
}

fn marked_corrupt_blob(root: &Path) -> PathBuf {
    let path = nested_blob_path(root);
    fs::create_dir_all(path.parent().expect("blob path has a parent directory"))
        .expect("the blob parent directory is created");
    fs::write(&path, CORRUPT_BYTES).expect("the corrupt blob is written");
    fs::write(corrupt_marker_path(&path), []).expect("the corruption marker is written");
    path
}

#[test]
fn restores_a_missing_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    assert!(!path.exists(), "precondition: the blob is missing");

    let stored = restore_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .expect("a missing blob is restored");

    assert_eq!(stored.path, path);
    assert_eq!(stored.size_bytes, PAYLOAD.len() as u64);
    assert_eq!(stored.content_hash, ContentHash::digest(PAYLOAD));
    assert_eq!(
        fs::read(&path).expect("the restored blob is readable"),
        PAYLOAD
    );
    assert!(
        !corrupt_marker_path(&path).exists(),
        "a successful restore leaves no corruption marker"
    );
}

#[test]
fn replaces_a_marked_corrupt_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = marked_corrupt_blob(temp.path());

    let stored = restore_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .expect("a marked-corrupt blob is replaced");

    assert_eq!(stored.path, path);
    assert_eq!(
        fs::read(&path).expect("the restored blob is readable"),
        PAYLOAD
    );
    assert!(
        !corrupt_marker_path(&path).exists(),
        "a successful restore removes the corruption marker"
    );
}

#[test]
fn refuses_an_unmarked_existing_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    fs::create_dir_all(path.parent().expect("blob path has a parent directory"))
        .expect("the blob parent directory is created");
    fs::write(&path, CORRUPT_BYTES).expect("the unmarked blob is written");

    let error = restore_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .expect_err("an unmarked existing blob is refused");

    assert!(matches!(error, RestoreError::ExistingBlob));
    assert_eq!(
        fs::read(&path).expect("the existing blob is readable"),
        CORRUPT_BYTES,
        "the refused restore leaves the original bytes unchanged"
    );
    assert!(
        !corrupt_marker_path(&path).exists(),
        "the refused restore does not create a marker"
    );
}

#[test]
fn failed_restore_keeps_a_corruption_marker() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = marked_corrupt_blob(temp.path());

    let error = restore_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(b"different bytes"),
        PAYLOAD.len() as u64,
    )
    .expect_err("a payload that does not match the expected hash is rejected");

    assert!(matches!(
        error,
        RestoreError::Write(BlobWriteError::HashMismatch)
    ));
    assert!(!path.exists(), "the rejected partial blob is removed");
    assert!(
        corrupt_marker_path(&path).exists(),
        "a failed restore keeps the corruption marker"
    );
}

#[test]
fn reader_failure_keeps_a_corruption_marker() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = marked_corrupt_blob(temp.path());

    let error = restore_blob_direct(
        &path,
        FailingReader::new(),
        ContentHash::digest(DELIVERED_ONCE),
        DELIVERED_ONCE.len() as u64,
    )
    .expect_err("a failing reader aborts the restore");

    assert!(matches!(
        error,
        RestoreError::Write(BlobWriteError::Read(_))
    ));
    assert!(!path.exists(), "the rejected partial blob is removed");
    assert!(
        corrupt_marker_path(&path).exists(),
        "a failed restore keeps the corruption marker"
    );
}
