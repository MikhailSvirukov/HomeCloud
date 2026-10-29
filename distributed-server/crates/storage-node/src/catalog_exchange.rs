//! Catalog page client exchange codecs.
//!
//! SRS: `CFG-CL-029 / 1`, `CFG-CL-031 / 1`, `INT-NET-050 / 4`,
//! `INT-NET-051 / 3`, `INT-NET-054 / 1`, `INT-NET-061 / 1`, `INT-NET-069 / 1`,
//! `INT-NET-080 / 1`, `FR-CAT-013 / 2`, `FR-CAT-015 / 1`.

use ciborium::value::Value;

use crate::catalog::{decode_request, encode_page, CatalogError, CatalogPage, CatalogRequest};
use crate::client_limits::ClientLimits;
use crate::metadata::UPLOAD_ID_LEN;

/// External protocol version supported by the catalog exchange codecs.
pub const EXTERNAL_PROTOCOL_VERSION: u32 = 1;
/// Length of a sender identifier in bytes.
pub const SENDER_ID_LEN: usize = 16;
/// Length of a request identifier in bytes.
pub const REQUEST_ID_LEN: usize = 16;
/// Length of the bootstrap Ed25519 signing public key in bytes.
pub const BOOTSTRAP_KEY_LEN: usize = 32;

/// Client operation code of a catalog page request.
const CATALOG_OPERATION: u32 = 3;
/// Number of positions in an external header.
const EXTERNAL_HEADER_LEN: usize = 7;
/// Number of leading positions in a client logical header.
const CLIENT_LOGICAL_HEADER_LEN: usize = 4;

/// Result alias for catalog exchange validation.
type Result<T> = core::result::Result<T, CatalogExchangeError>;

/// Seven-position external header of a protocol frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalHeader {
    /// Protocol version, which must be [`EXTERNAL_PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// Assigned sender identifier, or `None` for primary registration.
    pub sender_id: Option<[u8; SENDER_ID_LEN]>,
    /// Identifier correlating a request and its response.
    pub request_id: [u8; REQUEST_ID_LEN],
    /// Zero-based frame number within a sequence.
    pub frame_number: u32,
    /// Upload identifier when the message belongs to an upload, otherwise `None`.
    pub upload_id: Option<[u8; UPLOAD_ID_LEN]>,
    /// Label of the responding node when it applies to the message.
    pub responder_label: Option<String>,
    /// Bootstrap Ed25519 signing public key for primary registration only.
    pub bootstrap_signing_public_key: Option<[u8; BOOTSTRAP_KEY_LEN]>,
}

/// Decoded catalog page request with its client credentials.
pub struct CatalogPageRequest {
    login: String,
    password: String,
    request: CatalogRequest,
}

impl CatalogPageRequest {
    /// Returns the client login exactly as received.
    pub fn login(&self) -> &str {
        &self.login
    }

    /// Returns the client password exactly as received.
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Returns the catalog page arguments.
    pub fn request(&self) -> &CatalogRequest {
        &self.request
    }
}

/// Errors from encoding or decoding catalog exchange values.
#[derive(Debug, thiserror::Error)]
pub enum CatalogExchangeError {
    /// The exchange value is not a CBOR array.
    #[error("the exchange value is not a CBOR array")]
    NotAnArray,
    /// The exchange array does not contain the positions required by its schema.
    #[error("the exchange array has the wrong length")]
    WrongLength,
    /// The external protocol version is not the supported version.
    #[error("the external protocol version is not 1")]
    UnsupportedVersion,
    /// The operation type is not a catalog page request.
    #[error("the client operation is not a catalog page request")]
    InvalidOperation,
    /// The login is outside the configured byte range or contains a byte outside the permitted ASCII set.
    #[error("the client login is not a valid ASCII login")]
    InvalidLogin,
    /// The password is outside the configured UTF-8 byte range.
    #[error("the client password is outside the configured length range")]
    InvalidPassword,
    /// A header field violates its type, length, or null rule.
    #[error("the external header null rules are violated")]
    InvalidHeader,
    /// The responder label exceeds the configured UTF-8 byte limit.
    #[error("the responder label exceeds the configured byte limit")]
    InvalidResponderLabel,
    /// The exchange value could not be decoded, or trailing bytes followed it.
    #[error("the exchange value could not be decoded")]
    Decode,
    /// The exchange value could not be encoded.
    #[error("the exchange value could not be encoded")]
    Encode,
    /// The catalog page or request arguments are invalid.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}

/// Encodes a seven-position external header as CBOR, enforcing the configured responder-label limit.
pub fn encode_external_header(header: &ExternalHeader, limits: &ClientLimits) -> Result<Vec<u8>> {
    if header.protocol_version != EXTERNAL_PROTOCOL_VERSION {
        return Err(CatalogExchangeError::UnsupportedVersion);
    }
    if header.sender_id.is_some() == header.bootstrap_signing_public_key.is_some() {
        return Err(CatalogExchangeError::InvalidHeader);
    }
    if header
        .responder_label
        .as_deref()
        .is_some_and(|label| !limits.responder_label_allowed(label))
    {
        return Err(CatalogExchangeError::InvalidResponderLabel);
    }

    let value = Value::Array(vec![
        Value::Integer(header.protocol_version.into()),
        optional_bytes(header.sender_id.as_ref()),
        Value::Bytes(header.request_id.to_vec()),
        Value::Integer(header.frame_number.into()),
        optional_bytes(header.upload_id.as_ref()),
        header
            .responder_label
            .as_ref()
            .map_or(Value::Null, |label| Value::Text(label.clone())),
        optional_bytes(header.bootstrap_signing_public_key.as_ref()),
    ]);

    encode_value(&value)
}

/// Decodes a seven-position external header, rejects trailing bytes, and enforces the configured responder-label limit.
pub fn decode_external_header(bytes: &[u8], limits: &ClientLimits) -> Result<ExternalHeader> {
    let Value::Array(elements) = decode_value(bytes)? else {
        return Err(CatalogExchangeError::NotAnArray);
    };
    if elements.len() != EXTERNAL_HEADER_LEN {
        return Err(CatalogExchangeError::WrongLength);
    }
    let [protocol_version, sender_id, request_id, frame_number, upload_id, responder_label, bootstrap_signing_public_key] =
        elements
            .try_into()
            .map_err(|_| CatalogExchangeError::WrongLength)?;

    let protocol_version =
        integer_u32(protocol_version).ok_or(CatalogExchangeError::UnsupportedVersion)?;
    if protocol_version != EXTERNAL_PROTOCOL_VERSION {
        return Err(CatalogExchangeError::UnsupportedVersion);
    }

    let sender_id = optional_fixed::<SENDER_ID_LEN>(sender_id)?;
    let request_id = fixed::<REQUEST_ID_LEN>(request_id)?;
    let frame_number = integer_u32(frame_number).ok_or(CatalogExchangeError::Decode)?;
    let upload_id = optional_fixed::<UPLOAD_ID_LEN>(upload_id)?;
    let responder_label = match responder_label {
        Value::Null => None,
        Value::Text(label) => Some(label),
        _ => return Err(CatalogExchangeError::InvalidHeader),
    };
    if responder_label
        .as_deref()
        .is_some_and(|label| !limits.responder_label_allowed(label))
    {
        return Err(CatalogExchangeError::InvalidResponderLabel);
    }
    let bootstrap_signing_public_key =
        optional_fixed::<BOOTSTRAP_KEY_LEN>(bootstrap_signing_public_key)?;

    if sender_id.is_some() == bootstrap_signing_public_key.is_some() {
        return Err(CatalogExchangeError::InvalidHeader);
    }

    Ok(ExternalHeader {
        protocol_version,
        sender_id,
        request_id,
        frame_number,
        upload_id,
        responder_label,
        bootstrap_signing_public_key,
    })
}

/// Decodes a `[3, login, password, [snapshot_token, cursor]]` catalog page request within the configured credential limits.
pub fn decode_catalog_page_request(
    logical_header: &[u8],
    limits: &ClientLimits,
) -> Result<CatalogPageRequest> {
    let Value::Array(elements) = decode_value(logical_header)? else {
        return Err(CatalogExchangeError::NotAnArray);
    };
    if elements.len() != CLIENT_LOGICAL_HEADER_LEN {
        return Err(CatalogExchangeError::WrongLength);
    }
    let [operation, login, password, arguments] = elements
        .try_into()
        .map_err(|_| CatalogExchangeError::WrongLength)?;

    if integer_u32(operation) != Some(CATALOG_OPERATION) {
        return Err(CatalogExchangeError::InvalidOperation);
    }

    let Value::Text(login) = login else {
        return Err(CatalogExchangeError::InvalidLogin);
    };
    if !limits.login_allowed(&login) {
        return Err(CatalogExchangeError::InvalidLogin);
    }

    let Value::Text(password) = password else {
        return Err(CatalogExchangeError::Decode);
    };
    if !limits.password_allowed(&password) {
        return Err(CatalogExchangeError::InvalidPassword);
    }

    let mut encoded_arguments = Vec::new();
    ciborium::ser::into_writer(&arguments, &mut encoded_arguments)
        .map_err(|_| CatalogExchangeError::Encode)?;
    let request = decode_request(&encoded_arguments)?;

    Ok(CatalogPageRequest {
        login,
        password,
        request,
    })
}

/// Encodes a catalog page client logical header as CBOR within the configured credential limits.
pub fn encode_catalog_page_request(
    login: &str,
    password: &str,
    request: &CatalogRequest,
    limits: &ClientLimits,
) -> Result<Vec<u8>> {
    if !limits.login_allowed(login) {
        return Err(CatalogExchangeError::InvalidLogin);
    }
    if !limits.password_allowed(password) {
        return Err(CatalogExchangeError::InvalidPassword);
    }

    let arguments = match (request.snapshot_token, request.cursor) {
        (None, None) => Value::Array(vec![Value::Null, Value::Null]),
        (Some(snapshot_token), Some(cursor)) => Value::Array(vec![
            Value::Bytes(snapshot_token.to_vec()),
            Value::Bytes(cursor.to_vec()),
        ]),
        _ => return Err(CatalogExchangeError::Encode),
    };

    let value = Value::Array(vec![
        Value::Integer(CATALOG_OPERATION.into()),
        Value::Text(login.to_owned()),
        Value::Text(password.to_owned()),
        arguments,
    ]);

    encode_value(&value)
}

/// Encodes a `[result_code, result_args_array]` catalog page response as CBOR.
pub fn encode_catalog_page_response(
    result_code: u32,
    page: Option<&CatalogPage>,
) -> Result<Vec<u8>> {
    let (result_code, arguments) = match (result_code, page) {
        (0, Some(page)) => {
            let page_bytes = encode_page(page)?;
            (0, decode_value(&page_bytes)?)
        }
        (0, None) | (_, Some(_)) => return Err(CatalogExchangeError::Encode),
        (result_code, None) => (result_code, Value::Array(Vec::new())),
    };

    let value = Value::Array(vec![Value::Integer(result_code.into()), arguments]);
    encode_value(&value)
}

fn encode_value(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).map_err(|_| CatalogExchangeError::Encode)?;
    Ok(bytes)
}

fn decode_value(bytes: &[u8]) -> Result<Value> {
    let mut slice = bytes;
    let value = ciborium::de::from_reader(&mut slice).map_err(|_| CatalogExchangeError::Decode)?;
    if !slice.is_empty() {
        return Err(CatalogExchangeError::Decode);
    }
    Ok(value)
}

fn integer_u32(value: Value) -> Option<u32> {
    let Value::Integer(number) = value else {
        return None;
    };
    u32::try_from(number).ok()
}

fn fixed<const N: usize>(value: Value) -> Result<[u8; N]> {
    let Value::Bytes(bytes) = value else {
        return Err(CatalogExchangeError::InvalidHeader);
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| CatalogExchangeError::InvalidHeader)
}

fn optional_fixed<const N: usize>(value: Value) -> Result<Option<[u8; N]>> {
    match value {
        Value::Null => Ok(None),
        value => fixed(value).map(Some),
    }
}

fn optional_bytes<const N: usize>(bytes: Option<&[u8; N]>) -> Value {
    bytes.map_or(Value::Null, |bytes| Value::Bytes(bytes.to_vec()))
}
