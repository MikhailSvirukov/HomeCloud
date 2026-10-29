//! Integration tests exercising only the public `storage_node` blob-writer API.
//!
//! SRS: `FR-UP-012 / 1` (reject a blob whose SHA-256 differs from the declared
//! hash before commit), `FR-UP-013 / 1` (delete the incomplete data of a
//! rejected upload), `FR-UP-022 / 1` (write directly into the final blob
//! location without an intermediate file for a later rename or copy), and
//! `NFR-REL-008 / 2` (the blob is only accounted for after the content and its
//! discovery metadata are made durable with `fsync` or an equivalent).

use std::ffi::OsString;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use storage_node::{write_blob_direct, BlobWriteError, ContentHash};
use tempfile::TempDir;

const PAYLOAD: &[u8] = b"blob payload";
const DELIVERED_ONCE: &[u8] = b"data";
const BLOB_FILE_NAME: &str = "0123";

/// A reader that yields `DELIVERED_ONCE` and then fails, simulating a broken
/// upstream stream in the middle of a blob write.
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

fn directory_entries(directory: &Path) -> Vec<OsString> {
    let mut entries: Vec<OsString> = fs::read_dir(directory)
        .expect("the blob parent directory is readable")
        .map(|entry| entry.expect("each directory entry is readable").file_name())
        .collect();
    entries.sort();
    entries
}

#[test]
fn writes_directly_to_the_final_path_without_a_temp_file() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    let parent = path.parent().expect("blob path has a parent directory");

    let stored = write_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .expect("a matching blob is written");

    assert_eq!(stored.path, path);
    assert_eq!(stored.size_bytes, PAYLOAD.len() as u64);
    assert_eq!(stored.content_hash, ContentHash::digest(PAYLOAD));
    assert_eq!(
        fs::read(&path).expect("the written blob is readable"),
        PAYLOAD
    );
    assert_eq!(
        directory_entries(parent),
        vec![OsString::from(BLOB_FILE_NAME)],
        "the final directory holds only the blob, so no temp file was renamed"
    );
}

#[test]
fn rejects_a_hash_mismatch_and_removes_the_partial_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    let parent = path.parent().expect("blob path has a parent directory");
    let declared_hash = ContentHash::digest(b"different bytes");

    let error = write_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        declared_hash,
        PAYLOAD.len() as u64,
    )
    .expect_err("a hash mismatch is rejected");

    assert!(matches!(error, BlobWriteError::HashMismatch));
    assert!(!path.exists(), "the partial blob is removed");
    assert!(
        directory_entries(parent).is_empty(),
        "no file remains beside the rejected blob"
    );
}

#[test]
fn rejects_a_size_mismatch_and_removes_the_partial_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    let parent = path.parent().expect("blob path has a parent directory");
    let declared_size = PAYLOAD.len() as u64 + 1;

    let error = write_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(PAYLOAD),
        declared_size,
    )
    .expect_err("a declared size larger than the streamed bytes is rejected");

    assert!(matches!(
        error,
        BlobWriteError::SizeMismatch { expected, actual }
            if expected == declared_size && actual == PAYLOAD.len() as u64
    ));
    assert!(!path.exists(), "the partial blob is removed");
    assert!(
        directory_entries(parent).is_empty(),
        "no file remains beside the rejected blob"
    );
}

#[test]
fn refuses_to_overwrite_an_existing_blob() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    let parent = path.parent().expect("blob path has a parent directory");
    fs::create_dir_all(parent).expect("the blob parent directory is created");
    fs::write(&path, b"old").expect("the existing blob is written");

    let error = write_blob_direct(
        &path,
        Cursor::new(PAYLOAD),
        ContentHash::digest(PAYLOAD),
        PAYLOAD.len() as u64,
    )
    .expect_err("an existing blob is not overwritten");

    assert!(matches!(error, BlobWriteError::AlreadyExists(_)));
    assert_eq!(
        fs::read(&path).expect("the existing blob is still readable"),
        b"old"
    );
    assert_eq!(
        directory_entries(parent),
        vec![OsString::from(BLOB_FILE_NAME)],
        "the refusal leaves the existing blob in place"
    );
}

/// A failing reader leaves no partial blob behind.
#[test]
fn cleans_up_after_a_reader_failure() {
    let temp = TempDir::new().expect("temp directory is created");
    let path = nested_blob_path(temp.path());
    let parent = path.parent().expect("blob path has a parent directory");

    let error = write_blob_direct(
        &path,
        FailingReader::new(),
        ContentHash::digest(DELIVERED_ONCE),
        DELIVERED_ONCE.len() as u64,
    )
    .expect_err("a failing reader aborts the blob write");

    assert!(matches!(error, BlobWriteError::Read(_)));
    assert!(!path.exists(), "the partial blob is removed");
    assert!(
        directory_entries(parent).is_empty(),
        "no file remains beside the rejected blob"
    );
}
