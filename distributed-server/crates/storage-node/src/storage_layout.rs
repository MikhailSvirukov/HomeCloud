//! On-disk storage layout for a storage node.
//!
//! SRS: `CFG-CL-035 / 1` (reversible lowercase-hex account directory) and
//! `CFG-CL-036 / 1` (accounts/raft roots and the blob path).

use std::fmt::Write;
use std::path::PathBuf;

/// Directory holding the per-account blob directories.
const ACCOUNTS_DIR: &str = "accounts";
/// Directory holding Raft state.
const RAFT_DIR: &str = "raft";
/// Length of an upload id in bytes.
pub const UPLOAD_ID_LEN: usize = 16;

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
