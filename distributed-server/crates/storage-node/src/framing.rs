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
