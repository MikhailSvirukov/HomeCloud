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
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("key file must be readable only by its owner")]
    InsecurePermissions,
    #[error("key file has {len} bytes, expected {expected}")]
    WrongLength { len: usize, expected: usize },
}

type Result<T> = core::result::Result<T, KeyError>;

/// A 32-byte private key loaded from a local file.
#[derive(Clone)]
pub struct PrivateKey([u8; KEY_LEN]);

impl PrivateKey {
    /// Loads `path`, requiring owner-only permissions and exactly 32 bytes.
    pub fn load(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        check_owner_only(&metadata)?;
        let bytes = std::fs::read(path)?;
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
