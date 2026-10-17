//! Integration tests exercising only the public `storage_node` message-signing
//! API.
//!
//! SRS: `INT-NET-018 / 1` (messages are signed with Ed25519), `INT-NET-020 / 1`
//! (receivers verify the signature), `INT-NET-028 / 1` (Ed25519 is the signature
//! scheme), supporting `NFR-SEC-003 / 1` (signing keys are separate from
//! encryption keys) and `NFR-SEC-006 / 1` (standard cryptographic libraries).

use storage_node::{MessageSigningKey, MessageVerifyingKey, SigningError, SIGNING_KEY_LEN};

const MESSAGE: &[u8] = b"node-to-node raft frame";
const OTHER_MESSAGE: &[u8] = b"node-to-node raft frame!";
const PRIMARY_KEY_BYTES: [u8; SIGNING_KEY_LEN] = [0x11; SIGNING_KEY_LEN];
const ALTERNATE_KEY_BYTES: [u8; SIGNING_KEY_LEN] = [0x22; SIGNING_KEY_LEN];

fn primary_key() -> MessageSigningKey {
    MessageSigningKey::from_bytes(PRIMARY_KEY_BYTES)
}

fn alternate_key() -> MessageSigningKey {
    MessageSigningKey::from_bytes(ALTERNATE_KEY_BYTES)
}

#[test]
fn signs_and_verifies_a_message() {
    let key = primary_key();
    let signature = key.sign(MESSAGE);

    let result = key.verifying_key().verify(MESSAGE, &signature);

    assert_eq!(result, Ok(()));
}

#[test]
fn rejects_a_modified_message() {
    let key = primary_key();
    let signature = key.sign(MESSAGE);

    let result = key.verifying_key().verify(OTHER_MESSAGE, &signature);

    assert_eq!(result, Err(SigningError::InvalidSignature));
}

#[test]
fn rejects_a_tampered_signature() {
    let key = primary_key();
    let mut signature = key.sign(MESSAGE);
    signature[0] ^= 0xff;

    let result = key.verifying_key().verify(MESSAGE, &signature);

    assert_eq!(result, Err(SigningError::InvalidSignature));
}

#[test]
fn rejects_a_signature_from_a_different_key() {
    let signature = primary_key().sign(MESSAGE);

    let result = alternate_key().verifying_key().verify(MESSAGE, &signature);

    assert_eq!(result, Err(SigningError::InvalidSignature));
}

#[test]
fn exposes_the_public_key_bytes() {
    let verifying_key = primary_key().verifying_key();
    let round_tripped = MessageVerifyingKey::from_bytes(verifying_key.as_bytes())
        .expect("bytes from a verifying key parse again");

    assert_eq!(round_tripped.as_bytes(), verifying_key.as_bytes());
}
