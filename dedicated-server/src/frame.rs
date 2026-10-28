//! Application frame codec.
//!
//! Every message on the protocol is a 4-byte unsigned big-endian length prefix
//! followed by exactly that many payload bytes (`INT-NET-002`). The prefix
//! counts only the bytes after itself (`INT-NET-003`), and a receiver checks
//! its configured maximum against the announced length before allocating
//! memory for the payload (`INT-NET-004`).
//!
//! The codec is transport-agnostic: it works over any [`Read`] or [`Write`]
//! implementation, so the TCP listener and the tests share the same framing.

use std::io::{self, Read, Write};
use std::num::NonZeroU32;

/// Number of bytes in the frame length prefix.
pub const FRAME_PREFIX_BYTES: usize = 4;

/// Errors produced while encoding or decoding a frame.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The announced frame length exceeds the configured maximum.
    ///
    /// The receiver returns this before allocating the payload buffer.
    #[error("announced frame of {announced} bytes exceeds the configured maximum of {max} bytes")]
    TooLarge {
        /// Length announced by the peer, excluding the prefix.
        announced: u32,
        /// Configured maximum payload length.
        max: u32,
    },
    /// A payload does not fit into the 32-bit length prefix.
    #[error("payload of {bytes} bytes does not fit the 4-byte length prefix")]
    PayloadTooLarge {
        /// Actual payload length.
        bytes: usize,
    },
    /// The stream ended or failed while reading a frame.
    #[error("failed to read a frame: {0}")]
    Io(#[from] io::Error),
}

/// Write one frame: the big-endian length prefix followed by the payload.
///
/// The prefix counts only the payload bytes. The encoder does not apply the
/// configured receive limit; that limit belongs to the receiver.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<(), FrameError> {
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::PayloadTooLarge {
        bytes: payload.len(),
    })?;

    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(payload)?;

    Ok(())
}

/// Read one frame, rejecting an oversized announcement before allocating.
///
/// A zero-length frame is a valid frame; the message layer decides whether an
/// empty payload is meaningful.
pub fn read_frame<R: Read>(
    reader: &mut R,
    max_frame_bytes: NonZeroU32,
) -> Result<Vec<u8>, FrameError> {
    let mut prefix = [0_u8; FRAME_PREFIX_BYTES];
    reader.read_exact(&mut prefix)?;

    let announced = u32::from_be_bytes(prefix);
    if announced > max_frame_bytes.get() {
        return Err(FrameError::TooLarge {
            announced,
            max: max_frame_bytes.get(),
        });
    }

    let mut payload = vec![0_u8; announced as usize];
    reader.read_exact(&mut payload)?;

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn maximum(bytes: u32) -> NonZeroU32 {
        NonZeroU32::new(bytes).expect("the maximum must be non-zero")
    }

    #[test]
    fn the_prefix_is_four_bytes_big_endian_and_excludes_itself() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, b"abc").expect("the frame must be written");

        assert_eq!(encoded, [0, 0, 0, 3, b'a', b'b', b'c']);
        assert_eq!(encoded.len(), FRAME_PREFIX_BYTES + 3);
    }

    #[test]
    fn a_frame_round_trips() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, b"payload").expect("the frame must be written");

        let mut reader = Cursor::new(encoded);
        let payload = read_frame(&mut reader, maximum(1_024)).expect("the frame must be read");

        assert_eq!(payload, b"payload");
    }

    #[test]
    fn a_frame_at_the_configured_maximum_is_accepted() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &[7_u8; 16]).expect("the frame must be written");

        let mut reader = Cursor::new(encoded);
        let payload = read_frame(&mut reader, maximum(16)).expect("the limit is inclusive");

        assert_eq!(payload.len(), 16);
    }

    #[test]
    fn an_oversized_frame_is_rejected_before_the_payload_is_read() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, &[9_u8; 32]).expect("the frame must be written");

        let mut reader = Cursor::new(encoded);
        let error = read_frame(&mut reader, maximum(16)).expect_err("the frame is too large");

        assert!(matches!(
            error,
            FrameError::TooLarge {
                announced: 32,
                max: 16
            }
        ));
        assert_eq!(
            reader.position() as usize,
            FRAME_PREFIX_BYTES,
            "only the prefix is consumed; no payload is allocated or read"
        );
    }

    #[test]
    fn a_zero_length_frame_round_trips() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, b"").expect("the frame must be written");

        let mut reader = Cursor::new(encoded);
        let payload = read_frame(&mut reader, maximum(16)).expect("the frame must be read");

        assert!(payload.is_empty());
    }

    #[test]
    fn a_truncated_prefix_is_an_io_error() {
        let mut reader = Cursor::new(vec![0_u8, 0, 0]);
        let error = read_frame(&mut reader, maximum(16)).expect_err("the prefix is incomplete");

        assert!(matches!(error, FrameError::Io(_)));
    }

    #[test]
    fn a_truncated_payload_is_an_io_error() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, b"abcd").expect("the frame must be written");
        encoded.truncate(encoded.len() - 1);

        let mut reader = Cursor::new(encoded);
        let error = read_frame(&mut reader, maximum(16)).expect_err("the payload is incomplete");

        assert!(matches!(error, FrameError::Io(_)));
    }
}
