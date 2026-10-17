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
    use super::DecodeOutcome;
    use crate::framing::FRAME_LENGTH_PREFIX_LEN;

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
