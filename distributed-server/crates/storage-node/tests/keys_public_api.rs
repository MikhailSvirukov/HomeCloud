//! Integration tests exercising only the public `storage_node` private-key API.
//!
//! SRS: `CFG-CL-033 / 1` (separate private-key files loaded from a configured
//! path with owner-only access) and `NFR-SEC-002 / 1` (key material persists
//! across restarts).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use storage_node::{KeyError, PrivateKey, KEY_LEN};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
#[test]
fn loads_a_32_byte_key_with_owner_only_permissions() {
    let directory = unique_temp_dir();
    let bytes: [u8; KEY_LEN] = std::array::from_fn(|index| (index + 1) as u8);
    let path = write_owner_only_file(&directory, &bytes);

    let key = PrivateKey::load(&path).expect("owner-only 32-byte key loads");

    assert_eq!(key.as_bytes(), &bytes);

    remove_temp_dir(&directory);
}

#[cfg(unix)]
#[test]
fn rejects_group_or_other_access() {
    let directory = unique_temp_dir();
    let path = write_file(&directory, &[0x11; KEY_LEN]);
    set_mode(&path, 0o644);

    let result = PrivateKey::load(&path);

    assert!(matches!(result, Err(KeyError::InsecurePermissions)));

    remove_temp_dir(&directory);
}

#[test]
fn rejects_a_missing_file() {
    let directory = unique_temp_dir();
    std::fs::create_dir_all(&directory).expect("temp directory can be created");
    let path = directory.join("missing.key");

    let result = PrivateKey::load(&path);

    assert!(matches!(result, Err(KeyError::Io(_))));

    remove_temp_dir(&directory);
}

#[test]
fn rejects_a_non_regular_file() {
    let directory = unique_temp_dir();
    std::fs::create_dir_all(&directory).expect("temp directory can be created");
    #[cfg(unix)]
    set_mode(&directory, 0o700);

    let result = PrivateKey::load(&directory);

    assert!(matches!(result, Err(KeyError::NotAFile)));

    remove_temp_dir(&directory);
}

#[test]
fn rejects_wrong_length() {
    let directory = unique_temp_dir();
    let path = write_owner_only_file(&directory, &[0x5a; 31]);

    let result = PrivateKey::load(&path);

    assert!(matches!(
        result,
        Err(KeyError::WrongLength {
            len: 31,
            expected: KEY_LEN,
        })
    ));

    remove_temp_dir(&directory);
}

#[test]
fn debug_redacts_the_key() {
    let directory = unique_temp_dir();
    let bytes: [u8; KEY_LEN] = std::array::from_fn(|index| 0xa0 + index as u8);
    let path = write_owner_only_file(&directory, &bytes);

    let key = PrivateKey::load(&path).expect("owner-only 32-byte key loads");
    let rendered = format!("{key:?}");

    assert!(rendered.contains("redacted"));
    assert!(!rendered.contains(&hex(&bytes)));

    remove_temp_dir(&directory);
}

#[test]
fn loads_the_same_material_again() {
    let directory = unique_temp_dir();
    let bytes: [u8; KEY_LEN] = std::array::from_fn(|index| index as u8 * 3);
    let path = write_owner_only_file(&directory, &bytes);

    let first = PrivateKey::load(&path).expect("owner-only 32-byte key loads");
    let second = PrivateKey::load(&path).expect("the same key loads again");

    assert_eq!(first.as_bytes(), second.as_bytes());
    assert_eq!(first.as_bytes(), &bytes);

    remove_temp_dir(&directory);
}

fn unique_temp_dir() -> PathBuf {
    let unique = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("homecloud-keys-{}-{unique}", std::process::id()))
}

fn write_file(directory: &Path, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(directory).expect("temp directory can be created");
    let path = directory.join("node.key");
    std::fs::write(&path, bytes).expect("key file can be written");
    path
}

fn write_owner_only_file(directory: &Path, bytes: &[u8]) -> PathBuf {
    let path = write_file(directory, bytes);
    #[cfg(unix)]
    set_mode(&path, 0o600);
    path
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions).expect("key file permissions can be changed");
}

fn remove_temp_dir(directory: &Path) {
    std::fs::remove_dir_all(directory).expect("temp directory can be removed");
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}
