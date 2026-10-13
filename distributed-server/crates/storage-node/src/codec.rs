//! Whole-frame encode/decode over byte slices for the frame-only TCP actor.
//!
//! [`encode_frame`] serialises one frame as its [`FRAME_HEADER_LEN`]-byte header
//! followed by the payload bytes. [`decode_frame`] reads one frame from the
//! front of a byte buffer without consuming it: any trailing bytes belong to the
//! next frame and are reported as unconsumed through [`DecodeOutcome`].

use std::fmt;

use crate::framing::{FrameHeader, FramingError, FRAME_HEADER_LEN};

/// Result of decoding one frame from the front of a byte buffer.
#[derive(PartialEq, Eq)]
pub enum DecodeOutcome<'buf> {
    /// A complete frame was read: its header, its payload bytes, and the total
    /// number of buffer bytes it occupied (header plus payload).
    Complete {
        header: FrameHeader,
        payload: &'buf [u8],
        consumed: usize,
    },
    /// The buffer does not yet hold a complete frame; `needed` is the total
    /// buffer length required before `decode_frame` can make progress.
    Incomplete { needed: usize },
}

impl fmt::Debug for DecodeOutcome<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete {
                header,
                payload,
                consumed,
            } => formatter
                .debug_struct("Complete")
                .field("header", header)
                .field("payload_len", &payload.len())
                .field("consumed", consumed)
                .finish(),
            Self::Incomplete { needed } => formatter
                .debug_struct("Incomplete")
                .field("needed", needed)
                .finish(),
        }
    }
}

/// Encodes a frame as its 5-byte header followed by `payload`.
///
/// # Errors
///
/// [`FramingError::PayloadTooLarge`] when `payload` is longer than
/// [`crate::MAX_FRAME_PAYLOAD_LEN`] or does not fit in the header's length
/// field.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, FramingError> {
    let payload_len = u32::try_from(payload.len()).map_err(|_| FramingError::PayloadTooLarge)?;
    let header = FrameHeader::new(payload_len)?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(&header.encode());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decodes one frame from the front of `bytes`, ignoring any trailing bytes.
///
/// The returned [`DecodeOutcome::Complete`] reports how many bytes the frame
/// occupied as `consumed`; bytes beyond `consumed` are left untouched and
/// belong to the next frame.
///
/// # Errors
///
/// Propagates [`FramingError`] from [`FrameHeader::decode`] for an unsupported
/// version or an over-limit declared length.
pub fn decode_frame(bytes: &[u8]) -> Result<DecodeOutcome<'_>, FramingError> {
    if bytes.len() < FRAME_HEADER_LEN {
        Ok(DecodeOutcome::Incomplete {
            needed: FRAME_HEADER_LEN,
        })
    } else {
        let header = FrameHeader::decode(&bytes[..FRAME_HEADER_LEN])?;
        let total = FRAME_HEADER_LEN + header.payload_len() as usize;

        if bytes.len() < total {
            Ok(DecodeOutcome::Incomplete { needed: total })
        } else {
            Ok(DecodeOutcome::Complete {
                header,
                payload: &bytes[FRAME_HEADER_LEN..total],
                consumed: total,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_frame, encode_frame, DecodeOutcome};
    use crate::framing::{
        FrameHeader, FramingError, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
    };

    #[test]
    fn encode_then_decode_round_trips_header_payload_and_consumed() {
        let payload = b"hello frame".as_slice();
        let encoded = encode_frame(payload).expect("small payload is within the limit");

        assert_eq!(
            decode_frame(&encoded),
            Ok(DecodeOutcome::Complete {
                header: FrameHeader::new(payload.len() as u32)
                    .expect("small payload is within the limit"),
                payload,
                consumed: FRAME_HEADER_LEN + payload.len(),
            })
        );
    }

    #[test]
    fn complete_decode_ignores_trailing_bytes() {
        let first = encode_frame(b"one").expect("small payload is within the limit");
        let mut buffer = first.clone();
        buffer.extend_from_slice(b"two");

        let outcome = decode_frame(&buffer).expect("frame is complete");

        assert_eq!(
            outcome,
            DecodeOutcome::Complete {
                header: FrameHeader::new(3).expect("3 bytes is within the limit"),
                payload: b"one".as_slice(),
                consumed: FRAME_HEADER_LEN + 3,
            }
        );
    }

    #[test]
    fn empty_payload_round_trips() {
        let encoded = encode_frame(&[]).expect("empty payload is within the limit");

        assert_eq!(
            decode_frame(&encoded),
            Ok(DecodeOutcome::Complete {
                header: FrameHeader::new(0).expect("0 bytes is within the limit"),
                payload: &[],
                consumed: FRAME_HEADER_LEN,
            })
        );
    }

    #[test]
    fn buffer_shorter_than_header_is_incomplete() {
        let short = [PROTOCOL_VERSION, 0, 0];

        assert_eq!(
            decode_frame(&short),
            Ok(DecodeOutcome::Incomplete {
                needed: FRAME_HEADER_LEN,
            })
        );
    }

    #[test]
    fn header_with_missing_payload_is_incomplete() {
        let encoded = encode_frame(b"four").expect("small payload is within the limit");
        let partial = &encoded[..FRAME_HEADER_LEN + 2];
        let total = FRAME_HEADER_LEN + 4;

        assert_eq!(
            decode_frame(partial),
            Ok(DecodeOutcome::Incomplete { needed: total })
        );
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let bytes = [PROTOCOL_VERSION + 1, 0, 0, 0, 0];

        assert_eq!(decode_frame(&bytes), Err(FramingError::UnsupportedVersion));
    }

    #[test]
    fn encode_rejects_payload_over_limit() {
        let oversized = vec![0u8; MAX_FRAME_PAYLOAD_LEN as usize + 1];

        assert_eq!(encode_frame(&oversized), Err(FramingError::PayloadTooLarge));
    }

    #[test]
    fn debug_reports_payload_length_without_payload_bytes() {
        let payload = b"SECRET-PAYLOAD".as_slice();
        let outcome = DecodeOutcome::Complete {
            header: FrameHeader::new(payload.len() as u32)
                .expect("small payload is within the limit"),
            payload,
            consumed: FRAME_HEADER_LEN + payload.len(),
        };

        let text = format!("{outcome:?}");

        assert!(text.contains("payload_len: 14"));
        assert!(!text.contains("SECRET"));
    }
}
