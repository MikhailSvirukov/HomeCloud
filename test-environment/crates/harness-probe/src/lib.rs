//! Bounded TCP wire probe for negative protocol scenarios (`TST-021`).
//!
//! The probe sends raw bytes, reads one length-prefixed frame with a configured
//! body limit, and asserts that a peer closes the connection without sending an
//! application response. It performs no cryptography and no interpretation of
//! the decrypted payload.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use harness_wire::frame::{self, Frame, FrameError};

const LENGTH_PREFIX_LEN: usize = 4;

/// A bounded TCP connection to one component endpoint.
pub struct WireProbe {
    stream: TcpStream,
    timeout: Duration,
}

/// Reasons a probe operation failed or observed an unexpected peer response.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// Connecting to the endpoint failed.
    #[error("connecting to {address} failed: {source}")]
    Connect {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    /// Writing to the endpoint failed.
    #[error("writing to the endpoint failed: {0}")]
    Write(#[source] std::io::Error),
    /// Reading from the endpoint failed.
    #[error("reading from the endpoint failed: {0}")]
    Read(#[source] std::io::Error),
    /// The peer neither answered nor closed within the bound.
    #[error("the peer did not answer within {0:?}")]
    Timeout(Duration),
    /// The peer sent an unexpected byte instead of closing.
    #[error("the peer sent an unexpected byte 0x{0:02X} instead of closing")]
    UnexpectedData(u8),
    /// The peer sent a frame that is not valid under the published contract.
    #[error("the peer sent an invalid frame: {0}")]
    InvalidFrame(#[source] FrameError),
}

impl WireProbe {
    /// Connects with the given timeout for connect, read and write operations.
    ///
    /// A connect-phase timeout is reported as [`ProbeError::Connect`] so the
    /// failing endpoint is named; [`ProbeError::Timeout`] is reserved for read
    /// and write operations on an established connection.
    pub fn connect(address: SocketAddr, timeout: Duration) -> Result<Self, ProbeError> {
        let stream = TcpStream::connect_timeout(&address, timeout)
            .map_err(|source| ProbeError::Connect { address, source })?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(ProbeError::Read)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(ProbeError::Write)?;

        Ok(Self { stream, timeout })
    }

    /// Sends exactly the given bytes.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), ProbeError> {
        let timeout = self.timeout;
        self.stream
            .write_all(bytes)
            .map_err(|error| write_failure(error, timeout))
    }

    /// Encodes `frame` with `max_frame_len` and sends it.
    pub fn send_frame(&mut self, frame: &Frame, max_frame_len: u32) -> Result<(), ProbeError> {
        let bytes = frame::encode(frame, max_frame_len).map_err(ProbeError::InvalidFrame)?;
        self.send(&bytes)
    }

    /// Reads one frame. The 4-byte big-endian prefix is rejected above
    /// `max_frame_len` before the body is read.
    pub fn read_frame(&mut self, max_frame_len: u32) -> Result<Frame, ProbeError> {
        self.stream
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Read)?;

        let mut prefix = [0u8; LENGTH_PREFIX_LEN];
        self.read_exact_bounded(&mut prefix)?;
        let declared = u32::from_be_bytes(prefix);

        if declared > max_frame_len {
            return Err(ProbeError::InvalidFrame(FrameError::BodyTooLarge {
                declared,
                limit: max_frame_len,
            }));
        }

        let body_len = usize::try_from(declared)
            .map_err(|_| ProbeError::InvalidFrame(FrameError::Incomplete))?;
        let mut body = vec![0u8; body_len];
        self.read_exact_bounded(&mut body)?;

        let mut bytes = Vec::with_capacity(LENGTH_PREFIX_LEN);
        bytes.extend_from_slice(&prefix);
        bytes.extend_from_slice(&body);

        let (frame, _consumed) =
            frame::decode(&bytes, max_frame_len).map_err(ProbeError::InvalidFrame)?;
        Ok(frame)
    }

    /// Asserts the peer closes the connection without sending any byte.
    pub fn expect_closed(&mut self, timeout: Duration) -> Result<(), ProbeError> {
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(ProbeError::Read)?;

        let mut byte = [0u8; 1];
        let outcome = match self.stream.read(&mut byte) {
            Ok(0) => Ok(()),
            Ok(_) => Err(ProbeError::UnexpectedData(byte[0])),
            // A reset, aborted, broken, or EOF read means the peer closed
            // without an application response.
            Err(error) if is_connection_closed(error.kind()) => Ok(()),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Err(ProbeError::Timeout(timeout))
            }
            Err(error) => Err(ProbeError::Read(error)),
        };

        self.stream
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Read)?;
        outcome
    }

    fn read_exact_bounded(&mut self, buffer: &mut [u8]) -> Result<(), ProbeError> {
        let timeout = self.timeout;
        self.stream
            .read_exact(buffer)
            .map_err(|error| read_failure(error, timeout))
    }
}

fn read_failure(source: std::io::Error, timeout: Duration) -> ProbeError {
    match source.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => ProbeError::Timeout(timeout),
        _ => ProbeError::Read(source),
    }
}

fn write_failure(source: std::io::Error, timeout: Duration) -> ProbeError {
    match source.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => ProbeError::Timeout(timeout),
        _ => ProbeError::Write(source),
    }
}

fn is_connection_closed(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::{ProbeError, is_connection_closed, write_failure};
    use std::io::ErrorKind;
    use std::time::Duration;

    #[test]
    fn classifies_connection_closed_error_kinds() {
        for kind in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
            ErrorKind::UnexpectedEof,
        ] {
            assert!(is_connection_closed(kind));
        }

        assert!(!is_connection_closed(ErrorKind::ConnectionRefused));
        assert!(!is_connection_closed(ErrorKind::TimedOut));
    }

    #[test]
    fn maps_write_timeouts_and_failures() {
        let timeout = Duration::from_secs(1);

        assert!(matches!(
            write_failure(std::io::Error::from(ErrorKind::TimedOut), timeout),
            ProbeError::Timeout(observed) if observed == timeout
        ));
        assert!(matches!(
            write_failure(std::io::Error::from(ErrorKind::WouldBlock), timeout),
            ProbeError::Timeout(observed) if observed == timeout
        ));
        assert!(matches!(
            write_failure(std::io::Error::from(ErrorKind::BrokenPipe), timeout),
            ProbeError::Write(_)
        ));
    }
}
