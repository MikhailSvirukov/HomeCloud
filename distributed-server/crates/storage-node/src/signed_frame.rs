//! Signed application frame body.
//!
//! SRS: `INT-NET-018 / 1`, `INT-NET-019 / 1`, `INT-NET-020 / 1`,
//! `INT-NET-028 / 1`, `INT-NET-034 / 1`, `INT-NET-081 / 1`.

use core::num::NonZeroU32;
use std::fmt;
use std::io::Cursor;

use ciborium::value::Value;

use crate::catalog_exchange::{
    decode_external_header, encode_external_header, CatalogExchangeError, ExternalHeader,
};
use crate::client_limits::ClientLimits;
use crate::framing::{FrameLength, FramingError, FRAME_LENGTH_PREFIX_LEN};
use crate::signing::{MessageSigningKey, MessageVerifyingKey, SIGNATURE_LEN};

/// Length of an X25519 or Ed25519 public key in a bootstrap payload.
pub use crate::catalog_exchange::BOOTSTRAP_KEY_LEN;

/// Number of keys carried by a bootstrap payload.
const BOOTSTRAP_KEY_COUNT: usize = 2;

type Result<T> = core::result::Result<T, SignedFrameError>;

/// Application frame with a decoded external header and opaque payload.
#[derive(PartialEq, Eq)]
pub struct SignedApplicationFrame {
    /// Decoded deterministic external header.
    pub header: ExternalHeader,
    /// Opaque signed payload bytes.
    pub payload: Vec<u8>,
}

impl fmt::Debug for SignedApplicationFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SignedApplicationFrame")
            .field("header", &self.header)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

/// Public keys carried by an unencrypted first-contact bootstrap payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapPayload {
    /// Key used to seal frames to the server.
    pub x25519_public_key: [u8; BOOTSTRAP_KEY_LEN],
    /// Key used to verify the server's signatures.
    pub ed25519_public_key: [u8; BOOTSTRAP_KEY_LEN],
}

/// Errors from encoding or decoding a signed application frame.
#[derive(Debug, thiserror::Error)]
pub enum SignedFrameError {
    /// The frame is shorter than its length prefix.
    #[error("the frame is shorter than its length prefix")]
    TooShort,
    /// The declared frame length does not match the body.
    #[error("the declared frame length does not match the body")]
    LengthMismatch,
    /// The frame header is invalid.
    #[error("the frame header is invalid")]
    Header(#[from] CatalogExchangeError),
    /// The frame body could not be decoded.
    #[error("the frame body could not be decoded")]
    Decode,
    /// The frame length exceeds uint32.
    #[error("the frame length exceeds uint32")]
    LengthOverflow,
    /// The frame signature is invalid.
    #[error("the frame signature is invalid")]
    InvalidSignature,
    /// The frame length prefix is invalid.
    #[error(transparent)]
    Framing(#[from] FramingError),
    /// The bootstrap payload is invalid.
    #[error("the bootstrap payload is invalid")]
    Bootstrap,
    /// The value could not be encoded.
    #[error("the value could not be encoded")]
    Encode,
}

/// Encodes a signed frame as a length prefix, external header, signature, and payload.
pub fn encode_signed_frame(
    header: &ExternalHeader,
    payload: &[u8],
    signing_key: &MessageSigningKey,
    max_frame_len: NonZeroU32,
    limits: &ClientLimits,
) -> Result<Vec<u8>> {
    let header_bytes = encode_external_header(header, limits)?;
    let body_len = header_bytes
        .len()
        .checked_add(SIGNATURE_LEN)
        .and_then(|len| len.checked_add(payload.len()))
        .ok_or(SignedFrameError::LengthOverflow)?;
    let body_len = u32::try_from(body_len).map_err(|_| SignedFrameError::LengthOverflow)?;
    let frame_length = FrameLength::new(body_len, max_frame_len)?;
    let prefix = frame_length.encode();

    let mut signed = Vec::with_capacity(prefix.len() + header_bytes.len() + payload.len());
    signed.extend_from_slice(&prefix);
    signed.extend_from_slice(&header_bytes);
    signed.extend_from_slice(payload);
    let signature = signing_key.sign(&signed);

    let mut frame = Vec::with_capacity(prefix.len() + body_len as usize);
    frame.extend_from_slice(&prefix);
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(&signature);
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decodes and verifies a signed frame before returning its header and opaque payload.
pub fn decode_signed_frame(
    bytes: &[u8],
    verifying_key: &MessageVerifyingKey,
    max_frame_len: NonZeroU32,
    limits: &ClientLimits,
) -> Result<SignedApplicationFrame> {
    let Some((prefix, body)) = bytes.split_at_checked(FRAME_LENGTH_PREFIX_LEN) else {
        return Err(SignedFrameError::TooShort);
    };
    let prefix: [u8; FRAME_LENGTH_PREFIX_LEN] =
        prefix.try_into().map_err(|_| SignedFrameError::TooShort)?;
    let declared = FrameLength::decode(prefix, max_frame_len)?.get() as usize;
    if body.len() != declared {
        return Err(SignedFrameError::LengthMismatch);
    }

    let mut header_cursor = Cursor::new(body);
    ciborium::de::from_reader::<Value, _>(&mut header_cursor)
        .map_err(|_| SignedFrameError::Decode)?;
    let header_len =
        usize::try_from(header_cursor.position()).map_err(|_| SignedFrameError::Decode)?;
    if header_len > body.len() {
        return Err(SignedFrameError::Decode);
    }
    let header = decode_external_header(&body[..header_len], limits)?;

    let signature_end = header_len
        .checked_add(SIGNATURE_LEN)
        .filter(|end| *end <= body.len())
        .ok_or(SignedFrameError::Decode)?;
    let signature: [u8; SIGNATURE_LEN] = body[header_len..signature_end]
        .try_into()
        .map_err(|_| SignedFrameError::Decode)?;
    let payload = &body[signature_end..];

    let mut signed = Vec::with_capacity(prefix.len() + header_len + payload.len());
    signed.extend_from_slice(&prefix);
    signed.extend_from_slice(&body[..header_len]);
    signed.extend_from_slice(payload);
    verifying_key
        .verify(&signed, &signature)
        .map_err(|_| SignedFrameError::InvalidSignature)?;

    Ok(SignedApplicationFrame {
        header,
        payload: payload.to_vec(),
    })
}

/// Encodes the unencrypted first-contact bootstrap payload.
pub fn encode_bootstrap_payload(payload: &BootstrapPayload) -> Result<Vec<u8>> {
    let value = Value::Array(vec![
        Value::Bytes(payload.x25519_public_key.to_vec()),
        Value::Bytes(payload.ed25519_public_key.to_vec()),
    ]);
    encode_value(&value)
}

/// Decodes the unencrypted first-contact bootstrap payload.
pub fn decode_bootstrap_payload(bytes: &[u8]) -> Result<BootstrapPayload> {
    let mut slice = bytes;
    let value =
        ciborium::de::from_reader::<Value, _>(&mut slice).map_err(|_| SignedFrameError::Decode)?;
    if !slice.is_empty() {
        return Err(SignedFrameError::Decode);
    }
    let Value::Array(elements) = value else {
        return Err(SignedFrameError::Bootstrap);
    };
    let [x25519, ed25519] = <[Value; BOOTSTRAP_KEY_COUNT]>::try_from(elements)
        .map_err(|_| SignedFrameError::Bootstrap)?;
    Ok(BootstrapPayload {
        x25519_public_key: bootstrap_key(x25519)?,
        ed25519_public_key: bootstrap_key(ed25519)?,
    })
}

fn bootstrap_key(value: Value) -> Result<[u8; BOOTSTRAP_KEY_LEN]> {
    let Value::Bytes(bytes) = value else {
        return Err(SignedFrameError::Bootstrap);
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| SignedFrameError::Bootstrap)
}

fn encode_value(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes).map_err(|_| SignedFrameError::Encode)?;
    Ok(bytes)
}
