//! Length prefix for the frame-only TCP actor.
//!
//! Every frame opens with a 4-byte big-endian unsigned length prefix. The
//! value counts the payload bytes that follow the prefix and excludes the
//! prefix itself. Receivers validate the declared length against the
//! configured maximum before buffering the frame body.

use core::num::NonZeroU32;

/// Encoded length of the frame-length prefix in bytes.
pub const FRAME_LENGTH_PREFIX_LEN: usize = 4;

/// Protocol version negotiated when a connection opens; it is not part of the
/// length prefix.
pub const PROTOCOL_VERSION: u8 = 1;

type Result<T> = core::result::Result<T, FramingError>;

/// Validated payload length of one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLength(u32);

impl FrameLength {
    /// Creates a length that does not exceed `max_frame_len`.
    pub fn new(len: u32, max_frame_len: NonZeroU32) -> Result<Self> {
        if len > max_frame_len.get() {
            Err(FramingError::FrameTooLarge {
                len,
                max: max_frame_len.get(),
            })
        } else {
            Ok(Self(len))
        }
    }

    /// Payload length in bytes, excluding the prefix.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Encodes the length as a big-endian 4-byte prefix.
    pub const fn encode(self) -> [u8; FRAME_LENGTH_PREFIX_LEN] {
        self.0.to_be_bytes()
    }

    /// Decodes a big-endian 4-byte prefix that does not exceed `max_frame_len`.
    pub fn decode(bytes: [u8; FRAME_LENGTH_PREFIX_LEN], max_frame_len: NonZeroU32) -> Result<Self> {
        Self::new(u32::from_be_bytes(bytes), max_frame_len)
    }
}

/// Failure modes when validating a frame length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FramingError {
    /// The declared payload length exceeds the configured maximum.
    #[error("frame length {len} exceeds maximum {max}")]
    FrameTooLarge { len: u32, max: u32 },
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::{FrameLength, FramingError, FRAME_LENGTH_PREFIX_LEN};

    fn max_frame_len(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test limit is non-zero")
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let length =
            FrameLength::new(1024, max_frame_len(1024)).expect("length is within the limit");
        let encoded = length.encode();

        assert_eq!(encoded.len(), FRAME_LENGTH_PREFIX_LEN);
        assert_eq!(encoded, [0, 0, 0x04, 0x00]);
        assert_eq!(
            FrameLength::decode(encoded, max_frame_len(1024)),
            Ok(length)
        );
    }

    #[test]
    fn new_accepts_length_equal_to_maximum() {
        assert_eq!(FrameLength::new(16, max_frame_len(16)), Ok(FrameLength(16)));
    }

    #[test]
    fn new_rejects_length_above_maximum() {
        assert_eq!(
            FrameLength::new(17, max_frame_len(16)),
            Err(FramingError::FrameTooLarge { len: 17, max: 16 })
        );
    }

    #[test]
    fn decode_rejects_length_above_maximum() {
        assert_eq!(
            FrameLength::decode(17u32.to_be_bytes(), max_frame_len(16)),
            Err(FramingError::FrameTooLarge { len: 17, max: 16 })
        );
    }
}
