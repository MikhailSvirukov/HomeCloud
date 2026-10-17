//! Upload metadata and the idempotency ledger.
//!
//! Each accepted upload version is described by an [`UploadRecord`]: a stable
//! [`UploadId`], the owning login, the original file name, the byte size, the
//! caller-supplied SHA-256 [`ContentHash`], and the acceptance timestamp. The
//! [`UploadLedger`] maps an upload id to the first recorded version so retries
//! are idempotent: a repeated upload id never creates a second version.
//!
//! This module deliberately implements no cryptography. [`ContentHash`] only
//! wraps 32 bytes produced elsewhere.

use std::collections::HashMap;
use std::fmt;

use core::fmt::Write as _;

/// Length of an upload identifier in bytes (`INT-NET-011 / 1`).
pub const UPLOAD_ID_LEN: usize = 16;
/// Length of a SHA-256 content digest in bytes.
pub const CONTENT_HASH_LEN: usize = 32;

/// Result alias for metadata validation.
type Result<T> = core::result::Result<T, MetadataError>;

/// Stable per-version upload identifier transmitted as 16 raw bytes (`INT-NET-011 / 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UploadId([u8; UPLOAD_ID_LEN]);

impl UploadId {
    /// Wraps the 16 identifier bytes.
    pub const fn from_bytes(bytes: [u8; UPLOAD_ID_LEN]) -> Self {
        Self(bytes)
    }

    /// Returns the raw identifier bytes.
    pub const fn as_bytes(&self) -> &[u8; UPLOAD_ID_LEN] {
        &self.0
    }
}

impl fmt::Display for UploadId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// SHA-256 digest of the original file bytes, supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; CONTENT_HASH_LEN]);

impl ContentHash {
    /// Wraps caller-supplied digest bytes.
    pub const fn from_bytes(bytes: [u8; CONTENT_HASH_LEN]) -> Self {
        Self(bytes)
    }

    /// Returns the raw digest bytes.
    pub const fn as_bytes(&self) -> &[u8; CONTENT_HASH_LEN] {
        &self.0
    }

    /// Returns the digest as lowercase hex, exactly 64 characters.
    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(CONTENT_HASH_LEN * 2);
        for byte in self.0 {
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }

    /// Parses exactly 64 hex characters (upper or lower case).
    pub fn parse_hex(hex: &str) -> Result<Self> {
        let bytes = hex.as_bytes();
        if bytes.len() != CONTENT_HASH_LEN * 2 {
            return Err(MetadataError::InvalidContentHashLength { len: bytes.len() });
        }
        let mut digest = [0u8; CONTENT_HASH_LEN];
        for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
            if let Some(offending) = pair.iter().find(|byte| !byte.is_ascii_hexdigit()) {
                return Err(MetadataError::InvalidContentHashHex {
                    character: char::from(*offending),
                });
            }
            let text =
                core::str::from_utf8(pair).map_err(|_| MetadataError::InvalidContentHashHex {
                    character: char::from(pair[0]),
                })?;
            digest[index] =
                u8::from_str_radix(text, 16).map_err(|_| MetadataError::InvalidContentHashHex {
                    character: char::from(pair[0]),
                })?;
        }
        Ok(Self(digest))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// Metadata recorded for one accepted upload version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRecord {
    pub upload_id: UploadId,
    pub login: String,
    pub original_name: String,
    pub size_bytes: u64,
    pub content_hash: ContentHash,
    pub accepted_at_ms: u64,
}

impl UploadRecord {
    /// Builds a record, trimming `login` and `original_name`.
    pub fn new(
        upload_id: UploadId,
        login: &str,
        original_name: &str,
        size_bytes: u64,
        content_hash: ContentHash,
        accepted_at_ms: u64,
    ) -> Result<Self> {
        let login = login.trim();
        let original_name = original_name.trim();
        if login.is_empty() {
            return Err(MetadataError::EmptyLogin);
        }
        if original_name.is_empty() {
            return Err(MetadataError::EmptyOriginalName);
        }
        Ok(Self {
            upload_id,
            login: login.to_owned(),
            original_name: original_name.to_owned(),
            size_bytes,
            content_hash,
            accepted_at_ms,
        })
    }
}

/// Outcome of recording an upload in the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    /// The upload id was new and the record was stored.
    Recorded,
    /// The upload id already existed and the stored record was kept.
    AlreadyPresent,
}

/// Idempotency ledger mapping an `upload_id` to the first recorded upload.
#[derive(Default)]
pub struct UploadLedger {
    records: HashMap<UploadId, UploadRecord>,
}

impl fmt::Debug for UploadLedger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadLedger")
            .field("len", &self.records.len())
            .finish()
    }
}

impl UploadLedger {
    /// Builds an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `record` if its `upload_id` is new ([`RecordOutcome::Recorded`]);
    /// otherwise keeps the already-stored record unchanged and returns
    /// [`RecordOutcome::AlreadyPresent`].
    pub fn record(&mut self, record: UploadRecord) -> RecordOutcome {
        let upload_id = record.upload_id;
        if let std::collections::hash_map::Entry::Vacant(entry) = self.records.entry(upload_id) {
            entry.insert(record);
            RecordOutcome::Recorded
        } else {
            RecordOutcome::AlreadyPresent
        }
    }

    /// Returns the record stored for `upload_id`, if any.
    pub fn get(&self, upload_id: &UploadId) -> Option<&UploadRecord> {
        self.records.get(upload_id)
    }

    /// Number of distinct upload ids recorded.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` when no upload has been recorded.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Failure modes when validating upload metadata.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetadataError {
    /// The login was empty after trimming.
    #[error("login must not be empty")]
    EmptyLogin,
    /// The original file name was empty after trimming.
    #[error("original name must not be empty")]
    EmptyOriginalName,
    /// The content hash was not exactly 64 hex characters.
    #[error("content hash length {len} is not {} hex characters", CONTENT_HASH_LEN * 2)]
    InvalidContentHashLength { len: usize },
    /// The content hash contained a non-hex character.
    #[error("content hash contains invalid hex character {character:?}")]
    InvalidContentHashHex { character: char },
}

#[cfg(test)]
mod tests {
    use super::{
        ContentHash, MetadataError, UploadId, UploadLedger, UploadRecord, CONTENT_HASH_LEN,
        UPLOAD_ID_LEN,
    };

    fn upload_id(seed: u8) -> UploadId {
        UploadId::from_bytes([seed; UPLOAD_ID_LEN])
    }

    fn content_hash() -> ContentHash {
        ContentHash::from_bytes([0xab; CONTENT_HASH_LEN])
    }

    fn record(id: u8, login: &str, name: &str) -> UploadRecord {
        UploadRecord::new(upload_id(id), login, name, 10, content_hash(), 1_000)
            .expect("test record is valid")
    }

    #[test]
    fn content_hash_parse_hex_accepts_uppercase() {
        let hash = content_hash();
        let uppercase = hash.to_hex().to_uppercase();
        assert_eq!(ContentHash::parse_hex(&uppercase), Ok(hash));
    }

    #[test]
    fn content_hash_parse_hex_rejects_leading_plus() {
        let hex = "+0".repeat(CONTENT_HASH_LEN);
        assert_eq!(
            ContentHash::parse_hex(&hex),
            Err(MetadataError::InvalidContentHashHex { character: '+' })
        );
    }

    #[test]
    fn content_hash_parse_hex_rejects_wrong_length() {
        assert_eq!(
            ContentHash::parse_hex("abc"),
            Err(MetadataError::InvalidContentHashLength { len: 3 })
        );
    }

    #[test]
    fn content_hash_hex_round_trips() {
        let hash = content_hash();
        let hex = hash.to_hex();

        assert_eq!(hex, "ab".repeat(CONTENT_HASH_LEN));
        assert_eq!(hash.to_string(), hex);
        assert_eq!(ContentHash::parse_hex(&hex), Ok(hash));
        assert_eq!(hash.as_bytes(), &[0xab; CONTENT_HASH_LEN]);
    }

    #[test]
    fn ledger_debug_reports_only_entry_count() {
        let mut ledger = UploadLedger::new();
        ledger.record(record(1, "secret-login", "secret-name.txt"));

        assert_eq!(format!("{ledger:?}"), "UploadLedger { len: 1 }");
    }
}
