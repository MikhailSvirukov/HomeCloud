//! Frame-only async connection for the TCP actor.
//!
//! [`FrameConnection`] adapts an asynchronous byte stream into a stream of whole
//! protocol frames. It owns only the framing concern: reads are accumulated into
//! a [`FrameBuffer`] until a complete frame is available, and writes encode their
//! payload with [`encode_frame`] before handing the bytes to the stream. All
//! authentication, command dispatch, and payload interpretation live in the
//! actors layered above this type, so this type holds no session state and can be
//! composed into a larger actor without leaking wire details upward.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::codec::encode_frame;
use crate::frame_buffer::FrameBuffer;
use crate::framing::FramingError;

/// Number of bytes reserved for each read from the underlying stream.
const READ_CHUNK_LEN: usize = 16 * 1024;

/// Failure modes of the frame-only connection.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    /// The underlying stream failed to read or write.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// A frame was malformed or could not be encoded.
    #[error(transparent)]
    Framing(#[from] FramingError),
}

type Result<T> = core::result::Result<T, ConnectionError>;

/// Frame-only TCP connection: it reads and writes whole frames and leaves all
/// authentication and command handling to the actors above it.
pub struct FrameConnection<S> {
    stream: S,
    buffer: FrameBuffer,
    read_chunk: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> FrameConnection<S> {
    /// Wraps `stream`, pre-allocating the read chunk.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: FrameBuffer::new(),
            read_chunk: vec![0; READ_CHUNK_LEN],
        }
    }

    /// Reads until one complete frame is buffered and returns its bytes, or
    /// `None` at end of stream.
    pub async fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            if let Some(frame) = self.buffer.next_frame()? {
                return Ok(Some(frame));
            }

            let read = self.stream.read(&mut self.read_chunk).await?;
            if read == 0 {
                return Ok(None);
            }

            self.buffer.extend(&self.read_chunk[..read])?;
        }
    }

    /// Encodes `payload` as one frame and writes it out.
    pub async fn send_frame(&mut self, payload: &[u8]) -> Result<()> {
        let frame = encode_frame(payload)?;
        self.stream.write_all(&frame).await?;
        Ok(())
    }

    /// Returns the wrapped stream.
    pub fn into_inner(self) -> S {
        self.stream
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionError, FrameConnection};
    use crate::codec::encode_frame;
    use crate::framing::{FramingError, MAX_FRAME_PAYLOAD_LEN};
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn send_frame_is_returned_whole_by_next_frame() {
        let (writer_half, reader_half) = duplex(64);
        let mut writer = FrameConnection::new(writer_half);
        let mut reader = FrameConnection::new(reader_half);
        let payload = b"ping";

        writer
            .send_frame(payload)
            .await
            .expect("in-memory duplex write cannot fail");

        let frame = reader
            .next_frame()
            .await
            .expect("in-memory duplex read cannot fail")
            .expect("a whole frame was written");

        assert_eq!(
            frame,
            encode_frame(payload).expect("payload is within the limit")
        );
    }

    #[tokio::test]
    async fn next_frame_returns_none_after_peer_drop() {
        let (peer, stream) = duplex(64);
        let mut connection = FrameConnection::new(stream);
        drop(peer);

        let frame = connection
            .next_frame()
            .await
            .expect("read from a closed duplex cannot fail");

        assert!(frame.is_none());
    }

    #[tokio::test]
    async fn frame_split_across_writes_is_reassembled() {
        let payload = b"split across writes";
        let encoded = encode_frame(payload).expect("payload is within the limit");
        let midpoint = encoded.len() / 2;
        let head = encoded[..midpoint].to_vec();
        let tail = encoded[midpoint..].to_vec();

        let (mut writer_half, reader_half) = duplex(4);
        let mut reader = FrameConnection::new(reader_half);

        let writer = tokio::spawn(async move {
            writer_half
                .write_all(&head)
                .await
                .expect("in-memory duplex write cannot fail");
            writer_half
                .write_all(&tail)
                .await
                .expect("in-memory duplex write cannot fail");
        });

        let frame = reader
            .next_frame()
            .await
            .expect("in-memory duplex read cannot fail")
            .expect("both halves form one frame");

        writer.await.expect("writer task cannot panic");
        assert_eq!(frame, encoded);
    }

    #[tokio::test]
    async fn send_frame_rejects_payload_over_limit_without_writing() {
        let (mut peer, stream) = duplex(64);
        let mut connection = FrameConnection::new(stream);
        let payload = vec![0u8; MAX_FRAME_PAYLOAD_LEN as usize + 1];

        let error = connection
            .send_frame(&payload)
            .await
            .expect_err("payload over the limit must be rejected");
        assert!(matches!(
            error,
            ConnectionError::Framing(FramingError::PayloadTooLarge)
        ));

        drop(connection);
        let mut byte = [0u8; 1];
        let read = peer
            .read(&mut byte)
            .await
            .expect("in-memory duplex read cannot fail");
        assert_eq!(read, 0, "rejected payload must write no bytes");
    }
}
