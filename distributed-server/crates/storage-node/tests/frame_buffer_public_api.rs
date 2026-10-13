//! Integration tests exercising the public receive-side frame buffer API.

use storage_node::{
    encode_frame, FrameBuffer, FramingError, FRAME_HEADER_LEN, MAX_BUFFERED_FRAME_LEN,
    PROTOCOL_VERSION,
};

fn frame(payload: &[u8]) -> Vec<u8> {
    encode_frame(payload).expect("test payload is within the frame limit")
}

#[test]
fn frame_is_yielded_only_after_the_completing_chunk() {
    let encoded = frame(b"chunked-frame");
    let split = FRAME_HEADER_LEN + 2;
    let (first, second) = encoded.split_at(split);
    let mut buffer = FrameBuffer::new();

    buffer
        .extend(first)
        .expect("first chunk fits in the buffer");

    match buffer.next_frame() {
        Ok(None) => {}
        other => panic!("expected an incomplete frame, got {other:?}"),
    }
    assert_eq!(buffer.buffered_len(), first.len());

    buffer
        .extend(second)
        .expect("second chunk fits in the buffer");

    match buffer.next_frame() {
        Ok(Some(frame)) => assert_eq!(frame, encoded),
        other => panic!("expected a complete frame, got {other:?}"),
    }
    assert!(buffer.is_empty());
}

#[test]
fn two_frames_are_yielded_one_at_a_time_in_order() {
    let first = frame(b"one");
    let second = frame(b"two");
    let mut stream = first.clone();
    stream.extend_from_slice(&second);
    let mut buffer = FrameBuffer::new();

    buffer
        .extend(&stream)
        .expect("two frames fit in the buffer");

    match buffer.next_frame() {
        Ok(Some(frame)) => assert_eq!(frame, first),
        other => panic!("expected the first frame, got {other:?}"),
    }
    match buffer.next_frame() {
        Ok(Some(frame)) => assert_eq!(frame, second),
        other => panic!("expected the second frame, got {other:?}"),
    }
    match buffer.next_frame() {
        Ok(None) => {}
        other => panic!("expected no further frame, got {other:?}"),
    }
}

#[test]
fn partial_frame_reports_none_and_grows() {
    let encoded = frame(b"payload");
    let mut buffer = FrameBuffer::new();

    buffer
        .extend(&encoded[..FRAME_HEADER_LEN])
        .expect("header prefix fits in the buffer");

    assert!(buffer.buffered_len() > 0);
    match buffer.next_frame() {
        Ok(None) => {}
        other => panic!("expected an incomplete frame, got {other:?}"),
    }
    assert_eq!(buffer.buffered_len(), FRAME_HEADER_LEN);
}

#[test]
fn extend_beyond_max_is_rejected_and_leaves_buffer_unchanged() {
    let mut buffer = FrameBuffer::new();
    let oversized = vec![0u8; MAX_BUFFERED_FRAME_LEN + 1];

    let result = buffer.extend(&oversized);

    assert_eq!(result, Err(FramingError::PayloadTooLarge));
    assert_eq!(buffer.buffered_len(), 0);
    assert!(buffer.is_empty());
}

#[test]
fn next_frame_on_empty_buffer_is_none() {
    let mut buffer = FrameBuffer::new();

    assert_eq!(buffer.next_frame(), Ok(None));
}

#[test]
fn unsupported_version_header_is_rejected() {
    let bytes = [PROTOCOL_VERSION.wrapping_add(1), 0, 0, 0, 0];
    let mut buffer = FrameBuffer::new();

    buffer.extend(&bytes).expect("header fits in the buffer");

    match buffer.next_frame() {
        Err(FramingError::UnsupportedVersion) => {}
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}
