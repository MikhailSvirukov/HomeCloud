//! Inter-server blob-put logical messages.
//! SRS: `INT-NET-095 / 1`, `INT-NET-096 / 1`, `INT-NET-097 / 1`,
//! `INT-NET-114 / 1`, `INT-NET-119 / 1`, `INT-NET-128 / 1`.

use std::io::Cursor;

use ciborium::value::Value;

use crate::client_limits::ClientLimits;
use crate::message::MessageType;
use crate::metadata::{ContentHash, UploadId, CONTENT_HASH_LEN, UPLOAD_ID_LEN};

/// Result alias for blob-put logical message validation.
type Result<T> = core::result::Result<T, BlobPutError>;

/// Number of positions in a blob-put begin argument array.
const BEGIN_ARGUMENTS_LEN: usize = 4;
/// Number of positions in a blob metadata array.
const METADATA_FIELDS_LEN: usize = 5;
/// Number of positions in a blob-put data argument array.
const DATA_ARGUMENTS_LEN: usize = 1;
/// Number of positions in a blob-put result argument array.
const RESULT_ARGUMENTS_LEN: usize = 2;
/// Success result code.
const RESULT_CODE_SUCCESS: u32 = 0;

/// Metadata describing the original upload being replicated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMetadata {
    /// Source media file name.
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

/// Blob put begin message carrying the upload identity and transfer size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobPutBegin {
    /// Login owning the upload.
    pub login: String,
    /// Upload identifier.
    pub upload_id: UploadId,
    /// Metadata of the original upload.
    pub metadata: BlobMetadata,
    /// Compressed size of the current transfer stream.
    pub transfer_compressed_size: u64,
}

/// Blob put data header carrying the compressed payload length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobPutData {
    /// Number of compressed bytes following the CBOR header.
    pub payload_length: u32,
}

/// Blob put result carrying the outcome code and optional upload identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobPutResult {
    /// Protocol result code.
    pub result_code: u32,
    /// Upload identifier on success, absent on error.
    pub upload_id: Option<UploadId>,
}

/// Errors from encoding or decoding a blob-put logical message.
#[derive(Debug, thiserror::Error)]
pub enum BlobPutError {
    /// The decoded CBOR value is not an array.
    #[error("the blob-put value is not a CBOR array")]
    NotAnArray,
    /// The message type is not the expected blob-put type.
    #[error("the blob-put message has the wrong type")]
    WrongMessageType,
    /// The message envelope, arguments, or metadata array has the wrong length.
    #[error("the blob-put logical message has the wrong length")]
    WrongLength,
    /// The login is outside the configured byte range or uses a non-ASCII byte.
    #[error("the login is invalid")]
    InvalidLogin,
    /// The source name is empty or exceeds the configured byte limit.
    #[error("the source name is invalid")]
    InvalidSourceName,
    /// An original or compressed size exceeds the configured limit.
    #[error("a metadata size exceeds the configured limit")]
    InvalidSizes,
    /// The transfer compressed size is outside `1..=B(original_size)` or overflows.
    #[error("the transfer compressed size is invalid")]
    InvalidTransferSize,
    /// The upload identifier is not exactly 16 bytes.
    #[error("the upload id is invalid")]
    InvalidUploadId,
    /// The SHA-256 value is not exactly 32 bytes.
    #[error("the SHA-256 value is invalid")]
    InvalidHash,
    /// The payload length is zero or exceeds the supported range.
    #[error("the payload length is zero or does not fit")]
    InvalidPayloadLength,
    /// The data frame payload length does not match the bytes after the header.
    #[error("the data frame payload length does not match")]
    PayloadMismatch,
    /// The result code and result arguments do not form a valid combination.
    #[error("the result code/page combination is invalid")]
    InvalidResult,
    /// The blob-put value could not be decoded, or trailing bytes followed it.
    #[error("the blob-put value could not be decoded")]
    Decode,
    /// The blob-put value could not be encoded.
    #[error("the blob-put value could not be encoded")]
    Encode,
}

/// Encodes a blob-put begin message with the exact `[10, [login, upload_id, metadata, transfer_compressed_size]]` shape.
pub fn encode_blob_put_begin(
    begin: &BlobPutBegin,
    limits: &ClientLimits,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<Vec<u8>> {
    if !limits.login_allowed(&begin.login) {
        return Err(BlobPutError::InvalidLogin);
    }
    validate_metadata(&begin.metadata, max_source_name_len, max_file_bytes)?;
    validate_transfer_size(
        begin.metadata.original_size_bytes,
        begin.transfer_compressed_size,
    )?;

    let arguments = Value::Array(vec![
        Value::Text(begin.login.clone()),
        Value::Bytes(begin.upload_id.as_bytes().to_vec()),
        encode_metadata_value(&begin.metadata),
        Value::Integer(begin.transfer_compressed_size.into()),
    ]);
    encode_envelope(MessageType::BlobPutBegin, arguments)
}

/// Decodes a blob-put begin message with the exact `[10, [login, upload_id, metadata, transfer_compressed_size]]` shape.
pub fn decode_blob_put_begin(
    bytes: &[u8],
    limits: &ClientLimits,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<BlobPutBegin> {
    let Value::Array(elements) = decode_value(bytes)? else {
        return Err(BlobPutError::NotAnArray);
    };
    let [message_type, arguments] = elements.try_into().map_err(|_| BlobPutError::WrongLength)?;
    expect_message_type(message_type, MessageType::BlobPutBegin)?;

    let Value::Array(arguments) = arguments else {
        return Err(BlobPutError::WrongLength);
    };
    if arguments.len() != BEGIN_ARGUMENTS_LEN {
        return Err(BlobPutError::WrongLength);
    }
    let [login, upload_id, metadata, transfer_compressed_size] = arguments
        .try_into()
        .map_err(|_| BlobPutError::WrongLength)?;

    let Value::Text(login) = login else {
        return Err(BlobPutError::InvalidLogin);
    };
    if !limits.login_allowed(&login) {
        return Err(BlobPutError::InvalidLogin);
    }

    let upload_id = fixed::<UPLOAD_ID_LEN>(upload_id)
        .map(UploadId::from_bytes)
        .ok_or(BlobPutError::InvalidUploadId)?;
    let metadata = decode_metadata(metadata, max_source_name_len, max_file_bytes)?;
    let transfer_compressed_size =
        unsigned_u64(transfer_compressed_size).ok_or(BlobPutError::InvalidTransferSize)?;
    validate_transfer_size(metadata.original_size_bytes, transfer_compressed_size)?;

    Ok(BlobPutBegin {
        login,
        upload_id,
        metadata,
        transfer_compressed_size,
    })
}

/// Encodes a blob-put data frame as `[11, [payload_length]]` followed immediately by the compressed payload.
pub fn encode_blob_put_data_frame(payload: &[u8]) -> Result<Vec<u8>> {
    let payload_length =
        u32::try_from(payload.len()).map_err(|_| BlobPutError::InvalidPayloadLength)?;
    if payload_length == 0 {
        return Err(BlobPutError::InvalidPayloadLength);
    }

    let header = encode_envelope(
        MessageType::BlobPutData,
        Value::Array(vec![Value::Integer(payload_length.into())]),
    )?;
    let mut bytes = Vec::with_capacity(header.len() + payload.len());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

/// Decodes a blob-put data frame and returns its header and the compressed payload bytes.
pub fn decode_blob_put_data_frame(bytes: &[u8]) -> Result<(BlobPutData, &[u8])> {
    let mut cursor = Cursor::new(bytes);
    let value: Value = ciborium::de::from_reader(&mut cursor).map_err(|_| BlobPutError::Decode)?;
    let consumed = usize::try_from(cursor.position()).map_err(|_| BlobPutError::Decode)?;
    let payload = bytes.get(consumed..).ok_or(BlobPutError::Decode)?;

    let Value::Array(elements) = value else {
        return Err(BlobPutError::NotAnArray);
    };
    let [message_type, arguments] = elements.try_into().map_err(|_| BlobPutError::WrongLength)?;
    expect_message_type(message_type, MessageType::BlobPutData)?;

    let Value::Array(arguments) = arguments else {
        return Err(BlobPutError::WrongLength);
    };
    if arguments.len() != DATA_ARGUMENTS_LEN {
        return Err(BlobPutError::WrongLength);
    }
    let [payload_length] = arguments
        .try_into()
        .map_err(|_| BlobPutError::WrongLength)?;
    let payload_length = integer_u32(payload_length).ok_or(BlobPutError::InvalidPayloadLength)?;
    if payload_length == 0 {
        return Err(BlobPutError::InvalidPayloadLength);
    }
    if payload.len() != usize::try_from(payload_length).map_err(|_| BlobPutError::Decode)? {
        return Err(BlobPutError::PayloadMismatch);
    }

    Ok((BlobPutData { payload_length }, payload))
}

/// Encodes a blob-put result as the exact `[12, [result_code, result_args]]` shape.
pub fn encode_blob_put_result(result: &BlobPutResult) -> Result<Vec<u8>> {
    let result_args = match (result.result_code, result.upload_id) {
        (RESULT_CODE_SUCCESS, Some(upload_id)) => {
            vec![Value::Bytes(upload_id.as_bytes().to_vec())]
        }
        (RESULT_CODE_SUCCESS, None) => return Err(BlobPutError::InvalidResult),
        (_, Some(_)) => return Err(BlobPutError::InvalidResult),
        (_, None) => Vec::new(),
    };

    let arguments = Value::Array(vec![
        Value::Integer(result.result_code.into()),
        Value::Array(result_args),
    ]);
    encode_envelope(MessageType::BlobPutResult, arguments)
}

/// Decodes a blob-put result with the exact `[12, [result_code, result_args]]` shape.
pub fn decode_blob_put_result(bytes: &[u8]) -> Result<BlobPutResult> {
    let Value::Array(elements) = decode_value(bytes)? else {
        return Err(BlobPutError::NotAnArray);
    };
    let [message_type, arguments] = elements.try_into().map_err(|_| BlobPutError::WrongLength)?;
    expect_message_type(message_type, MessageType::BlobPutResult)?;

    let Value::Array(arguments) = arguments else {
        return Err(BlobPutError::WrongLength);
    };
    if arguments.len() != RESULT_ARGUMENTS_LEN {
        return Err(BlobPutError::WrongLength);
    }
    let [result_code, result_args] = arguments
        .try_into()
        .map_err(|_| BlobPutError::WrongLength)?;
    let result_code = integer_u32(result_code).ok_or(BlobPutError::InvalidResult)?;
    let Value::Array(result_args) = result_args else {
        return Err(BlobPutError::InvalidResult);
    };

    if result_code == RESULT_CODE_SUCCESS {
        let [upload_id] = result_args
            .try_into()
            .map_err(|_| BlobPutError::InvalidResult)?;
        let upload_id = fixed::<UPLOAD_ID_LEN>(upload_id)
            .map(UploadId::from_bytes)
            .ok_or(BlobPutError::InvalidResult)?;
        Ok(BlobPutResult {
            result_code,
            upload_id: Some(upload_id),
        })
    } else {
        if !result_args.is_empty() {
            return Err(BlobPutError::InvalidResult);
        }
        Ok(BlobPutResult {
            result_code,
            upload_id: None,
        })
    }
}

fn validate_metadata(
    metadata: &BlobMetadata,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<()> {
    if !valid_source_name(&metadata.source_name, max_source_name_len) {
        return Err(BlobPutError::InvalidSourceName);
    }
    if metadata.original_size_bytes > max_file_bytes
        || metadata.compressed_size_bytes > max_file_bytes
    {
        return Err(BlobPutError::InvalidSizes);
    }
    Ok(())
}

fn decode_metadata(
    value: Value,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<BlobMetadata> {
    let Value::Array(fields) = value else {
        return Err(BlobPutError::WrongLength);
    };
    if fields.len() != METADATA_FIELDS_LEN {
        return Err(BlobPutError::WrongLength);
    }
    let [source_name, timestamp_ms, original_size_bytes, compressed_size_bytes, sha256] =
        fields.try_into().map_err(|_| BlobPutError::WrongLength)?;

    let Value::Text(source_name) = source_name else {
        return Err(BlobPutError::InvalidSourceName);
    };
    if !valid_source_name(&source_name, max_source_name_len) {
        return Err(BlobPutError::InvalidSourceName);
    }
    let timestamp_ms = signed_i64(timestamp_ms).ok_or(BlobPutError::Decode)?;
    let original_size_bytes =
        unsigned_u64(original_size_bytes).ok_or(BlobPutError::InvalidSizes)?;
    let compressed_size_bytes =
        unsigned_u64(compressed_size_bytes).ok_or(BlobPutError::InvalidSizes)?;
    if original_size_bytes > max_file_bytes || compressed_size_bytes > max_file_bytes {
        return Err(BlobPutError::InvalidSizes);
    }
    let sha256 = fixed::<CONTENT_HASH_LEN>(sha256)
        .map(ContentHash::from_bytes)
        .ok_or(BlobPutError::InvalidHash)?;

    Ok(BlobMetadata {
        source_name,
        timestamp_ms,
        original_size_bytes,
        compressed_size_bytes,
        sha256,
    })
}

fn encode_metadata_value(metadata: &BlobMetadata) -> Value {
    Value::Array(vec![
        Value::Text(metadata.source_name.clone()),
        Value::Integer(metadata.timestamp_ms.into()),
        Value::Integer(metadata.original_size_bytes.into()),
        Value::Integer(metadata.compressed_size_bytes.into()),
        Value::Bytes(metadata.sha256.as_bytes().to_vec()),
    ])
}

fn encode_envelope(message_type: MessageType, arguments: Value) -> Result<Vec<u8>> {
    let envelope = Value::Array(vec![
        Value::Integer((message_type as u32).into()),
        arguments,
    ]);
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&envelope, &mut bytes).map_err(|_| BlobPutError::Encode)?;
    Ok(bytes)
}

fn decode_value(bytes: &[u8]) -> Result<Value> {
    let mut slice = bytes;
    let value = ciborium::de::from_reader(&mut slice).map_err(|_| BlobPutError::Decode)?;
    if !slice.is_empty() {
        return Err(BlobPutError::Decode);
    }
    Ok(value)
}

fn expect_message_type(value: Value, expected: MessageType) -> Result<()> {
    let code = integer_u32(value).ok_or(BlobPutError::WrongMessageType)?;
    let message_type = MessageType::try_from(code).map_err(|_| BlobPutError::WrongMessageType)?;
    if message_type == expected {
        Ok(())
    } else {
        Err(BlobPutError::WrongMessageType)
    }
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

fn fixed<const N: usize>(value: Value) -> Option<[u8; N]> {
    let Value::Bytes(bytes) = value else {
        return None;
    };
    bytes.as_slice().try_into().ok()
}

fn valid_source_name(source_name: &str, max_source_name_len: usize) -> bool {
    !source_name.is_empty() && source_name.len() <= max_source_name_len
}

fn validate_transfer_size(original_size_bytes: u64, transfer_compressed_size: u64) -> Result<()> {
    let bound =
        transfer_size_bound(original_size_bytes).ok_or(BlobPutError::InvalidTransferSize)?;
    if (1..=bound).contains(&transfer_compressed_size) {
        Ok(())
    } else {
        Err(BlobPutError::InvalidTransferSize)
    }
}

fn transfer_size_bound(original_size_bytes: u64) -> Option<u64> {
    original_size_bytes
        .checked_add(original_size_bytes / 256)?
        .checked_add(64)
}
