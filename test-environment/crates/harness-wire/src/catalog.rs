//! Catalog page request and answer structures.
//!
//! A catalog page is requested directly from a storage node and answered with
//! the result argument array `[snapshot_token, next_cursor, entries]`. Each
//! entry is the six-position array fixed by `FR-CAT-012 / 3`, and the page
//! structures are fixed by `FR-CAT-013 / 2` and `INT-NET-080 / 1`.

use crate::cbor::Value;
use crate::inter_server::encode_int64;

const UPLOAD_ID: &str = "upload_id";
const SOURCE_NAME: &str = "source_name";
const TIMESTAMP_MS: &str = "timestamp_ms";
const ORIGINAL_SIZE_BYTES: &str = "original_size_bytes";
const COMPRESSED_SIZE_BYTES: &str = "compressed_size_bytes";
const SHA256: &str = "sha256";
const SNAPSHOT_TOKEN: &str = "snapshot_token";
const NEXT_CURSOR: &str = "next_cursor";
const CURSOR: &str = "cursor";
const ENTRIES: &str = "entries";

/// One catalog entry (`FR-CAT-012 / 3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// UUID of the upload.
    pub upload_id: [u8; 16],
    /// Original source name, not normalized and limited in UTF-8 bytes.
    pub source_name: String,
    /// Upload timestamp in Unix milliseconds.
    pub timestamp_ms: i64,
    /// Size of the original bytes.
    pub original_size_bytes: u64,
    /// Size of the compressed bytes.
    pub compressed_size_bytes: u64,
    /// SHA-256 of the original bytes.
    pub sha256: [u8; 32],
}

impl CatalogEntry {
    /// `[upload_id, source_name, timestamp_ms, original_size_bytes, compressed_size_bytes, sha256]`.
    pub fn to_value(&self, max_source_name_bytes: usize) -> Result<Value, CatalogError> {
        if self.source_name.len() > max_source_name_bytes {
            return Err(CatalogError::SourceNameTooLong {
                actual: self.source_name.len(),
                limit: max_source_name_bytes,
            });
        }

        Ok(Value::Array(vec![
            Value::Bytes(self.upload_id.to_vec()),
            Value::Text(self.source_name.clone()),
            encode_int64(self.timestamp_ms),
            Value::Uint(self.original_size_bytes),
            Value::Uint(self.compressed_size_bytes),
            Value::Bytes(self.sha256.to_vec()),
        ]))
    }

    /// Decodes and validates one six-position catalog entry.
    pub fn from_value(value: &Value, max_source_name_bytes: usize) -> Result<Self, CatalogError> {
        let Value::Array(items) = value else {
            return Err(CatalogError::WrongEntryShape);
        };

        let [
            upload_id,
            source_name,
            timestamp_ms,
            original_size_bytes,
            compressed_size_bytes,
            sha256,
        ] = items.as_slice()
        else {
            return Err(CatalogError::WrongEntryShape);
        };

        let upload_id = fixed_bytes::<16>(upload_id, UPLOAD_ID)?;

        let Value::Text(source_name) = source_name else {
            return Err(CatalogError::WrongType(SOURCE_NAME));
        };
        if source_name.len() > max_source_name_bytes {
            return Err(CatalogError::SourceNameTooLong {
                actual: source_name.len(),
                limit: max_source_name_bytes,
            });
        }

        let timestamp_ms = match timestamp_ms {
            Value::Uint(number) => {
                i64::try_from(*number).map_err(|_| CatalogError::TimestampOutOfRange(*number))?
            }
            Value::Int(number) => *number,
            _ => return Err(CatalogError::WrongType(TIMESTAMP_MS)),
        };

        let original_size_bytes = decode_size(original_size_bytes, ORIGINAL_SIZE_BYTES)?;
        let compressed_size_bytes = decode_size(compressed_size_bytes, COMPRESSED_SIZE_BYTES)?;
        let sha256 = fixed_bytes::<32>(sha256, SHA256)?;

        Ok(Self {
            upload_id,
            source_name: source_name.clone(),
            timestamp_ms,
            original_size_bytes,
            compressed_size_bytes,
            sha256,
        })
    }
}

/// The successful catalog page answer arguments `[snapshot_token, next_cursor, entries]` (`FR-CAT-013 / 2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPage {
    /// Snapshot token present on every page.
    pub snapshot_token: [u8; 16],
    /// Cursor for the next page, or `None` on the last page.
    pub next_cursor: Option<[u8; 16]>,
    /// Entries of this page, preserving their order.
    pub entries: Vec<CatalogEntry>,
}

impl CatalogPage {
    /// `[snapshot_token, next_cursor, entries]`; `next_cursor` is `null` on the last page.
    pub fn to_args(&self, max_source_name_bytes: usize) -> Result<Vec<Value>, CatalogError> {
        let entries = self
            .entries
            .iter()
            .map(|entry| entry.to_value(max_source_name_bytes))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(vec![
            Value::Bytes(self.snapshot_token.to_vec()),
            optional_bytes(self.next_cursor),
            Value::Array(entries),
        ])
    }

    /// Reads `[snapshot_token, next_cursor, entries]` and validates every entry.
    pub fn from_args(args: &[Value], max_source_name_bytes: usize) -> Result<Self, CatalogError> {
        let [snapshot_token, next_cursor, entries] = args else {
            return Err(CatalogError::WrongPageShape);
        };

        let snapshot_token = fixed_bytes::<16>(snapshot_token, SNAPSHOT_TOKEN)?;
        let next_cursor = optional_fixed_bytes::<16>(next_cursor, NEXT_CURSOR)?;

        let Value::Array(entries) = entries else {
            return Err(CatalogError::WrongType(ENTRIES));
        };
        let entries = entries
            .iter()
            .map(|entry| CatalogEntry::from_value(entry, max_source_name_bytes))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            snapshot_token,
            next_cursor,
            entries,
        })
    }
}

/// Catalog page request arguments `[snapshot_token, cursor]` (`INT-NET-080 / 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPageRequest {
    /// Snapshot token for a continuation page, or `None` for the first page.
    pub snapshot_token: Option<[u8; 16]>,
    /// Cursor for a continuation page, or `None` for the first page.
    pub cursor: Option<[u8; 16]>,
}

impl CatalogPageRequest {
    /// `[null, null]` for the first page, `[snapshot_token, cursor]` for a continuation.
    pub fn to_args(&self) -> Result<Vec<Value>, CatalogError> {
        if self.snapshot_token.is_some() != self.cursor.is_some() {
            return Err(CatalogError::IncompleteCursorPair);
        }

        Ok(vec![
            optional_bytes(self.snapshot_token),
            optional_bytes(self.cursor),
        ])
    }

    /// Decodes `[snapshot_token, cursor]` and enforces the cursor pair rule.
    pub fn from_args(args: &[Value]) -> Result<Self, CatalogError> {
        let [snapshot_token, cursor] = args else {
            return Err(CatalogError::WrongRequestShape);
        };

        let snapshot_token = optional_fixed_bytes::<16>(snapshot_token, SNAPSHOT_TOKEN)?;
        let cursor = optional_fixed_bytes::<16>(cursor, CURSOR)?;

        if snapshot_token.is_some() != cursor.is_some() {
            return Err(CatalogError::IncompleteCursorPair);
        }

        Ok(Self {
            snapshot_token,
            cursor,
        })
    }
}

/// Reasons a catalog structure is not valid for protocol version 1.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// A catalog entry is not a six-element array.
    #[error("catalog entry must be a six-element array")]
    WrongEntryShape,
    /// The catalog page answer is not a three-element array.
    #[error("catalog page answer must be a three-element array")]
    WrongPageShape,
    /// The catalog page request is not a two-element array.
    #[error("catalog page request must be a two-element array")]
    WrongRequestShape,
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// A byte string position has the wrong length.
    #[error("invalid byte-string length for {0}")]
    InvalidLength(&'static str),
    /// The timestamp does not fit `int64`.
    #[error("timestamp {0} does not fit int64")]
    TimestampOutOfRange(u64),
    /// The source name exceeds the configured UTF-8 byte limit.
    #[error("source_name is {actual} bytes, limit is {limit}")]
    SourceNameTooLong {
        /// Actual UTF-8 byte length of the source name.
        actual: usize,
        /// Configured maximum source-name length in UTF-8 bytes (`CFG-CL-032`).
        limit: usize,
    },
    /// The snapshot token and cursor are not both present or both null.
    #[error("snapshot_token and cursor must both be present or both be null")]
    IncompleteCursorPair,
}

fn decode_size(value: &Value, field: &'static str) -> Result<u64, CatalogError> {
    match value {
        Value::Uint(number) => Ok(*number),
        _ => Err(CatalogError::WrongType(field)),
    }
}

fn optional_bytes<const N: usize>(bytes: Option<[u8; N]>) -> Value {
    match bytes {
        Some(bytes) => Value::Bytes(bytes.to_vec()),
        None => Value::Null,
    }
}

fn fixed_bytes<const N: usize>(
    value: &Value,
    field: &'static str,
) -> Result<[u8; N], CatalogError> {
    match value {
        Value::Bytes(bytes) => {
            <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| CatalogError::InvalidLength(field))
        }
        _ => Err(CatalogError::WrongType(field)),
    }
}

fn optional_fixed_bytes<const N: usize>(
    value: &Value,
    field: &'static str,
) -> Result<Option<[u8; N]>, CatalogError> {
    match value {
        Value::Null => Ok(None),
        Value::Bytes(_) => fixed_bytes::<N>(value, field).map(Some),
        _ => Err(CatalogError::WrongType(field)),
    }
}
