//! Inter-server blob-get logical messages.
//! SRS: `INT-NET-098 / 1`, `INT-NET-099 / 1`, `INT-NET-116 / 1`,
//! `INT-NET-119 / 1`, `INT-NET-128 / 1`.

use core::fmt;

use ciborium::value::Value;

use crate::blob_put::{
    canonical_bytes, decode_metadata, decode_value, encode_metadata_value, transfer_size_bound,
    validate_metadata, BlobMetadata, BlobPutError,
};
use crate::client_limits::ClientLimits;
use crate::message::MessageType;
use crate::metadata::{UploadId, UPLOAD_ID_LEN};

/// Result alias for blob-get logical message validation.
type Result<T> = core::result::Result<T, BlobGetError>;

/// Number of positions in a blob-get request argument array.
const REQUEST_ARGUMENTS_LEN: usize = 2;
/// Number of positions in a blob-get result argument array.
const RESULT_ARGUMENTS_LEN: usize = 2;
/// Number of positions in a successful blob-get result argument array.
const SUCCESS_ARGUMENTS_LEN: usize = 2;
/// Success result code.
const RESULT_CODE_SUCCESS: u32 = 0;

/// Blob get request carrying the upload identity to fetch.
#[derive(Clone, PartialEq, Eq)]
pub struct BlobGet {
    /// Login owning the upload.
    pub login: String,
    /// Identifier of the upload to fetch.
    pub upload_id: UploadId,
}

impl fmt::Debug for BlobGet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlobGet")
            .field("login_len", &self.login.len())
            .field("upload_id", &self.upload_id)
            .finish()
    }
}

/// Blob get result carrying the outcome code and the success payload description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobGetResult {
    /// Protocol result code.
    pub result_code: u32,
    /// Upload metadata on success, absent on error.
    pub metadata: Option<BlobMetadata>,
    /// Compressed transfer size on success, absent on error.
    pub transfer_compressed_size: Option<u64>,
}

/// Errors from encoding or decoding a blob-get logical message.
#[derive(Debug, thiserror::Error)]
pub enum BlobGetError {
    /// The blob-get value failed blob-put validation.
    #[error(transparent)]
    Blob(#[from] BlobPutError),
    /// The envelope or argument array has the wrong length or shape.
    #[error("the blob-get logical message has the wrong length")]
    WrongLength,
    /// The result code and result arguments do not form a valid combination.
    #[error("the result code/argument combination is invalid")]
    InvalidResult,
}

/// Encodes a blob-get request as the exact `[13, [login, upload_id]]` shape.
pub fn encode_blob_get(get: &BlobGet, limits: &ClientLimits) -> Result<Vec<u8>> {
    if !limits.login_allowed(&get.login) {
        return Err(BlobPutError::InvalidLogin.into());
    }

    let arguments = Value::Array(vec![
        Value::Text(get.login.clone()),
        Value::Bytes(get.upload_id.as_bytes().to_vec()),
    ]);
    encode_envelope(MessageType::BlobGet, arguments)
}

/// Decodes a blob-get request with the exact `[13, [login, upload_id]]` shape.
pub fn decode_blob_get(bytes: &[u8], limits: &ClientLimits) -> Result<BlobGet> {
    let (message_type, arguments) = decode_envelope(bytes)?;
    expect_message_type(message_type, MessageType::BlobGet)?;

    let Value::Array(arguments) = arguments else {
        return Err(BlobGetError::WrongLength);
    };
    if arguments.len() != REQUEST_ARGUMENTS_LEN {
        return Err(BlobGetError::WrongLength);
    }
    let [login, upload_id] = arguments
        .try_into()
        .map_err(|_| BlobGetError::WrongLength)?;

    let Value::Text(login) = login else {
        return Err(BlobPutError::InvalidLogin.into());
    };
    if !limits.login_allowed(&login) {
        return Err(BlobPutError::InvalidLogin.into());
    }

    Ok(BlobGet {
        login,
        upload_id: decode_upload_id(upload_id)?,
    })
}

/// Encodes a blob-get result as the exact `[14, [result_code, result_args]]` shape.
pub fn encode_blob_get_result(
    result: &BlobGetResult,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<Vec<u8>> {
    let result_args = if result.result_code == RESULT_CODE_SUCCESS {
        let (Some(metadata), Some(transfer_compressed_size)) =
            (result.metadata.as_ref(), result.transfer_compressed_size)
        else {
            return Err(BlobGetError::InvalidResult);
        };
        validate_metadata(metadata, max_source_name_len, max_file_bytes)?;
        validate_transfer_size(metadata.original_size_bytes, transfer_compressed_size)?;
        vec![
            encode_metadata_value(metadata),
            Value::Integer(transfer_compressed_size.into()),
        ]
    } else {
        if result.metadata.is_some() || result.transfer_compressed_size.is_some() {
            return Err(BlobGetError::InvalidResult);
        }
        Vec::new()
    };

    let arguments = Value::Array(vec![
        Value::Integer(result.result_code.into()),
        Value::Array(result_args),
    ]);
    encode_envelope(MessageType::BlobGetResult, arguments)
}

/// Decodes a blob-get result with the exact `[14, [result_code, result_args]]` shape.
pub fn decode_blob_get_result(
    bytes: &[u8],
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<BlobGetResult> {
    let (message_type, arguments) = decode_envelope(bytes)?;
    expect_message_type(message_type, MessageType::BlobGetResult)?;

    let Value::Array(arguments) = arguments else {
        return Err(BlobGetError::WrongLength);
    };
    if arguments.len() != RESULT_ARGUMENTS_LEN {
        return Err(BlobGetError::WrongLength);
    }
    let [result_code, result_args] = arguments
        .try_into()
        .map_err(|_| BlobGetError::WrongLength)?;
    let result_code = integer_u32(result_code).ok_or(BlobGetError::InvalidResult)?;

    let Value::Array(result_args) = result_args else {
        return Err(BlobGetError::InvalidResult);
    };
    if result_code == RESULT_CODE_SUCCESS {
        if result_args.len() != SUCCESS_ARGUMENTS_LEN {
            return Err(BlobGetError::InvalidResult);
        }
        let [metadata, transfer_compressed_size] = result_args
            .try_into()
            .map_err(|_| BlobGetError::InvalidResult)?;
        let metadata = decode_metadata(metadata, max_source_name_len, max_file_bytes)?;
        let transfer_compressed_size =
            unsigned_u64(transfer_compressed_size).ok_or(BlobGetError::InvalidResult)?;
        validate_transfer_size(metadata.original_size_bytes, transfer_compressed_size)?;

        Ok(BlobGetResult {
            result_code,
            metadata: Some(metadata),
            transfer_compressed_size: Some(transfer_compressed_size),
        })
    } else {
        if !result_args.is_empty() {
            return Err(BlobGetError::InvalidResult);
        }
        Ok(BlobGetResult {
            result_code,
            metadata: None,
            transfer_compressed_size: None,
        })
    }
}

fn encode_envelope(message_type: MessageType, arguments: Value) -> Result<Vec<u8>> {
    let envelope = Value::Array(vec![
        Value::Integer((message_type as u32).into()),
        arguments,
    ]);
    Ok(canonical_bytes(&envelope)?)
}

fn decode_envelope(bytes: &[u8]) -> Result<(Value, Value)> {
    let Value::Array(elements) = decode_value(bytes)? else {
        return Err(BlobGetError::WrongLength);
    };
    let [message_type, arguments] = elements.try_into().map_err(|_| BlobGetError::WrongLength)?;
    Ok((message_type, arguments))
}

fn expect_message_type(value: Value, expected: MessageType) -> Result<()> {
    let code = integer_u32(value).ok_or(BlobPutError::WrongMessageType)?;
    let message_type = MessageType::try_from(code).map_err(|_| BlobPutError::WrongMessageType)?;
    if message_type == expected {
        Ok(())
    } else {
        Err(BlobPutError::WrongMessageType.into())
    }
}

fn decode_upload_id(value: Value) -> Result<UploadId> {
    let Value::Bytes(bytes) = value else {
        return Err(BlobPutError::InvalidUploadId.into());
    };
    let bytes: [u8; UPLOAD_ID_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| BlobPutError::InvalidUploadId)?;
    Ok(UploadId::from_bytes(bytes))
}

fn validate_transfer_size(original_size_bytes: u64, transfer_compressed_size: u64) -> Result<()> {
    let bound =
        transfer_size_bound(original_size_bytes).ok_or(BlobPutError::InvalidTransferSize)?;
    if (1..=bound).contains(&transfer_compressed_size) {
        Ok(())
    } else {
        Err(BlobPutError::InvalidTransferSize.into())
    }
}

fn integer_u32(value: Value) -> Option<u32> {
    let Value::Integer(number) = value else {
        return None;
    };
    u32::try_from(number).ok()
}

fn unsigned_u64(value: Value) -> Option<u64> {
    let Value::Integer(number) = value else {
        return None;
    };
    u64::try_from(number).ok()
}
