//! Fixed v1 application frame layout.
//!
//! A frame is `[4-byte big-endian length prefix][external header][signature][payload]`.
//! The prefix covers the header, the signature, and the payload, excluding the
//! prefix itself.

use crate::cbor::{self, CborError};

const LENGTH_PREFIX_LEN: usize = 4;
const SIGNATURE_LEN: usize = 64;

/// A signed application frame body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The deterministic CBOR external frame header.
    pub header: Vec<u8>,
    /// The Ed25519 signature over the length prefix, header, and payload.
    pub signature: [u8; SIGNATURE_LEN],
    /// The encrypted payload, or the plaintext bootstrap payload.
    pub payload: Vec<u8>,
}

/// Reasons a frame cannot be encoded or decoded.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The declared body length exceeds the configured maximum.
    #[error("frame body is {declared} bytes, limit is {limit}")]
    BodyTooLarge {
        /// Body length declared by the frame or computed from its parts.
        declared: u32,
        /// Configured maximum frame body length (`CFG-CL-011`).
        limit: u32,
    },
    /// The input ended before the whole declared body was available.
    #[error("frame input is incomplete")]
    Incomplete,
    /// The declared body length disagrees with its header and signature parts.
    #[error("frame length disagrees with its header and signature")]
    LengthMismatch,
    /// The external header is not valid deterministic CBOR.
    #[error("invalid frame header: {0}")]
    Header(#[source] CborError),
}

/// Encodes `[4-byte BE length][header][signature][payload]`; the length covers
/// header+signature+payload and must not exceed `max_frame_len` (`CFG-CL-011`).
pub fn encode(frame: &Frame, max_frame_len: u32) -> Result<Vec<u8>, FrameError> {
    let declared = checked_body_length(frame, max_frame_len)?;
    let mut out = Vec::new();
    out.extend_from_slice(&declared.to_be_bytes());
    out.extend_from_slice(&frame.header);
    out.extend_from_slice(&frame.signature);
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

/// Decodes one frame, rejecting a declared body length above `max_frame_len`
/// before reading the body; returns the frame and consumed byte count.
pub fn decode(bytes: &[u8], max_frame_len: u32) -> Result<(Frame, usize), FrameError> {
    let prefix = bytes
        .get(..LENGTH_PREFIX_LEN)
        .ok_or(FrameError::Incomplete)?;
    let declared = u32::from_be_bytes(
        <[u8; LENGTH_PREFIX_LEN]>::try_from(prefix).map_err(|_| FrameError::Incomplete)?,
    );

    if declared > max_frame_len {
        return Err(FrameError::BodyTooLarge {
            declared,
            limit: max_frame_len,
        });
    }

    let declared_len = usize::try_from(declared).map_err(|_| FrameError::Incomplete)?;

    if declared_len < SIGNATURE_LEN {
        return Err(FrameError::LengthMismatch);
    }

    let body = bytes
        .get(LENGTH_PREFIX_LEN..)
        .and_then(|remaining| remaining.get(..declared_len))
        .ok_or(FrameError::Incomplete)?;

    let (_, header_len) = cbor::decode_prefix(body).map_err(FrameError::Header)?;
    let signature_end = header_len
        .checked_add(SIGNATURE_LEN)
        .ok_or(FrameError::LengthMismatch)?;

    if signature_end > body.len() {
        return Err(FrameError::LengthMismatch);
    }

    let signature = <[u8; SIGNATURE_LEN]>::try_from(&body[header_len..signature_end])
        .map_err(|_| FrameError::LengthMismatch)?;

    Ok((
        Frame {
            header: body[..header_len].to_vec(),
            signature,
            payload: body[signature_end..].to_vec(),
        },
        LENGTH_PREFIX_LEN + declared_len,
    ))
}

/// The exact Ed25519-signed byte range: `[length prefix][header][payload]`.
pub fn signed_message(frame: &Frame, max_frame_len: u32) -> Result<Vec<u8>, FrameError> {
    let declared = checked_body_length(frame, max_frame_len)?;
    let mut out = Vec::new();
    out.extend_from_slice(&declared.to_be_bytes());
    out.extend_from_slice(&frame.header);
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

fn checked_body_length(frame: &Frame, max_frame_len: u32) -> Result<u32, FrameError> {
    let signature_len = SIGNATURE_LEN as u32;
    let header_len = u32::try_from(frame.header.len()).ok();
    let payload_len = u32::try_from(frame.payload.len()).ok();

    let declared = header_len
        .and_then(|len| len.checked_add(signature_len))
        .and_then(|len| payload_len.and_then(|payload| len.checked_add(payload)));

    match declared {
        Some(declared) if declared <= max_frame_len => Ok(declared),
        Some(declared) => Err(FrameError::BodyTooLarge {
            declared,
            limit: max_frame_len,
        }),
        None => Err(FrameError::BodyTooLarge {
            declared: u32::MAX,
            limit: max_frame_len,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, FrameError, decode, encode, signed_message};

    fn frame(header: Vec<u8>, payload: Vec<u8>) -> Frame {
        Frame {
            header,
            signature: [0; 64],
            payload,
        }
    }

    #[test]
    fn length_prefix_is_big_endian_and_excludes_itself() {
        let encoded = encode(&frame(vec![0x01], vec![0xAA]), 128).expect("frame is valid");
        assert_eq!(&encoded[..4], &[0, 0, 0, 66]);
        assert_eq!(encoded.len(), 4 + 66);
    }

    #[test]
    fn rejects_body_above_limit() {
        let error = encode(&frame(vec![0x01], vec![0xAA]), 65).expect_err("body is too large");
        assert_eq!(
            error,
            FrameError::BodyTooLarge {
                declared: 66,
                limit: 65,
            }
        );
    }

    #[test]
    fn accepts_body_exactly_at_limit() {
        let encoded = encode(&frame(vec![0x01], vec![0xAA]), 66).expect("body is at the limit");
        assert_eq!(&encoded[..4], &[0, 0, 0, 66]);
    }

    #[test]
    fn round_trips_frame_and_reports_consumed_bytes() {
        let original = frame(vec![0x81, 0x01], vec![0x02, 0x03]);
        let encoded = encode(&original, 128).expect("frame is valid");
        let (decoded, consumed) = decode(&encoded, 128).expect("frame is valid");
        assert_eq!(decoded, original);
        assert_eq!(consumed, encoded.len());
    }

    #[test]
    fn signed_message_excludes_signature_but_keeps_body_length() {
        let signed = signed_message(&frame(vec![0x01], vec![0xAA]), 128).expect("frame is valid");
        assert_eq!(&signed[..4], &[0, 0, 0, 66]);
        assert_eq!(&signed[4..], &[0x01, 0xAA]);
    }

    #[test]
    fn decode_rejects_declared_body_shorter_than_signature() {
        assert_eq!(decode(&[0, 0, 0, 63], 128), Err(FrameError::LengthMismatch));
    }

    #[test]
    fn decode_rejects_body_above_limit_before_reading() {
        assert_eq!(
            decode(&[0, 0, 0, 10], 9),
            Err(FrameError::BodyTooLarge {
                declared: 10,
                limit: 9,
            })
        );
    }
}
