//! Whole-frame encode/decode over byte slices for the frame-only TCP actor.
//!
//! [`encode_frame`] serialises one frame as its [`FRAME_HEADER_LEN`]-byte header
//! followed by the payload bytes. [`decode_frame`] reads one frame from the
//! front of a byte buffer without consuming it: any trailing bytes belong to the
//! next frame and are reported as unconsumed through [`DecodeOutcome`].

use std::fmt;

use crate::framing::{FrameHeader, FramingError, FRAME_HEADER_LEN};

type Result<T> = core::result::Result<T, FramingError>;

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
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>> {
    let payload_len = u32::try_from(payload.len()).map_err(|_| FramingError::PayloadTooLarge)?;
    let header = FrameHeader::new(payload_len)?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(&header.encode());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decodes one frame from the front of `bytes`, ignoring any trailing bytes;
/// [`DecodeOutcome::Complete`] reports the consumed length so bytes beyond it
/// remain available for the next frame.
pub fn decode_frame(bytes: &[u8]) -> Result<DecodeOutcome<'_>> {
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
    use crate::framing::{FrameHeader, FRAME_HEADER_LEN};

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
