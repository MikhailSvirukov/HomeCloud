//! Ordered payload chains and pooled chunking for streamed file payloads.
//!
//! [`PayloadChain`] is an ordered list of pooled [`PayloadBuffer`] chunks with
//! an exact total length and a trailing partial block. [`ChunkStream`] seals
//! full buffers and returns [`ChunkError::Backpressure`] at the in-flight
//! limit; the caller drains sealed chunks with [`ChunkStream::take_sealed`],
//! hands them to the next actor, and resumes writing, so the limit bounds live
//! buffer count, not payload size.

use std::collections::VecDeque;

use crate::buffer_pool::{PayloadBuffer, PayloadBufferPool, PoolError};

/// Default maximum number of payload buffers a chain keeps in flight.
pub const DEFAULT_MAX_CHUNKS_IN_FLIGHT: usize = 16;

/// Ordered chain of bounded payload buffers forming one logical payload.
///
/// `total_len` tracks the summed initialized chunk lengths.
#[derive(Debug, Default)]
pub struct PayloadChain {
    chunks: VecDeque<PayloadBuffer>,
    total_len: usize,
}

impl PayloadChain {
    /// Creates an empty chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a non-empty buffer and adds its length to [`PayloadChain::total_len`].
    ///
    /// # Errors
    /// Returns [`ChunkError::EmptyChunk`] when `buffer` is empty; the buffer is
    /// then dropped rather than pooled.
    pub fn push(&mut self, buffer: PayloadBuffer) -> Result<(), ChunkError> {
        if buffer.is_empty() {
            Err(ChunkError::EmptyChunk)
        } else {
            self.total_len += buffer.len();
            self.chunks.push_back(buffer);
            Ok(())
        }
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

    /// Removes and returns the oldest chunk, decreasing
    /// [`PayloadChain::total_len`] by its length.
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
#[derive(Debug)]
pub struct ChunkStream<'pool> {
    pool: &'pool mut PayloadBufferPool,
    chunk_capacity: usize,
    max_chunks: usize,
    chain: PayloadChain,
    current: Option<PayloadBuffer>,
}

impl<'pool> ChunkStream<'pool> {
    /// Creates a stream filling buffers of at least `chunk_capacity` bytes and
    /// sealing at most `max_chunks` buffers before applying backpressure.
    ///
    /// # Errors
    /// Returns [`ChunkError::InvalidConfiguration`] when `chunk_capacity` or
    /// `max_chunks` is zero.
    pub fn new(
        pool: &'pool mut PayloadBufferPool,
        chunk_capacity: usize,
        max_chunks: usize,
    ) -> Result<Self, ChunkError> {
        if chunk_capacity == 0 || max_chunks == 0 {
            Err(ChunkError::InvalidConfiguration)
        } else {
            Ok(Self {
                pool,
                chunk_capacity,
                max_chunks,
                chain: PayloadChain::new(),
                current: None,
            })
        }
    }

    /// Copies as much of `data` as fits into the current buffer and returns the
    /// count. A buffer that becomes exactly full is sealed into the chain, so
    /// the next call starts a new buffer.
    ///
    /// # Errors
    /// Returns [`ChunkError::Backpressure`] when no buffer is open and the
    /// in-flight limit is reached, or [`ChunkError::Pool`] when acquiring or
    /// extending a buffer fails.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, ChunkError> {
        if data.is_empty() {
            return Ok(0);
        }
        if self.current.is_none() {
            if self.chain.chunk_count() >= self.max_chunks {
                return Err(ChunkError::Backpressure {
                    limit: self.max_chunks,
                });
            }
            self.current = Some(self.pool.acquire(self.chunk_capacity)?);
        }
        let Some(buffer) = self.current.as_mut() else {
            return Ok(0);
        };
        let start = buffer.len();
        let count = (buffer.capacity() - start).min(data.len());
        let end = start + count;
        buffer.set_len(end)?;
        buffer.as_mut_slice()[start..end].copy_from_slice(&data[..count]);

        let sealed =
            matches!(self.current.as_ref(), Some(buffer) if buffer.len() == buffer.capacity());
        if sealed {
            if let Some(buffer) = self.current.take() {
                self.chain.push(buffer)?;
            }
        }
        Ok(count)
    }

    /// Calls [`ChunkStream::write`] until `data` is exhausted.
    ///
    /// # Errors
    /// Propagates any error returned by [`ChunkStream::write`].
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), ChunkError> {
        let mut remaining = data;
        while !remaining.is_empty() {
            let written = self.write(remaining)?;
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
    ///
    /// # Errors
    /// Propagates [`ChunkError::EmptyChunk`] if sealing an open buffer fails.
    pub fn finish(&mut self) -> Result<PayloadChain, ChunkError> {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkError {
    /// A buffer pool operation failed.
    Pool(PoolError),
    /// The in-flight sealed-buffer limit was reached.
    Backpressure {
        /// Configured maximum number of in-flight chunks.
        limit: usize,
    },
    /// A zero-length buffer was offered as a chunk.
    EmptyChunk,
    /// The stream configuration was rejected (zero capacity or limit).
    InvalidConfiguration,
}

impl From<PoolError> for ChunkError {
    fn from(error: PoolError) -> Self {
        Self::Pool(error)
    }
}

impl std::fmt::Display for ChunkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pool(error) => write!(formatter, "payload pool error: {error}"),
            Self::Backpressure { limit } => {
                write!(formatter, "in-flight chunk limit {limit} reached")
            }
            Self::EmptyChunk => write!(formatter, "empty chunk"),
            Self::InvalidConfiguration => write!(formatter, "invalid chunk stream configuration"),
        }
    }
}

impl std::error::Error for ChunkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pool(error) => Some(error),
            Self::Backpressure { .. } | Self::EmptyChunk | Self::InvalidConfiguration => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChunkError, ChunkStream, PayloadChain};
    use crate::buffer_pool::{PayloadBuffer, PayloadBufferPool, PoolError, SizeClass};

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
    fn new_rejects_zero_capacity_or_limit() {
        let mut pool = pool(4, 2);
        assert_eq!(
            ChunkStream::new(&mut pool, 0, 4).err(),
            Some(ChunkError::InvalidConfiguration)
        );
        assert_eq!(
            ChunkStream::new(&mut pool, 4, 0).err(),
            Some(ChunkError::InvalidConfiguration)
        );
    }

    #[test]
    fn write_copies_seals_when_full_and_ignores_empty() {
        let mut pool = pool(4, 4);
        let mut stream = ChunkStream::new(&mut pool, 4, 4).expect("valid");
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
    fn write_all_spans_multiple_boundaries() {
        let mut pool = pool(4, 4);
        let mut stream = ChunkStream::new(&mut pool, 4, 4).expect("valid");
        stream
            .write_all(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
            .expect("write");
        assert_eq!(stream.sealed_chunks(), 2);
        assert_eq!(stream.buffered_len(), 2);

        let chain = stream.finish().expect("finish");
        assert_eq!(chain.chunk_count(), 3);
        assert_eq!(chain.total_len(), 10);
        assert!(chain.last_chunk_is_partial());
    }

    #[test]
    fn take_sealed_relieves_backpressure_and_writing_resumes() {
        let mut pool = pool(2, 4);
        let mut stream = ChunkStream::new(&mut pool, 2, 2).expect("valid");

        stream.write_all(&[1, 2, 3, 4]).expect("write");
        assert_eq!(stream.sealed_chunks(), 2);
        assert_eq!(
            stream.write(&[5]),
            Err(ChunkError::Backpressure { limit: 2 })
        );

        let drained = stream.take_sealed();
        assert_eq!(drained.chunk_count(), 2);
        assert_eq!(drained.total_len(), 4);
        assert!(!drained.last_chunk_is_partial());
        assert_eq!(stream.sealed_chunks(), 0);

        // Draining freed both slots, so the stream accepts >4 bytes overall.
        stream.write_all(&[5, 6, 7]).expect("write resumes");
        assert_eq!(stream.sealed_chunks(), 1);
        assert_eq!(stream.buffered_len(), 1);

        let final_chain = stream.finish().expect("finish");
        assert_eq!(final_chain.chunk_count(), 2);
        assert!(final_chain.last_chunk_is_partial());
        assert_eq!(drained.total_len() + final_chain.total_len(), 7);

        drop(stream);
        drained.return_buffers(&mut pool);
        final_chain.return_buffers(&mut pool);
        assert_eq!(pool.in_use(), 0);
    }

    #[test]
    fn finish_seals_partial_buffer_and_empties_stream() {
        let mut pool = pool(4, 2);
        let mut stream = ChunkStream::new(&mut pool, 4, 4).expect("valid");
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
                chunk_capacity: 4,
                max_chunks: 4,
                chain: PayloadChain::new(),
                current: Some(empty),
            };
            stream.finish().expect("finish")
        };
        assert!(chain.is_empty());
        assert_eq!(pool.available(), initial);
        assert_eq!(pool.in_use(), 0);
    }

    #[test]
    fn pool_exhaustion_surfaces_as_pool_error() {
        let mut pool = pool(2, 1);
        let mut stream = ChunkStream::new(&mut pool, 2, 4).expect("valid");
        stream.write_all(&[1, 2]).expect("write");
        assert_eq!(stream.sealed_chunks(), 1);
        assert_eq!(
            stream.write(&[3]),
            Err(ChunkError::Pool(PoolError::Exhausted { requested: 2 }))
        );
    }
}
