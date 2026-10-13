//! Integration tests exercising the public frame codec API of `storage_node`.

use storage_node::{
    decode_frame, encode_frame, DecodeOutcome, FrameHeader, FramingError, FRAME_HEADER_LEN,
    MAX_FRAME_PAYLOAD_LEN, PROTOCOL_VERSION,
};

#[test]
fn encode_then_decode_round_trips() {
    let input = b"round trip payload".as_slice();
    let encoded = encode_frame(input).expect("payload is within the limit");
    assert_eq!(encoded[0], PROTOCOL_VERSION);

    match decode_frame(&encoded) {
        Ok(DecodeOutcome::Complete {
            header,
            payload,
            consumed,
        }) => {
            assert_eq!(consumed, FRAME_HEADER_LEN + input.len());
            assert_eq!(payload, input);
            assert_eq!(header.payload_len(), input.len() as u32);
        }
        other => panic!("expected a complete frame, got {other:?}"),
    }
}

#[test]
fn decode_ignores_trailing_bytes() {
    let input = b"first frame".as_slice();
    let mut buffer = encode_frame(input).expect("payload is within the limit");
    buffer.extend_from_slice(b"second frame payload");

    match decode_frame(&buffer) {
        Ok(DecodeOutcome::Complete {
            payload, consumed, ..
        }) => {
            assert_eq!(consumed, FRAME_HEADER_LEN + input.len());
            assert_eq!(payload, input);
        }
        other => panic!("expected a complete frame, got {other:?}"),
    }
}

#[test]
fn decode_incomplete_header() {
    let short = [PROTOCOL_VERSION, 0, 0];

    match decode_frame(&short) {
        Ok(DecodeOutcome::Incomplete { needed }) => {
            assert_eq!(needed, FRAME_HEADER_LEN);
        }
        other => panic!("expected an incomplete header, got {other:?}"),
    }
}

#[test]
fn decode_incomplete_payload() {
    let declared_len = 8u32;
    let header = FrameHeader::new(declared_len).expect("8 bytes is within the limit");
    let mut buffer = header.encode().to_vec();
    buffer.extend_from_slice(&[0u8; 3]);

    match decode_frame(&buffer) {
        Ok(DecodeOutcome::Incomplete { needed }) => {
            assert_eq!(needed, FRAME_HEADER_LEN + declared_len as usize);
        }
        other => panic!("expected an incomplete payload, got {other:?}"),
    }
}

#[test]
fn decode_rejects_unsupported_version() {
    let bytes = [PROTOCOL_VERSION.wrapping_add(1), 0, 0, 0, 0];

    match decode_frame(&bytes) {
        Err(FramingError::UnsupportedVersion) => {}
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn encode_rejects_oversized_payload() {
    let oversized = vec![0u8; MAX_FRAME_PAYLOAD_LEN as usize + 1];

    match encode_frame(&oversized) {
        Err(FramingError::PayloadTooLarge) => {}
        other => panic!("expected PayloadTooLarge, got {other:?}"),
    }
}
