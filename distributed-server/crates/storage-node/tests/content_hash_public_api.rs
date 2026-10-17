//! Public `ContentHash` digest behaviour (`INT-NET-031 / 1`).
//!
//! The digest is computed from the original uncompressed bytes.

use storage_node::{ContentHash, CONTENT_HASH_LEN};

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn digest_of_empty_input_matches_the_known_vector() {
    assert_eq!(ContentHash::digest(b"").to_hex(), EMPTY_SHA256);
}

#[test]
fn digest_of_abc_matches_the_known_vector() {
    assert_eq!(ContentHash::digest(b"abc").to_hex(), ABC_SHA256);
}

#[test]
fn digest_round_trips_through_hex() {
    let hash = ContentHash::digest(b"abc");
    let hex = hash.to_hex();

    assert_eq!(hash.as_bytes().len(), CONTENT_HASH_LEN);
    assert_eq!(hex.len(), CONTENT_HASH_LEN * 2);
    assert!(hex
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')));
    assert_eq!(ContentHash::parse_hex(&hex), Ok(hash));
}

#[test]
fn different_inputs_have_different_digests() {
    assert_ne!(ContentHash::digest(b"a"), ContentHash::digest(b"b"));
}
