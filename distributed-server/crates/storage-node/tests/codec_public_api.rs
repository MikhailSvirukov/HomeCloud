//! Integration tests exercising the public frame codec API of `storage_node`.

use core::num::NonZeroU32;

use storage_node::{
    decode_frame, encode_frame, DecodeOutcome, FramingError, FRAME_LENGTH_PREFIX_LEN,
};

fn max_frame_len() -> NonZeroU32 {
    NonZeroU32::new(64).expect("test limit is non-zero")
}

#[test]
fn encode_then_decode_round_trips() {
    let input = b"round trip payload".as_slice();
    let encoded = encode_frame(input, max_frame_len()).expect("payload is within the limit");

    match decode_frame(&encoded, max_frame_len()) {
        Ok(DecodeOutcome::Complete { payload, consumed }) => {
            assert_eq!(consumed, FRAME_LENGTH_PREFIX_LEN + input.len());
            assert_eq!(payload, input);
        }
        other => panic!("expected a complete frame, got {other:?}"),
    }
}

#[test]
fn empty_payload_round_trips() {
    let encoded = encode_frame(&[], max_frame_len()).expect("empty payload is within the limit");

    assert_eq!(encoded, [0, 0, 0, 0]);
    assert_eq!(
        decode_frame(&encoded, max_frame_len()),
        Ok(DecodeOutcome::Complete {
            payload: &[],
            consumed: FRAME_LENGTH_PREFIX_LEN,
        })
    );
}

#[test]
fn decode_incomplete_prefix() {
    let short = [0, 0, 1];

    match decode_frame(&short, max_frame_len()) {
        Ok(DecodeOutcome::Incomplete { needed }) => {
            assert_eq!(needed, FRAME_LENGTH_PREFIX_LEN);
        }
        other => panic!("expected an incomplete prefix, got {other:?}"),
    }
}

#[test]
fn decode_incomplete_payload() {
    let declared_len = 8u32;
    let mut buffer = declared_len.to_be_bytes().to_vec();
    buffer.extend_from_slice(&[0u8; 3]);

    match decode_frame(&buffer, max_frame_len()) {
        Ok(DecodeOutcome::Incomplete { needed }) => {
            assert_eq!(needed, FRAME_LENGTH_PREFIX_LEN + declared_len as usize);
        }
        other => panic!("expected an incomplete payload, got {other:?}"),
    }
}

#[test]
fn decode_rejects_oversized_length_before_payload_arrives() {
    let limit = max_frame_len();
    let declared_len = limit.get() + 1;
    let prefix = declared_len.to_be_bytes();

    match decode_frame(&prefix, limit) {
        Err(FramingError::FrameTooLarge { len, max }) => {
            assert_eq!(len, declared_len);
            assert_eq!(max, limit.get());
        }
        other => panic!("expected FrameTooLarge, got {other:?}"),
    }
}

#[test]
fn decode_ignores_trailing_bytes() {
    let input = b"first frame".as_slice();
    let mut buffer = encode_frame(input, max_frame_len()).expect("payload is within the limit");
    let frame_len = buffer.len();
    buffer.extend_from_slice(b"second frame payload");

    match decode_frame(&buffer, max_frame_len()) {
        Ok(DecodeOutcome::Complete { payload, consumed }) => {
            assert_eq!(consumed, frame_len);
            assert_eq!(payload, input);
        }
        other => panic!("expected a complete frame, got {other:?}"),
    }
}

#[test]
fn encode_rejects_oversized_payload() {
    let limit = max_frame_len();
    let oversized = vec![0u8; limit.get() as usize + 1];

    match encode_frame(&oversized, limit) {
        Err(FramingError::FrameTooLarge { len, max }) => {
            assert_eq!(len, limit.get() + 1);
            assert_eq!(max, limit.get());
        }
        other => panic!("expected FrameTooLarge, got {other:?}"),
    }
}
