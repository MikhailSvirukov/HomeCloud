//! Receive-side frame accumulator for the frame-only TCP actor.
//!
//! A TCP stream delivers arbitrary byte chunks, so a frame can arrive split
//! across reads or bundled with following frames. [`FrameBuffer`] accumulates
//! those chunks and hands whole frames back to the caller, leaving any bytes
//! past the current frame buffered for the next call.
//!
//! Callers must drain complete frames by calling [`FrameBuffer::next_frame`]
//! until it returns `None`; only then is the buffer guaranteed to hold at most
//! one maximum frame. Ignoring that contract lets the buffer grow without
//! bound and eventually makes [`FrameBuffer::extend`] fail.

use core::num::NonZeroU32;

use crate::codec::{decode_frame, DecodeOutcome};
use crate::framing::{FramingError, FRAME_LENGTH_PREFIX_LEN};

/// Result alias for fallible [`FrameBuffer`] operations.
type Result<T> = core::result::Result<T, FramingError>;

/// Receive-side accumulator turning a chunked byte stream into whole frames.
pub struct FrameBuffer {
    buffer: Vec<u8>,
    max_frame_len: NonZeroU32,
}

impl std::fmt::Debug for FrameBuffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FrameBuffer")
            .field("buffered_len", &self.buffered_len())
            .finish()
    }
}

impl FrameBuffer {
    /// Creates an empty buffer for frames whose payloads do not exceed
    /// `max_frame_len`.
    pub fn new(max_frame_len: NonZeroU32) -> Self {
        Self {
            buffer: Vec::new(),
            max_frame_len,
        }
    }

    /// Appends `data` to the buffered bytes, leaving the buffer unchanged when
    /// it would exceed one maximum frame (prefix plus payload).
    pub fn extend(&mut self, data: &[u8]) -> Result<()> {
        let total = self.buffer.len().saturating_add(data.len());
        if total > self.max_buffered_len() {
            return Err(FramingError::FrameTooLarge {
                len: u32::try_from(total.saturating_sub(FRAME_LENGTH_PREFIX_LEN))
                    .unwrap_or(u32::MAX),
                max: self.max_frame_len.get(),
            });
        }

        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Removes and returns the next complete frame (prefix plus payload) as
    /// owned bytes, or `None` when no complete frame is buffered yet.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        match decode_frame(&self.buffer, self.max_frame_len)? {
            DecodeOutcome::Complete { consumed, .. } => {
                let frame = self.buffer[..consumed].to_vec();
                self.buffer.drain(..consumed);
                Ok(Some(frame))
            }
            DecodeOutcome::Incomplete { .. } => Ok(None),
        }
    }

    /// Number of bytes currently retained.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// Number of bytes that can still be appended before the buffer holds one
    /// maximum frame.
    pub fn remaining_capacity(&self) -> usize {
        self.max_buffered_len().saturating_sub(self.buffer.len())
    }

    /// Returns `true` when no bytes are retained.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn max_buffered_len(&self) -> usize {
        FRAME_LENGTH_PREFIX_LEN.saturating_add(self.max_frame_len.get() as usize)
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::FrameBuffer;
    use crate::framing::FramingError;

    fn max_frame_len(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test limit is non-zero")
    }

    #[test]
    fn debug_reports_buffered_length_without_payload_bytes() {
        let mut buffer = FrameBuffer::new(max_frame_len(64));
        buffer
            .extend(b"SECRET-PAYLOAD-BYTES")
            .expect("chunk fits in the buffer");

        let text = format!("{buffer:?}");

        assert!(text.contains(&format!("buffered_len: {}", buffer.buffered_len())));
        assert!(!text.contains("SECRET-PAYLOAD-BYTES"));
    }

    #[test]
    fn extend_rejects_data_beyond_capacity_without_changing_buffer() {
        let mut buffer = FrameBuffer::new(max_frame_len(4));
        buffer
            .extend(&[0; 8])
            .expect("a maximum frame fits in the buffer");

        assert_eq!(buffer.remaining_capacity(), 0);
        assert_eq!(
            buffer.extend(&[0]),
            Err(FramingError::FrameTooLarge { len: 5, max: 4 })
        );
        assert_eq!(buffer.buffered_len(), 8);
    }
}
