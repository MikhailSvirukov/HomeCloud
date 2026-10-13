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

/// Receive-side accumulator turning a chunked byte stream into whole frames.
#[derive(Debug, Default)]
pub struct FrameBuffer {
    buffer: Vec<u8>,
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

    /// Appends `data` to the buffered bytes.
    ///
    /// # Errors
    ///
    /// [`FramingError::PayloadTooLarge`] when the retained bytes would exceed
    /// [`MAX_BUFFERED_FRAME_LEN`]; the buffer is left unchanged in that case.
    pub fn extend(&mut self, data: &[u8]) -> Result<(), FramingError> {
        if self.buffer.len() + data.len() > MAX_BUFFERED_FRAME_LEN {
            Err(FramingError::PayloadTooLarge)
        } else {
            self.buffer.extend_from_slice(data);
            Ok(())
        }
    }

    /// Removes and returns the next complete frame (header + payload) as owned
    /// bytes, or `None` when no complete frame is buffered yet.
    ///
    /// # Errors
    ///
    /// Propagates [`FramingError`] from [`decode_frame`] for a frame that can
    /// never be valid (unsupported version, declared length over the limit).
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FramingError> {
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
    use super::{FrameBuffer, MAX_BUFFERED_FRAME_LEN};
    use crate::codec::encode_frame;
    use crate::framing::{FramingError, FRAME_HEADER_LEN, PROTOCOL_VERSION};

    fn frame(payload: &[u8]) -> Vec<u8> {
        encode_frame(payload).expect("test payload is within the frame limit")
    }

    #[test]
    fn new_buffer_is_empty() {
        let buffer = FrameBuffer::new();

        assert!(buffer.is_empty());
        assert_eq!(buffer.buffered_len(), 0);
    }

    #[test]
    fn with_capacity_starts_empty() {
        let buffer = FrameBuffer::with_capacity(1024);

        assert!(buffer.is_empty());
        assert_eq!(buffer.buffered_len(), 0);
    }

    #[test]
    fn next_frame_on_empty_buffer_returns_none() {
        let mut buffer = FrameBuffer::new();

        assert_eq!(buffer.next_frame(), Ok(None));
    }

    #[test]
    fn frame_is_emitted_only_after_the_completing_chunk() {
        let encoded = frame(b"hello");
        let split = FRAME_HEADER_LEN - 2;
        let (first, second) = encoded.split_at(split);
        let mut buffer = FrameBuffer::new();

        buffer.extend(first).expect("chunk fits in the buffer");
        assert_eq!(buffer.next_frame(), Ok(None));
        assert_eq!(buffer.buffered_len(), split);

        buffer.extend(second).expect("chunk fits in the buffer");
        assert_eq!(buffer.next_frame(), Ok(Some(encoded)));
        assert!(buffer.is_empty());
    }

    #[test]
    fn two_frames_are_yielded_one_at_a_time_in_order() {
        let first = frame(b"one");
        let second = frame(b"two");
        let mut stream = first.clone();
        stream.extend_from_slice(&second);
        let mut buffer = FrameBuffer::new();

        buffer.extend(&stream).expect("chunks fit in the buffer");

        assert_eq!(buffer.next_frame(), Ok(Some(first)));
        assert_eq!(buffer.next_frame(), Ok(Some(second)));
        assert_eq!(buffer.next_frame(), Ok(None));
        assert!(buffer.is_empty());
    }

    #[test]
    fn partial_frame_reports_growth_and_yields_none() {
        let encoded = frame(b"payload");
        let partial = &encoded[..FRAME_HEADER_LEN + 2];
        let mut buffer = FrameBuffer::new();

        buffer.extend(partial).expect("chunk fits in the buffer");

        assert_eq!(buffer.buffered_len(), FRAME_HEADER_LEN + 2);
        assert_eq!(buffer.next_frame(), Ok(None));
        assert_eq!(buffer.buffered_len(), FRAME_HEADER_LEN + 2);
    }

    #[test]
    fn extend_beyond_max_is_rejected_without_touching_the_buffer() {
        let mut buffer = FrameBuffer::new();
        buffer
            .extend(&[0u8; FRAME_HEADER_LEN])
            .expect("small chunk");
        let oversized = vec![0u8; MAX_BUFFERED_FRAME_LEN];

        let result = buffer.extend(&oversized);

        assert_eq!(result, Err(FramingError::PayloadTooLarge));
        assert_eq!(buffer.buffered_len(), FRAME_HEADER_LEN);
    }

    #[test]
    fn unsupported_version_header_errors_and_keeps_the_buffer() {
        let bytes = [PROTOCOL_VERSION + 1, 0, 0, 0, 0];
        let mut buffer = FrameBuffer::new();
        buffer.extend(&bytes).expect("small chunk");

        assert_eq!(buffer.next_frame(), Err(FramingError::UnsupportedVersion));
        assert_eq!(buffer.buffered_len(), FRAME_HEADER_LEN);
    }
}
