//! Catalog entry, page, and request CBOR codecs.
//!
//! SRS: `FR-CAT-012 / 3`, `FR-CAT-013 / 2`, `FR-CAT-015 / 1`, `INT-NET-053 / 2`.

use ciborium::value::Value;

use crate::metadata::{ContentHash, UploadId};
use crate::raft::CommittedVersion;

/// Length of a snapshot token or cursor in bytes.
pub const CATALOG_TOKEN_LEN: usize = 16;

/// Result alias for catalog codec validation.
type Result<T> = core::result::Result<T, CatalogError>;

/// One catalog entry describing an accepted upload version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Identifier of the upload version.
    pub upload_id: UploadId,
    /// Original source file name without trimming or Unicode normalization.
    pub source_name: String,
    /// Client-supplied capture/creation time in milliseconds.
    pub timestamp_ms: i64,
    /// Original uncompressed byte size.
    pub original_size_bytes: u64,
    /// Compressed payload byte size.
    pub compressed_size_bytes: u64,
    /// SHA-256 digest of the original uncompressed bytes.
    pub sha256: ContentHash,
}

/// One page of catalog entries with its snapshot token and continuation cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPage {
    /// Snapshot token repeated on every page.
    pub snapshot_token: [u8; CATALOG_TOKEN_LEN],
    /// Continuation cursor, or `None` on the last page.
    pub next_cursor: Option<[u8; CATALOG_TOKEN_LEN]>,
    /// Entries in catalog order.
    pub entries: Vec<CatalogEntry>,
}

/// First-page or continuation catalog request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogRequest {
    /// Snapshot token from the previous response, or `None` for the first page.
    pub snapshot_token: Option<[u8; CATALOG_TOKEN_LEN]>,
    /// Continuation cursor from the previous response, or `None` for the first page.
    pub cursor: Option<[u8; CATALOG_TOKEN_LEN]>,
}

/// Failure modes when validating or coding catalog values.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The source name exceeds the configured UTF-8 byte limit.
    #[error("the source name exceeds the configured limit")]
    SourceNameTooLong,
    /// The capture time does not fit a signed 64-bit millisecond timestamp.
    #[error("the capture time does not fit a signed 64-bit millisecond timestamp")]
    TimestampOutOfRange,
    /// The request is not a two-element CBOR array.
    #[error("the catalog request must be a two-element CBOR array")]
    RequestShape,
    /// A request token is neither null nor a 16-byte CBOR byte string.
    #[error("a catalog request token must be null or a 16-byte CBOR byte string")]
    RequestToken,
    /// The request could not be decoded, or trailing bytes followed it.
    #[error("the catalog request could not be decoded")]
    Decode,
    /// The page could not be encoded.
    #[error("the catalog page could not be encoded")]
    Encode,
}

impl CatalogEntry {
    /// Builds an entry, rejecting a source name longer than `max_source_name_len` UTF-8 bytes without trimming or normalizing it.
    pub fn new(
        upload_id: UploadId,
        source_name: &str,
        timestamp_ms: i64,
        original_size_bytes: u64,
        compressed_size_bytes: u64,
        sha256: ContentHash,
        max_source_name_len: usize,
    ) -> Result<Self> {
        if source_name.len() > max_source_name_len {
            return Err(CatalogError::SourceNameTooLong);
        }
        Ok(Self {
            upload_id,
            source_name: source_name.to_owned(),
            timestamp_ms,
            original_size_bytes,
            compressed_size_bytes,
            sha256,
        })
    }

    /// Maps a committed version to its catalog entry using the capture time and stored sizes.
    pub fn from_committed(version: &CommittedVersion, max_source_name_len: usize) -> Result<Self> {
        let timestamp_ms = i64::try_from(version.record.captured_at_ms)
            .map_err(|_| CatalogError::TimestampOutOfRange)?;
        Self::new(
            version.record.upload_id,
            &version.record.original_name,
            timestamp_ms,
            version.record.size_bytes,
            version.record.compressed_size_bytes,
            version.record.content_hash,
            max_source_name_len,
        )
    }
}

/// Encodes `[snapshot_token, next_cursor, entries]` as CBOR.
pub fn encode_page(page: &CatalogPage) -> Result<Vec<u8>> {
    let entries: Vec<Value> = page.entries.iter().map(entry_value).collect();
    let value = Value::Array(vec![
        Value::Bytes(page.snapshot_token.to_vec()),
        page.next_cursor
            .map_or(Value::Null, |cursor| Value::Bytes(cursor.to_vec())),
        Value::Array(entries),
    ]);

    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes).map_err(|_| CatalogError::Encode)?;
    Ok(bytes)
}

/// Decodes a `[snapshot_token, cursor]` request; both elements are null or both are 16-byte byte strings.
pub fn decode_request(bytes: &[u8]) -> Result<CatalogRequest> {
    let mut slice = bytes;
    let request: Value = ciborium::de::from_reader(&mut slice).map_err(|_| CatalogError::Decode)?;
    if !slice.is_empty() {
        return Err(CatalogError::Decode);
    }

    let Value::Array(elements) = request else {
        return Err(CatalogError::RequestShape);
    };
    let [snapshot_token, cursor] = elements
        .try_into()
        .map_err(|_| CatalogError::RequestShape)?;

    match (request_token(snapshot_token)?, request_token(cursor)?) {
        (None, None) => Ok(CatalogRequest {
            snapshot_token: None,
            cursor: None,
        }),
        (Some(snapshot_token), Some(cursor)) => Ok(CatalogRequest {
            snapshot_token: Some(snapshot_token),
            cursor: Some(cursor),
        }),
        _ => Err(CatalogError::RequestToken),
    }
}

fn entry_value(entry: &CatalogEntry) -> Value {
    Value::Array(vec![
        Value::Bytes(entry.upload_id.as_bytes().to_vec()),
        Value::Text(entry.source_name.clone()),
        Value::Integer(entry.timestamp_ms.into()),
        Value::Integer(entry.original_size_bytes.into()),
        Value::Integer(entry.compressed_size_bytes.into()),
        Value::Bytes(entry.sha256.as_bytes().to_vec()),
    ])
}

fn request_token(value: Value) -> Result<Option<[u8; CATALOG_TOKEN_LEN]>> {
    match value {
        Value::Null => Ok(None),
        Value::Bytes(bytes) => {
            let token = bytes
                .as_slice()
                .try_into()
                .map_err(|_| CatalogError::RequestToken)?;
            Ok(Some(token))
        }
        _ => Err(CatalogError::RequestToken),
    }
}
