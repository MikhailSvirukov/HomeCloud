//! External frame header for protocol version 1.
//!
//! The header is the seven-position CBOR array fixed by `INT-NET-050 / 4`.
//! Optional positions are `null` and are never omitted.

use crate::cbor::{self, Value};

const PROTOCOL_VERSION: u64 = 1;
const PROTOCOL_VERSION_FIELD: &str = "protocol_version";

const SENDER_ID: &str = "sender_id";
const REQUEST_ID: &str = "request_id";
const FRAME_NUMBER: &str = "frame_number";
const UPLOAD_ID: &str = "upload_id";
const RESPONDER_LABEL: &str = "responder_label";
const BOOTSTRAP_SIGNING_PUBLIC_KEY: &str = "bootstrap_signing_public_key";

/// The seven-position external frame header for protocol version 1
/// (`INT-NET-050/4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFrameHeader {
    /// The assigned sender identifier, or `None` for primary registration.
    pub sender_id: Option<[u8; 16]>,
    /// The request identifier shared by a request and its response.
    pub request_id: [u8; 16],
    /// The sequence number of the frame, starting at `0`.
    pub frame_number: u32,
    /// The upload identifier, or `None` when no upload applies.
    pub upload_id: Option<[u8; 16]>,
    /// The responder label, or `None` when the label does not apply.
    pub responder_label: Option<String>,
    /// The installation's open Ed25519 key for primary registration.
    pub bootstrap_signing_public_key: Option<[u8; 32]>,
}

impl ExternalFrameHeader {
    /// Encodes `[1, sender_id, request_id, frame_number, upload_id, responder_label, bootstrap_signing_public_key]`.
    pub fn encode(&self, max_label_bytes: usize) -> Result<Vec<u8>, HeaderError> {
        let value = Value::Array(vec![
            Value::Uint(PROTOCOL_VERSION),
            optional_bytes(self.sender_id),
            Value::Bytes(self.request_id.to_vec()),
            Value::Uint(u64::from(self.frame_number)),
            optional_bytes(self.upload_id),
            optional_label_value(&self.responder_label),
            optional_bytes(self.bootstrap_signing_public_key),
        ]);

        Self::decode(&value, max_label_bytes)?;
        Ok(cbor::encode(&value))
    }

    /// Decodes and validates positions, CBOR types, lengths, label limit, and
    /// null rules.
    pub fn decode(value: &Value, max_label_bytes: usize) -> Result<Self, HeaderError> {
        let Value::Array(items) = value else {
            return Err(HeaderError::WrongShape);
        };

        let [
            version,
            sender_id,
            request_id,
            frame_number,
            upload_id,
            responder_label,
            bootstrap,
        ] = items.as_slice()
        else {
            return Err(HeaderError::WrongShape);
        };

        let Value::Uint(version) = version else {
            return Err(HeaderError::WrongType(PROTOCOL_VERSION_FIELD));
        };
        if *version != PROTOCOL_VERSION {
            return Err(HeaderError::WrongVersion(*version));
        }

        let sender_id = optional_fixed_bytes::<16>(sender_id, SENDER_ID)?;
        let request_id = fixed_bytes::<16>(request_id, REQUEST_ID)?;

        let Value::Uint(frame_number) = frame_number else {
            return Err(HeaderError::WrongType(FRAME_NUMBER));
        };
        let frame_number = u32::try_from(*frame_number)
            .map_err(|_| HeaderError::FrameNumberOutOfRange(*frame_number))?;

        let upload_id = optional_fixed_bytes::<16>(upload_id, UPLOAD_ID)?;
        let responder_label = optional_label(responder_label, max_label_bytes)?;
        let bootstrap_signing_public_key =
            optional_fixed_bytes::<32>(bootstrap, BOOTSTRAP_SIGNING_PUBLIC_KEY)?;

        if sender_id.is_none() != bootstrap_signing_public_key.is_some() {
            return Err(HeaderError::InvalidRegistrationShape);
        }

        Ok(Self {
            sender_id,
            request_id,
            frame_number,
            upload_id,
            responder_label,
            bootstrap_signing_public_key,
        })
    }
}

/// Reasons an external frame header is not valid for protocol version 1.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    /// The value is not a seven-element CBOR array.
    #[error("external frame header must be a seven-element array")]
    WrongShape,
    /// The protocol version is not supported.
    #[error("unsupported protocol version {0}")]
    WrongVersion(u64),
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// A byte string position has the wrong length.
    #[error("invalid byte-string length for {0}")]
    InvalidLength(&'static str),
    /// The frame number does not fit `uint32`.
    #[error("frame number {0} does not fit uint32")]
    FrameNumberOutOfRange(u64),
    /// The responder label exceeds the configured UTF-8 byte limit.
    #[error("responder label is {actual} bytes, limit is {limit}")]
    LabelTooLong {
        /// Actual UTF-8 byte length of the label.
        actual: usize,
        /// Configured maximum label length in UTF-8 bytes (`CFG-CL-031`).
        limit: usize,
    },
    /// `sender_id` and `bootstrap_signing_public_key` presence disagree with
    /// the primary-registration rule.
    #[error("sender_id and bootstrap_signing_public_key presence disagree")]
    InvalidRegistrationShape,
}

fn optional_bytes<const N: usize>(bytes: Option<[u8; N]>) -> Value {
    match bytes {
        Some(bytes) => Value::Bytes(bytes.to_vec()),
        None => Value::Null,
    }
}

fn optional_label_value(label: &Option<String>) -> Value {
    match label {
        Some(label) => Value::Text(label.clone()),
        None => Value::Null,
    }
}

fn fixed_bytes<const N: usize>(value: &Value, field: &'static str) -> Result<[u8; N], HeaderError> {
    match value {
        Value::Bytes(bytes) => {
            <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| HeaderError::InvalidLength(field))
        }
        _ => Err(HeaderError::WrongType(field)),
    }
}

fn optional_fixed_bytes<const N: usize>(
    value: &Value,
    field: &'static str,
) -> Result<Option<[u8; N]>, HeaderError> {
    match value {
        Value::Null => Ok(None),
        Value::Bytes(_) => fixed_bytes::<N>(value, field).map(Some),
        _ => Err(HeaderError::WrongType(field)),
    }
}

fn optional_label(value: &Value, max_label_bytes: usize) -> Result<Option<String>, HeaderError> {
    match value {
        Value::Null => Ok(None),
        Value::Text(label) => {
            if label.len() > max_label_bytes {
                Err(HeaderError::LabelTooLong {
                    actual: label.len(),
                    limit: max_label_bytes,
                })
            } else {
                Ok(Some(label.clone()))
            }
        }
        _ => Err(HeaderError::WrongType(RESPONDER_LABEL)),
    }
}

#[cfg(test)]
mod tests {
    use super::{ExternalFrameHeader, HeaderError};
    use crate::cbor::{self, Value};

    const MAX_LABEL_BYTES: usize = 8;

    fn ordinary_header() -> ExternalFrameHeader {
        ExternalFrameHeader {
            sender_id: Some([0x11; 16]),
            request_id: [0x22; 16],
            frame_number: 0,
            upload_id: None,
            responder_label: Some("node-1".to_owned()),
            bootstrap_signing_public_key: None,
        }
    }

    fn primary_registration_header() -> ExternalFrameHeader {
        ExternalFrameHeader {
            sender_id: None,
            request_id: [0x33; 16],
            frame_number: 7,
            upload_id: Some([0x44; 16]),
            responder_label: None,
            bootstrap_signing_public_key: Some([0x55; 32]),
        }
    }

    fn encoded_items(header: &ExternalFrameHeader) -> Vec<Value> {
        let bytes = header.encode(MAX_LABEL_BYTES).expect("header is valid");
        let value = cbor::decode(&bytes).expect("header bytes are valid CBOR");
        let Value::Array(items) = value else {
            panic!("external frame header encodes to an array");
        };
        items
    }

    #[test]
    fn round_trips_ordinary_shape() {
        let header = ordinary_header();
        let items = Value::Array(encoded_items(&header));
        assert_eq!(
            ExternalFrameHeader::decode(&items, MAX_LABEL_BYTES),
            Ok(header)
        );
    }

    #[test]
    fn round_trips_primary_registration_shape() {
        let header = primary_registration_header();
        let items = Value::Array(encoded_items(&header));
        assert_eq!(
            ExternalFrameHeader::decode(&items, MAX_LABEL_BYTES),
            Ok(header)
        );
    }

    #[test]
    fn rejects_wrong_element_count() {
        let mut six_items = encoded_items(&ordinary_header());
        six_items.pop();
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(six_items), MAX_LABEL_BYTES),
            Err(HeaderError::WrongShape)
        );

        let mut eight_items = encoded_items(&ordinary_header());
        eight_items.push(Value::Null);
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(eight_items), MAX_LABEL_BYTES),
            Err(HeaderError::WrongShape)
        );
    }

    #[test]
    fn rejects_wrong_version() {
        let mut items = encoded_items(&ordinary_header());
        items[0] = Value::Uint(2);
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(items), MAX_LABEL_BYTES),
            Err(HeaderError::WrongVersion(2))
        );
    }

    #[test]
    fn rejects_null_request_id() {
        let mut items = encoded_items(&ordinary_header());
        items[2] = Value::Null;
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(items), MAX_LABEL_BYTES),
            Err(HeaderError::WrongType("request_id"))
        );
    }

    #[test]
    fn rejects_sender_id_of_wrong_length() {
        let mut items = encoded_items(&ordinary_header());
        items[1] = Value::Bytes(vec![0; 17]);
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(items), MAX_LABEL_BYTES),
            Err(HeaderError::InvalidLength("sender_id"))
        );
    }

    #[test]
    fn rejects_label_over_limit() {
        let mut items = encoded_items(&ordinary_header());
        items[5] = Value::Text("nine-byte".to_owned());
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(items), MAX_LABEL_BYTES),
            Err(HeaderError::LabelTooLong {
                actual: 9,
                limit: MAX_LABEL_BYTES,
            })
        );
    }

    #[test]
    fn rejects_label_over_limit_when_encoding() {
        let header = ExternalFrameHeader {
            responder_label: Some("nine-byte".to_owned()),
            ..ordinary_header()
        };
        assert_eq!(
            header.encode(MAX_LABEL_BYTES),
            Err(HeaderError::LabelTooLong {
                actual: 9,
                limit: MAX_LABEL_BYTES,
            })
        );
    }

    #[test]
    fn rejects_sender_and_bootstrap_key_presence_mismatch() {
        let mut items = encoded_items(&ordinary_header());
        items[6] = Value::Bytes(vec![0; 32]);
        assert_eq!(
            ExternalFrameHeader::decode(&Value::Array(items), MAX_LABEL_BYTES),
            Err(HeaderError::InvalidRegistrationShape)
        );
    }
}
