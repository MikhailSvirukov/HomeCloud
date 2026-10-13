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
use std::str::FromStr;

/// Maximum accepted length of an upload identifier in bytes.
pub const MAX_UPLOAD_ID_LEN: usize = 64;
/// Length of a SHA-256 content digest in bytes.
pub const CONTENT_HASH_LEN: usize = 32;

/// Lowercase hexadecimal digits indexed by nibble value.
const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";

/// Stable per-version upload identifier enabling idempotent retries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UploadId(String);

impl UploadId {
    /// Validates `value` and builds an [`UploadId`].
    ///
    /// # Errors
    ///
    /// [`MetadataError::EmptyUploadId`] for an empty value,
    /// [`MetadataError::UploadIdTooLong`] beyond [`MAX_UPLOAD_ID_LEN`] bytes,
    /// or [`MetadataError::InvalidUploadIdCharacter`] for a character outside
    /// `[A-Za-z0-9_-]`.
    pub fn parse(value: &str) -> Result<Self, MetadataError> {
        if value.is_empty() {
            Err(MetadataError::EmptyUploadId)
        } else if value.len() > MAX_UPLOAD_ID_LEN {
            Err(MetadataError::UploadIdTooLong {
                len: value.len(),
                max: MAX_UPLOAD_ID_LEN,
            })
        } else if let Some(character) = value.chars().find(|c| !is_valid_upload_id_char(*c)) {
            Err(MetadataError::InvalidUploadIdCharacter { character })
        } else {
            Ok(Self(value.to_owned()))
        }
    }

    /// Returns the identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UploadId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for UploadId {
    type Err = MetadataError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
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
            hex.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
            hex.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }
        hex
    }

    /// Parses exactly 64 hex characters (upper or lower case).
    ///
    /// # Errors
    ///
    /// [`MetadataError::InvalidContentHashLength`] when `hex` is not 64 bytes,
    /// or [`MetadataError::InvalidContentHashHex`] for a non-hex character.
    pub fn parse_hex(hex: &str) -> Result<Self, MetadataError> {
        let bytes = hex.as_bytes();
        if bytes.len() != CONTENT_HASH_LEN * 2 {
            Err(MetadataError::InvalidContentHashLength { len: bytes.len() })
        } else {
            let mut digest = [0u8; CONTENT_HASH_LEN];
            for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
                digest[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
            }
            Ok(Self(digest))
        }
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
    upload_id: UploadId,
    login: String,
    original_name: String,
    size_bytes: u64,
    content_hash: ContentHash,
    accepted_at_ms: u64,
}

impl UploadRecord {
    /// Builds a record, trimming `login` and `original_name`.
    ///
    /// # Errors
    ///
    /// [`MetadataError::EmptyLogin`] when `login` is empty or whitespace-only,
    /// [`MetadataError::EmptyOriginalName`] when `original_name` is empty or
    /// whitespace-only.
    pub fn new(
        upload_id: UploadId,
        login: &str,
        original_name: &str,
        size_bytes: u64,
        content_hash: ContentHash,
        accepted_at_ms: u64,
    ) -> Result<Self, MetadataError> {
        let login = login.trim();
        let original_name = original_name.trim();
        if login.is_empty() {
            Err(MetadataError::EmptyLogin)
        } else if original_name.is_empty() {
            Err(MetadataError::EmptyOriginalName)
        } else {
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

    /// Upload identifier this version was recorded under.
    pub fn upload_id(&self) -> &UploadId {
        &self.upload_id
    }

    /// Trimmed login that owns the upload.
    pub fn login(&self) -> &str {
        &self.login
    }

    /// Trimmed original file name.
    pub fn original_name(&self) -> &str {
        &self.original_name
    }

    /// Size of the original file in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// SHA-256 digest of the original file bytes.
    pub fn content_hash(&self) -> ContentHash {
        self.content_hash
    }

    /// Acceptance time as milliseconds since the Unix epoch.
    pub fn accepted_at_ms(&self) -> u64 {
        self.accepted_at_ms
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
        let upload_id = record.upload_id.clone();
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    /// The upload identifier was empty.
    EmptyUploadId,
    /// The upload identifier exceeded [`MAX_UPLOAD_ID_LEN`] bytes.
    UploadIdTooLong { len: usize, max: usize },
    /// The upload identifier contained a character outside `[A-Za-z0-9_-]`.
    InvalidUploadIdCharacter { character: char },
    /// The login was empty after trimming.
    EmptyLogin,
    /// The original file name was empty after trimming.
    EmptyOriginalName,
    /// The content hash was not exactly 64 hex characters.
    InvalidContentHashLength { len: usize },
    /// The content hash contained a non-hex character.
    InvalidContentHashHex { character: char },
}

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyUploadId => write!(formatter, "upload id must not be empty"),
            Self::UploadIdTooLong { len, max } => {
                write!(formatter, "upload id length {len} exceeds maximum {max}")
            }
            Self::InvalidUploadIdCharacter { character } => {
                write!(
                    formatter,
                    "upload id contains invalid character {character:?}"
                )
            }
            Self::EmptyLogin => write!(formatter, "login must not be empty"),
            Self::EmptyOriginalName => write!(formatter, "original name must not be empty"),
            Self::InvalidContentHashLength { len } => {
                write!(
                    formatter,
                    "content hash length {len} is not {} hex characters",
                    CONTENT_HASH_LEN * 2
                )
            }
            Self::InvalidContentHashHex { character } => {
                write!(
                    formatter,
                    "content hash contains invalid hex character {character:?}"
                )
            }
        }
    }
}

impl std::error::Error for MetadataError {}

fn is_valid_upload_id_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

fn hex_nibble(byte: u8) -> Result<u8, MetadataError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        other => Err(MetadataError::InvalidContentHashHex {
            character: char::from(other),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContentHash, MetadataError, RecordOutcome, UploadId, UploadLedger, UploadRecord,
        CONTENT_HASH_LEN, MAX_UPLOAD_ID_LEN,
    };

    fn upload_id(value: &str) -> UploadId {
        UploadId::parse(value).expect("test upload id is valid")
    }

    fn content_hash() -> ContentHash {
        ContentHash::from_bytes([0xab; CONTENT_HASH_LEN])
    }

    fn new_record(login: &str, name: &str) -> Result<UploadRecord, MetadataError> {
        UploadRecord::new(
            upload_id("upload-1"),
            login,
            name,
            10,
            content_hash(),
            1_000,
        )
    }

    fn record(id: &str, login: &str, name: &str) -> UploadRecord {
        UploadRecord::new(upload_id(id), login, name, 10, content_hash(), 1_000)
            .expect("test record is valid")
    }

    #[test]
    fn upload_id_parse_round_trips_via_display_and_from_str() {
        let id = upload_id("upload-01_A");
        assert_eq!(id.as_str(), "upload-01_A");
        assert_eq!(id.to_string(), "upload-01_A");
        assert_eq!("upload-01_A".parse::<UploadId>(), Ok(id));
    }

    #[test]
    fn upload_id_parse_accepts_maximum_length() {
        assert!(UploadId::parse(&"a".repeat(MAX_UPLOAD_ID_LEN)).is_ok());
    }

    #[test]
    fn upload_id_parse_rejects_empty() {
        assert_eq!(UploadId::parse(""), Err(MetadataError::EmptyUploadId));
    }

    #[test]
    fn upload_id_parse_rejects_too_long() {
        let over = "a".repeat(MAX_UPLOAD_ID_LEN + 1);
        let expected = MetadataError::UploadIdTooLong {
            len: MAX_UPLOAD_ID_LEN + 1,
            max: MAX_UPLOAD_ID_LEN,
        };
        assert_eq!(UploadId::parse(&over), Err(expected));
    }

    #[test]
    fn upload_id_parse_rejects_invalid_character() {
        let expected = MetadataError::InvalidUploadIdCharacter { character: '!' };
        assert_eq!(UploadId::parse("upload!01"), Err(expected));
    }

    #[test]
    fn content_hash_round_trips_through_lowercase_hex() {
        let hash = content_hash();
        let hex = hash.to_hex();
        assert_eq!(hex.len(), CONTENT_HASH_LEN * 2);
        assert_eq!(hex, "ab".repeat(CONTENT_HASH_LEN));
        assert_eq!(hash.to_string(), hex);
        assert_eq!(ContentHash::parse_hex(&hex), Ok(hash));
    }

    #[test]
    fn content_hash_parse_hex_accepts_uppercase() {
        let hash = content_hash();
        let uppercase = hash.to_hex().to_uppercase();
        assert_eq!(ContentHash::parse_hex(&uppercase), Ok(hash));
    }

    #[test]
    fn content_hash_parse_hex_rejects_wrong_length() {
        let expected = MetadataError::InvalidContentHashLength { len: 3 };
        assert_eq!(ContentHash::parse_hex("abc"), Err(expected));
    }

    #[test]
    fn content_hash_parse_hex_rejects_invalid_hex_character() {
        let invalid = "z".repeat(CONTENT_HASH_LEN * 2);
        let expected = MetadataError::InvalidContentHashHex { character: 'z' };
        assert_eq!(ContentHash::parse_hex(&invalid), Err(expected));
    }

    #[test]
    fn upload_record_new_trims_and_stores_values() {
        let hash = content_hash();
        let record = UploadRecord::new(
            upload_id("upload-1"),
            "  alice  ",
            "  photo.jpg  ",
            4_096,
            hash,
            1_700_000_000_000,
        )
        .expect("record is valid");

        assert_eq!(record.upload_id().as_str(), "upload-1");
        assert_eq!(record.login(), "alice");
        assert_eq!(record.original_name(), "photo.jpg");
        assert_eq!(record.size_bytes(), 4_096);
        assert_eq!(record.content_hash(), hash);
        assert_eq!(record.accepted_at_ms(), 1_700_000_000_000);
    }

    #[test]
    fn upload_record_new_rejects_blank_login() {
        assert_eq!(new_record("", "photo.jpg"), Err(MetadataError::EmptyLogin));
        assert_eq!(
            new_record("   ", "photo.jpg"),
            Err(MetadataError::EmptyLogin)
        );
    }

    #[test]
    fn upload_record_new_rejects_blank_original_name() {
        assert_eq!(
            new_record("alice", ""),
            Err(MetadataError::EmptyOriginalName)
        );
        assert_eq!(
            new_record("alice", "   "),
            Err(MetadataError::EmptyOriginalName)
        );
    }

    #[test]
    fn ledger_record_returns_recorded_then_already_present() {
        let mut ledger = UploadLedger::new();
        assert_eq!(
            ledger.record(record("upload-1", "alice", "a.txt")),
            RecordOutcome::Recorded
        );
        assert_eq!(
            ledger.record(record("upload-1", "alice", "a.txt")),
            RecordOutcome::AlreadyPresent
        );
    }

    #[test]
    fn ledger_record_keeps_first_record_on_duplicate() {
        let mut ledger = UploadLedger::new();
        ledger.record(record("upload-1", "alice", "a.txt"));
        ledger.record(record("upload-1", "bob", "b.txt"));

        assert_eq!(ledger.len(), 1);
        let stored = ledger
            .get(&upload_id("upload-1"))
            .expect("record is present");
        assert_eq!(stored.login(), "alice");
        assert_eq!(stored.original_name(), "a.txt");
    }

    #[test]
    fn ledger_get_returns_none_for_unknown_id() {
        let ledger = UploadLedger::new();
        assert_eq!(ledger.get(&upload_id("missing")), None);
    }

    #[test]
    fn ledger_is_empty_transitions_after_recording() {
        let mut ledger = UploadLedger::new();
        assert!(ledger.is_empty());
        assert_eq!(ledger.len(), 0);

        ledger.record(record("upload-1", "alice", "a.txt"));

        assert!(!ledger.is_empty());
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn ledger_debug_reports_only_entry_count() {
        let mut ledger = UploadLedger::new();
        ledger.record(record("upload-1", "secret-login", "secret-name.txt"));

        assert_eq!(format!("{ledger:?}"), "UploadLedger { len: 1 }");
    }
}
