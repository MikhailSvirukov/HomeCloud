//! On-disk storage layout for a storage node.
//!
//! SRS: `CFG-CL-035 / 1` (reversible lowercase-hex account directory) and
//! `CFG-CL-036 / 1` (accounts/raft roots and the blob path).

use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::metadata::UPLOAD_ID_LEN;

/// Directory holding the per-account blob directories.
const ACCOUNTS_DIR: &str = "accounts";
/// Directory holding Raft state.
const RAFT_DIR: &str = "raft";

/// Failure modes when building a storage path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// The login was empty or contained a byte outside ASCII `[A-Za-z0-9._-]`.
    #[error("login must be non-empty ASCII [A-Za-z0-9._-]")]
    InvalidLogin,
}

type Result<T> = core::result::Result<T, LayoutError>;

/// Data-directory layout of a storage node.
#[derive(Debug, Clone)]
pub struct StorageLayout {
    /// Configured data root.
    pub root: PathBuf,
}

impl StorageLayout {
    /// Builds the layout rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `root/accounts`.
    pub fn accounts_dir(&self) -> PathBuf {
        self.root.join(ACCOUNTS_DIR)
    }

    /// `root/raft`.
    pub fn raft_dir(&self) -> PathBuf {
        self.root.join(RAFT_DIR)
    }

    /// Blob path `root/accounts/<hex-login>/<hex-upload-id>`.
    pub fn blob_path(&self, login: &str, upload_id: [u8; UPLOAD_ID_LEN]) -> Result<PathBuf> {
        if !is_valid_login(login) {
            return Err(LayoutError::InvalidLogin);
        }
        Ok(self
            .accounts_dir()
            .join(hex_encode(login.as_bytes()))
            .join(hex_encode(&upload_id)))
    }

    /// Creates the `accounts` and `raft` directories if missing.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.accounts_dir())?;
        std::fs::create_dir_all(self.raft_dir())
    }

    /// Returns the total bytes of regular files under the accounts and raft roots; missing roots count as zero and symlinks are skipped.
    pub fn usage_bytes(&self) -> std::io::Result<u64> {
        let accounts = usage_under(&self.accounts_dir())?;
        let raft = usage_under(&self.raft_dir())?;
        accounts
            .checked_add(raft)
            .ok_or_else(|| std::io::Error::other("storage usage exceeds u64 range"))
    }
}

/// Sums regular-file lengths below `root`, treating a missing root as empty.
fn usage_under(root: &Path) -> std::io::Result<u64> {
    let mut entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    entries.try_fold(0_u64, |total, entry| {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let added = if file_type.is_dir() {
            usage_under(&entry.path())?
        } else if file_type.is_file() {
            entry.metadata()?.len()
        } else {
            0
        };
        total
            .checked_add(added)
            .ok_or_else(|| std::io::Error::other("storage usage exceeds u64 range"))
    })
}

fn is_valid_login(login: &str) -> bool {
    !login.is_empty()
        && login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
