//! Frame-only async connection for the TCP actor.
//!
//! The framing concern is split in two halves: [`FrameReader`] accumulates
//! inbound bytes into a [`FrameBuffer`] until whole frames are available and
//! [`FrameWriter`] encodes outbound payloads with [`encode_frame`] before
//! writing them out. [`split_frames`] produces both halves from one async
//! stream, and [`pump_frames`] forwards whole frames between a stream and a
//! pair of channels. All authentication, command dispatch, and payload
//! interpretation live in the actors layered above, so these types hold no
//! session state and can be composed into a larger actor without leaking wire
//! details upward.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc;
use tracing::{debug, trace};

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

/// Reads whole frames from an async byte source.
pub struct FrameReader<R> {
    reader: R,
    buffer: FrameBuffer,
    read_chunk: Vec<u8>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    /// Wraps `reader`, pre-allocating the read chunk.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
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
                trace!(frame_len = frame.len(), "frame read");
                return Ok(Some(frame));
            }

            let remaining = MAX_BUFFERED_FRAME_LEN.saturating_sub(self.buffer.buffered_len());
            debug_assert!(
                remaining > 0,
                "an incomplete frame always leaves room to complete it"
            );
            let read = self
                .reader
                .read(&mut self.read_chunk[..remaining.min(READ_CHUNK_LEN)])
                .await?;
            if read == 0 {
                return Ok(None);
            }

            self.buffer.extend(&self.read_chunk[..read])?;
        }
    }
}

/// Writes whole frames to an async byte sink.
pub struct FrameWriter<W> {
    writer: W,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    /// Wraps `writer`.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Encodes `payload` as one frame and writes it out.
    pub async fn send_frame(&mut self, payload: &[u8]) -> Result<()> {
        let frame = encode_frame(payload)?;
        self.writer.write_all(&frame).await?;
        trace!(payload_len = payload.len(), "frame written");
        Ok(())
    }
}

/// Splits an async stream into a frame reader and a frame writer.
pub fn split_frames<S>(stream: S) -> (FrameReader<ReadHalf<S>>, FrameWriter<WriteHalf<S>>)
where
    S: AsyncRead + AsyncWrite,
{
    let (reader, writer) = tokio::io::split(stream);
    (FrameReader::new(reader), FrameWriter::new(writer))
}

/// Runs the frame-only connection: frames read from the stream are sent to
/// `inbound`, and payloads received from `outbound` are framed and written to
/// the stream. Returns when the stream ends or either channel closes.
pub async fn pump_frames<S>(
    stream: S,
    inbound: mpsc::Sender<Vec<u8>>,
    mut outbound: mpsc::Receiver<Vec<u8>>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = split_frames(stream);
    loop {
        tokio::select! {
            frame = reader.next_frame() => match frame? {
                Some(frame) => {
                    if inbound.send(frame).await.is_err() {
                        debug!("frame pump stopped");
                        return Ok(());
                    }
                }
                None => {
                    debug!("frame pump stopped");
                    return Ok(());
                }
            },
            payload = outbound.recv() => match payload {
                Some(payload) => writer.send_frame(&payload).await?,
                None => {
                    debug!("frame pump stopped");
                    return Ok(());
                }
            },
        }
    }
}
