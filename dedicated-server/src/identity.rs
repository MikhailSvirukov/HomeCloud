//! Loading and validation of the dedicated server's own key identity.
//!
//! The dedicated server keeps two separate private keys persisted on disk: one
//! to decrypt incoming messages and one to sign outgoing messages, as required
//! by the v1 specification. This module is the boundary that reads those files
//! and binds the caller-supplied node label to the loaded material.
//!
//! The key bytes are opaque: the v1 specification leaves key encoding and
//! algorithm choices open, so this module never parses or interprets them. It
//! only enforces non-empty, bounded-size files and rejects material shared
//! between the two roles. No cryptography happens here, and the bytes are
//! never printed.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Maximum key file size accepted, in bytes.
pub(crate) const MAX_KEY_BYTES: usize = 64 * 1024;

/// Result of loading or validating a [`ServerIdentity`].
type Result<T> = core::result::Result<T, IdentityError>;

/// Which of the two key roles an operation or error refers to.
#[derive(Debug)]
pub enum KeyRole {
    /// The key that decrypts incoming messages.
    Encryption,
    /// The key that signs outgoing messages.
    Signing,
}

impl fmt::Display for KeyRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Encryption => "encryption",
            Self::Signing => "signing",
        };
        formatter.write_str(name)
    }
}

/// The dedicated server's own key identity.
///
/// Holds the node label and the private key material read from the given
/// files. The bytes are opaque to this crate: key encoding and algorithm choices
/// are open parameters in the v1 specification, so this module never parses or
/// interprets them.
pub struct ServerIdentity {
    label: String,
    encryption_private_key: Vec<u8>,
    signing_private_key: Vec<u8>,
}

impl ServerIdentity {
    /// Read and validate the key identity for `label` from the two key file paths.
    pub fn load(label: &str, encryption_key: &Path, signing_key: &Path) -> Result<Self> {
        let encryption_private_key = read_key(KeyRole::Encryption, encryption_key)?;
        let signing_private_key = read_key(KeyRole::Signing, signing_key)?;

        validate_key(KeyRole::Encryption, encryption_key, &encryption_private_key)?;
        validate_key(KeyRole::Signing, signing_key, &signing_private_key)?;

        if encryption_private_key == signing_private_key {
            return Err(IdentityError::SharedKeyMaterial {
                path: encryption_key.to_path_buf(),
            });
        }

        Ok(Self {
            label: label.to_string(),
            encryption_private_key,
            signing_private_key,
        })
    }

    /// The node label bound to this key identity.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Opaque private key material used to decrypt incoming messages.
    pub fn encryption_private_key(&self) -> &[u8] {
        &self.encryption_private_key
    }

    /// Opaque private key material used to sign outgoing messages.
    pub fn signing_private_key(&self) -> &[u8] {
        &self.signing_private_key
    }
}

impl fmt::Debug for ServerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerIdentity")
            .field("label", &self.label)
            .field(
                "encryption_private_key",
                &format_args!("[{} bytes]", self.encryption_private_key.len()),
            )
            .field(
                "signing_private_key",
                &format_args!("[{} bytes]", self.signing_private_key.len()),
            )
            .finish()
    }
}

/// Errors produced while loading the key identity.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// Reading the key file of the given role failed.
    #[error("failed to read {role} key file '{path}': {source}")]
    Read {
        /// Role whose key file was read.
        role: KeyRole,
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The key file of the given role was empty.
    #[error("{role} key file '{path}' is empty")]
    Empty {
        /// Role whose key file was empty.
        role: KeyRole,
        /// Path that was empty.
        path: PathBuf,
    },
    /// The key file of the given role exceeded [`MAX_KEY_BYTES`].
    #[error("{role} key file '{path}' is larger than {MAX_KEY_BYTES} bytes")]
    Oversized {
        /// Role whose key file was oversized.
        role: KeyRole,
        /// Path that was oversized.
        path: PathBuf,
    },
    /// Both roles would be served by the same key material.
    #[error("encryption and signing key material must differ, but both come from '{path}'")]
    SharedKeyMaterial {
        /// Encryption path reported for the shared material.
        path: PathBuf,
    },
}

/// Read at most [`MAX_KEY_BYTES`] + 1 bytes, enough to detect an oversized file.
fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_KEY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Read the key file for `role`, mapping I/O failures to [`IdentityError`].
fn read_key(role: KeyRole, path: &Path) -> Result<Vec<u8>> {
    read_bounded(path).map_err(|source| IdentityError::Read {
        role,
        path: path.to_path_buf(),
        source,
    })
}

/// Reject an empty or oversized key for `role`, checking emptiness first.
fn validate_key(role: KeyRole, path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(IdentityError::Empty {
            role,
            path: path.to_path_buf(),
        });
    }
    if bytes.len() > MAX_KEY_BYTES {
        return Err(IdentityError::Oversized {
            role,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: &str = "dedicated-1";
    const ENCRYPTION_MARKER: &str = "encryption-marker-9f2c";
    const SIGNING_MARKER: &str = "signing-marker-4b81";

    /// Owns a per-test temporary directory and removes it on drop.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dedicated-server-identity-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("test directory must be created");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_key(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.file(name);
        std::fs::write(&path, bytes).expect("test key file must be written");
        path
    }

    #[test]
    fn load_reads_a_valid_pair_and_exposes_label_and_exact_bytes() {
        let dir = TempDir::new("valid-pair");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let loaded =
            ServerIdentity::load(LABEL, &encryption, &signing).expect("a valid pair must load");

        assert_eq!(loaded.label(), LABEL);
        assert_eq!(
            loaded.encryption_private_key(),
            ENCRYPTION_MARKER.as_bytes()
        );
        assert_eq!(loaded.signing_private_key(), SIGNING_MARKER.as_bytes());
    }

    #[test]
    fn load_reports_a_read_failure_for_each_role_with_its_path() {
        let dir = TempDir::new("read-failure");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());
        let missing_encryption = dir.file("absent-encryption.key");
        let missing_signing = dir.file("absent-signing.key");

        let encryption_error = ServerIdentity::load(LABEL, &missing_encryption, &signing)
            .expect_err("a missing encryption key must fail");
        let signing_error = ServerIdentity::load(LABEL, &encryption, &missing_signing)
            .expect_err("a missing signing key must fail");

        assert!(matches!(
            encryption_error,
            IdentityError::Read {
                role: KeyRole::Encryption,
                path,
                ..
            } if path == missing_encryption
        ));
        assert!(matches!(
            signing_error,
            IdentityError::Read {
                role: KeyRole::Signing,
                path,
                ..
            } if path == missing_signing
        ));
    }

    #[test]
    fn load_rejects_an_empty_key_file() {
        let dir = TempDir::new("empty");
        let encryption = write_key(&dir, "encryption.key", &[]);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let error =
            ServerIdentity::load(LABEL, &encryption, &signing).expect_err("an empty key must fail");

        assert!(matches!(
            error,
            IdentityError::Empty {
                role: KeyRole::Encryption,
                path
            } if path == encryption
        ));
    }

    #[test]
    fn load_rejects_a_far_oversized_key_file() {
        let dir = TempDir::new("oversized");
        let oversized = vec![0x55_u8; 4 * MAX_KEY_BYTES];
        let encryption = write_key(&dir, "encryption.key", &oversized);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let error = ServerIdentity::load(LABEL, &encryption, &signing)
            .expect_err("a far oversized key must fail");

        assert!(matches!(
            error,
            IdentityError::Oversized {
                role: KeyRole::Encryption,
                path
            } if path == encryption
        ));
    }

    #[test]
    fn load_accepts_a_key_of_exactly_the_max_size() {
        let dir = TempDir::new("max-size");
        let at_limit = vec![0x33_u8; MAX_KEY_BYTES];
        let encryption = write_key(&dir, "encryption.key", &at_limit);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let loaded = ServerIdentity::load(LABEL, &encryption, &signing)
            .expect("a key of exactly MAX_KEY_BYTES must load");

        assert_eq!(loaded.encryption_private_key().len(), MAX_KEY_BYTES);
    }

    #[test]
    fn load_rejects_distinct_files_holding_identical_bytes() {
        let dir = TempDir::new("shared-bytes");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", ENCRYPTION_MARKER.as_bytes());

        let error = ServerIdentity::load(LABEL, &encryption, &signing)
            .expect_err("identical key bytes must fail");

        assert!(matches!(error, IdentityError::SharedKeyMaterial { path } if path == encryption));
    }

    #[test]
    fn debug_prints_label_and_lengths_without_key_bytes() {
        let dir = TempDir::new("debug-redaction");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());
        let loaded =
            ServerIdentity::load(LABEL, &encryption, &signing).expect("a valid pair must load");

        let debug = format!("{loaded:?}");

        assert!(debug.contains(LABEL), "debug must show the label: {debug}");
        assert!(
            debug.contains(&format!("[{} bytes]", ENCRYPTION_MARKER.len())),
            "debug must show the encryption key length: {debug}"
        );
        assert!(
            debug.contains(&format!("[{} bytes]", SIGNING_MARKER.len())),
            "debug must show the signing key length: {debug}"
        );
        assert!(
            !debug.contains(ENCRYPTION_MARKER),
            "debug must not show encryption key bytes: {debug}"
        );
        assert!(
            !debug.contains(SIGNING_MARKER),
            "debug must not show signing key bytes: {debug}"
        );
    }
}
