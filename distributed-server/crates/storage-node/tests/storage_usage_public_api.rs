//! Integration tests exercising only the public `storage_node` storage-usage API.
//!
//! SRS: `NFR-OBS-005 / 3` requires storage nodes to expose occupied storage
//! capacity; `StorageLayout::usage_bytes` supplies the local byte total of the
//! accounts and Raft roots that the metrics endpoint reports.

use std::fs;
use std::path::Path;

use storage_node::StorageLayout;
use tempfile::TempDir;

const ACCOUNT_BLOB_NAME: &str = "0123456789abcdef";
const ACCOUNT_MARKER_NAME: &str = "0123456789abcdef.corrupt";
const RAFT_STATE_NAME: &str = "state.cbor";
const ACCOUNT_BLOB_BYTES: usize = 7;
const ACCOUNT_MARKER_BYTES: usize = 3;
const RAFT_STATE_BYTES: usize = 11;

fn write_bytes(path: &Path, len: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("creating fixture directories cannot fail");
    }
    fs::write(path, vec![0_u8; len]).expect("writing a fixture file cannot fail");
}

#[test]
fn sums_files_under_accounts_and_raft() {
    let temp = TempDir::new().expect("creating a temporary directory cannot fail");
    let layout = StorageLayout::new(temp.path());
    layout
        .ensure_dirs()
        .expect("creating the layout roots cannot fail");
    let account_dir = layout.accounts_dir().join("ab").join("cd");

    write_bytes(&account_dir.join(ACCOUNT_BLOB_NAME), ACCOUNT_BLOB_BYTES);
    write_bytes(&account_dir.join(ACCOUNT_MARKER_NAME), ACCOUNT_MARKER_BYTES);
    write_bytes(&layout.raft_dir().join(RAFT_STATE_NAME), RAFT_STATE_BYTES);

    let used = layout
        .usage_bytes()
        .expect("walking the layout roots cannot fail");

    assert_eq!(
        used,
        (ACCOUNT_BLOB_BYTES + ACCOUNT_MARKER_BYTES + RAFT_STATE_BYTES) as u64
    );
}

#[test]
fn missing_roots_count_as_zero() {
    let temp = TempDir::new().expect("creating a temporary directory cannot fail");
    let layout = StorageLayout::new(temp.path());

    assert!(
        !layout.accounts_dir().exists(),
        "the fixture must not create the accounts root"
    );
    assert!(
        !layout.raft_dir().exists(),
        "the fixture must not create the Raft root"
    );
    assert_eq!(
        layout
            .usage_bytes()
            .expect("a missing root is not an error"),
        0
    );
}

#[test]
fn skips_directories_and_counts_nested_files() {
    let temp = TempDir::new().expect("creating a temporary directory cannot fail");
    let layout = StorageLayout::new(temp.path());
    layout
        .ensure_dirs()
        .expect("creating the layout roots cannot fail");
    let nested = layout.accounts_dir().join("ab").join("cd").join("ef");
    fs::create_dir_all(&nested).expect("creating nested directories cannot fail");

    assert_eq!(
        layout
            .usage_bytes()
            .expect("walking empty layout roots cannot fail"),
        0,
        "directory entries themselves contribute no bytes"
    );

    write_bytes(&nested.join(ACCOUNT_BLOB_NAME), ACCOUNT_BLOB_BYTES);

    assert_eq!(
        layout
            .usage_bytes()
            .expect("walking the layout roots cannot fail"),
        ACCOUNT_BLOB_BYTES as u64,
        "regular files are summed recursively while directories add nothing"
    );
}
