//! Integration tests exercising only the public `storage_node` API.

use core::num::NonZeroU32;

use storage_node::{
    decode_frame, encode_frame, DecodeOutcome, PayloadBufferPool, PoolError,
    DEFAULT_BUFFERS_PER_CLASS, FRAME_LENGTH_PREFIX_LEN,
};

fn max_frame_len() -> NonZeroU32 {
    NonZeroU32::new(64 * 1024).expect("test limit is non-zero")
}

#[test]
fn frame_prefix_is_the_big_endian_payload_length() {
    let payload = b"public api payload".as_slice();
    let encoded = encode_frame(payload, max_frame_len()).expect("payload is within the limit");

    assert_eq!(
        &encoded[..FRAME_LENGTH_PREFIX_LEN],
        &(payload.len() as u32).to_be_bytes()
    );
    assert_eq!(encoded.len(), FRAME_LENGTH_PREFIX_LEN + payload.len());

    match decode_frame(&encoded, max_frame_len()) {
        Ok(DecodeOutcome::Complete {
            payload: decoded,
            consumed,
        }) => {
            assert_eq!(consumed, FRAME_LENGTH_PREFIX_LEN + payload.len());
            assert_eq!(decoded, payload);
        }
        other => panic!("expected a complete frame, got {other:?}"),
    }
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
