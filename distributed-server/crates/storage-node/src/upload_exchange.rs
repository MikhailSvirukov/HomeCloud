//! Upload request client logical-header codec.
//!
//! SRS: `INT-NET-051 / 3`, `INT-NET-052 / 4`, `INT-NET-053 / 2`,
//! `INT-NET-055 / 2`.

use ciborium::value::Value;

use crate::metadata::{ContentHash, CONTENT_HASH_LEN};

/// Result alias for upload request validation.
type Result<T> = core::result::Result<T, UploadExchangeError>;

/// Client operation code of a media upload request.
const UPLOAD_OPERATION: u32 = 4;
/// Number of leading positions in a client logical header.
const CLIENT_LOGICAL_HEADER_LEN: usize = 4;
/// Number of positions in the upload operation arguments.
const UPLOAD_ARGUMENTS_LEN: usize = 5;

/// Decoded upload operation arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRequest {
    /// Source media file name, stored exactly as received.
    pub source_name: String,
    /// Upload time as a signed Unix timestamp in milliseconds.
    pub timestamp_ms: i64,
    /// Original uncompressed size in bytes.
    pub original_size_bytes: u64,
    /// Compressed size in bytes.
    pub compressed_size_bytes: u64,
    /// SHA-256 digest of the original bytes.
    pub sha256: ContentHash,
}

/// Decoded upload request with its client credentials.
pub struct UploadRequestHeader {
    login: String,
    password: String,
    request: UploadRequest,
}

impl UploadRequestHeader {
    /// Returns the client login exactly as received.
    pub fn login(&self) -> &str {
        &self.login
    }

    /// Returns the client password exactly as received.
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Returns the upload operation arguments.
    pub fn request(&self) -> &UploadRequest {
        &self.request
    }
}

/// Errors from encoding or decoding an upload request logical header.
#[derive(Debug, thiserror::Error)]
pub enum UploadExchangeError {
    /// The upload request is not a CBOR array.
    #[error("the upload request is not a CBOR array")]
    NotAnArray,
    /// The header or argument array does not have the required length.
    #[error("the upload request has the wrong array length")]
    WrongLength,
    /// The client operation is not upload.
    #[error("the client operation is not upload")]
    InvalidOperation,
    /// The login is empty or contains a byte outside the permitted ASCII set.
    #[error("the client login is not a valid ASCII login")]
    InvalidLogin,
    /// The source name is empty or exceeds the configured byte limit.
    #[error("the source name is empty or exceeds the configured byte limit")]
    InvalidSourceName,
    /// An upload size exceeds the configured maximum file size.
    #[error("an upload size exceeds the configured maximum file size")]
    InvalidSizes,
    /// The upload request could not be decoded, or trailing bytes followed it.
    #[error("the upload request could not be decoded")]
    Decode,
    /// The upload request could not be encoded.
    #[error("the upload request could not be encoded")]
    Encode,
}

/// Decodes a four-position upload client logical header.
pub fn decode_upload_request(
    logical_header: &[u8],
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<UploadRequestHeader> {
    let Value::Array(elements) = decode_value(logical_header)? else {
        return Err(UploadExchangeError::NotAnArray);
    };
    if elements.len() != CLIENT_LOGICAL_HEADER_LEN {
        return Err(UploadExchangeError::WrongLength);
    }
    let [operation, login, password, arguments] = elements
        .try_into()
        .map_err(|_| UploadExchangeError::WrongLength)?;

    if integer_u32(operation) != Some(UPLOAD_OPERATION) {
        return Err(UploadExchangeError::InvalidOperation);
    }

    let Value::Text(login) = login else {
        return Err(UploadExchangeError::InvalidLogin);
    };
    if !valid_login(&login) {
        return Err(UploadExchangeError::InvalidLogin);
    }

    let Value::Text(password) = password else {
        return Err(UploadExchangeError::Decode);
    };

    let Value::Array(arguments) = arguments else {
        return Err(UploadExchangeError::Decode);
    };
    if arguments.len() != UPLOAD_ARGUMENTS_LEN {
        return Err(UploadExchangeError::WrongLength);
    }
    let [source_name, timestamp_ms, original_size_bytes, compressed_size_bytes, sha256] = arguments
        .try_into()
        .map_err(|_| UploadExchangeError::WrongLength)?;

    let Value::Text(source_name) = source_name else {
        return Err(UploadExchangeError::Decode);
    };
    if !valid_source_name(&source_name, max_source_name_len) {
        return Err(UploadExchangeError::InvalidSourceName);
    }

    let timestamp_ms = signed_i64(timestamp_ms).ok_or(UploadExchangeError::Decode)?;
    let original_size_bytes =
        unsigned_u64(original_size_bytes).ok_or(UploadExchangeError::Decode)?;
    let compressed_size_bytes =
        unsigned_u64(compressed_size_bytes).ok_or(UploadExchangeError::Decode)?;
    if original_size_bytes > max_file_bytes || compressed_size_bytes > max_file_bytes {
        return Err(UploadExchangeError::InvalidSizes);
    }

    let sha256 = fixed::<CONTENT_HASH_LEN>(sha256)
        .map(ContentHash::from_bytes)
        .map_err(|_| UploadExchangeError::Decode)?;

    Ok(UploadRequestHeader {
        login,
        password,
        request: UploadRequest {
            source_name,
            timestamp_ms,
            original_size_bytes,
            compressed_size_bytes,
            sha256,
        },
    })
}

/// Encodes a four-position upload client logical header.
pub fn encode_upload_request(
    login: &str,
    password: &str,
    request: &UploadRequest,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<Vec<u8>> {
    if !valid_login(login) {
        return Err(UploadExchangeError::InvalidLogin);
    }
    if !valid_source_name(&request.source_name, max_source_name_len) {
        return Err(UploadExchangeError::InvalidSourceName);
    }
    if request.original_size_bytes > max_file_bytes
        || request.compressed_size_bytes > max_file_bytes
    {
        return Err(UploadExchangeError::InvalidSizes);
    }

    let arguments = Value::Array(vec![
        Value::Text(request.source_name.clone()),
        Value::Integer(request.timestamp_ms.into()),
        Value::Integer(request.original_size_bytes.into()),
        Value::Integer(request.compressed_size_bytes.into()),
        Value::Bytes(request.sha256.as_bytes().to_vec()),
    ]);
    let value = Value::Array(vec![
        Value::Integer(UPLOAD_OPERATION.into()),
        Value::Text(login.to_owned()),
        Value::Text(password.to_owned()),
        arguments,
    ]);

    encode_value(&value)
}

fn encode_value(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).map_err(|_| UploadExchangeError::Encode)?;
    Ok(bytes)
}

fn decode_value(bytes: &[u8]) -> Result<Value> {
    let mut slice = bytes;
    let value = ciborium::de::from_reader(&mut slice).map_err(|_| UploadExchangeError::Decode)?;
    if !slice.is_empty() {
        return Err(UploadExchangeError::Decode);
    }
    Ok(value)
}

fn integer_u32(value: Value) -> Option<u32> {
    let Value::Integer(number) = value else {
        return None;
    };
    u32::try_from(number).ok()
}

fn signed_i64(value: Value) -> Option<i64> {
    let Value::Integer(number) = value else {
        return None;
    };
    i64::try_from(number).ok()
}

fn unsigned_u64(value: Value) -> Option<u64> {
    let Value::Integer(number) = value else {
        return None;
    };
    u64::try_from(number).ok()
}

fn fixed<const N: usize>(value: Value) -> Result<[u8; N]> {
    let Value::Bytes(bytes) = value else {
        return Err(UploadExchangeError::Decode);
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| UploadExchangeError::Decode)
}

fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_source_name(source_name: &str, max_source_name_len: usize) -> bool {
    !source_name.is_empty() && source_name.len() <= max_source_name_len
}
