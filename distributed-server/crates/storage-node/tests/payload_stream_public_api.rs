//! Integration tests driving the public payload streaming path over real duplex
//! streams, so `send_payload` and `write_chain` are exercised end to end rather
//! than through in-memory buffer helpers.

use core::num::NonZeroUsize;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use storage_node::{
    decode_frame, send_payload, write_chain, ChunkStream, DecodeOutcome, FrameReader, FrameWriter,
    PayloadBufferPool, SizeClass, StreamError,
};
use tokio::io::{duplex, AsyncRead, ReadBuf};

/// Builds a non-zero capacity for the streaming entry points.
fn non_zero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test capacity is non-zero")
}

/// An async source that yields its bytes and then fails its next read.
struct FailingSource {
    pending: Vec<u8>,
}

impl FailingSource {
    fn new(bytes: &[u8]) -> Self {
        Self {
            pending: bytes.to_vec(),
        }
    }
}

impl AsyncRead for FailingSource {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pending.is_empty() {
            return Poll::Ready(Err(io::Error::other("source read failure")));
        }
        let count = self.pending.len().min(buf.remaining());
        buf.put_slice(&self.pending[..count]);
        self.pending.drain(..count);
        Poll::Ready(Ok(()))
    }
}

/// Builds a single-class pool with `buffers` buffers of `capacity` bytes.
fn pool(size_class: SizeClass) -> PayloadBufferPool {
    PayloadBufferPool::new(&[size_class]).expect("valid size class")
}

/// Reads decoded frame payloads from `reader` until the peer closes, in order.
async fn read_all_payloads<R>(reader: &mut FrameReader<R>) -> Vec<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut payloads = Vec::new();
    while let Some(frame) = reader
        .next_frame()
        .await
        .expect("in-memory duplex read cannot fail")
    {
        match decode_frame(&frame).expect("the reader yields whole frames") {
            DecodeOutcome::Complete { payload, .. } => payloads.push(payload.to_vec()),
            DecodeOutcome::Incomplete { needed } => {
                panic!("the reader yielded a frame needing {needed} more bytes")
            }
        }
    }
    payloads
}

#[tokio::test]
async fn send_payload_streams_a_source_as_frames() {
    const SOURCE_LEN: usize = 4096 + 37;
    let source: Vec<u8> = (0..SOURCE_LEN).map(|index| (index % 251) as u8).collect();
    let expected = source.clone();

    let (peer, writer_half) = duplex(64 * 1024);
    let mut reader = FrameReader::new(peer);

    let producer = tokio::spawn(async move {
        let mut pool = pool(SizeClass::new(1024, 4));
        let mut source = source.as_slice();
        let mut writer = FrameWriter::new(writer_half);
        send_payload(
            &mut writer,
            &mut source,
            &mut pool,
            non_zero(1024),
            non_zero(2),
        )
        .await
        .expect("in-memory duplex write cannot fail");
    });

    let payloads = read_all_payloads(&mut reader).await;
    producer.await.expect("producer task does not panic");

    assert!(
        payloads.len() > 1,
        "a multi-chunk source produces more than one frame"
    );
    assert_eq!(payloads.concat(), expected);
}

#[tokio::test]
async fn send_payload_of_an_empty_source_writes_nothing() {
    let (peer, writer_half) = duplex(64 * 1024);
    let mut reader = FrameReader::new(peer);
    let mut pool = pool(SizeClass::new(1024, 2));
    let mut source: &[u8] = &[];

    {
        let mut writer = FrameWriter::new(writer_half);
        send_payload(
            &mut writer,
            &mut source,
            &mut pool,
            non_zero(1024),
            non_zero(2),
        )
        .await
        .expect("in-memory duplex write cannot fail");
    }

    let frame = reader
        .next_frame()
        .await
        .expect("in-memory duplex read cannot fail");

    assert!(frame.is_none(), "an empty source writes no frame");
}

#[tokio::test]
async fn send_payload_returns_buffers_to_the_pool() {
    let mut pool = pool(SizeClass::new(16, 8));
    let initial_available = pool.available();
    let data: Vec<u8> = (0..80u8).collect();
    let mut source = data.as_slice();
    let mut sink = Vec::new();

    {
        let mut writer = FrameWriter::new(&mut sink);
        send_payload(
            &mut writer,
            &mut source,
            &mut pool,
            non_zero(16),
            non_zero(2),
        )
        .await
        .expect("in-memory write cannot fail");
    }

    assert_eq!(pool.in_use(), 0, "no pooled buffer stays checked out");
    assert_eq!(
        pool.available(),
        initial_available,
        "drained and tail buffers are returned to the pool"
    );
}

#[tokio::test]
async fn send_payload_returns_buffers_when_the_source_fails() {
    let mut pool = pool(SizeClass::new(16, 8));
    let initial_available = pool.available();
    let data: Vec<u8> = (0..40u8).collect();
    let mut source = FailingSource::new(&data);

    let (peer, writer_half) = duplex(64 * 1024);
    let outcome = {
        let mut writer = FrameWriter::new(writer_half);
        send_payload(
            &mut writer,
            &mut source,
            &mut pool,
            non_zero(16),
            non_zero(2),
        )
        .await
    };
    drop(peer);

    assert!(
        matches!(outcome, Err(StreamError::Io(_))),
        "a source read error surfaces as StreamError::Io"
    );
    assert_eq!(pool.in_use(), 0, "no pooled buffer stays checked out");
    assert_eq!(
        pool.available(),
        initial_available,
        "a failed read still returns every buffer to the pool"
    );
}

#[tokio::test]
async fn write_chain_writes_each_chunk_as_a_frame() {
    let mut pool = pool(SizeClass::new(4, 4));
    let chunks = {
        let mut stream = ChunkStream::new(&mut pool, non_zero(4), non_zero(4));
        stream
            .write_all(&[1, 2, 3, 4, 5, 6])
            .expect("payload fits the in-flight limit");
        stream.finish().expect("finish seals the partial block")
    };
    assert_eq!(chunks.chunk_count(), 2);

    let (peer, writer_half) = duplex(64 * 1024);
    let mut reader = FrameReader::new(peer);
    {
        let mut writer = FrameWriter::new(writer_half);
        write_chain(&mut writer, &chunks)
            .await
            .expect("in-memory duplex write cannot fail");
    }

    let payloads = read_all_payloads(&mut reader).await;
    assert_eq!(payloads, vec![vec![1, 2, 3, 4], vec![5, 6]]);

    chunks.return_buffers(&mut pool);
    assert_eq!(pool.in_use(), 0);
}
