//! Integration tests exercising only the public `storage_node` API.

use storage_node::{
    FrameHeader, FramingError, PayloadBufferPool, PoolError, DEFAULT_BUFFERS_PER_CLASS,
    FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
};

#[test]
fn frame_header_round_trips_through_public_api() {
    let header = FrameHeader::new(4096).expect("4096 bytes is within the limit");
    let encoded = header.encode();
    assert_eq!(encoded.len(), FRAME_HEADER_LEN);
    assert_eq!(encoded[0], PROTOCOL_VERSION);

    let decoded = FrameHeader::decode(&encoded).expect("encoded header is valid");
    assert_eq!(decoded, header);

    assert_eq!(
        FrameHeader::new(MAX_FRAME_PAYLOAD_LEN + 1).err(),
        Some(FramingError::PayloadTooLarge)
    );
}

#[test]
fn pool_acquires_the_smallest_suitable_class() {
    let mut pool = PayloadBufferPool::with_default_classes().expect("defaults are valid");

    let first = pool.acquire(1).expect("smallest class has free buffers");
    let second = pool
        .acquire(first.capacity() + 1)
        .expect("next larger class has free buffers");

    assert!(second.capacity() > first.capacity());
}

#[test]
fn pool_balances_to_a_larger_class_when_the_smallest_is_drained() {
    let mut pool = PayloadBufferPool::with_default_classes().expect("defaults are valid");

    let mut held = Vec::new();
    for _ in 0..DEFAULT_BUFFERS_PER_CLASS {
        held.push(
            pool.acquire(1)
                .expect("smallest class starts with free buffers"),
        );
    }
    let smallest_capacity = held[0].capacity();

    let balanced = pool.acquire(1).expect("next larger class is free");
    assert!(balanced.capacity() > smallest_capacity);
}

#[test]
fn pool_returns_controlled_errors_instead_of_growing_memory() {
    let mut pool = PayloadBufferPool::with_default_classes().expect("defaults are valid");

    let mut held = Vec::new();
    loop {
        match pool.acquire(1) {
            Ok(buffer) => held.push(buffer),
            Err(error) => {
                assert_eq!(error, PoolError::Exhausted { requested: 1 });
                break;
            }
        }
    }

    assert_eq!(pool.available(), 0);

    let too_large = pool
        .acquire(usize::MAX)
        .expect_err("request beyond the largest class fails");
    assert_eq!(
        too_large,
        PoolError::NoSuitableClass {
            requested: usize::MAX
        }
    );
}
