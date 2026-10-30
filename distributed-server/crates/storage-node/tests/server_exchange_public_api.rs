//! Integration tests exercising only the public encrypted signed inter-server
//! message composition.
//!
//! Covers `INT-NET-019 / 1`: the signature covers the length prefix, the encoded
//! external header, and the sealed payload. Covers `INT-NET-020 / 1`: the
//! receiver verifies the signature before decrypting the payload. Covers
//! `INT-NET-021 / 3` and `NFR-SEC-009 / 1`: the caller supplies the sender
//! verifying key resolved from the trusted peer registry, so a key that does not
//! match the sender is rejected before any payload is opened. Covers
//! `INT-NET-024 / 2`: the payload is encrypted for the recipient before
//! transmission. Covers `INT-NET-027 / 1`: HPKE Base mode with X25519,
//! HKDF-SHA-256, and AES-256-GCM, using empty `info` and `aad`. Covers
//! `INT-NET-028 / 1`: the frame signature is Ed25519. Covers `INT-NET-037 / 1`:
//! an inter-server message is encrypted for an authenticated recipient. Covers
//! `INT-NET-072 / 2` and `INT-NET-074 / 2`: the decrypted plaintext is the
//! `[message_type, args]` envelope, here the `[3, [rpc_bytes]]` Raft RPC shape.
//! Covers `INT-NET-081 / 1`: the payload is the 32-byte HPKE encapsulated key
//! followed by the AES-256-GCM ciphertext and tag.

use std::num::{NonZeroU32, NonZeroUsize};

use storage_node::{
    decode_server_message, encode_external_header, encode_raft_rpc, encode_server_message,
    ClientLimits, ExternalHeader, FramingError, MessageSigningKey, MessageVerifyingKey,
    RecipientPrivateKey, RecipientPublicKey, SealingError, ServerExchangeError, ServerMessage,
    SignedFrameError, EXTERNAL_PROTOCOL_VERSION, FRAME_LENGTH_PREFIX_LEN, REQUEST_ID_LEN,
    SENDER_ID_LEN, SIGNING_KEY_LEN, X25519_KEY_LEN,
};

const RECIPIENT_PRIVATE_KEY_BYTES: [u8; X25519_KEY_LEN] = [9; X25519_KEY_LEN];
const OTHER_RECIPIENT_PRIVATE_KEY_BYTES: [u8; X25519_KEY_LEN] = [10; X25519_KEY_LEN];
const SIGNING_KEY_BYTES: [u8; SIGNING_KEY_LEN] = [7; SIGNING_KEY_LEN];
const OTHER_SIGNING_KEY_BYTES: [u8; SIGNING_KEY_LEN] = [8; SIGNING_KEY_LEN];
const SENDER_ID: [u8; SENDER_ID_LEN] = [0x11; SENDER_ID_LEN];
const REQUEST_ID: [u8; REQUEST_ID_LEN] = [0x22; REQUEST_ID_LEN];
const MAX_FRAME_LEN: u32 = 4096;
const SMALL_FRAME_LEN: u32 = 64;
const OVERSIZED_PLAINTEXT_LEN: usize = 128;
const OVERSIZED_PLAINTEXT_BYTE: u8 = 0x5A;
const RPC_BYTES: &[u8] = b"raft-rpc-plaintext-marker";

fn recipient_private_key() -> RecipientPrivateKey {
    RecipientPrivateKey::from_bytes(&RECIPIENT_PRIVATE_KEY_BYTES)
        .expect("the fixed X25519 private key is valid")
}

fn other_recipient_private_key() -> RecipientPrivateKey {
    RecipientPrivateKey::from_bytes(&OTHER_RECIPIENT_PRIVATE_KEY_BYTES)
        .expect("the fixed alternative X25519 private key is valid")
}

fn recipient_public_key() -> RecipientPublicKey {
    recipient_private_key().public_key()
}

fn signing_key() -> MessageSigningKey {
    MessageSigningKey::from_bytes(SIGNING_KEY_BYTES)
}

fn verifying_key() -> MessageVerifyingKey {
    signing_key().verifying_key()
}

fn other_verifying_key() -> MessageVerifyingKey {
    MessageSigningKey::from_bytes(OTHER_SIGNING_KEY_BYTES).verifying_key()
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

fn max_frame_len() -> NonZeroU32 {
    NonZeroU32::new(MAX_FRAME_LEN).expect("the test frame limit is non-zero")
}

fn rpc_plaintext() -> Vec<u8> {
    encode_raft_rpc(RPC_BYTES).expect("the test Raft RPC envelope encodes")
}

fn encode(
    header: &ExternalHeader,
    plaintext: &[u8],
    recipient: &RecipientPublicKey,
    max_frame_len: NonZeroU32,
) -> Result<Vec<u8>, ServerExchangeError> {
    encode_server_message(
        header,
        plaintext,
        recipient,
        &signing_key(),
        max_frame_len,
        &limits(),
    )
}

fn decode(
    frame: &[u8],
    sender: &MessageVerifyingKey,
    recipient: &RecipientPrivateKey,
    max_frame_len: NonZeroU32,
) -> Result<ServerMessage, ServerExchangeError> {
    decode_server_message(frame, sender, recipient, max_frame_len, &limits())
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[test]
fn round_trips_an_encrypted_signed_server_message() {
    let header = ordinary_header();
    let plaintext = rpc_plaintext();
    let frame = encode(
        &header,
        &plaintext,
        &recipient_public_key(),
        max_frame_len(),
    )
    .expect("the test message fits the configured frame limit");

    let message = decode(
        &frame,
        &verifying_key(),
        &recipient_private_key(),
        max_frame_len(),
    )
    .expect("the encoded server message decodes");

    assert_eq!(message.header, header);
    assert_eq!(message.plaintext, plaintext);
    assert!(
        frame.as_slice() != plaintext.as_slice(),
        "the encoded frame must differ from the plaintext"
    );
    assert!(
        !contains_subslice(&frame, &plaintext),
        "the encoded frame must not contain the plaintext contiguously"
    );
}

#[test]
fn rejects_a_wrong_sender_signature_before_decryption() {
    let frame = encode(
        &ordinary_header(),
        &rpc_plaintext(),
        &recipient_public_key(),
        max_frame_len(),
    )
    .expect("the test message fits the configured frame limit");

    assert!(matches!(
        decode(
            &frame,
            &other_verifying_key(),
            &recipient_private_key(),
            max_frame_len()
        ),
        Err(ServerExchangeError::Frame(
            SignedFrameError::InvalidSignature
        ))
    ));
}

#[test]
fn rejects_a_wrong_recipient_key() {
    let frame = encode(
        &ordinary_header(),
        &rpc_plaintext(),
        &recipient_public_key(),
        max_frame_len(),
    )
    .expect("the test message fits the configured frame limit");

    assert!(matches!(
        decode(
            &frame,
            &verifying_key(),
            &other_recipient_private_key(),
            max_frame_len()
        ),
        Err(ServerExchangeError::Sealing(SealingError::Open))
    ));
}

#[test]
fn rejects_a_tampered_ciphertext_or_signature() {
    let header = ordinary_header();
    let frame = encode(
        &header,
        &rpc_plaintext(),
        &recipient_public_key(),
        max_frame_len(),
    )
    .expect("the test message fits the configured frame limit");

    let mut tampered_payload = frame.clone();
    let last_byte = tampered_payload
        .last_mut()
        .expect("the encoded frame has a sealed payload");
    *last_byte ^= 0x01;
    assert!(matches!(
        decode(
            &tampered_payload,
            &verifying_key(),
            &recipient_private_key(),
            max_frame_len()
        ),
        Err(ServerExchangeError::Frame(
            SignedFrameError::InvalidSignature
        ))
    ));

    let header_len = encode_external_header(&header, &limits())
        .expect("the test external header encodes")
        .len();
    let mut tampered_signature = frame;
    let signature_byte = &mut tampered_signature[FRAME_LENGTH_PREFIX_LEN + header_len];
    *signature_byte ^= 0x01;
    assert!(matches!(
        decode(
            &tampered_signature,
            &verifying_key(),
            &recipient_private_key(),
            max_frame_len()
        ),
        Err(ServerExchangeError::Frame(
            SignedFrameError::InvalidSignature
        ))
    ));
}

#[test]
fn enforces_the_frame_body_limit() {
    let small_frame_len =
        NonZeroU32::new(SMALL_FRAME_LEN).expect("the test frame limit is non-zero");
    assert!(matches!(
        encode(
            &ordinary_header(),
            &[OVERSIZED_PLAINTEXT_BYTE; OVERSIZED_PLAINTEXT_LEN],
            &recipient_public_key(),
            small_frame_len,
        ),
        Err(ServerExchangeError::Frame(SignedFrameError::Framing(
            FramingError::FrameTooLarge { .. }
        )))
    ));

    let frame = encode(
        &ordinary_header(),
        &[],
        &recipient_public_key(),
        max_frame_len(),
    )
    .expect("an empty plaintext fits the configured frame limit");
    let message = decode(
        &frame,
        &verifying_key(),
        &recipient_private_key(),
        max_frame_len(),
    )
    .expect("an encrypted empty plaintext decodes");
    assert!(message.plaintext.is_empty());
}

#[test]
fn debug_does_not_print_plaintext() {
    let message = decode(
        &encode(
            &ordinary_header(),
            &rpc_plaintext(),
            &recipient_public_key(),
            max_frame_len(),
        )
        .expect("the test message fits the configured frame limit"),
        &verifying_key(),
        &recipient_private_key(),
        max_frame_len(),
    )
    .expect("the encoded server message decodes");

    let rendered = format!("{message:?}");
    assert!(rendered.contains("plaintext_len"));
    assert!(!rendered.contains("raft-rpc-plaintext-marker"));
    assert!(!rendered.contains(&format!("{:?}", message.plaintext)));
}
