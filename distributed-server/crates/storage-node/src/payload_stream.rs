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
/// chunks are written out and their buffers returned to free slots.
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

    loop {
        let read = source.read(&mut read_buffer).await?;
        if read == 0 {
            break;
        }

        let mut pending = &read_buffer[..read];
        while !pending.is_empty() {
            match stream.write(pending) {
                Ok(0) | Err(ChunkError::Backpressure { .. }) => {
                    drain_sealed(&mut stream, writer).await?;
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

    use super::{send_payload, write_chain};
    use crate::buffer_pool::{PayloadBuffer, PayloadBufferPool, SizeClass};
    use crate::codec::{decode_frame, DecodeOutcome};
    use crate::connection::FrameWriter;
    use crate::payload::PayloadChain;

    fn non_zero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test value is non-zero")
    }

    fn pool(capacity: usize, buffers: usize) -> PayloadBufferPool {
        PayloadBufferPool::new(&[SizeClass::new(capacity, buffers)]).expect("valid size class")
    }

    fn buffer(pool: &mut PayloadBufferPool, capacity: usize, data: &[u8]) -> PayloadBuffer {
        let mut buffer = pool.acquire(capacity).expect("free buffer");
        buffer.set_len(data.len()).expect("data fits");
        buffer.as_mut_slice().copy_from_slice(data);
        buffer
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
    async fn write_chain_writes_every_chunk_in_order() {
        let mut pool = pool(4, 2);
        let mut chain = PayloadChain::new();
        chain
            .push(buffer(&mut pool, 4, &[1, 2, 3, 4]))
            .expect("non-empty buffer is accepted");
        chain
            .push(buffer(&mut pool, 4, &[5, 6]))
            .expect("non-empty buffer is accepted");

        let mut sink = Vec::new();
        {
            let mut writer = FrameWriter::new(&mut sink);
            write_chain(&mut writer, &chain)
                .await
                .expect("in-memory write cannot fail");
        }

        assert_eq!(payloads(&sink), vec![vec![1, 2, 3, 4], vec![5, 6]]);
    }

    #[tokio::test]
    async fn write_chain_writes_nothing_for_an_empty_chain() {
        let mut sink = Vec::new();
        {
            let mut writer = FrameWriter::new(&mut sink);
            write_chain(&mut writer, &PayloadChain::new())
                .await
                .expect("in-memory write cannot fail");
        }

        assert!(sink.is_empty());
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
            "five sealed chunks are written across two drains and the tail"
        );
        assert_eq!(payloads.concat(), data);
    }

    #[tokio::test]
    async fn send_payload_returns_drained_buffers_to_the_pool() {
        let data: Vec<u8> = (0..20u8).collect();
        let mut source = data.as_slice();
        let mut pool = pool(4, 8);
        let initial = pool.available();

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

        assert_eq!(pool.in_use(), 0, "no pooled buffer stays checked out");
        assert_eq!(
            pool.available(),
            initial,
            "drained and tail buffers are returned to the pool"
        );
        assert_eq!(payloads(&sink).concat(), data);
    }

    #[tokio::test]
    async fn send_payload_of_an_empty_source_writes_nothing() {
        let mut source: &[u8] = &[];
        let mut pool = pool(4, 2);
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

        assert!(sink.is_empty());
    }
}
