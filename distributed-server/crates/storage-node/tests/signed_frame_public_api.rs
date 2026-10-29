//! Integration tests exercising only the public signed application frame and
//! bootstrap payload codecs.
//!
//! Covers `INT-NET-018 / 1`: every network message is signed by its sender with
//! Ed25519. Covers `INT-NET-019 / 1`: the signed bytes are the length prefix, the
//! deterministic external header, and the payload. Covers `INT-NET-020 / 1`: the
//! receiver verifies the signature before the payload is used. Covers
//! `INT-NET-034 / 1`: the first-contact bootstrap payload carries the server's
//! X25519 and Ed25519 public keys. Covers `INT-NET-028 / 1`: signatures use
//! Ed25519. Covers `INT-NET-081 / 1`: the frame body is the header, the 64-byte
//! signature, and the payload in that order, with the prefix equal to their total
//! length.

use std::num::{NonZeroU32, NonZeroUsize};

use storage_node::{
    decode_bootstrap_payload, decode_signed_frame, encode_bootstrap_payload,
    encode_external_header, encode_signed_frame, BootstrapPayload, ClientLimits, ExternalHeader,
    FramingError, MessageSigningKey, MessageVerifyingKey, SignedApplicationFrame, SignedFrameError,
    BOOTSTRAP_KEY_LEN, EXTERNAL_PROTOCOL_VERSION, FRAME_LENGTH_PREFIX_LEN, REQUEST_ID_LEN,
    SENDER_ID_LEN, SIGNATURE_LEN, SIGNING_KEY_LEN,
};

const SIGNING_KEY_BYTES: [u8; SIGNING_KEY_LEN] = [7; SIGNING_KEY_LEN];
const SENDER_ID: [u8; SENDER_ID_LEN] = [0x11; SENDER_ID_LEN];
const REQUEST_ID: [u8; REQUEST_ID_LEN] = [0x22; REQUEST_ID_LEN];
const MAX_BODY_LEN: u32 = 4096;
const PAYLOAD: &[u8] = b"opaque payload";

const X25519_PUBLIC_KEY: [u8; BOOTSTRAP_KEY_LEN] = [0x01; BOOTSTRAP_KEY_LEN];
const ED25519_PUBLIC_KEY: [u8; BOOTSTRAP_KEY_LEN] = [0x02; BOOTSTRAP_KEY_LEN];

/// CBOR `[x25519_public_key, ed25519_public_key]` with two 32-byte byte strings.
const BOOTSTRAP_PAYLOAD_CBOR: [u8; 69] = [
    0x82, 0x58, 0x20, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x58, 0x20, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
    0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
    0x02, 0x02, 0x02, 0x02, 0x02,
];

fn signing_key() -> MessageSigningKey {
    MessageSigningKey::from_bytes(SIGNING_KEY_BYTES)
}

fn verifying_key() -> MessageVerifyingKey {
    signing_key().verifying_key()
}

fn ordinary_header() -> ExternalHeader {
    ExternalHeader {
        protocol_version: EXTERNAL_PROTOCOL_VERSION,
        sender_id: Some(SENDER_ID),
        request_id: REQUEST_ID,
        frame_number: 0,
        upload_id: None,
        responder_label: None,
        bootstrap_signing_public_key: None,
    }
}

fn bootstrap_payload() -> BootstrapPayload {
    BootstrapPayload {
        x25519_public_key: X25519_PUBLIC_KEY,
        ed25519_public_key: ED25519_PUBLIC_KEY,
    }
}

fn limits() -> ClientLimits {
    let non_zero = |value| NonZeroUsize::new(value).expect("the test client limit is non-zero");
    ClientLimits::new(
        non_zero(1),
        non_zero(64),
        non_zero(1),
        non_zero(128),
        non_zero(64),
    )
    .expect("the test client limits are consistent")
}

fn max_body_len() -> NonZeroU32 {
    NonZeroU32::new(MAX_BODY_LEN).expect("the test body limit is non-zero")
}

fn encoded_header(header: &ExternalHeader) -> Vec<u8> {
    encode_external_header(header, &limits()).expect("the test external header encodes")
}

fn encode(header: &ExternalHeader, payload: &[u8]) -> Vec<u8> {
    encode_signed_frame(header, payload, &signing_key(), max_body_len(), &limits())
        .expect("the test frame fits the configured body limit")
}

fn decode(frame: &[u8]) -> Result<SignedApplicationFrame, SignedFrameError> {
    decode_signed_frame(frame, &verifying_key(), max_body_len(), &limits())
}

fn declared_body_len(frame: &[u8]) -> usize {
    let prefix: [u8; FRAME_LENGTH_PREFIX_LEN] = frame[..FRAME_LENGTH_PREFIX_LEN]
        .try_into()
        .expect("a signed frame starts with a four-byte length prefix");
    usize::try_from(u32::from_be_bytes(prefix)).expect("the test frame body length fits in a usize")
}

#[test]
fn encodes_the_frame_body_in_fixed_order() {
    let header = ordinary_header();
    let header_bytes = encoded_header(&header);
    let encoded = encode(&header, PAYLOAD);

    assert_eq!(
        declared_body_len(&encoded),
        encoded.len() - FRAME_LENGTH_PREFIX_LEN
    );
    assert_eq!(
        declared_body_len(&encoded),
        header_bytes.len() + SIGNATURE_LEN + PAYLOAD.len()
    );
    assert_eq!(
        &encoded[FRAME_LENGTH_PREFIX_LEN..FRAME_LENGTH_PREFIX_LEN + header_bytes.len()],
        header_bytes.as_slice()
    );
    assert_eq!(
        &encoded[FRAME_LENGTH_PREFIX_LEN + header_bytes.len() + SIGNATURE_LEN..],
        PAYLOAD
    );
}

#[test]
fn round_trips_a_signed_frame() {
    let header = ordinary_header();
    let decoded = decode(&encode(&header, PAYLOAD)).expect("the signed frame decodes");
    assert_eq!(decoded.header, header);
    assert_eq!(decoded.payload.as_slice(), PAYLOAD);
}

#[test]
fn debug_reports_lengths_not_payload_or_signature_bytes() {
    let decoded = decode(&encode(&ordinary_header(), PAYLOAD)).expect("the signed frame decodes");
    let rendered = format!("{decoded:?}");
    assert!(rendered.contains("payload_len"));
    assert!(!rendered.contains("opaque"));
    assert!(!rendered.contains(&format!("{PAYLOAD:?}")));
}

#[test]
fn rejects_a_tampered_payload() {
    let encoded = encode(&ordinary_header(), PAYLOAD);
    let mut changed = encoded.clone();
    let last = changed.len() - 1;
    changed[last] ^= 0x01;
    assert!(matches!(
        decode(&changed),
        Err(SignedFrameError::InvalidSignature)
    ));
}

#[test]
fn rejects_a_tampered_header() {
    let header = ordinary_header();
    let header_bytes = encoded_header(&header);
    let sender_offset = header_bytes
        .windows(SENDER_ID_LEN)
        .position(|window| window == &SENDER_ID[..])
        .expect("the encoded header contains the sender identifier");
    let mut changed = encode(&header, PAYLOAD);
    changed[FRAME_LENGTH_PREFIX_LEN + sender_offset] ^= 0x01;
    assert!(matches!(
        decode(&changed),
        Err(SignedFrameError::InvalidSignature)
    ));
}

#[test]
fn rejects_a_tampered_signature() {
    let header = ordinary_header();
    let signature_start = FRAME_LENGTH_PREFIX_LEN + encoded_header(&header).len();
    let mut changed = encode(&header, PAYLOAD);
    changed[signature_start] ^= 0x01;
    assert!(matches!(
        decode(&changed),
        Err(SignedFrameError::InvalidSignature)
    ));
}

#[test]
fn rejects_a_wrong_verifying_key() {
    let encoded = encode(&ordinary_header(), PAYLOAD);
    let wrong_key = MessageSigningKey::from_bytes([9; SIGNING_KEY_LEN]).verifying_key();
    assert!(matches!(
        decode_signed_frame(&encoded, &wrong_key, max_body_len(), &limits()),
        Err(SignedFrameError::InvalidSignature)
    ));
}

#[test]
fn rejects_a_signature_made_over_a_different_prefix() {
    let mut changed = encode(&ordinary_header(), PAYLOAD);
    let longer_body = u32::try_from(declared_body_len(&changed) + 1)
        .expect("the test frame body length fits in u32");
    changed[..FRAME_LENGTH_PREFIX_LEN].copy_from_slice(&longer_body.to_be_bytes());
    changed.push(0);
    assert!(matches!(
        decode(&changed),
        Err(SignedFrameError::InvalidSignature)
    ));
}

#[test]
fn rejects_an_oversized_encoded_body() {
    let small_body_len = NonZeroU32::new(64).expect("the test body limit is non-zero");
    assert!(matches!(
        encode_signed_frame(
            &ordinary_header(),
            &[0u8; 8],
            &signing_key(),
            small_body_len,
            &limits()
        ),
        Err(SignedFrameError::Framing(
            FramingError::FrameTooLarge { .. }
        ))
    ));
}

#[test]
fn rejects_an_oversized_declared_body() {
    let encoded = encode(&ordinary_header(), PAYLOAD);
    let small_body_len = NonZeroU32::new(64).expect("the test body limit is non-zero");
    assert!(matches!(
        decode_signed_frame(&encoded, &verifying_key(), small_body_len, &limits()),
        Err(SignedFrameError::Framing(
            FramingError::FrameTooLarge { .. }
        ))
    ));
}

#[test]
fn rejects_a_frame_shorter_than_the_prefix() {
    assert!(matches!(
        decode_signed_frame(
            &[0; FRAME_LENGTH_PREFIX_LEN - 1],
            &verifying_key(),
            max_body_len(),
            &limits()
        ),
        Err(SignedFrameError::TooShort)
    ));
}

#[test]
fn rejects_a_body_shorter_than_the_header_and_signature() {
    let header_bytes = encoded_header(&ordinary_header());
    let mut body = header_bytes;
    body.extend([0u8; 10]);
    let body_len = u32::try_from(body.len()).expect("the test body length fits in u32");
    let mut frame = body_len.to_be_bytes().to_vec();
    frame.extend(body);
    assert!(matches!(decode(&frame), Err(SignedFrameError::TooShort)));
}

#[test]
fn rejects_malformed_header_cbor() {
    let mut body = vec![0xff];
    body.extend([0u8; SIGNATURE_LEN]);
    let body_len = u32::try_from(body.len()).expect("the test body length fits in u32");
    let mut frame = body_len.to_be_bytes().to_vec();
    frame.extend(body);
    assert!(matches!(
        decode(&frame),
        Err(SignedFrameError::Decode) | Err(SignedFrameError::Header(_))
    ));
}

#[test]
fn rejects_a_declared_length_greater_than_the_received_body() {
    let mut changed = encode(&ordinary_header(), PAYLOAD);
    let longer_body = u32::try_from(declared_body_len(&changed) + 1)
        .expect("the test frame body length fits in u32");
    changed[..FRAME_LENGTH_PREFIX_LEN].copy_from_slice(&longer_body.to_be_bytes());
    assert!(matches!(
        decode(&changed),
        Err(SignedFrameError::LengthMismatch)
    ));
}

#[test]
fn rejects_trailing_bytes_after_the_declared_body() {
    let mut changed = encode(&ordinary_header(), PAYLOAD);
    changed.push(0);
    assert!(matches!(
        decode(&changed),
        Err(SignedFrameError::LengthMismatch)
    ));
}

#[test]
fn encodes_the_bootstrap_payload_golden() {
    let encoded =
        encode_bootstrap_payload(&bootstrap_payload()).expect("the bootstrap payload encodes");
    assert_eq!(encoded.as_slice(), BOOTSTRAP_PAYLOAD_CBOR.as_slice());
}

#[test]
fn round_trips_the_bootstrap_payload() {
    let payload = bootstrap_payload();
    let encoded = encode_bootstrap_payload(&payload).expect("the bootstrap payload encodes");
    let decoded = decode_bootstrap_payload(&encoded).expect("the bootstrap payload decodes");
    assert_eq!(decoded, payload);
}

#[test]
fn rejects_bootstrap_wrong_arity() {
    let mut wrong_arity = vec![0x81, 0x58, 0x20];
    wrong_arity.extend(X25519_PUBLIC_KEY);
    assert!(matches!(
        decode_bootstrap_payload(&wrong_arity),
        Err(SignedFrameError::Bootstrap)
    ));
}

#[test]
fn rejects_bootstrap_wrong_key_length() {
    let mut wrong_length = vec![0x82, 0x58, 0x1F];
    wrong_length.extend([0x01; BOOTSTRAP_KEY_LEN - 1]);
    wrong_length.extend([0x58, 0x20]);
    wrong_length.extend(ED25519_PUBLIC_KEY);
    assert!(matches!(
        decode_bootstrap_payload(&wrong_length),
        Err(SignedFrameError::Bootstrap)
    ));
}

#[test]
fn rejects_bootstrap_trailing_bytes() {
    let mut trailing = BOOTSTRAP_PAYLOAD_CBOR.to_vec();
    trailing.push(0x00);
    assert!(matches!(
        decode_bootstrap_payload(&trailing),
        Err(SignedFrameError::Decode)
    ));
}
