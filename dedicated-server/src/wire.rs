//! Positional CBOR structures carried inside a frame.
//!
//! The protocol fixes every header as a positional CBOR array with explicit
//! types, lengths and null rules (`INT-NET-047`, `INT-NET-050`). This module
//! encodes and decodes the three client-side structures the server needs: the
//! external header (`INT-NET-050 / 4`), the client logical header
//! (`INT-NET-051 / 3`) and the result envelope (`INT-NET-061 / 1`).
//!
//! Encoding uses `ciborium`, which writes definite-length arrays, text strings
//! and minimal integers, so the output is the deterministic minimal
//! representation the requirements demand (`INT-NET-006 / 2`,
//! `INT-NET-055 / 2`). Decoding is strict: a wrong element count, a wrong CBOR
//! type, an unknown operation code, a bad byte-string length, a broken null
//! rule or trailing bytes are rejected (`INT-NET-038 / 2`).

use std::fmt;
use std::num::NonZeroU32;

use ciborium::Value;
use thiserror::Error;

/// Protocol version this codec implements.
pub const PROTOCOL_VERSION: u32 = 1;

/// Number of elements in the external header.
pub const EXTERNAL_HEADER_ELEMENTS: usize = 7;

/// Number of elements in a client logical header; later positions are undefined.
pub const CLIENT_HEADER_ELEMENTS: usize = 4;

/// Length of a sender or request identifier.
pub const IDENTIFIER_BYTES: usize = 16;

/// Length of a public key or SHA-256 digest.
pub const KEY_BYTES: usize = 32;

/// Number of elements in a result envelope; later positions are undefined.
pub const RESULT_ENVELOPE_ELEMENTS: usize = 2;

/// Header name used in diagnostics for the external header.
const EXTERNAL_HEADER: &str = "external header";

/// Header name used in diagnostics for the client logical header.
const CLIENT_HEADER: &str = "client logical header";

/// Header name used in diagnostics for the result envelope.
const RESULT_ENVELOPE: &str = "result envelope";

/// Errors produced while encoding or decoding a wire structure.
#[derive(Debug, Error)]
pub enum WireError {
    /// The decoded value is not a CBOR array.
    #[error("{0} must be a CBOR array")]
    NotAnArray(&'static str),
    /// The array has the wrong number of elements.
    #[error("{header} must have {expected} elements, got {actual}")]
    WrongLength {
        /// Header whose length is fixed.
        header: &'static str,
        /// Required element count.
        expected: usize,
        /// Observed element count.
        actual: usize,
    },
    /// An element has a different CBOR type than the schema fixes.
    #[error("{header} element {position} has the wrong CBOR type")]
    WrongType {
        /// Header being decoded.
        header: &'static str,
        /// One-based element position.
        position: usize,
    },
    /// A byte string has a different length than the schema fixes.
    #[error("{header} element {position} must be {expected} bytes long")]
    ByteStringLength {
        /// Header being decoded.
        header: &'static str,
        /// One-based element position.
        position: usize,
        /// Required byte length.
        expected: usize,
    },
    /// An integer does not fit the width the schema fixes.
    #[error("{header} element {position} is outside the range the schema allows")]
    IntegerOutOfRange {
        /// Header being decoded.
        header: &'static str,
        /// One-based element position.
        position: usize,
    },
    /// A label is longer than the configured limit.
    #[error("label is {actual} bytes, above the configured limit of {max}")]
    LabelTooLong {
        /// Observed label length in UTF-8 bytes.
        actual: usize,
        /// Configured maximum.
        max: usize,
    },
    /// The protocol version is not the one this codec implements.
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u64),
    /// The operation code is not one of the four v1 operations.
    #[error("unknown client operation code {0}")]
    UnknownOperation(u32),
    /// A header carries both or neither of the two mutually exclusive identities.
    #[error(
        "external header must carry exactly one of sender_id and bootstrap_signing_public_key"
    )]
    IdentityRule,
    /// A nonzero result code carries arguments.
    #[error("a nonzero result code carries an empty argument array")]
    FailureArguments,
    /// A catalog page carries only one of the two page tokens.
    #[error("client logical header must carry both snapshot_token and cursor, or neither")]
    CatalogTokenPair,
    /// An operation argument array has the wrong number of elements.
    #[error("operation {operation:?} takes {expected} arguments, got {actual}")]
    WrongArgumentCount {
        /// Operation whose schema is fixed.
        operation: ClientOperation,
        /// Required argument count.
        expected: usize,
        /// Observed argument count.
        actual: usize,
    },
    /// An operation argument has a different CBOR type than the schema fixes.
    #[error("operation {operation:?} argument {index} has the wrong CBOR type")]
    WrongArgumentType {
        /// Operation whose schema is fixed.
        operation: ClientOperation,
        /// Zero-based argument index.
        index: usize,
    },
    /// Bytes remain after the CBOR value.
    #[error("unexpected trailing bytes after the CBOR value")]
    TrailingBytes,
    /// Encoding failed.
    #[error("CBOR encoding failed: {0}")]
    Encode(String),
    /// Decoding failed.
    #[error("CBOR decoding failed: {0}")]
    Decode(String),
}

/// Result alias for this module.
type Result<T> = core::result::Result<T, WireError>;

/// External header of every message (`INT-NET-050 / 4`).
///
/// A sender either carries an assigned `sender_id` or, for primary
/// registration only, a `bootstrap_signing_public_key`; exactly one of them is
/// present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalHeader {
    /// Identifier of the sending installation, `null` for primary registration.
    pub sender_id: Option<[u8; IDENTIFIER_BYTES]>,
    /// Identifier of the request this message belongs to.
    pub request_id: [u8; IDENTIFIER_BYTES],
    /// Sequence number of the frame inside the request.
    pub frame_number: u32,
    /// Upload this frame belongs to, when applicable.
    pub upload_id: Option<[u8; IDENTIFIER_BYTES]>,
    /// Label of the responding node, when applicable.
    pub responder_label: Option<String>,
    /// Bootstrap Ed25519 key of the installation, for primary registration only.
    pub bootstrap_signing_public_key: Option<[u8; KEY_BYTES]>,
}

impl ExternalHeader {
    /// Encode the header as the deterministic positional CBOR array.
    pub fn encode(&self, max_label_bytes: usize) -> Result<Vec<u8>> {
        self.validate(max_label_bytes)?;

        let value = Value::Array(vec![
            Value::Integer(u64::from(PROTOCOL_VERSION).into()),
            optional_bytes(self.sender_id),
            Value::Bytes(self.request_id.to_vec()),
            Value::Integer(u64::from(self.frame_number).into()),
            optional_bytes(self.upload_id),
            optional_text(self.responder_label.as_deref()),
            optional_bytes(self.bootstrap_signing_public_key),
        ]);

        encode_value(&value)
    }

    /// Decode and validate an external header.
    pub fn decode(bytes: &[u8], max_label_bytes: usize) -> Result<Self> {
        let Value::Array(elements) = decode_value(bytes)? else {
            return Err(WireError::NotAnArray(EXTERNAL_HEADER));
        };
        require_length(&elements, EXTERNAL_HEADER, EXTERNAL_HEADER_ELEMENTS)?;

        let version = uint(&elements[0], EXTERNAL_HEADER, 1)?;
        if version != u64::from(PROTOCOL_VERSION) {
            return Err(WireError::UnsupportedVersion(version));
        }

        let frame_number = uint32(&elements[3], EXTERNAL_HEADER, 4)?;

        let header = Self {
            sender_id: optional_identifier(&elements[1], EXTERNAL_HEADER, 2)?,
            request_id: identifier(&elements[2], EXTERNAL_HEADER, 3)?,
            frame_number,
            upload_id: optional_identifier(&elements[4], EXTERNAL_HEADER, 5)?,
            responder_label: optional_text_value(&elements[5], EXTERNAL_HEADER, 6)?,
            bootstrap_signing_public_key: optional_key(&elements[6], EXTERNAL_HEADER, 7)?,
        };
        header.validate(max_label_bytes)?;

        Ok(header)
    }

    fn validate(&self, max_label_bytes: usize) -> Result<()> {
        match (&self.sender_id, &self.bootstrap_signing_public_key) {
            (Some(_), None) | (None, Some(_)) => {}
            _ => return Err(WireError::IdentityRule),
        }
        if let Some(label) = &self.responder_label {
            if label.len() > max_label_bytes {
                return Err(WireError::LabelTooLong {
                    actual: label.len(),
                    max: max_label_bytes,
                });
            }
        }

        Ok(())
    }
}

/// Credentials carried in a client logical header (`INT-NET-054 / 1`).
///
/// The manual [`fmt::Debug`] implementation redacts the password so a stray
/// debug print cannot leak it.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Login of the account the request authenticates.
    pub login: String,
    /// Password of the account, without trimming or normalization.
    pub password: String,
}

/// Client logical header of an account-authenticated request (`INT-NET-051 / 3`).
///
/// The four leading elements are the operation type, the login, the password
/// and the operation arguments. Elements beyond the fourth are not fixed by
/// the v1 requirements (`OPEN-003`), so exactly four are accepted.
///
/// The manual [`fmt::Debug`] implementation redacts the credentials.
#[derive(Clone, PartialEq, Eq)]
pub enum ClientHeader {
    /// Account or installation registration (`INT-NET-058 / 1`).
    Registration {
        /// Account credentials.
        credentials: Credentials,
        /// Open X25519 key of the installation.
        encryption_public_key: [u8; KEY_BYTES],
        /// Open Ed25519 key of the installation.
        signing_public_key: [u8; KEY_BYTES],
    },
    /// Topology retrieval with an empty argument array (`INT-NET-060 / 1`).
    Topology {
        /// Account credentials.
        credentials: Credentials,
    },
    /// Catalog page request (`FR-CAT-015 / 1`).
    CatalogPage {
        /// Account credentials.
        credentials: Credentials,
        /// Snapshot token of the previous page, `null` for the first page.
        snapshot_token: Option<[u8; IDENTIFIER_BYTES]>,
        /// Cursor of the previous page, `null` for the first page.
        cursor: Option<[u8; IDENTIFIER_BYTES]>,
    },
    /// Media upload request (`INT-NET-052 / 4`).
    Upload {
        /// Account credentials.
        credentials: Credentials,
        /// Original name of the media file.
        source_name: String,
        /// Capture timestamp in Unix milliseconds.
        timestamp_ms: i64,
        /// Original size in bytes.
        original_size_bytes: u64,
        /// Compressed size in bytes.
        compressed_size_bytes: u64,
        /// SHA-256 of the original bytes.
        sha256: [u8; KEY_BYTES],
    },
}

impl ClientHeader {
    /// Operation code of this header (`INT-NET-057 / 1`).
    pub fn operation(&self) -> ClientOperation {
        match self {
            Self::Registration { .. } => ClientOperation::Registration,
            Self::Topology { .. } => ClientOperation::Topology,
            Self::CatalogPage { .. } => ClientOperation::CatalogPage,
            Self::Upload { .. } => ClientOperation::Upload,
        }
    }

    /// Account credentials carried by this header.
    pub fn credentials(&self) -> &Credentials {
        match self {
            Self::Registration { credentials, .. }
            | Self::Topology { credentials }
            | Self::CatalogPage { credentials, .. }
            | Self::Upload { credentials, .. } => credentials,
        }
    }

    /// Encode the header as the deterministic positional CBOR array.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let (credentials, arguments) = match self {
            Self::Registration {
                credentials,
                encryption_public_key,
                signing_public_key,
            } => (
                credentials,
                vec![
                    Value::Bytes(encryption_public_key.to_vec()),
                    Value::Bytes(signing_public_key.to_vec()),
                ],
            ),
            Self::Topology { credentials } => (credentials, Vec::new()),
            Self::CatalogPage {
                credentials,
                snapshot_token,
                cursor,
            } => {
                if snapshot_token.is_some() != cursor.is_some() {
                    return Err(WireError::CatalogTokenPair);
                }
                (
                    credentials,
                    vec![optional_bytes(*snapshot_token), optional_bytes(*cursor)],
                )
            }
            Self::Upload {
                credentials,
                source_name,
                timestamp_ms,
                original_size_bytes,
                compressed_size_bytes,
                sha256,
            } => (
                credentials,
                vec![
                    Value::Text(source_name.clone()),
                    Value::Integer((*timestamp_ms).into()),
                    Value::Integer((*original_size_bytes).into()),
                    Value::Integer((*compressed_size_bytes).into()),
                    Value::Bytes(sha256.to_vec()),
                ],
            ),
        };

        let value = Value::Array(vec![
            Value::Integer(u64::from(self.operation().code()).into()),
            Value::Text(credentials.login.clone()),
            Value::Text(credentials.password.clone()),
            Value::Array(arguments),
        ]);

        encode_value(&value)
    }

    /// Decode and validate a client logical header.
    ///
    /// Credentials are returned as sent: login syntax and password limits are
    /// enforced by the registration layer, not by this schema codec.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let Value::Array(elements) = decode_value(bytes)? else {
            return Err(WireError::NotAnArray(CLIENT_HEADER));
        };
        require_length(&elements, CLIENT_HEADER, CLIENT_HEADER_ELEMENTS)?;

        let code = uint32(&elements[0], CLIENT_HEADER, 1)?;
        let operation = ClientOperation::from_code(code)?;
        let credentials = Credentials {
            login: text(&elements[1], CLIENT_HEADER, 2)?.to_owned(),
            password: text(&elements[2], CLIENT_HEADER, 3)?.to_owned(),
        };
        let Value::Array(arguments) = &elements[3] else {
            return Err(WireError::WrongType {
                header: CLIENT_HEADER,
                position: 4,
            });
        };

        match operation {
            ClientOperation::Registration => {
                require_arguments(operation, arguments, 2)?;
                Ok(Self::Registration {
                    credentials,
                    encryption_public_key: key(&arguments[0], CLIENT_HEADER, 4)
                        .map_err(argument_error(operation, 0))?,
                    signing_public_key: key(&arguments[1], CLIENT_HEADER, 4)
                        .map_err(argument_error(operation, 1))?,
                })
            }
            ClientOperation::Topology => {
                require_arguments(operation, arguments, 0)?;
                Ok(Self::Topology { credentials })
            }
            ClientOperation::CatalogPage => {
                require_arguments(operation, arguments, 2)?;
                let snapshot_token = optional_identifier(&arguments[0], CLIENT_HEADER, 4)
                    .map_err(argument_error(operation, 0))?;
                let cursor = optional_identifier(&arguments[1], CLIENT_HEADER, 4)
                    .map_err(argument_error(operation, 1))?;
                if snapshot_token.is_some() != cursor.is_some() {
                    return Err(WireError::CatalogTokenPair);
                }
                Ok(Self::CatalogPage {
                    credentials,
                    snapshot_token,
                    cursor,
                })
            }
            ClientOperation::Upload => {
                require_arguments(operation, arguments, 5)?;
                let timestamp_ms = match &arguments[1] {
                    Value::Integer(value) => {
                        i64::try_from(*value).map_err(|_| WireError::IntegerOutOfRange {
                            header: CLIENT_HEADER,
                            position: 4,
                        })?
                    }
                    _ => {
                        return Err(WireError::WrongArgumentType {
                            operation,
                            index: 1,
                        })
                    }
                };
                Ok(Self::Upload {
                    credentials,
                    source_name: text(&arguments[0], CLIENT_HEADER, 4)
                        .map_err(argument_error(operation, 0))?
                        .to_owned(),
                    timestamp_ms,
                    original_size_bytes: uint(&arguments[2], CLIENT_HEADER, 4)
                        .map_err(argument_error(operation, 2))?,
                    compressed_size_bytes: uint(&arguments[3], CLIENT_HEADER, 4)
                        .map_err(argument_error(operation, 3))?,
                    sha256: key(&arguments[4], CLIENT_HEADER, 4)
                        .map_err(argument_error(operation, 4))?,
                })
            }
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("login", &self.login)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for ClientHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientHeader")
            .field("operation", &self.operation())
            .field("credentials", self.credentials())
            .finish_non_exhaustive()
    }
}

/// Client operation codes of v1 (`INT-NET-057 / 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientOperation {
    /// Code `1`: account or installation registration.
    Registration,
    /// Code `2`: topology retrieval.
    Topology,
    /// Code `3`: catalog page request.
    CatalogPage,
    /// Code `4`: media upload.
    Upload,
}

impl ClientOperation {
    /// Numeric code of the operation.
    pub fn code(self) -> u32 {
        match self {
            Self::Registration => 1,
            Self::Topology => 2,
            Self::CatalogPage => 3,
            Self::Upload => 4,
        }
    }

    /// Operation for a numeric code, rejecting `0` and unknown values.
    pub fn from_code(code: u32) -> Result<Self> {
        match code {
            1 => Ok(Self::Registration),
            2 => Ok(Self::Topology),
            3 => Ok(Self::CatalogPage),
            4 => Ok(Self::Upload),
            other => Err(WireError::UnknownOperation(other)),
        }
    }
}

/// Logical header of a server response (`INT-NET-061 / 1`).
///
/// `code` is `0` for success and nonzero for every failure. A failure carries
/// an empty argument array (`INT-NET-069 / 1`). Positions beyond the two
/// elements are not fixed by the v1 requirements (`OPEN-003`), so exactly two
/// are accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultEnvelope {
    /// Result code; `0` means success (`INT-NET-062 / 1`).
    pub code: u32,
    /// Result arguments; empty for every nonzero code.
    pub arguments: Vec<Value>,
}

impl ResultEnvelope {
    /// Successful response with its argument array.
    pub fn success(arguments: Vec<Value>) -> Self {
        Self { code: 0, arguments }
    }

    /// Failure response without arguments; the code is nonzero by construction.
    pub fn failure(code: NonZeroU32) -> Self {
        Self {
            code: code.get(),
            arguments: Vec::new(),
        }
    }

    /// Encode the envelope, enforcing the empty-arguments rule for failures.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.code != 0 && !self.arguments.is_empty() {
            return Err(WireError::FailureArguments);
        }
        let value = Value::Array(vec![
            Value::Integer(u64::from(self.code).into()),
            Value::Array(self.arguments.clone()),
        ]);

        encode_value(&value)
    }

    /// Decode an envelope, enforcing the same rule.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let Value::Array(elements) = decode_value(bytes)? else {
            return Err(WireError::NotAnArray(RESULT_ENVELOPE));
        };
        require_length(&elements, RESULT_ENVELOPE, RESULT_ENVELOPE_ELEMENTS)?;

        let code = uint32(&elements[0], RESULT_ENVELOPE, 1)?;
        let Value::Array(arguments) = &elements[1] else {
            return Err(WireError::WrongType {
                header: RESULT_ENVELOPE,
                position: 2,
            });
        };
        if code != 0 && !arguments.is_empty() {
            return Err(WireError::FailureArguments);
        }

        Ok(Self {
            code,
            arguments: arguments.clone(),
        })
    }
}

fn encode_value(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes)
        .map_err(|error| WireError::Encode(error.to_string()))?;
    Ok(bytes)
}

fn decode_value(bytes: &[u8]) -> Result<Value> {
    let mut reader = bytes;
    let value = ciborium::de::from_reader::<Value, _>(&mut reader)
        .map_err(|error| WireError::Decode(error.to_string()))?;
    if !reader.is_empty() {
        return Err(WireError::TrailingBytes);
    }
    Ok(value)
}

fn require_length(elements: &[Value], header: &'static str, expected: usize) -> Result<()> {
    if elements.len() == expected {
        Ok(())
    } else {
        Err(WireError::WrongLength {
            header,
            expected,
            actual: elements.len(),
        })
    }
}

fn argument_error(operation: ClientOperation, index: usize) -> impl Fn(WireError) -> WireError {
    move |error| match error {
        WireError::WrongType { .. } => WireError::WrongArgumentType { operation, index },
        other => other,
    }
}

fn require_arguments(
    operation: ClientOperation,
    arguments: &[Value],
    expected: usize,
) -> Result<()> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(WireError::WrongArgumentCount {
            operation,
            expected,
            actual: arguments.len(),
        })
    }
}

fn uint32(value: &Value, header: &'static str, position: usize) -> Result<u32> {
    u32::try_from(uint(value, header, position)?)
        .map_err(|_| WireError::IntegerOutOfRange { header, position })
}

fn uint(value: &Value, header: &'static str, position: usize) -> Result<u64> {
    match value {
        Value::Integer(integer) => {
            u64::try_from(*integer).map_err(|_| WireError::IntegerOutOfRange { header, position })
        }
        _ => Err(WireError::WrongType { header, position }),
    }
}

fn text<'a>(value: &'a Value, header: &'static str, position: usize) -> Result<&'a str> {
    match value {
        Value::Text(text) => Ok(text.as_str()),
        _ => Err(WireError::WrongType { header, position }),
    }
}

fn identifier(
    value: &Value,
    header: &'static str,
    position: usize,
) -> Result<[u8; IDENTIFIER_BYTES]> {
    fixed_bytes::<IDENTIFIER_BYTES>(value, header, position)
}

fn key(value: &Value, header: &'static str, position: usize) -> Result<[u8; KEY_BYTES]> {
    fixed_bytes::<KEY_BYTES>(value, header, position)
}

fn optional_identifier(
    value: &Value,
    header: &'static str,
    position: usize,
) -> Result<Option<[u8; IDENTIFIER_BYTES]>> {
    optional_fixed_bytes::<IDENTIFIER_BYTES>(value, header, position)
}

fn optional_key(
    value: &Value,
    header: &'static str,
    position: usize,
) -> Result<Option<[u8; KEY_BYTES]>> {
    optional_fixed_bytes::<KEY_BYTES>(value, header, position)
}

fn fixed_bytes<const N: usize>(
    value: &Value,
    header: &'static str,
    position: usize,
) -> Result<[u8; N]> {
    match value {
        Value::Bytes(bytes) => {
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| WireError::ByteStringLength {
                    header,
                    position,
                    expected: N,
                })
        }
        _ => Err(WireError::WrongType { header, position }),
    }
}

fn optional_fixed_bytes<const N: usize>(
    value: &Value,
    header: &'static str,
    position: usize,
) -> Result<Option<[u8; N]>> {
    match value {
        Value::Null => Ok(None),
        other => fixed_bytes(other, header, position).map(Some),
    }
}

fn optional_bytes<const N: usize>(bytes: Option<[u8; N]>) -> Value {
    match bytes {
        Some(bytes) => Value::Bytes(bytes.to_vec()),
        None => Value::Null,
    }
}

fn optional_text(text: Option<&str>) -> Value {
    match text {
        Some(text) => Value::Text(text.to_owned()),
        None => Value::Null,
    }
}

fn optional_text_value(
    value: &Value,
    header: &'static str,
    position: usize,
) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(text) => Ok(Some(text.clone())),
        _ => Err(WireError::WrongType { header, position }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_LABEL_BYTES: usize = 64;

    fn header() -> ExternalHeader {
        ExternalHeader {
            sender_id: Some([0x11; IDENTIFIER_BYTES]),
            request_id: [0x22; IDENTIFIER_BYTES],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: None,
        }
    }

    fn credentials() -> Credentials {
        Credentials {
            login: "alice".to_owned(),
            password: "secret".to_owned(),
        }
    }

    #[test]
    fn the_external_header_encodes_the_documented_layout() {
        let mut expected = vec![0x87, 0x01, 0x50];
        expected.extend([0x11; IDENTIFIER_BYTES]);
        expected.push(0x50);
        expected.extend([0x22; IDENTIFIER_BYTES]);
        expected.extend([0x00, 0xf6, 0xf6, 0xf6]);

        assert_eq!(
            header()
                .encode(MAX_LABEL_BYTES)
                .expect("encoding must work"),
            expected
        );
    }

    #[test]
    fn the_external_header_round_trips_with_every_optional_element() {
        let header = ExternalHeader {
            sender_id: Some([0x11; IDENTIFIER_BYTES]),
            request_id: [0x22; IDENTIFIER_BYTES],
            frame_number: 7,
            upload_id: Some([0x33; IDENTIFIER_BYTES]),
            responder_label: Some("storage-1".to_owned()),
            bootstrap_signing_public_key: None,
        };

        let bytes = header.encode(MAX_LABEL_BYTES).expect("encoding must work");
        let decoded = ExternalHeader::decode(&bytes, MAX_LABEL_BYTES).expect("decoding must work");

        assert_eq!(decoded, header);
    }

    #[test]
    fn a_bootstrap_header_carries_the_key_instead_of_a_sender_id() {
        let header = ExternalHeader {
            sender_id: None,
            request_id: [0x22; IDENTIFIER_BYTES],
            frame_number: 0,
            upload_id: None,
            responder_label: None,
            bootstrap_signing_public_key: Some([0x44; KEY_BYTES]),
        };

        let bytes = header.encode(MAX_LABEL_BYTES).expect("encoding must work");
        let decoded = ExternalHeader::decode(&bytes, MAX_LABEL_BYTES).expect("decoding must work");

        assert_eq!(decoded, header);
    }

    #[test]
    fn a_header_without_any_identity_is_rejected() {
        let mut neither = header();
        neither.sender_id = None;

        assert!(matches!(
            neither.encode(MAX_LABEL_BYTES),
            Err(WireError::IdentityRule)
        ));
    }

    #[test]
    fn a_header_with_both_identities_is_rejected() {
        let mut both = header();
        both.bootstrap_signing_public_key = Some([0x44; KEY_BYTES]);

        assert!(matches!(
            both.encode(MAX_LABEL_BYTES),
            Err(WireError::IdentityRule)
        ));
    }

    #[test]
    fn an_unsupported_version_is_rejected() {
        let mut bytes = header()
            .encode(MAX_LABEL_BYTES)
            .expect("encoding must work");
        bytes[1] = 0x02;

        assert!(matches!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES),
            Err(WireError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn a_wrong_element_count_is_rejected() {
        let bytes = encode_value(&Value::Array(vec![Value::Integer(1.into())]))
            .expect("encoding must work");

        assert!(matches!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES),
            Err(WireError::WrongLength { actual: 1, .. })
        ));
    }

    #[test]
    fn a_label_above_the_configured_limit_is_rejected() {
        let header = ExternalHeader {
            responder_label: Some("x".repeat(MAX_LABEL_BYTES + 1)),
            ..header()
        };

        assert!(matches!(
            header.encode(MAX_LABEL_BYTES),
            Err(WireError::LabelTooLong { .. })
        ));
    }

    #[test]
    fn a_byte_string_of_the_wrong_length_is_rejected() {
        let value = Value::Array(vec![
            Value::Integer(1.into()),
            Value::Bytes(vec![0x11; IDENTIFIER_BYTES - 1]),
            Value::Bytes(vec![0x22; IDENTIFIER_BYTES]),
            Value::Integer(0.into()),
            Value::Null,
            Value::Null,
            Value::Null,
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES),
            Err(WireError::ByteStringLength { .. })
        ));
    }

    #[test]
    fn the_topology_request_encodes_the_documented_layout() {
        let header = ClientHeader::Topology {
            credentials: credentials(),
        };
        let expected = vec![
            0x84, 0x02, 0x65, b'a', b'l', b'i', b'c', b'e', 0x66, b's', b'e', b'c', b'r', b'e',
            b't', 0x80,
        ];

        assert_eq!(header.encode().expect("encoding must work"), expected);
        assert_eq!(
            ClientHeader::decode(&expected).expect("decoding must work"),
            header
        );
    }

    #[test]
    fn every_client_operation_round_trips() {
        let headers = [
            ClientHeader::Registration {
                credentials: credentials(),
                encryption_public_key: [0x55; KEY_BYTES],
                signing_public_key: [0x66; KEY_BYTES],
            },
            ClientHeader::Topology {
                credentials: credentials(),
            },
            ClientHeader::CatalogPage {
                credentials: credentials(),
                snapshot_token: None,
                cursor: None,
            },
            ClientHeader::CatalogPage {
                credentials: credentials(),
                snapshot_token: Some([0x77; IDENTIFIER_BYTES]),
                cursor: Some([0x88; IDENTIFIER_BYTES]),
            },
            ClientHeader::Upload {
                credentials: credentials(),
                source_name: "photo.jpg".to_owned(),
                timestamp_ms: -1,
                original_size_bytes: 1_000,
                compressed_size_bytes: 500,
                sha256: [0x99; KEY_BYTES],
            },
        ];

        for header in headers {
            let bytes = header.encode().expect("encoding must work");
            let decoded = ClientHeader::decode(&bytes).expect("decoding must work");
            assert_eq!(decoded, header);
            assert_eq!(decoded.operation(), header.operation());
        }
    }

    #[test]
    fn a_catalog_page_with_only_one_token_is_rejected() {
        let header = ClientHeader::CatalogPage {
            credentials: credentials(),
            snapshot_token: Some([0x77; IDENTIFIER_BYTES]),
            cursor: None,
        };

        assert!(matches!(header.encode(), Err(WireError::CatalogTokenPair)));
    }

    #[test]
    fn unknown_operation_codes_are_rejected() {
        for code in [0_u64, 5] {
            let value = Value::Array(vec![
                Value::Integer(code.into()),
                Value::Text("alice".to_owned()),
                Value::Text("secret".to_owned()),
                Value::Array(Vec::new()),
            ]);
            let bytes = encode_value(&value).expect("encoding must work");

            assert!(matches!(
                ClientHeader::decode(&bytes),
                Err(WireError::UnknownOperation(_))
            ));
        }
    }

    #[test]
    fn operation_argument_counts_are_enforced() {
        let value = Value::Array(vec![
            Value::Integer(2.into()),
            Value::Text("alice".to_owned()),
            Value::Text("secret".to_owned()),
            Value::Array(vec![Value::Bytes(vec![0x11; KEY_BYTES])]),
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ClientHeader::decode(&bytes),
            Err(WireError::WrongLength { .. })
        ));
    }

    #[test]
    fn the_result_envelope_round_trips() {
        let success = ResultEnvelope::success(vec![Value::Bytes(vec![0xAA; IDENTIFIER_BYTES])]);
        let bytes = success.encode().expect("encoding must work");
        assert_eq!(
            ResultEnvelope::decode(&bytes).expect("decoding must work"),
            success
        );

        let failure =
            ResultEnvelope::failure(NonZeroU32::new(0x0004_0002).expect("a nonzero result code"));
        let bytes = failure.encode().expect("encoding must work");
        assert_eq!(
            ResultEnvelope::decode(&bytes).expect("decoding must work"),
            failure
        );
    }

    #[test]
    fn a_failure_with_arguments_is_rejected_both_ways() {
        let envelope = ResultEnvelope {
            code: 1,
            arguments: vec![Value::Null],
        };

        assert!(matches!(
            envelope.encode(),
            Err(WireError::FailureArguments)
        ));

        let value = Value::Array(vec![
            Value::Integer(1.into()),
            Value::Array(vec![Value::Null]),
        ]);
        let bytes = encode_value(&value).expect("encoding must work");
        assert!(matches!(
            ResultEnvelope::decode(&bytes),
            Err(WireError::FailureArguments)
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = ClientHeader::Topology {
            credentials: credentials(),
        }
        .encode()
        .expect("encoding must work");
        bytes.push(0x00);

        assert!(matches!(
            ClientHeader::decode(&bytes),
            Err(WireError::TrailingBytes)
        ));
    }

    #[test]
    fn the_registration_request_encodes_the_documented_layout() {
        let header = ClientHeader::Registration {
            credentials: credentials(),
            encryption_public_key: [0x55; KEY_BYTES],
            signing_public_key: [0x66; KEY_BYTES],
        };
        let mut expected = vec![
            0x84, 0x01, 0x65, b'a', b'l', b'i', b'c', b'e', 0x66, b's', b'e', b'c', b'r', b'e',
            b't', 0x82, 0x58, 0x20,
        ];
        expected.extend([0x55; KEY_BYTES]);
        expected.extend([0x58, 0x20]);
        expected.extend([0x66; KEY_BYTES]);

        assert_eq!(header.encode().expect("encoding must work"), expected);
    }

    #[test]
    fn a_catalog_first_page_encodes_two_nulls() {
        let header = ClientHeader::CatalogPage {
            credentials: credentials(),
            snapshot_token: None,
            cursor: None,
        };
        let expected = vec![
            0x84, 0x03, 0x65, b'a', b'l', b'i', b'c', b'e', 0x66, b's', b'e', b'c', b'r', b'e',
            b't', 0x82, 0xf6, 0xf6,
        ];

        assert_eq!(header.encode().expect("encoding must work"), expected);
    }

    #[test]
    fn a_catalog_continuation_encodes_both_tokens() {
        let header = ClientHeader::CatalogPage {
            credentials: credentials(),
            snapshot_token: Some([0x77; IDENTIFIER_BYTES]),
            cursor: Some([0x88; IDENTIFIER_BYTES]),
        };
        let mut expected = vec![
            0x84, 0x03, 0x65, b'a', b'l', b'i', b'c', b'e', 0x66, b's', b'e', b'c', b'r', b'e',
            b't', 0x82, 0x50,
        ];
        expected.extend([0x77; IDENTIFIER_BYTES]);
        expected.push(0x50);
        expected.extend([0x88; IDENTIFIER_BYTES]);

        assert_eq!(header.encode().expect("encoding must work"), expected);
    }

    #[test]
    fn the_upload_request_encodes_the_documented_layout() {
        let header = ClientHeader::Upload {
            credentials: credentials(),
            source_name: "photo.jpg".to_owned(),
            timestamp_ms: -1,
            original_size_bytes: 1_000,
            compressed_size_bytes: 500,
            sha256: [0x99; KEY_BYTES],
        };
        let mut expected = vec![
            0x84, 0x04, 0x65, b'a', b'l', b'i', b'c', b'e', 0x66, b's', b'e', b'c', b'r', b'e',
            b't', 0x85, 0x69, b'p', b'h', b'o', b't', b'o', b'.', b'j', b'p', b'g', 0x20, 0x19,
            0x03, 0xe8, 0x19, 0x01, 0xf4, 0x58, 0x20,
        ];
        expected.extend([0x99; KEY_BYTES]);

        assert_eq!(header.encode().expect("encoding must work"), expected);
    }

    #[test]
    fn the_result_envelope_encodes_the_documented_layout() {
        let success = ResultEnvelope::success(vec![Value::Bytes(vec![0xAA; IDENTIFIER_BYTES])]);
        let mut expected = vec![0x82, 0x00, 0x81, 0x50];
        expected.extend([0xAA; IDENTIFIER_BYTES]);
        assert_eq!(success.encode().expect("encoding must work"), expected);

        let failure =
            ResultEnvelope::failure(NonZeroU32::new(0x0004_0002).expect("a nonzero code"));
        assert_eq!(
            failure.encode().expect("encoding must work"),
            vec![0x82, 0x1a, 0x00, 0x04, 0x00, 0x02, 0x80]
        );
    }

    #[test]
    fn an_out_of_range_result_code_is_rejected() {
        let value = Value::Array(vec![
            Value::Integer(0x1_0000_0000_u64.into()),
            Value::Array(Vec::new()),
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ResultEnvelope::decode(&bytes),
            Err(WireError::IntegerOutOfRange { .. })
        ));
    }

    #[test]
    fn the_largest_result_code_is_accepted() {
        let value = Value::Array(vec![
            Value::Integer(u64::from(u32::MAX).into()),
            Value::Array(Vec::new()),
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert_eq!(
            ResultEnvelope::decode(&bytes)
                .expect("decoding must work")
                .code,
            u32::MAX
        );
    }

    #[test]
    fn an_out_of_range_operation_code_is_rejected() {
        let value = Value::Array(vec![
            Value::Integer(0x1_0000_0000_u64.into()),
            Value::Text("alice".to_owned()),
            Value::Text("secret".to_owned()),
            Value::Array(Vec::new()),
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ClientHeader::decode(&bytes),
            Err(WireError::IntegerOutOfRange { .. })
        ));
    }

    #[test]
    fn an_out_of_range_version_is_reported_as_unsupported() {
        let value = Value::Array(vec![
            Value::Integer(0x1_0000_0000_u64.into()),
            Value::Bytes(vec![0x11; IDENTIFIER_BYTES]),
            Value::Bytes(vec![0x22; IDENTIFIER_BYTES]),
            Value::Integer(0.into()),
            Value::Null,
            Value::Null,
            Value::Null,
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES),
            Err(WireError::UnsupportedVersion(version)) if version == 0x1_0000_0000
        ));
    }

    #[test]
    fn a_mixed_catalog_page_is_rejected_on_decode() {
        let value = Value::Array(vec![
            Value::Integer(3.into()),
            Value::Text("alice".to_owned()),
            Value::Text("secret".to_owned()),
            Value::Array(vec![
                Value::Bytes(vec![0x77; IDENTIFIER_BYTES]),
                Value::Null,
            ]),
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ClientHeader::decode(&bytes),
            Err(WireError::CatalogTokenPair)
        ));
    }

    #[test]
    fn a_label_above_the_limit_is_rejected_on_decode() {
        let value = Value::Array(vec![
            Value::Integer(1.into()),
            Value::Bytes(vec![0x11; IDENTIFIER_BYTES]),
            Value::Bytes(vec![0x22; IDENTIFIER_BYTES]),
            Value::Integer(0.into()),
            Value::Null,
            Value::Text("x".repeat(MAX_LABEL_BYTES + 1)),
            Value::Null,
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES),
            Err(WireError::LabelTooLong { .. })
        ));
    }

    #[test]
    fn a_wrong_cbor_type_in_a_header_position_is_rejected() {
        let value = Value::Array(vec![
            Value::Integer(1.into()),
            Value::Text("sender".to_owned()),
            Value::Bytes(vec![0x22; IDENTIFIER_BYTES]),
            Value::Integer(0.into()),
            Value::Null,
            Value::Null,
            Value::Null,
        ]);
        let bytes = encode_value(&value).expect("encoding must work");

        assert!(matches!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES),
            Err(WireError::WrongType { position: 2, .. })
        ));
    }

    #[test]
    fn text_fields_preserve_spaces_and_unicode() {
        let header = ExternalHeader {
            responder_label: Some("узел 1".to_owned()),
            ..header()
        };
        let bytes = header.encode(MAX_LABEL_BYTES).expect("encoding must work");
        assert_eq!(
            ExternalHeader::decode(&bytes, MAX_LABEL_BYTES)
                .expect("decoding must work")
                .responder_label,
            Some("узел 1".to_owned())
        );

        let client = ClientHeader::Topology {
            credentials: Credentials {
                login: "alice".to_owned(),
                password: "пароль с пробелом".to_owned(),
            },
        };
        let bytes = client.encode().expect("encoding must work");
        assert_eq!(
            ClientHeader::decode(&bytes).expect("decoding must work"),
            client
        );
    }

    #[test]
    fn cbor_undefined_is_treated_as_null_by_this_value_layer() {
        // ciborium maps CBOR `undefined` (0xf7) to the same value as `null`, so
        // the codec cannot distinguish them. Senders must emit `null`.
        let mut bytes = vec![
            0x84, 0x03, 0x65, b'a', b'l', b'i', b'c', b'e', 0x66, b's', b'e', b'c', b'r', b'e',
            b't', 0x82,
        ];
        bytes.extend([0xf7, 0xf7]);

        let decoded = ClientHeader::decode(&bytes).expect("undefined is read as null");
        assert_eq!(
            decoded,
            ClientHeader::CatalogPage {
                credentials: credentials(),
                snapshot_token: None,
                cursor: None,
            }
        );
    }
}
