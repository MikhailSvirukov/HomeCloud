//! Loading and validation of the dedicated server's own key identity.
//!
//! The dedicated server keeps two separate private keys persisted on disk: one
//! to decrypt incoming messages and one to sign outgoing messages, as required
//! by the v1 specification. This module is the boundary that reads those files
//! and binds the configured node label to the loaded material.
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

use crate::config::IdentityConfig;

/// Maximum key file size accepted, in bytes.
pub const MAX_KEY_BYTES: usize = 64 * 1024;

/// The dedicated server's own key identity.
///
/// Holds the node label and the private key material read from the configured
/// files. The bytes are opaque to this crate: key encoding and algorithm choices
/// are open parameters in the v1 specification, so this module never parses or
/// interprets them.
pub struct ServerIdentity {
    label: String,
    encryption_private_key: Vec<u8>,
    signing_private_key: Vec<u8>,
}

impl ServerIdentity {
    /// Read and validate the identity described by `identity`.
    ///
    /// # Errors
    /// Returns [`IdentityError`] when a key file cannot be read, is empty, is
    /// larger than [`MAX_KEY_BYTES`], or when the same material would serve both
    /// roles.
    pub fn load(identity: &IdentityConfig) -> Result<Self, IdentityError> {
        let encryption_private_key = read_encryption_key(&identity.encryption_private_key)?;
        let signing_private_key = read_signing_key(&identity.signing_private_key)?;

        validate_encryption_key(&identity.encryption_private_key, &encryption_private_key)?;
        validate_signing_key(&identity.signing_private_key, &signing_private_key)?;

        if identity.encryption_private_key == identity.signing_private_key
            || encryption_private_key == signing_private_key
        {
            return Err(IdentityError::SharedKeyMaterial {
                path: identity.encryption_private_key.clone(),
            });
        }

        Ok(Self {
            label: identity.label.clone(),
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
    /// Reading the encryption key file failed.
    #[error("failed to read encryption key file '{path}': {source}")]
    ReadEncryptionKey {
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// Reading the signing key file failed.
    #[error("failed to read signing key file '{path}': {source}")]
    ReadSigningKey {
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The encryption key file was empty.
    #[error("encryption key file '{path}' is empty")]
    EmptyEncryptionKey {
        /// Path that was empty.
        path: PathBuf,
    },
    /// The signing key file was empty.
    #[error("signing key file '{path}' is empty")]
    EmptySigningKey {
        /// Path that was empty.
        path: PathBuf,
    },
    /// The encryption key file exceeded [`MAX_KEY_BYTES`].
    #[error("encryption key file '{path}' is larger than {MAX_KEY_BYTES} bytes")]
    OversizedEncryptionKey {
        /// Path that was oversized.
        path: PathBuf,
    },
    /// The signing key file exceeded [`MAX_KEY_BYTES`].
    #[error("signing key file '{path}' is larger than {MAX_KEY_BYTES} bytes")]
    OversizedSigningKey {
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

/// Read at most [`MAX_KEY_BYTES`] + 1 bytes from `path`.
///
/// The read is capped at one byte past the limit: that is enough to detect an
/// oversized file, so a misconfigured path pointing at a huge file is rejected
/// without being loaded into memory in full.
fn read_bounded(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_KEY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Read the encryption key file, mapping I/O failures to [`IdentityError`].
fn read_encryption_key(path: &Path) -> Result<Vec<u8>, IdentityError> {
    read_bounded(path).map_err(|source| IdentityError::ReadEncryptionKey {
        path: path.to_path_buf(),
        source,
    })
}

/// Read the signing key file, mapping I/O failures to [`IdentityError`].
fn read_signing_key(path: &Path) -> Result<Vec<u8>, IdentityError> {
    read_bounded(path).map_err(|source| IdentityError::ReadSigningKey {
        path: path.to_path_buf(),
        source,
    })
}

/// Reject an empty or oversized encryption key, checking emptiness first.
fn validate_encryption_key(path: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    if bytes.is_empty() {
        Err(IdentityError::EmptyEncryptionKey {
            path: path.to_path_buf(),
        })
    } else if bytes.len() > MAX_KEY_BYTES {
        Err(IdentityError::OversizedEncryptionKey {
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

/// Reject an empty or oversized signing key, checking emptiness first.
fn validate_signing_key(path: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    if bytes.is_empty() {
        Err(IdentityError::EmptySigningKey {
            path: path.to_path_buf(),
        })
    } else if bytes.len() > MAX_KEY_BYTES {
        Err(IdentityError::OversizedSigningKey {
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
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

    fn identity(encryption_private_key: PathBuf, signing_private_key: PathBuf) -> IdentityConfig {
        IdentityConfig {
            label: LABEL.to_string(),
            encryption_private_key,
            signing_private_key,
        }
    }

    #[test]
    fn load_reads_a_valid_pair_and_exposes_label_and_exact_bytes() {
        let dir = TempDir::new("valid-pair");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let loaded =
            ServerIdentity::load(&identity(encryption, signing)).expect("a valid pair must load");

        assert_eq!(loaded.label(), LABEL);
        assert_eq!(
            loaded.encryption_private_key(),
            ENCRYPTION_MARKER.as_bytes()
        );
        assert_eq!(loaded.signing_private_key(), SIGNING_MARKER.as_bytes());
    }

    #[test]
    fn load_reports_a_missing_encryption_key_with_its_path() {
        let dir = TempDir::new("missing-encryption");
        let encryption = dir.file("absent-encryption.key");
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let error = ServerIdentity::load(&identity(encryption.clone(), signing))
            .expect_err("a missing encryption key must fail");

        assert!(
            matches!(error, IdentityError::ReadEncryptionKey { path, .. } if path == encryption)
        );
    }

    #[test]
    fn load_reports_a_missing_signing_key_with_its_path() {
        let dir = TempDir::new("missing-signing");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = dir.file("absent-signing.key");

        let error = ServerIdentity::load(&identity(encryption, signing.clone()))
            .expect_err("a missing signing key must fail");

        assert!(matches!(error, IdentityError::ReadSigningKey { path, .. } if path == signing));
    }

    #[test]
    fn load_reports_an_empty_encryption_key() {
        let dir = TempDir::new("empty-encryption");
        let encryption = write_key(&dir, "encryption.key", &[]);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let error = ServerIdentity::load(&identity(encryption.clone(), signing))
            .expect_err("an empty encryption key must fail");

        assert!(matches!(error, IdentityError::EmptyEncryptionKey { path } if path == encryption));
    }

    #[test]
    fn load_reports_an_empty_signing_key() {
        let dir = TempDir::new("empty-signing");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", &[]);

        let error = ServerIdentity::load(&identity(encryption, signing.clone()))
            .expect_err("an empty signing key must fail");

        assert!(matches!(error, IdentityError::EmptySigningKey { path } if path == signing));
    }

    #[test]
    fn load_reports_an_oversized_encryption_key() {
        let dir = TempDir::new("oversized-encryption");
        let oversized = vec![0x11_u8; MAX_KEY_BYTES + 1];
        let encryption = write_key(&dir, "encryption.key", &oversized);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let error = ServerIdentity::load(&identity(encryption.clone(), signing))
            .expect_err("an oversized encryption key must fail");

        assert!(
            matches!(error, IdentityError::OversizedEncryptionKey { path } if path == encryption)
        );
    }

    #[test]
    fn load_reports_an_oversized_signing_key() {
        let dir = TempDir::new("oversized-signing");
        let oversized = vec![0x22_u8; MAX_KEY_BYTES + 1];
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", &oversized);

        let error = ServerIdentity::load(&identity(encryption, signing.clone()))
            .expect_err("an oversized signing key must fail");

        assert!(matches!(error, IdentityError::OversizedSigningKey { path } if path == signing));
    }

    #[test]
    fn load_accepts_an_encryption_key_of_exactly_the_max_size() {
        let dir = TempDir::new("max-size-encryption");
        let at_limit = vec![0x33_u8; MAX_KEY_BYTES];
        let encryption = write_key(&dir, "encryption.key", &at_limit);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let loaded = ServerIdentity::load(&identity(encryption, signing))
            .expect("a key of exactly MAX_KEY_BYTES must load");

        assert_eq!(loaded.encryption_private_key().len(), MAX_KEY_BYTES);
    }

    #[test]
    fn load_accepts_a_signing_key_of_exactly_the_max_size() {
        let dir = TempDir::new("max-size-signing");
        let at_limit = vec![0x44_u8; MAX_KEY_BYTES];
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", &at_limit);

        let loaded = ServerIdentity::load(&identity(encryption, signing))
            .expect("a key of exactly MAX_KEY_BYTES must load");

        assert_eq!(loaded.signing_private_key().len(), MAX_KEY_BYTES);
    }

    #[test]
    fn load_rejects_a_far_oversized_encryption_key() {
        let dir = TempDir::new("far-oversized-encryption");
        let oversized = vec![0x55_u8; 4 * MAX_KEY_BYTES];
        let encryption = write_key(&dir, "encryption.key", &oversized);
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());

        let error = ServerIdentity::load(&identity(encryption.clone(), signing))
            .expect_err("a far oversized encryption key must fail");

        assert!(
            matches!(error, IdentityError::OversizedEncryptionKey { path } if path == encryption)
        );
    }

    #[test]
    fn load_rejects_both_roles_pointing_at_the_same_file() {
        let dir = TempDir::new("shared-path");
        let shared = write_key(&dir, "shared.key", ENCRYPTION_MARKER.as_bytes());

        let error = ServerIdentity::load(&identity(shared.clone(), shared.clone()))
            .expect_err("the same file for both roles must fail");

        assert!(matches!(error, IdentityError::SharedKeyMaterial { path } if path == shared));
    }

    #[test]
    fn load_rejects_distinct_files_holding_identical_bytes() {
        let dir = TempDir::new("shared-bytes");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", ENCRYPTION_MARKER.as_bytes());

        let error = ServerIdentity::load(&identity(encryption.clone(), signing))
            .expect_err("identical key bytes must fail");

        assert!(matches!(error, IdentityError::SharedKeyMaterial { path } if path == encryption));
    }

    #[test]
    fn load_reports_read_failures_before_emptiness_checks() {
        let dir = TempDir::new("read-before-checks");
        let encryption = write_key(&dir, "encryption.key", &[]);
        let signing = dir.file("absent-signing.key");

        let error = ServerIdentity::load(&identity(encryption, signing.clone()))
            .expect_err("a missing signing key must outrank the empty encryption key");

        assert!(matches!(error, IdentityError::ReadSigningKey { path, .. } if path == signing));
    }

    #[test]
    fn debug_prints_label_and_lengths_without_key_bytes() {
        let dir = TempDir::new("debug-redaction");
        let encryption = write_key(&dir, "encryption.key", ENCRYPTION_MARKER.as_bytes());
        let signing = write_key(&dir, "signing.key", SIGNING_MARKER.as_bytes());
        let loaded =
            ServerIdentity::load(&identity(encryption, signing)).expect("a valid pair must load");

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
