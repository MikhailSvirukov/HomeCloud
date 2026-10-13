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

use crate::codec::{decode_frame, DecodeOutcome};
use crate::framing::{FramingError, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN};

/// Largest number of bytes the buffer retains: one maximum frame (header + payload).
pub const MAX_BUFFERED_FRAME_LEN: usize = FRAME_HEADER_LEN + MAX_FRAME_PAYLOAD_LEN as usize;

/// Result alias for fallible [`FrameBuffer`] operations.
type Result<T> = core::result::Result<T, FramingError>;

/// Receive-side accumulator turning a chunked byte stream into whole frames.
#[derive(Default)]
pub struct FrameBuffer {
    buffer: Vec<u8>,
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
    /// Creates an empty buffer with no pre-allocated capacity.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty buffer that pre-allocates `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Appends `data` to the buffered bytes, leaving the buffer unchanged when
    /// it would exceed [`MAX_BUFFERED_FRAME_LEN`].
    pub fn extend(&mut self, data: &[u8]) -> Result<()> {
        if self.buffer.len() + data.len() > MAX_BUFFERED_FRAME_LEN {
            Err(FramingError::PayloadTooLarge)
        } else {
            self.buffer.extend_from_slice(data);
            Ok(())
        }
    }

    /// Removes and returns the next complete frame (header + payload) as owned
    /// bytes, or `None` when no complete frame is buffered yet.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        match decode_frame(&self.buffer)? {
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

    /// Returns `true` when no bytes are retained.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::FrameBuffer;

    #[test]
    fn debug_reports_only_buffered_length_without_bytes() {
        let mut buffer = FrameBuffer::new();
        buffer
            .extend(b"SECRET-BYTES")
            .expect("chunk fits in the buffer");

        assert_eq!(format!("{buffer:?}"), "FrameBuffer { buffered_len: 12 }");
    }
}
