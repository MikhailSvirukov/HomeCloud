//! Frame header for the frame-only TCP actor.
//!
//! The storage-node protocol opens every frame with a fixed unencrypted prefix
//! carrying only framing information; passwords, file names, and metadata stay
//! inside the encrypted payload. Wire layout, 5 bytes total:
//!
//! ```text
//! byte 0      protocol version (`u8`)
//! bytes 1..5  payload length, big-endian `u32`
//! ```

/// Protocol version emitted by [`FrameHeader::new`] and required by
/// [`FrameHeader::decode`].
pub const PROTOCOL_VERSION: u8 = 1;

/// Encoded length of a [`FrameHeader`] in bytes.
pub const FRAME_HEADER_LEN: usize = 5;

/// Largest accepted payload length, 16 MiB.
pub const MAX_FRAME_PAYLOAD_LEN: u32 = 16 * 1024 * 1024;

/// Decoded frame header of the storage-node wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    version: u8,
    payload_len: u32,
}

impl FrameHeader {
    /// Creates a header for [`PROTOCOL_VERSION`].
    ///
    /// # Errors
    ///
    /// [`FramingError::PayloadTooLarge`] when `payload_len` exceeds
    /// [`MAX_FRAME_PAYLOAD_LEN`].
    pub fn new(payload_len: u32) -> Result<Self, FramingError> {
        if payload_len > MAX_FRAME_PAYLOAD_LEN {
            Err(FramingError::PayloadTooLarge)
        } else {
            Ok(Self {
                version: PROTOCOL_VERSION,
                payload_len,
            })
        }
    }

    /// Protocol version carried by this header.
    pub const fn version(&self) -> u8 {
        self.version
    }

    /// Payload length in bytes.
    pub const fn payload_len(&self) -> u32 {
        self.payload_len
    }

    /// Encodes the header into its fixed-size wire representation.
    pub fn encode(&self) -> [u8; FRAME_HEADER_LEN] {
        let length = self.payload_len.to_be_bytes();
        [self.version, length[0], length[1], length[2], length[3]]
    }

    /// Decodes a header from the start of `bytes`, ignoring trailing bytes.
    ///
    /// # Errors
    ///
    /// [`FramingError::TruncatedHeader`] when `bytes` is shorter than
    /// [`FRAME_HEADER_LEN`], [`FramingError::UnsupportedVersion`] when the
    /// version byte is not [`PROTOCOL_VERSION`], or
    /// [`FramingError::PayloadTooLarge`] when the declared length exceeds
    /// [`MAX_FRAME_PAYLOAD_LEN`].
    pub fn decode(bytes: &[u8]) -> Result<Self, FramingError> {
        if bytes.len() < FRAME_HEADER_LEN {
            Err(FramingError::TruncatedHeader)
        } else if bytes[0] != PROTOCOL_VERSION {
            Err(FramingError::UnsupportedVersion)
        } else {
            let payload_len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
            if payload_len > MAX_FRAME_PAYLOAD_LEN {
                Err(FramingError::PayloadTooLarge)
            } else {
                Ok(Self {
                    version: bytes[0],
                    payload_len,
                })
            }
        }
    }
}

/// Failure modes when building or parsing a [`FrameHeader`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    /// Fewer than [`FRAME_HEADER_LEN`] bytes were supplied.
    TruncatedHeader,
    /// The wire version is not [`PROTOCOL_VERSION`].
    UnsupportedVersion,
    /// The declared payload length exceeds [`MAX_FRAME_PAYLOAD_LEN`].
    PayloadTooLarge,
}

impl std::fmt::Display for FramingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedHeader => write!(formatter, "frame header truncated"),
            Self::UnsupportedVersion => write!(formatter, "unsupported protocol version"),
            Self::PayloadTooLarge => write!(formatter, "payload length exceeds maximum"),
        }
    }
}

impl std::error::Error for FramingError {}

#[cfg(test)]
mod tests {
    use super::{
        FrameHeader, FramingError, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
    };

    #[test]
    fn encode_then_decode_round_trips() {
        let header = FrameHeader::new(1024).expect("1024 bytes is within the limit");
        let encoded = header.encode();

        assert_eq!(encoded.len(), FRAME_HEADER_LEN);
        assert_eq!(encoded[0], PROTOCOL_VERSION);
        assert_eq!(FrameHeader::decode(&encoded), Ok(header));
        assert_eq!(header.version(), PROTOCOL_VERSION);
        assert_eq!(header.payload_len(), 1024);
    }

    #[test]
    fn decode_ignores_trailing_payload_bytes() {
        let header = FrameHeader::new(2).expect("2 bytes is within the limit");
        let mut encoded = header.encode().to_vec();
        encoded.extend_from_slice(&[0xAA, 0xBB]);

        assert_eq!(FrameHeader::decode(&encoded), Ok(header));
    }

    #[test]
    fn decode_rejects_truncated_header() {
        let short = [PROTOCOL_VERSION, 0, 0];
        assert_eq!(
            FrameHeader::decode(&short),
            Err(FramingError::TruncatedHeader)
        );
    }

    #[test]
    fn decode_rejects_unsupported_version() {
        let bytes = [PROTOCOL_VERSION + 1, 0, 0, 0, 0];
        assert_eq!(
            FrameHeader::decode(&bytes),
            Err(FramingError::UnsupportedVersion)
        );
    }

    #[test]
    fn payload_length_over_limit_is_rejected() {
        let length = (MAX_FRAME_PAYLOAD_LEN + 1).to_be_bytes();
        let bytes = [PROTOCOL_VERSION, length[0], length[1], length[2], length[3]];

        assert_eq!(
            FrameHeader::decode(&bytes),
            Err(FramingError::PayloadTooLarge)
        );
        assert_eq!(
            FrameHeader::new(MAX_FRAME_PAYLOAD_LEN + 1),
            Err(FramingError::PayloadTooLarge)
        );
    }
}
