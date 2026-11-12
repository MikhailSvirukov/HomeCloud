//! The dedicated server's registration secret and content tag (`INT-NET-109 / 1`).
//!
//! A repeated registration is recognised by a content tag,
//! `HMAC-SHA-256(K, deterministic_cbor(["homecloud.registration.v1", login,
//! password, x25519_key, ed25519_key]))`, where `K` is a 32-byte secret kept in
//! its own file at an operator-configured path (`CFG-CL-048 / 1`). The secret is
//! never created, replaced or logged here, and the tag is computed over the
//! exact bytes of the array with no string normalisation.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use ciborium::Value;
use ring::hmac;
use thiserror::Error;

/// Length of the registration secret `K` in bytes (`CFG-CL-048 / 1`).
pub const REGISTRATION_SECRET_BYTES: usize = 32;

/// Largest file the loader reads: one byte above the secret length.
const MAX_SECRET_FILE_BYTES: u64 = REGISTRATION_SECRET_BYTES as u64 + 1;

/// Domain separator of the content-tag input (`INT-NET-109 / 1`).
const TAG_DOMAIN: &str = "homecloud.registration.v1";

/// Bits that must not be set on the secret file: group and other access.
#[cfg(unix)]
const FORBIDDEN_PERMISSION_BITS: u32 = 0o077;

/// Errors produced while loading the secret or computing a content tag.
#[derive(Debug, Error)]
pub enum RegistrationSecretError {
    /// The secret file could not be read.
    #[error("failed to read the registration secret '{path}': {source}")]
    Read {
        /// Configured path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The secret file was larger than one secret plus one byte.
    #[error("the registration secret '{path}' is larger than {REGISTRATION_SECRET_BYTES} bytes")]
    TooLarge {
        /// Configured path.
        path: PathBuf,
    },
    /// The secret file does not hold exactly 32 bytes.
    #[error("the registration secret is {actual} bytes, expected {REGISTRATION_SECRET_BYTES}")]
    WrongLength {
        /// Observed length.
        actual: usize,
    },
    /// The secret file is readable by group or other.
    #[cfg(unix)]
    #[error("the registration secret '{path}' has permissions {mode:o}; group and other access must be removed")]
    InsecurePermissions {
        /// Configured path.
        path: PathBuf,
        /// Observed permission bits.
        mode: u32,
    },
    /// The tag input could not be encoded.
    #[error("the registration tag input could not be encoded: {0}")]
    Encode(String),
}

/// The dedicated server's registration secret `K`.
///
/// The manual [`fmt::Debug`] implementation prints no bytes.
pub struct RegistrationSecret([u8; REGISTRATION_SECRET_BYTES]);

impl RegistrationSecret {
    /// Read `K` from `path`, refusing anything but exactly 32 bytes.
    ///
    /// The file is never created or replaced. On Unix the file must not be
    /// readable by group or other (`CFG-CL-033 / 1`).
    pub fn load(path: &Path) -> Result<Self, RegistrationSecretError> {
        let metadata = fs::metadata(path).map_err(|source| RegistrationSecretError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.len() > REGISTRATION_SECRET_BYTES as u64 {
            return Err(RegistrationSecretError::TooLarge {
                path: path.to_path_buf(),
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = metadata.permissions().mode();
            if mode & FORBIDDEN_PERMISSION_BITS != 0 {
                return Err(RegistrationSecretError::InsecurePermissions {
                    path: path.to_path_buf(),
                    mode,
                });
            }
        }

        let file = fs::File::open(path).map_err(|source| RegistrationSecretError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let mut bytes = Vec::with_capacity(MAX_SECRET_FILE_BYTES as usize);
        file.take(MAX_SECRET_FILE_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|source| RegistrationSecretError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        let actual = bytes.len();
        let secret: [u8; REGISTRATION_SECRET_BYTES] = bytes
            .try_into()
            .map_err(|_| RegistrationSecretError::WrongLength { actual })?;

        Ok(Self(secret))
    }

    fn key(&self) -> hmac::Key {
        hmac::Key::new(hmac::HMAC_SHA256, &self.0)
    }
}

impl fmt::Debug for RegistrationSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistrationSecret")
            .field("bytes", &REGISTRATION_SECRET_BYTES)
            .finish()
    }
}

/// Compute the content tag of one registration command (`INT-NET-109 / 1`).
///
/// The input is the deterministic CBOR array
/// `["homecloud.registration.v1", login, password, x25519_key, ed25519_key]`;
/// strings are used exactly as given, without trimming or normalisation.
pub fn content_tag(
    secret: &RegistrationSecret,
    login: &str,
    password: &str,
    x25519_key: &[u8; 32],
    ed25519_key: &[u8; 32],
) -> Result<[u8; 32], RegistrationSecretError> {
    let input = Value::Array(vec![
        Value::Text(TAG_DOMAIN.to_owned()),
        Value::Text(login.to_owned()),
        Value::Text(password.to_owned()),
        Value::Bytes(x25519_key.to_vec()),
        Value::Bytes(ed25519_key.to_vec()),
    ]);
    let mut encoded = Vec::new();
    ciborium::ser::into_writer(&input, &mut encoded)
        .map_err(|error| RegistrationSecretError::Encode(error.to_string()))?;
    let tag = hmac::sign(&secret.key(), &encoded);
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(tag.as_ref());

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const SECRET: [u8; REGISTRATION_SECRET_BYTES] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ];
    const LOGIN: &str = "alice";
    const PASSWORD: &str = "secret";
    const X25519_KEY: [u8; 32] = [0x11; 32];
    const ED25519_KEY: [u8; 32] = [0x22; 32];
    /// HMAC-SHA-256 of the exact array with the secret above.
    const TAG: [u8; 32] = [
        0x27, 0xb1, 0x5e, 0x20, 0x0d, 0xdd, 0x5b, 0x26, 0x02, 0x53, 0xf3, 0x55, 0x14, 0x4c, 0xea,
        0x7c, 0x8c, 0x16, 0xf8, 0x6d, 0x2c, 0xaa, 0x75, 0x62, 0x0d, 0x09, 0x55, 0x6e, 0x7d, 0x22,
        0xe7, 0x42,
    ];

    struct TempFile(PathBuf);

    impl TempFile {
        fn write(contents: &[u8]) -> Self {
            Self::write_with_mode(contents, 0o600)
        }

        fn write_with_mode(contents: &[u8], mode: u32) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};

            static NEXT_ID: AtomicU64 = AtomicU64::new(0);

            let path = std::env::temp_dir().join(format!(
                "homecloud-registration-secret-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = fs::File::create(&path).expect("the secret file must be created");
            file.write_all(contents)
                .expect("the secret file must be written");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                    .expect("the secret permissions must be set");
            }
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn secret() -> RegistrationSecret {
        let file = TempFile::write(&SECRET);
        RegistrationSecret::load(file.path()).expect("the secret must load")
    }

    #[test]
    fn loads_exactly_thirty_two_bytes() {
        let file = TempFile::write(&SECRET);

        let secret = RegistrationSecret::load(file.path()).expect("the secret must load");

        assert!(format!("{secret:?}").contains("bytes: 32"));
    }

    #[test]
    fn rejects_a_secret_of_the_wrong_length() {
        let file = TempFile::write(&SECRET[..31]);

        assert!(matches!(
            RegistrationSecret::load(file.path()),
            Err(RegistrationSecretError::WrongLength { actual: 31 })
        ));
    }

    #[test]
    fn rejects_an_oversized_secret_without_reading_it_fully() {
        let mut contents = SECRET.to_vec();
        contents.push(0x00);

        let file = TempFile::write(&contents);

        assert!(matches!(
            RegistrationSecret::load(file.path()),
            Err(RegistrationSecretError::TooLarge { .. })
        ));
    }

    #[test]
    fn rejects_a_missing_secret_file() {
        let path = std::env::temp_dir().join(format!(
            "homecloud-registration-secret-absent-{}",
            std::process::id()
        ));

        assert!(matches!(
            RegistrationSecret::load(&path),
            Err(RegistrationSecretError::Read { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_group_or_world_readable_secret() {
        let file = TempFile::write_with_mode(&SECRET, 0o644);

        assert!(matches!(
            RegistrationSecret::load(file.path()),
            Err(RegistrationSecretError::InsecurePermissions { .. })
        ));
    }

    #[test]
    fn the_content_tag_matches_the_pinned_vector() {
        let tag = content_tag(&secret(), LOGIN, PASSWORD, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed");

        assert_eq!(tag, TAG);
    }

    #[test]
    fn every_content_tag_input_changes_the_tag() {
        let secret = secret();
        let base = content_tag(&secret, LOGIN, PASSWORD, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed");

        for changed in [
            content_tag(&secret, "alice2", PASSWORD, &X25519_KEY, &ED25519_KEY),
            content_tag(&secret, LOGIN, "secret2", &X25519_KEY, &ED25519_KEY),
            content_tag(&secret, LOGIN, PASSWORD, &[0x12; 32], &ED25519_KEY),
            content_tag(&secret, LOGIN, PASSWORD, &X25519_KEY, &[0x23; 32]),
        ] {
            assert_ne!(
                changed.expect("the tag must be computed"),
                base,
                "a changed input must change the tag"
            );
        }
    }

    #[test]
    fn the_secret_survives_a_restart_unchanged() {
        let file = TempFile::write(&SECRET);

        let first = RegistrationSecret::load(file.path()).expect("the first load must work");
        let second = RegistrationSecret::load(file.path()).expect("the second load must work");
        let first_tag = content_tag(&first, LOGIN, PASSWORD, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed");
        let second_tag = content_tag(&second, LOGIN, PASSWORD, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed");

        assert_eq!(first_tag, second_tag);
        assert_eq!(
            fs::metadata(file.path())
                .expect("the secret file must remain")
                .len(),
            REGISTRATION_SECRET_BYTES as u64
        );
    }

    #[test]
    fn the_content_tag_is_stable_for_the_same_inputs() {
        let secret = secret();

        let first = content_tag(&secret, LOGIN, PASSWORD, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed");
        let second = content_tag(&secret, LOGIN, PASSWORD, &X25519_KEY, &ED25519_KEY)
            .expect("the tag must be computed");

        assert_eq!(first, second);
    }

    #[test]
    fn the_secret_debug_output_omits_the_bytes() {
        let rendered = format!("{:?}", secret());

        assert!(!rendered.contains("00010203"), "{rendered}");
        assert!(rendered.contains("RegistrationSecret"), "{rendered}");
    }
}
