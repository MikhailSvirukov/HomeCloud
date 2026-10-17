//! Whole-frame encode/decode over byte slices for the frame-only TCP actor.
//!
//! [`encode_frame`] serialises one frame as its 4-byte big-endian payload-length
//! prefix followed by the payload bytes. [`decode_frame`] reads one frame from
//! the front of a byte buffer without consuming it: any trailing bytes belong to
//! the next frame and are reported as unconsumed through [`DecodeOutcome`].

use core::num::NonZeroU32;
use std::fmt;

use crate::framing::{FrameLength, FramingError, FRAME_LENGTH_PREFIX_LEN};

type Result<T> = core::result::Result<T, FramingError>;

/// Result of decoding one frame from the front of a byte buffer.
#[derive(PartialEq, Eq)]
pub enum DecodeOutcome<'buf> {
    /// A complete frame was read: its payload bytes and the total number of
    /// buffer bytes it occupied (prefix plus payload).
    Complete {
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
            Self::Complete { payload, consumed } => formatter
                .debug_struct("Complete")
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

/// Encodes a frame as its 4-byte big-endian length prefix followed by `payload`.
pub fn encode_frame(payload: &[u8], max_frame_len: NonZeroU32) -> Result<Vec<u8>> {
    let len = u32::try_from(payload.len()).map_err(|_| FramingError::FrameTooLarge {
        len: u32::MAX,
        max: max_frame_len.get(),
    })?;
    let length = FrameLength::new(len, max_frame_len)?;

    let mut frame = Vec::with_capacity(FRAME_LENGTH_PREFIX_LEN + payload.len());
    frame.extend_from_slice(&length.encode());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decodes one frame from the front of `bytes`, rejecting a declared payload
/// length above `max_frame_len` before waiting for the payload; trailing bytes
/// beyond the returned `consumed` length belong to the next frame.
pub fn decode_frame(bytes: &[u8], max_frame_len: NonZeroU32) -> Result<DecodeOutcome<'_>> {
    if bytes.len() < FRAME_LENGTH_PREFIX_LEN {
        return Ok(DecodeOutcome::Incomplete {
            needed: FRAME_LENGTH_PREFIX_LEN,
        });
    }

    let length = FrameLength::decode([bytes[0], bytes[1], bytes[2], bytes[3]], max_frame_len)?;
    let total = FRAME_LENGTH_PREFIX_LEN.saturating_add(length.get() as usize);

    if bytes.len() < total {
        Ok(DecodeOutcome::Incomplete { needed: total })
    } else {
        Ok(DecodeOutcome::Complete {
            payload: &bytes[FRAME_LENGTH_PREFIX_LEN..total],
            consumed: total,
        })
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::{decode_frame, encode_frame, DecodeOutcome};
    use crate::framing::{FramingError, FRAME_LENGTH_PREFIX_LEN};

    fn max_frame_len(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test limit is non-zero")
    }

    #[test]
    fn empty_payload_round_trips() {
        let encoded = encode_frame(&[], max_frame_len(16)).expect("empty payload fits");

        assert_eq!(encoded, [0, 0, 0, 0]);
        assert_eq!(
            decode_frame(&encoded, max_frame_len(16)),
            Ok(DecodeOutcome::Complete {
                payload: &[],
                consumed: FRAME_LENGTH_PREFIX_LEN,
            })
        );
    }

    #[test]
    fn prefix_counts_payload_bytes_excluding_itself() {
        let encoded = encode_frame(b"abc", max_frame_len(16)).expect("payload fits");

        assert_eq!(&encoded[..FRAME_LENGTH_PREFIX_LEN], &[0, 0, 0, 3]);
        assert_eq!(encoded.len(), FRAME_LENGTH_PREFIX_LEN + 3);
    }

    #[test]
    fn incomplete_prefix_reports_prefix_length() {
        assert_eq!(
            decode_frame(&[0, 0, 1], max_frame_len(16)),
            Ok(DecodeOutcome::Incomplete {
                needed: FRAME_LENGTH_PREFIX_LEN,
            })
        );
    }

    #[test]
    fn incomplete_payload_reports_total_length() {
        let bytes = [0, 0, 0, 4, 0xAA, 0xBB];

        assert_eq!(
            decode_frame(&bytes, max_frame_len(4)),
            Ok(DecodeOutcome::Incomplete {
                needed: FRAME_LENGTH_PREFIX_LEN + 4,
            })
        );
    }

    #[test]
    fn oversized_length_is_rejected_before_payload_arrives() {
        assert_eq!(
            decode_frame(&[0, 0, 0, 5], max_frame_len(4)),
            Err(FramingError::FrameTooLarge { len: 5, max: 4 })
        );
    }

    #[test]
    fn trailing_bytes_are_left_for_the_next_frame() {
        let bytes = [0, 0, 0, 1, 0xAA, 0, 0, 0, 1, 0xBB];

        assert_eq!(
            decode_frame(&bytes, max_frame_len(4)),
            Ok(DecodeOutcome::Complete {
                payload: &[0xAA],
                consumed: FRAME_LENGTH_PREFIX_LEN + 1,
            })
        );
    }

    #[test]
    fn debug_reports_payload_length_without_payload_bytes() {
        let payload = b"SECRET-PAYLOAD".as_slice();
        let outcome = DecodeOutcome::Complete {
            payload,
            consumed: FRAME_LENGTH_PREFIX_LEN + payload.len(),
        };

        let text = format!("{outcome:?}");

        assert!(text.contains("payload_len: 14"));
        assert!(!text.contains("SECRET"));
    }
}
