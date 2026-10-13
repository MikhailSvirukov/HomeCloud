//! Integration tests exercising only the public `storage_node` payload API.

use storage_node::{
    ChunkError, ChunkStream, PayloadBufferPool, PayloadChain, PoolError, SizeClass,
};

#[test]
fn chain_accumulates_and_returns_buffers() {
    let mut pool = PayloadBufferPool::new(&[SizeClass::new(16, 2)]).expect("valid size class");
    let initial_available = pool.available();

    let mut buffer = pool.acquire(16).expect("free buffer");
    buffer.set_len(5).expect("payload fits the buffer");
    buffer.as_mut_slice().copy_from_slice(b"hello");

    let mut chain = PayloadChain::new();
    chain.push(buffer).expect("non-empty buffer is accepted");

    assert_eq!(chain.total_len(), 5);
    assert_eq!(chain.chunk_count(), 1);
    assert_eq!(pool.in_use(), 1);

    chain.return_buffers(&mut pool);
    assert_eq!(pool.available(), initial_available);
    assert_eq!(pool.in_use(), 0);
}

#[test]
fn chunk_stream_seals_full_buffers_and_finishes_partial_block() {
    let mut pool = PayloadBufferPool::with_default_classes().expect("defaults are valid");
    let mut stream = ChunkStream::new(&mut pool, 64 * 1024, 4).expect("valid configuration");

    let data = vec![7u8; 100_000];
    stream
        .write_all(&data)
        .expect("payload fits the in-flight limit");

    assert_eq!(stream.sealed_chunks(), 1);
    assert_eq!(stream.buffered_len(), 100_000 - 65_536);

    let chain = stream.finish().expect("finish seals the open buffer");
    assert_eq!(chain.chunk_count(), 2);
    assert_eq!(chain.total_len(), 100_000);
    assert!(chain.last_chunk_is_partial());

    drop(stream);
    chain.return_buffers(&mut pool);
    assert_eq!(pool.in_use(), 0);
}

#[test]
fn empty_write_consumes_nothing() {
    let mut pool = PayloadBufferPool::new(&[SizeClass::new(8, 1)]).expect("valid size class");
    let mut stream = ChunkStream::new(&mut pool, 8, 1).expect("valid configuration");

    assert_eq!(stream.write(&[]).expect("empty write succeeds"), 0);
    assert_eq!(stream.buffered_len(), 0);
    assert_eq!(stream.sealed_chunks(), 0);

    drop(stream);
    assert_eq!(pool.in_use(), 0);
    assert_eq!(pool.available(), 1);
}

#[test]
fn backpressure_is_relieved_by_take_sealed() {
    let mut pool = PayloadBufferPool::new(&[SizeClass::new(1024, 4)]).expect("valid size class");
    let mut stream = ChunkStream::new(&mut pool, 1024, 2).expect("valid configuration");

    let first_batch = [1u8; 2048];
    stream
        .write_all(&first_batch)
        .expect("first batch fits the in-flight limit");
    assert_eq!(stream.sealed_chunks(), 2);

    assert_eq!(
        stream.write(&[9]),
        Err(ChunkError::Backpressure { limit: 2 })
    );

    let drained = stream.take_sealed();
    assert_eq!(drained.chunk_count(), 2);
    assert_eq!(drained.total_len(), 2048);
    assert_eq!(stream.sealed_chunks(), 0);

    let second_batch = [2u8; 2048];
    stream
        .write_all(&second_batch)
        .expect("draining sealed chunks relieves backpressure");
    let final_chain = stream.finish().expect("finish empties the stream");
    drop(stream);

    assert!(first_batch.len() + second_batch.len() > 2 * 1024);
    assert_eq!(
        drained.total_len() + final_chain.total_len(),
        first_batch.len() + second_batch.len()
    );

    drained.return_buffers(&mut pool);
    final_chain.return_buffers(&mut pool);
    assert_eq!(pool.in_use(), 0);
}

#[test]
fn pool_exhaustion_is_reported() {
    let mut pool = PayloadBufferPool::new(&[SizeClass::new(1024, 1)]).expect("valid size class");
    let mut stream = ChunkStream::new(&mut pool, 1024, 4).expect("valid configuration");

    stream
        .write_all(&[0u8; 1024])
        .expect("single buffer fills and seals");
    assert_eq!(stream.sealed_chunks(), 1);

    assert!(matches!(
        stream.write(&[1]),
        Err(ChunkError::Pool(PoolError::Exhausted { .. }))
    ));
}
