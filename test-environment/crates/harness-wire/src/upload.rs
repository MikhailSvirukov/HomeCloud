//! Client upload metadata and successful upload result.
//!
//! Upload metadata is the five-position array
//! `[source_name, timestamp_ms, original_size, compressed_size, sha256]`
//! (`INT-NET-084 / 1`). The successful upload response carries the single
//! 16-byte `upload_id`.

use crate::cbor::Value;
use crate::inter_server::encode_int64;

const SOURCE_NAME: &str = "source_name";
const TIMESTAMP_MS: &str = "timestamp_ms";
const ORIGINAL_SIZE: &str = "original_size";
const COMPRESSED_SIZE: &str = "compressed_size";
const SHA256: &str = "sha256";
const UPLOAD_ID: &str = "upload_id";

/// Client upload metadata (`INT-NET-084 / 1`, protocol-details §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadMetadata {
    /// Original source name, not normalized and limited in UTF-8 bytes.
    pub source_name: String,
    /// Upload timestamp in Unix milliseconds.
    pub timestamp_ms: i64,
    /// Size of the original bytes.
    pub original_size: u64,
    /// Size of the compressed bytes.
    pub compressed_size: u64,
    /// SHA-256 of the original bytes.
    pub sha256: [u8; 32],
}

impl UploadMetadata {
    /// `[source_name, timestamp_ms, original_size, compressed_size, sha256]`.
    pub fn to_args(
        &self,
        max_source_name_bytes: usize,
        max_size_bytes: u64,
    ) -> Result<Vec<Value>, UploadError> {
        if self.source_name.len() > max_source_name_bytes {
            return Err(UploadError::SourceNameTooLong {
                actual: self.source_name.len(),
                limit: max_source_name_bytes,
            });
        }
        validate_size(ORIGINAL_SIZE, self.original_size, max_size_bytes)?;
        validate_size(COMPRESSED_SIZE, self.compressed_size, max_size_bytes)?;

        Ok(vec![
            Value::Text(self.source_name.clone()),
            encode_int64(self.timestamp_ms),
            Value::Uint(self.original_size),
            Value::Uint(self.compressed_size),
            Value::Bytes(self.sha256.to_vec()),
        ])
    }

    /// Decodes and validates the five metadata positions.
    pub fn from_args(
        args: &[Value],
        max_source_name_bytes: usize,
        max_size_bytes: u64,
    ) -> Result<Self, UploadError> {
        let [
            source_name,
            timestamp_ms,
            original_size,
            compressed_size,
            sha256,
        ] = args
        else {
            return Err(UploadError::WrongMetadataShape);
        };

        let Value::Text(source_name) = source_name else {
            return Err(UploadError::WrongType(SOURCE_NAME));
        };
        if source_name.len() > max_source_name_bytes {
            return Err(UploadError::SourceNameTooLong {
                actual: source_name.len(),
                limit: max_source_name_bytes,
            });
        }

        let timestamp_ms = match timestamp_ms {
            Value::Uint(number) => {
                i64::try_from(*number).map_err(|_| UploadError::TimestampOutOfRange(*number))?
            }
            Value::Int(number) => *number,
            _ => return Err(UploadError::WrongType(TIMESTAMP_MS)),
        };

        Ok(Self {
            source_name: source_name.clone(),
            timestamp_ms,
            original_size: decode_size(original_size, ORIGINAL_SIZE, max_size_bytes)?,
            compressed_size: decode_size(compressed_size, COMPRESSED_SIZE, max_size_bytes)?,
            sha256: fixed_bytes::<32>(sha256, SHA256)?,
        })
    }
}

/// Successful upload response arguments `[upload_id]` (`INT-NET-084 / 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadResult {
    /// UUID assigned to the accepted upload.
    pub upload_id: [u8; 16],
}

impl UploadResult {
    /// `[upload_id]`.
    pub fn to_args(&self) -> Vec<Value> {
        vec![Value::Bytes(self.upload_id.to_vec())]
    }

    /// Decodes and validates the single upload-id position.
    pub fn from_args(args: &[Value]) -> Result<Self, UploadError> {
        let [upload_id] = args else {
            return Err(UploadError::WrongResultShape);
        };

        Ok(Self {
            upload_id: fixed_bytes::<16>(upload_id, UPLOAD_ID)?,
        })
    }
}

/// Reasons upload metadata or an upload result is not valid for protocol version 1.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum UploadError {
    /// The metadata is not a five-element CBOR array.
    #[error("upload metadata must be a five-element array")]
    WrongMetadataShape,
    /// The upload result is not a one-element CBOR array.
    #[error("upload result must be a one-element array")]
    WrongResultShape,
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// A byte string position has the wrong length.
    #[error("invalid byte-string length for {0}")]
    InvalidLength(&'static str),
    /// The source name exceeds the configured UTF-8 byte limit.
    #[error("source_name is {actual} bytes, limit is {limit}")]
    SourceNameTooLong {
        /// Actual UTF-8 byte length of the source name.
        actual: usize,
        /// Configured maximum source-name length in UTF-8 bytes (`CFG-CL-032`).
        limit: usize,
    },
    /// An upload size exceeds the configured limit.
    #[error("{field} is {value}, limit is {limit}")]
    SizeOutOfRange {
        /// Metadata field name.
        field: &'static str,
        /// Value carried by the field.
        value: u64,
        /// Configured maximum original or compressed size (`CFG-CL-020`).
        limit: u64,
    },
    /// The timestamp does not fit `int64`.
    #[error("timestamp {0} does not fit int64")]
    TimestampOutOfRange(u64),
}

fn validate_size(field: &'static str, value: u64, limit: u64) -> Result<(), UploadError> {
    if value > limit {
        Err(UploadError::SizeOutOfRange {
            field,
            value,
            limit,
        })
    } else {
        Ok(())
    }
}

fn decode_size(value: &Value, field: &'static str, limit: u64) -> Result<u64, UploadError> {
    match value {
        Value::Uint(number) => {
            validate_size(field, *number, limit)?;
            Ok(*number)
        }
        _ => Err(UploadError::WrongType(field)),
    }
}

fn fixed_bytes<const N: usize>(value: &Value, field: &'static str) -> Result<[u8; N], UploadError> {
    match value {
        Value::Bytes(bytes) => {
            <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| UploadError::InvalidLength(field))
        }
        _ => Err(UploadError::WrongType(field)),
    }
}
