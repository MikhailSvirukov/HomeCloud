//! Streams a file payload as consecutive frames using the payload buffer pools.

use core::num::NonZeroUsize;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

use crate::buffer_pool::PayloadBufferPool;
use crate::connection::{ConnectionError, FrameWriter};
use crate::payload::{ChunkError, ChunkStream, PayloadChain};

/// Failure modes of the payload streaming path.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// Reading the payload source failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Filling or sealing a pooled chunk failed.
    #[error(transparent)]
    Chunk(#[from] ChunkError),
    /// Writing a frame to the connection failed.
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

type Result<T> = core::result::Result<T, StreamError>;

/// Writes every chunk of `chain` as one frame, in order.
pub async fn write_chain<W>(writer: &mut FrameWriter<W>, chain: &PayloadChain) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    for chunk in chain.iter() {
        writer.send_frame(chunk).await?;
    }
    Ok(())
}

/// Reads `source` to the end and writes it as consecutive frames, filling
/// pooled buffers of at least `chunk_capacity` and keeping at most `max_chunks`
/// buffers in flight; when the limit applies backpressure, already-sealed
/// chunks are written out and their buffers returned to the pool. Every buffer
/// the stream checks out is returned before this function returns.
pub async fn send_payload<W, R>(
    writer: &mut FrameWriter<W>,
    source: &mut R,
    pool: &mut PayloadBufferPool,
    chunk_capacity: NonZeroUsize,
    max_chunks: NonZeroUsize,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    let mut stream = ChunkStream::new(pool, chunk_capacity, max_chunks);
    let mut read_buffer = vec![0u8; chunk_capacity.get()];

    let outcome = run_payload(&mut stream, writer, source, &mut read_buffer).await;
    if let Ok(remaining) = stream.finish() {
        stream.release(remaining);
    }
    outcome
}

/// Fills `stream` from `source` until it ends, writing sealed chunks under
/// backpressure and releasing their buffers.
async fn run_payload<W, R>(
    stream: &mut ChunkStream<'_>,
    writer: &mut FrameWriter<W>,
    source: &mut R,
    read_buffer: &mut [u8],
) -> Result<()>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    loop {
        let read = source.read(read_buffer).await?;
        if read == 0 {
            break;
        }

        let mut pending = &read_buffer[..read];
        while !pending.is_empty() {
            match stream.write(pending) {
                Err(ChunkError::Backpressure { .. }) => {
                    drain_sealed(stream, writer).await?;
                }
                Ok(written) => {
                    debug_assert!(written > 0, "a non-empty slice always makes progress");
                    pending = &pending[written..];
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    let tail = stream.finish()?;
    let result = write_chain(writer, &tail).await;
    stream.release(tail);
    result
}

/// Writes the sealed chunks currently held by `stream` and returns their
/// buffers to the pool, freeing in-flight slots so filling can resume after
/// backpressure.
async fn drain_sealed<W>(stream: &mut ChunkStream<'_>, writer: &mut FrameWriter<W>) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let sealed = stream.take_sealed();
    let result = write_chain(writer, &sealed).await;
    stream.release(sealed);
    result
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroUsize;

    use super::send_payload;
    use crate::buffer_pool::{PayloadBufferPool, SizeClass};
    use crate::codec::{decode_frame, DecodeOutcome};
    use crate::connection::FrameWriter;

    fn non_zero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test value is non-zero")
    }

    fn pool(capacity: usize, buffers: usize) -> PayloadBufferPool {
        PayloadBufferPool::new(&[SizeClass::new(capacity, buffers)]).expect("valid size class")
    }

    fn payloads(mut bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut payloads = Vec::new();
        while !bytes.is_empty() {
            match decode_frame(bytes).expect("frame decodes") {
                DecodeOutcome::Complete {
                    payload, consumed, ..
                } => {
                    payloads.push(payload.to_vec());
                    bytes = &bytes[consumed..];
                }
                DecodeOutcome::Incomplete { needed } => {
                    panic!("stream ended mid-frame, needed {needed} bytes")
                }
            }
        }
        payloads
    }

    #[tokio::test]
    async fn send_payload_drains_sealed_chunks_under_backpressure() {
        let data: Vec<u8> = (0..20u8).collect();
        let mut source = data.as_slice();
        let mut pool = pool(4, 8);
        let mut sink = Vec::new();
        {
            let mut writer = FrameWriter::new(&mut sink);
            send_payload(
                &mut writer,
                &mut source,
                &mut pool,
                non_zero(4),
                non_zero(2),
            )
            .await
            .expect("in-memory write cannot fail");
        }

        let payloads = payloads(&sink);
        assert_eq!(
            payloads.len(),
            5,
            "more frames than the in-flight limit are written across two drains and the tail"
        );
        assert_eq!(payloads.concat(), data);
    }
}
