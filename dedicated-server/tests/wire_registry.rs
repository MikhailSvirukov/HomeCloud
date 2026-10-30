//! External construction of the registry messages from the crate root.
//!
//! These tests prove that the registry records are part of the public API, so
//! an external consumer such as a test fixture can build and parse message
//! types 5 and 6 without reaching into the private wire module.

use dedicated_server::{
    AccountRecord, InstallationId, InstallationRecord, InterServerMessage, RegistrationRecord,
    RegistrySnapshot, WireError, IDENTIFIER_BYTES, KEY_BYTES,
};

const MAX_VERIFIER_BYTES: usize = 512;

fn snapshot() -> RegistrySnapshot {
    RegistrySnapshot {
        read_index: 7,
        account: Some(AccountRecord {
            verifier: "phc".to_owned(),
        }),
        registration: Some(RegistrationRecord {
            content_tag: [0x22; KEY_BYTES],
            installation_id: [0x33; IDENTIFIER_BYTES],
        }),
        installation: Some(InstallationRecord {
            login: "alice".to_owned(),
            installation_id: [0x33; IDENTIFIER_BYTES],
            x25519_key: [0x44; KEY_BYTES],
            ed25519_key: [0x55; KEY_BYTES],
        }),
    }
}

#[test]
fn an_external_consumer_can_build_and_parse_a_lookup_result() {
    let message = InterServerMessage::RegistryLookupResult {
        code: 0,
        snapshot: Some(snapshot()),
    };

    let bytes = message
        .encode(MAX_VERIFIER_BYTES)
        .expect("the message must encode");
    let decoded =
        InterServerMessage::decode(&bytes, MAX_VERIFIER_BYTES).expect("the message must decode");

    assert_eq!(decoded, message);
}

#[test]
fn an_external_consumer_can_build_a_registration_propose_with_a_uuid() {
    let installation = InstallationId::from_bytes({
        let mut bytes = [0xAA; IDENTIFIER_BYTES];
        bytes[6] = 0x47;
        bytes[8] = 0x99;
        bytes
    })
    .expect("a UUIDv4 installation");

    let message = InterServerMessage::RegistrationPropose {
        login: "alice".to_owned(),
        registration_request_id: [0x11; IDENTIFIER_BYTES],
        content_tag: [0x22; KEY_BYTES],
        installation_id: *installation.as_bytes(),
        x25519_key: [0x44; KEY_BYTES],
        ed25519_key: [0x55; KEY_BYTES],
        expected_verifier: None,
        new_verifier: Some("phc".to_owned()),
    };

    let bytes = message
        .encode(MAX_VERIFIER_BYTES)
        .expect("the message must encode");
    assert_eq!(
        InterServerMessage::decode(&bytes, MAX_VERIFIER_BYTES).expect("the message must decode"),
        message
    );
}

#[test]
fn an_oversized_verifier_is_rejected_through_the_public_api() {
    let message = InterServerMessage::RegistrationPropose {
        login: "alice".to_owned(),
        registration_request_id: [0x11; IDENTIFIER_BYTES],
        content_tag: [0x22; KEY_BYTES],
        installation_id: [0x33; IDENTIFIER_BYTES],
        x25519_key: [0x44; KEY_BYTES],
        ed25519_key: [0x55; KEY_BYTES],
        expected_verifier: None,
        new_verifier: Some("abcd".to_owned()),
    };

    assert!(matches!(
        message.encode(3),
        Err(WireError::VerifierTooLong { actual: 4, max: 3 })
    ));
}
