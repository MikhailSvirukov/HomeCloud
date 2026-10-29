//! Integration tests exercising only the public signed application frame and
//! bootstrap payload codecs.
//!
//! Covers `INT-NET-018 / 1`: every network message is signed by its sender with
//! Ed25519. Covers `INT-NET-019 / 1`: the signed bytes are the length prefix, the
//! deterministic external header, and the payload. Covers `INT-NET-020 / 1`: the
//! receiver verifies the signature before the payload is used. Covers
//! `INT-NET-021 / 3` and `INT-NET-034 / 1`: the first-contact bootstrap payload
//! carries the server's X25519 and Ed25519 public keys. Covers `INT-NET-028 / 1`:
//! signatures use Ed25519. Covers `INT-NET-081 / 1`: the frame body is the header,
//! the 64-byte signature, and the payload in that order, with the prefix equal to
//! their total length.

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
fn encodes_and_decodes_a_signed_frame() {
    let header = ordinary_header();
    let payload = b"opaque payload";
    let encoded = encode(&header, payload);

    let header_len = encoded_header(&header).len();
    let body_len = declared_body_len(&encoded);

    assert_eq!(body_len, encoded.len() - FRAME_LENGTH_PREFIX_LEN);
    assert_eq!(body_len, header_len + SIGNATURE_LEN + payload.len());

    let decoded = decode(&encoded).expect("the signed frame decodes with the matching key");
    assert_eq!(decoded.header, header);
    assert_eq!(decoded.payload.as_slice(), &payload[..]);

    let rendered = format!("{decoded:?}");
    assert!(rendered.contains("payload_len"));
    assert!(
        !rendered.contains(&format!("{payload:?}")),
        "the debug output omits the payload bytes"
    );
}

#[test]
fn signature_covers_prefix_header_and_payload() {
    let header = ordinary_header();
    let payload = b"opaque payload";
    let encoded = encode(&header, payload);
    let header_bytes = encoded_header(&header);
    let signature_start = FRAME_LENGTH_PREFIX_LEN + header_bytes.len();
    let payload_start = signature_start + SIGNATURE_LEN;

    let mut changed_payload = encoded.clone();
    changed_payload[payload_start] ^= 0x01;
    assert!(matches!(
        decode(&changed_payload),
        Err(SignedFrameError::InvalidSignature)
    ));

    let sender_offset = header_bytes
        .windows(SENDER_ID_LEN)
        .position(|window| window == &SENDER_ID[..])
        .expect("the encoded header contains the sender identifier");
    let mut changed_header = encoded.clone();
    changed_header[FRAME_LENGTH_PREFIX_LEN + sender_offset] ^= 0x01;
    assert!(matches!(
        decode(&changed_header),
        Err(SignedFrameError::InvalidSignature)
    ));

    let mut changed_signature = encoded.clone();
    changed_signature[signature_start] ^= 0x01;
    assert!(matches!(
        decode(&changed_signature),
        Err(SignedFrameError::InvalidSignature)
    ));

    let wrong_key = MessageSigningKey::from_bytes([9; SIGNING_KEY_LEN]).verifying_key();
    assert!(matches!(
        decode_signed_frame(&encoded, &wrong_key, max_body_len(), &limits()),
        Err(SignedFrameError::InvalidSignature)
    ));

    let mut changed_prefix = encoded.clone();
    let shorter_body_len = u32::try_from(declared_body_len(&encoded) - 1)
        .expect("the test frame body length fits in u32");
    changed_prefix[..FRAME_LENGTH_PREFIX_LEN].copy_from_slice(&shorter_body_len.to_be_bytes());
    assert!(matches!(
        decode(&changed_prefix),
        Err(SignedFrameError::LengthMismatch) | Err(SignedFrameError::Framing(_))
    ));
}

#[test]
fn enforces_the_frame_body_limit() {
    let header = ordinary_header();
    let payload = [0u8; 8];
    let small_body_len = NonZeroU32::new(64).expect("the test body limit is non-zero");

    assert!(matches!(
        encode_signed_frame(&header, &payload, &signing_key(), small_body_len, &limits()),
        Err(SignedFrameError::Framing(
            FramingError::FrameTooLarge { .. }
        ))
    ));

    let encoded = encode(&header, &payload);
    assert!(matches!(
        decode_signed_frame(&encoded, &verifying_key(), small_body_len, &limits()),
        Err(SignedFrameError::Framing(
            FramingError::FrameTooLarge { .. }
        ))
    ));

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
fn round_trips_the_bootstrap_payload() {
    let payload = bootstrap_payload();
    let encoded = encode_bootstrap_payload(&payload).expect("the bootstrap payload encodes");
    assert_eq!(encoded.as_slice(), BOOTSTRAP_PAYLOAD_CBOR.as_slice());

    let decoded = decode_bootstrap_payload(&encoded).expect("the bootstrap payload decodes");
    assert_eq!(decoded, payload);

    let mut wrong_arity = vec![0x81, 0x58, 0x20];
    wrong_arity.extend(X25519_PUBLIC_KEY);
    assert!(matches!(
        decode_bootstrap_payload(&wrong_arity),
        Err(SignedFrameError::Bootstrap)
    ));

    let mut wrong_length = vec![0x82, 0x58, 0x1F];
    wrong_length.extend([0x01; BOOTSTRAP_KEY_LEN - 1]);
    wrong_length.extend([0x58, 0x20]);
    wrong_length.extend(ED25519_PUBLIC_KEY);
    assert!(matches!(
        decode_bootstrap_payload(&wrong_length),
        Err(SignedFrameError::Bootstrap)
    ));

    let mut trailing = BOOTSTRAP_PAYLOAD_CBOR.to_vec();
    trailing.push(0x00);
    assert!(matches!(
        decode_bootstrap_payload(&trailing),
        Err(SignedFrameError::Decode)
    ));
}
