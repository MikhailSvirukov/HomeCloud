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
use crate::frame_buffer::{FrameBuffer, MAX_BUFFERED_FRAME_LEN};
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
    ///
    /// Each read is bounded by the buffer's remaining frame capacity, so a
    /// near-maximum frame followed by the next frame's header in one stream
    /// chunk can never be rejected as a whole by [`FrameBuffer::extend`].
    pub async fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            if let Some(frame) = self.buffer.next_frame()? {
                return Ok(Some(frame));
            }

            let remaining = MAX_BUFFERED_FRAME_LEN.saturating_sub(self.buffer.buffered_len());
            debug_assert!(
                remaining > 0,
                "an incomplete frame always leaves room to complete it"
            );
            let read = self
                .stream
                .read(&mut self.read_chunk[..remaining.min(READ_CHUNK_LEN)])
                .await?;
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
}
