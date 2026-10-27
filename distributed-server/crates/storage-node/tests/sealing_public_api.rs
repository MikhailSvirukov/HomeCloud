//! Integration tests exercising only the public `storage_node` message-sealing
//! API.
//!
//! SRS: `INT-NET-027 / 1` (HPKE Base mode with DHKEM(X25519, HKDF-SHA-256),
//! HKDF-SHA-256, and AES-256-GCM), supporting `INT-NET-015 / 1` (the logical
//! header is encrypted with the payload), `INT-NET-024 / 2` (each application
//! frame is encrypted for its recipient before transmission), `INT-NET-033 / 1`
//! (the reply is encrypted to the requesting installation's X25519 public key),
//! and `NFR-SEC-006 / 1` (cryptography uses vetted standard-scheme libraries).

use storage_node::{
    open, seal, RecipientPrivateKey, RecipientPublicKey, SealingError, X25519_KEY_LEN,
};

const INFO: &[u8] = b"homecloud-v1";
const OTHER_INFO: &[u8] = b"homecloud-v2";
const AAD: &[u8] = b"header";
const OTHER_AAD: &[u8] = b"other-header";
const MESSAGE: &[u8] = b"inter-server payload";
const PRIMARY_KEY_BYTES: [u8; X25519_KEY_LEN] = [0x11; X25519_KEY_LEN];
const ALTERNATE_KEY_BYTES: [u8; X25519_KEY_LEN] = [0x22; X25519_KEY_LEN];

fn private_key(bytes: [u8; X25519_KEY_LEN]) -> RecipientPrivateKey {
    RecipientPrivateKey::from_bytes(bytes).expect("a 32-byte private key is valid")
}

fn seal_message(recipient: &RecipientPublicKey, plaintext: &[u8]) -> Vec<u8> {
    seal(recipient, INFO, AAD, plaintext).expect("sealing succeeds for a valid recipient key")
}

#[test]
fn seals_and_opens_a_message() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let sealed = seal_message(&private.public_key(), MESSAGE);

    assert!(sealed.len() > X25519_KEY_LEN);

    let opened = open(&private, INFO, AAD, &sealed).expect("the sealed message opens");

    assert_eq!(opened.as_slice(), MESSAGE);
}

#[test]
fn round_trips_an_empty_plaintext() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let sealed = seal_message(&private.public_key(), b"");

    let opened = open(&private, INFO, AAD, &sealed).expect("the sealed message opens");

    assert!(opened.is_empty());
}

#[test]
fn rejects_a_wrong_recipient_key() {
    let recipient = private_key(PRIMARY_KEY_BYTES);
    let other = private_key(ALTERNATE_KEY_BYTES);
    let sealed = seal_message(&recipient.public_key(), MESSAGE);

    let opened = open(&other, INFO, AAD, &sealed);

    assert!(matches!(opened, Err(SealingError::Open)));
}

#[test]
fn rejects_a_modified_ciphertext() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let mut sealed = seal_message(&private.public_key(), MESSAGE);
    let last = sealed
        .last_mut()
        .expect("a sealed message has a ciphertext");
    *last ^= 0xff;

    let opened = open(&private, INFO, AAD, &sealed);

    assert!(matches!(opened, Err(SealingError::Open)));
}

#[test]
fn rejects_a_modified_aad() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let sealed = seal_message(&private.public_key(), MESSAGE);

    let opened = open(&private, INFO, OTHER_AAD, &sealed);

    assert!(matches!(opened, Err(SealingError::Open)));
}

#[test]
fn rejects_a_modified_info() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let sealed = seal_message(&private.public_key(), MESSAGE);

    let opened = open(&private, OTHER_INFO, AAD, &sealed);

    assert!(matches!(opened, Err(SealingError::Open)));
}

#[test]
fn rejects_a_truncated_sealed_message() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let sealed = seal_message(&private.public_key(), MESSAGE);

    let without_full_key = open(&private, INFO, AAD, &sealed[..X25519_KEY_LEN - 1]);
    let empty = open(&private, INFO, AAD, &[]);

    assert!(matches!(without_full_key, Err(SealingError::Truncated)));
    assert!(matches!(empty, Err(SealingError::Truncated)));
}

#[test]
fn uses_a_fresh_encapsulated_key_per_message() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let recipient = private.public_key();
    let first = seal_message(&recipient, MESSAGE);
    let second = seal_message(&recipient, MESSAGE);

    assert_ne!(first, second);

    let first_opened = open(&private, INFO, AAD, &first).expect("the first message opens");
    let second_opened = open(&private, INFO, AAD, &second).expect("the second message opens");

    assert_eq!(first_opened.as_slice(), MESSAGE);
    assert_eq!(second_opened.as_slice(), MESSAGE);
}

#[test]
fn exposes_the_recipient_public_key() {
    let private = private_key(PRIMARY_KEY_BYTES);
    let public = private.public_key();

    let reparsed = RecipientPublicKey::from_bytes(public.as_bytes())
        .expect("bytes from a derived public key parse again");

    assert_eq!(reparsed.as_bytes(), public.as_bytes());
}
