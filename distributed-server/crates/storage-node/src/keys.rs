//! Private key material loaded from configured local files.
//!
//! SRS: `CFG-CL-033 / 1`, `NFR-SEC-002 / 1`.

use std::fmt;
use std::io;
use std::path::Path;

/// Length in bytes of an X25519 or Ed25519 private key.
pub const KEY_LEN: usize = 32;

/// Failure modes when loading private key material.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// The key file could not be opened, inspected, or read.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The key file is accessible by a user other than its owner.
    #[error("key file must be readable only by its owner")]
    InsecurePermissions,
    /// The key file does not contain exactly [`KEY_LEN`] bytes.
    #[error("key file has {len} bytes, expected {expected}")]
    WrongLength { len: usize, expected: usize },
    /// The key path refers to something other than a regular file.
    #[error("key path is not a regular file")]
    NotAFile,
}

type Result<T> = core::result::Result<T, KeyError>;

/// A 32-byte private key loaded from a local file.
pub struct PrivateKey([u8; KEY_LEN]);

impl PrivateKey {
    /// Loads `path`, requiring owner-only permissions and exactly 32 bytes.
    pub fn load(path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(KeyError::NotAFile);
        }
        check_owner_only(&metadata)?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes)?;
        let len = bytes.len();
        let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyError::WrongLength {
            len,
            expected: KEY_LEN,
        })?;
        Ok(Self(key))
    }

    /// Exposes the key bytes to the crypto layer.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivateKey { key: <redacted> }")
    }
}

#[cfg(unix)]
fn check_owner_only(metadata: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(KeyError::InsecurePermissions);
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_owner_only(_metadata: &std::fs::Metadata) -> Result<()> {
    Ok(())
}
