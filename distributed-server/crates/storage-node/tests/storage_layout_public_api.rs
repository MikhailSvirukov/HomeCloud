//! Integration tests exercising only the public `storage_node` storage layout API.
//!
//! SRS: `CFG-CL-035 / 1` (reversible lowercase-hex account directory) and
//! `CFG-CL-036 / 1` (accounts/raft roots and the exact blob path), supporting
//! `FR-NODE-001 / 1`, `FR-NODE-002 / 1`, and `FR-NODE-012 / 1`.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use storage_node::{LayoutError, StorageLayout, UPLOAD_ID_LEN};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

#[test]
fn encoded_login_and_upload_id_make_the_blob_path() {
    let layout = StorageLayout::new("/data");

    let blob = layout
        .blob_path(
            "alice",
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f,
            ],
        )
        .expect("valid login and upload id");

    assert_eq!(
        blob,
        PathBuf::from("/data/accounts/616c696365/000102030405060708090a0b0c0d0e0f")
    );
}

#[test]
fn case_is_preserved_in_the_account_directory() {
    let layout = StorageLayout::new("/data");

    let lowercase = layout
        .blob_path("alice", [0u8; UPLOAD_ID_LEN])
        .expect("valid login and upload id");
    let mixed_case = layout
        .blob_path("Alice", [0u8; UPLOAD_ID_LEN])
        .expect("valid login and upload id");

    assert_eq!(
        lowercase.parent(),
        Some(Path::new("/data/accounts/616c696365"))
    );
    assert_eq!(
        mixed_case.parent(),
        Some(Path::new("/data/accounts/416c696365"))
    );
}

#[test]
fn dot_only_login_is_hex_encoded_and_cannot_traverse() {
    let layout = StorageLayout::new("/data");

    let blob = layout
        .blob_path("..", [0u8; UPLOAD_ID_LEN])
        .expect("dot-only login is valid input and hex-encoded");

    assert_eq!(
        blob.parent(),
        Some(layout.accounts_dir().join("2e2e").as_path())
    );
    assert!(
        blob.components()
            .all(|component| component != Component::ParentDir),
        "blob path must not contain a parent-directory component"
    );
}

#[test]
fn rejects_invalid_logins() {
    let layout = StorageLayout::new("/data");

    for login in ["", "a/b", "a b", "é"] {
        assert_eq!(
            layout.blob_path(login, [0u8; UPLOAD_ID_LEN]),
            Err(LayoutError::InvalidLogin),
            "login {login:?} must be rejected"
        );
    }
}

#[test]
fn ensure_dirs_creates_both_directories() {
    let root = unique_temp_dir();
    let layout = StorageLayout::new(&root);

    assert_eq!(layout.accounts_dir(), layout.root.join("accounts"));
    assert_eq!(layout.raft_dir(), layout.root.join("raft"));
    assert!(!layout.accounts_dir().exists());
    assert!(!layout.raft_dir().exists());

    layout
        .ensure_dirs()
        .expect("temp directories can be created");

    assert_eq!(layout.accounts_dir(), layout.root.join("accounts"));
    assert_eq!(layout.raft_dir(), layout.root.join("raft"));
    assert!(layout.accounts_dir().is_dir());
    assert!(layout.raft_dir().is_dir());

    std::fs::remove_dir_all(&root).expect("temp directory can be removed");
    assert!(!root.exists());
}

fn unique_temp_dir() -> PathBuf {
    let unique = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("homecloud-layout-{}-{unique}", std::process::id()))
}
