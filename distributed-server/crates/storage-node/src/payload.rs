//! Ordered payload chains and pooled chunking for streamed file payloads.
//!
//! [`PayloadChain`] is an ordered list of pooled [`PayloadBuffer`] chunks with
//! an exact total length and a trailing partial block. [`ChunkStream`] seals
//! full buffers and returns [`ChunkError::Backpressure`] at the in-flight
//! limit; the caller drains sealed chunks with [`ChunkStream::take_sealed`],
//! hands them to the next actor, and resumes writing, so the limit bounds live
//! buffer count, not payload size.

use core::num::NonZeroUsize;
use std::collections::VecDeque;

use crate::buffer_pool::{PayloadBuffer, PayloadBufferPool, PoolError};

type Result<T> = core::result::Result<T, ChunkError>;

/// Default maximum number of payload buffers a chain keeps in flight.
pub const DEFAULT_MAX_CHUNKS_IN_FLIGHT: usize = 16;

/// Ordered chain of bounded payload buffers forming one logical payload.
///
/// `total_len` tracks the summed initialized chunk lengths.
#[derive(Default)]
pub struct PayloadChain {
    chunks: VecDeque<PayloadBuffer>,
    total_len: usize,
}

impl PayloadChain {
    /// Creates an empty chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a non-empty buffer and adds its length to the chain total.
    pub fn push(&mut self, buffer: PayloadBuffer) -> Result<()> {
        if buffer.is_empty() {
            return Err(ChunkError::EmptyChunk);
        }
        self.total_len += buffer.len();
        self.chunks.push_back(buffer);
        Ok(())
    }

    /// Total number of initialized bytes across every chunk.
    pub fn total_len(&self) -> usize {
        self.total_len
    }

    /// Number of chunks currently held.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Whether the chain holds no chunks.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Whether the last chunk is smaller than its own capacity (a trailing
    /// partial block). An empty chain is never partial.
    pub fn last_chunk_is_partial(&self) -> bool {
        matches!(self.chunks.back(), Some(buffer) if buffer.len() < buffer.capacity())
    }

    /// Yields each chunk's initialized bytes in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.chunks.iter().map(|buffer| buffer.as_slice())
    }

    /// Removes and returns the oldest chunk, decreasing the chain total by its
    /// length.
    pub fn pop_front(&mut self) -> Option<PayloadBuffer> {
        match self.chunks.pop_front() {
            Some(buffer) => {
                self.total_len -= buffer.len();
                Some(buffer)
            }
            None => None,
        }
    }

    /// Returns every chunk to `pool` and consumes the chain.
    pub fn return_buffers(self, pool: &mut PayloadBufferPool) {
        for buffer in self.chunks {
            pool.release(buffer);
        }
    }
}

/// Fills pooled buffers from a byte source, sealing full buffers into a
/// [`PayloadChain`] while enforcing an in-flight buffer limit.
///
/// A caller that hits [`ChunkError::Backpressure`] drains sealed chunks with
/// [`ChunkStream::take_sealed`], hands them to the next actor, and resumes;
/// the limit bounds live buffer count, not total payload size.
pub struct ChunkStream<'pool> {
    pool: &'pool mut PayloadBufferPool,
    chunk_capacity: NonZeroUsize,
    max_chunks: NonZeroUsize,
    chain: PayloadChain,
    current: Option<PayloadBuffer>,
}

impl<'pool> ChunkStream<'pool> {
    /// Creates a stream filling buffers of at least `chunk_capacity` bytes and
    /// sealing at most `max_chunks` buffers before applying backpressure.
    pub fn new(
        pool: &'pool mut PayloadBufferPool,
        chunk_capacity: NonZeroUsize,
        max_chunks: NonZeroUsize,
    ) -> Self {
        Self {
            pool,
            chunk_capacity,
            max_chunks,
            chain: PayloadChain::new(),
            current: None,
        }
    }

    /// Copies as much of `data` as fits into the current buffer and returns the
    /// count. A buffer that becomes exactly full is sealed into the chain, so
    /// the next call starts a new buffer.
    pub fn write(&mut self, data: &[u8]) -> Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }

        let mut buffer = match self.current.take() {
            Some(buffer) => buffer,
            None => {
                if self.chain.chunk_count() >= self.max_chunks.get() {
                    return Err(ChunkError::Backpressure {
                        limit: self.max_chunks.get(),
                    });
                }
                self.pool.acquire(self.chunk_capacity.get())?
            }
        };

        let start = buffer.len();
        let count = (buffer.capacity() - start).min(data.len());
        let end = start + count;
        buffer.set_len(end)?;
        buffer.as_mut_slice()[start..end].copy_from_slice(&data[..count]);

        if buffer.len() == buffer.capacity() {
            self.chain.push(buffer)?;
        } else {
            self.current = Some(buffer);
        }
        Ok(count)
    }

    /// Calls [`ChunkStream::write`] until `data` is exhausted.
    pub fn write_all(&mut self, data: &[u8]) -> Result<()> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let written = self.write(remaining)?;
            debug_assert!(written > 0, "write must progress on a non-empty slice");
            remaining = &remaining[written..];
        }
        Ok(())
    }

    /// Number of bytes written into the currently open, not-yet-sealed buffer.
    pub fn buffered_len(&self) -> usize {
        match self.current.as_ref() {
            Some(buffer) => buffer.len(),
            None => 0,
        }
    }

    /// Number of sealed buffers currently held in the chain.
    pub fn sealed_chunks(&self) -> usize {
        self.chain.chunk_count()
    }

    /// Removes and returns all currently sealed chunks as their own
    /// [`PayloadChain`], freeing in-flight slots so writing can resume after
    /// [`ChunkError::Backpressure`]. The stream keeps any open (unsealed)
    /// buffer.
    pub fn take_sealed(&mut self) -> PayloadChain {
        std::mem::take(&mut self.chain)
    }

    /// Seals the open buffer (returning an empty one to the pool), takes the
    /// complete chain, and leaves the stream empty with no open buffer.
    pub fn finish(&mut self) -> Result<PayloadChain> {
        if let Some(buffer) = self.current.take() {
            if buffer.is_empty() {
                self.pool.release(buffer);
            } else {
                self.chain.push(buffer)?;
            }
        }
        Ok(std::mem::take(&mut self.chain))
    }
}

/// Failure modes when building or filling a [`PayloadChain`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChunkError {
    /// A buffer pool operation failed.
    #[error(transparent)]
    Pool(#[from] PoolError),
    /// The in-flight sealed-buffer limit was reached.
    #[error("in-flight chunk limit {limit} reached")]
    Backpressure {
        /// Configured maximum number of in-flight chunks.
        limit: usize,
    },
    /// A zero-length buffer was offered as a chunk.
    #[error("empty chunk")]
    EmptyChunk,
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroUsize;

    use super::{ChunkError, ChunkStream, PayloadChain};
    use crate::buffer_pool::{PayloadBuffer, PayloadBufferPool, SizeClass};

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

    fn chain(pool: &mut PayloadBufferPool, capacity: usize, chunks: &[&[u8]]) -> PayloadChain {
        let mut chain = PayloadChain::new();
        for data in chunks {
            chain.push(buffer(pool, capacity, data)).expect("accepted");
        }
        chain
    }

    #[test]
    fn push_appends_and_rejects_empty() {
        let mut pool = pool(8, 4);
        let mut chain = PayloadChain::new();
        assert!(chain.is_empty());
        chain
            .push(buffer(&mut pool, 8, &[1, 2, 3]))
            .expect("accepted");
        chain.push(buffer(&mut pool, 8, &[4, 5])).expect("accepted");
        assert_eq!(chain.total_len(), 5);
        assert_eq!(chain.chunk_count(), 2);
        assert!(!chain.is_empty());

        let empty = pool.acquire(8).expect("free buffer");
        assert_eq!(chain.push(empty), Err(ChunkError::EmptyChunk));
        assert_eq!(chain.total_len(), 5);
    }

    #[test]
    fn last_chunk_is_partial_tracks_fill() {
        let mut pool = pool(4, 2);
        assert!(!PayloadChain::new().last_chunk_is_partial());
        let mut chain = chain(&mut pool, 4, &[&[1, 2]]);
        assert!(chain.last_chunk_is_partial());
        chain
            .push(buffer(&mut pool, 4, &[1, 2, 3, 4]))
            .expect("accepted");
        assert!(!chain.last_chunk_is_partial());
    }

    #[test]
    fn iter_yields_chunks_in_order() {
        let mut pool = pool(8, 4);
        let chain = chain(&mut pool, 8, &[&[1, 2, 3], &[4, 5]]);
        let chunks: Vec<Vec<u8>> = chain.iter().map(<[u8]>::to_vec).collect();
        assert_eq!(chunks, vec![vec![1, 2, 3], vec![4, 5]]);
    }

    #[test]
    fn pop_front_returns_oldest_and_updates_len() {
        let mut pool = pool(8, 4);
        let mut chain = chain(&mut pool, 8, &[&[1, 2, 3], &[4, 5]]);
        let oldest = chain.pop_front().expect("not empty");
        assert_eq!(oldest.as_slice(), &[1, 2, 3]);
        assert_eq!(chain.total_len(), 2);
        assert_eq!(chain.chunk_count(), 1);
        pool.release(oldest);
        assert_eq!(pool.in_use(), 1);
        assert!(PayloadChain::new().pop_front().is_none());
    }

    #[test]
    fn return_buffers_restores_available_count() {
        let mut pool = pool(8, 4);
        let initial = pool.available();
        let chain = chain(&mut pool, 8, &[&[1, 2, 3], &[4, 5]]);
        assert_eq!(pool.available(), initial - 2);
        chain.return_buffers(&mut pool);
        assert_eq!(pool.available(), initial);
        assert_eq!(pool.in_use(), 0);
    }

    #[test]
    fn write_copies_seals_when_full_and_ignores_empty() {
        let mut pool = pool(4, 4);
        let mut stream = ChunkStream::new(&mut pool, non_zero(4), non_zero(4));
        assert_eq!(stream.write(&[]).expect("empty write"), 0);
        assert_eq!(stream.buffered_len(), 0);
        assert_eq!(stream.sealed_chunks(), 0);

        assert_eq!(stream.write(&[1, 2, 3]).expect("write"), 3);
        assert_eq!(stream.buffered_len(), 3);
        assert_eq!(stream.sealed_chunks(), 0);
        assert_eq!(stream.write(&[4, 5]).expect("write"), 1);
        assert_eq!(stream.buffered_len(), 0);
        assert_eq!(stream.sealed_chunks(), 1);
    }

    #[test]
    fn finish_seals_partial_buffer_and_empties_stream() {
        let mut pool = pool(4, 2);
        let mut stream = ChunkStream::new(&mut pool, non_zero(4), non_zero(4));
        stream.write_all(&[1, 2, 3]).expect("write");
        let chain = stream.finish().expect("finish");
        assert_eq!(chain.chunk_count(), 1);
        assert_eq!(chain.total_len(), 3);
        assert!(chain.last_chunk_is_partial());
        assert_eq!(stream.buffered_len(), 0);
        assert_eq!(stream.sealed_chunks(), 0);
        assert!(stream.finish().expect("finish").is_empty());
    }

    #[test]
    fn finish_releases_an_empty_open_buffer() {
        let mut pool = pool(4, 1);
        let initial = pool.available();
        let empty = pool.acquire(4).expect("free buffer");
        let chain = {
            let mut stream = ChunkStream {
                pool: &mut pool,
                chunk_capacity: non_zero(4),
                max_chunks: non_zero(4),
                chain: PayloadChain::new(),
                current: Some(empty),
            };
            stream.finish().expect("finish")
        };
        assert!(chain.is_empty());
        assert_eq!(pool.available(), initial);
        assert_eq!(pool.in_use(), 0);
    }
}
