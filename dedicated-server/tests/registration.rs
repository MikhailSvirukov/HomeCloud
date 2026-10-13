//! Integration tests that drive the in-memory account registration model
//! through its public API, one behavior per test.

use dedicated_server::registration::{
    AccountRegistry, Login, LoginError, RegistrationError, RegistrationRequest, MAX_LOGIN_LENGTH,
};

const ALICE: &str = "alice";
const BOB: &str = "bob";
const CAROL: &str = "carol";
const MISSING: &str = "missing";

const ALICE_VERIFIER: &[u8] = &[0x01, 0x02, 0x03];
const ALICE_KEY: &[u8] = &[0x0a, 0x0b, 0x0c];
const BOB_VERIFIER: &[u8] = &[0x11, 0x12];
const BOB_KEY: &[u8] = &[0x21];
const REPLACEMENT_VERIFIER: &[u8] = &[0x09, 0x09];
const REPLACEMENT_KEY: &[u8] = &[0x08];

fn login(value: &str) -> Login {
    Login::parse(value).expect("test login must be valid")
}

fn request(name: &str, verifier: &[u8], key: &[u8]) -> RegistrationRequest {
    RegistrationRequest::new(login(name), verifier.to_vec(), key.to_vec())
        .expect("test request must be valid")
}

fn registered_logins(registry: &AccountRegistry) -> Vec<&str> {
    registry
        .records()
        .map(|record| record.login().as_str())
        .collect()
}

#[test]
fn register_two_accounts_echoes_payloads_and_agrees_on_len_and_records() {
    let mut registry = AccountRegistry::new();

    {
        let alice = registry
            .register(request(ALICE, ALICE_VERIFIER, ALICE_KEY))
            .expect("first registration must succeed");
        assert_eq!(alice.login().as_str(), ALICE);
        assert_eq!(alice.password_verifier(), ALICE_VERIFIER);
        assert_eq!(alice.client_encryption_public_key(), ALICE_KEY);
    }
    {
        let bob = registry
            .register(request(BOB, BOB_VERIFIER, BOB_KEY))
            .expect("second registration must succeed");
        assert_eq!(bob.login().as_str(), BOB);
        assert_eq!(bob.password_verifier(), BOB_VERIFIER);
        assert_eq!(bob.client_encryption_public_key(), BOB_KEY);
    }

    assert_eq!(registry.len(), 2);
    assert_eq!(registry.records().count(), 2);
}

#[test]
fn register_rejects_duplicate_login_carrying_the_login_and_keeps_original_bytes() {
    let mut registry = AccountRegistry::new();
    registry
        .register(request(ALICE, ALICE_VERIFIER, ALICE_KEY))
        .expect("first registration must succeed");

    let error = registry
        .register(request(ALICE, REPLACEMENT_VERIFIER, REPLACEMENT_KEY))
        .expect_err("duplicate login must fail");

    assert!(matches!(
        error,
        RegistrationError::LoginAlreadyRegistered(ref login) if login.as_str() == ALICE
    ));

    let stored = registry.get(&login(ALICE)).expect("record must remain");
    assert_eq!(stored.password_verifier(), ALICE_VERIFIER);
    assert_eq!(stored.client_encryption_public_key(), ALICE_KEY);
    assert_eq!(registry.len(), 1);
}

#[test]
fn records_yields_accounts_in_login_order_for_deterministic_replication() {
    let mut registry = AccountRegistry::new();
    for name in [CAROL, ALICE, BOB] {
        registry
            .register(request(name, ALICE_VERIFIER, ALICE_KEY))
            .expect("registration must succeed");
    }

    assert_eq!(registered_logins(&registry), vec![ALICE, BOB, CAROL]);
}

#[test]
fn get_returns_the_record_for_a_registered_login() {
    let mut registry = AccountRegistry::new();
    registry
        .register(request(ALICE, ALICE_VERIFIER, ALICE_KEY))
        .expect("registration must succeed");

    let stored = registry.get(&login(ALICE)).expect("record must be found");

    assert_eq!(stored.login().as_str(), ALICE);
    assert_eq!(stored.password_verifier(), ALICE_VERIFIER);
}

#[test]
fn get_returns_none_for_an_unknown_login() {
    let registry = AccountRegistry::new();

    assert!(registry.get(&login(MISSING)).is_none());
}

#[test]
fn parse_accepts_a_login_at_the_max_length_boundary() {
    let value = "a".repeat(MAX_LOGIN_LENGTH);
    let mut registry = AccountRegistry::new();

    registry
        .register(request(&value, ALICE_VERIFIER, ALICE_KEY))
        .expect("boundary-length login must be accepted");

    let stored = registry
        .get(&login(&value))
        .expect("boundary-length login must be retrievable");
    assert_eq!(stored.login().as_str(), value);
    assert_eq!(stored.login().as_str().chars().count(), MAX_LOGIN_LENGTH);
}

#[test]
fn parse_rejects_an_empty_login_with_the_empty_variant() {
    assert!(matches!(Login::parse(""), Err(LoginError::Empty)));
}

#[test]
fn parse_rejects_a_login_over_the_max_length_with_the_too_long_variant() {
    let value = "a".repeat(MAX_LOGIN_LENGTH + 1);

    assert!(matches!(Login::parse(&value), Err(LoginError::TooLong)));
}

#[test]
fn parse_reports_the_disallowed_character_it_found() {
    assert!(matches!(
        Login::parse("user/name"),
        Err(LoginError::InvalidCharacter('/'))
    ));
}

#[test]
fn request_new_rejects_empty_verifier_and_empty_client_key_distinctly() {
    let empty_verifier = RegistrationRequest::new(login(ALICE), Vec::new(), ALICE_KEY.to_vec());
    let empty_client_key =
        RegistrationRequest::new(login(ALICE), ALICE_VERIFIER.to_vec(), Vec::new());

    assert!(matches!(
        empty_verifier,
        Err(RegistrationError::EmptyPasswordVerifier)
    ));
    assert!(matches!(
        empty_client_key,
        Err(RegistrationError::EmptyClientEncryptionPublicKey)
    ));
}
