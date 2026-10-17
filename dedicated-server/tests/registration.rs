//! Integration tests for account and installation registration.
//!
//! These tests drive the public crate API from a consumer's perspective; the
//! module unit tests cover the detailed replay semantics.

use std::num::{NonZeroU32, NonZeroUsize};

use dedicated_server::{
    AccountRegistry, Argon2Params, InstallationKeys, Login, RegistrationError, RegistrationOutcome,
    RegistrationRequest, RequestId,
};

const LOGIN: &str = "user-1.2_x";
const PASSWORD: &str = "correct horse battery staple";
const WRONG_PASSWORD: &str = "correct horse battery stapler";
const MEMORY_KIB: u32 = 8;
const ITERATIONS: u32 = 1;
const PARALLELISM: u32 = 1;
const SALT_LENGTH_BYTES: usize = 16;
const FIRST_REQUEST: [u8; 16] = [0x01; 16];
const SECOND_REQUEST: [u8; 16] = [0x02; 16];

fn non_zero(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("test values must be non-zero")
}

fn params() -> Argon2Params {
    Argon2Params {
        memory_kib: non_zero(MEMORY_KIB),
        iterations: non_zero(ITERATIONS),
        parallelism: non_zero(PARALLELISM),
        salt_length_bytes: NonZeroUsize::new(SALT_LENGTH_BYTES)
            .expect("test salt length must be non-zero"),
    }
}

fn login(value: &str) -> Login {
    Login::parse(value).expect("test login must be valid")
}

fn keys(seed: u8) -> InstallationKeys {
    InstallationKeys {
        encryption_public_key: [seed; 32],
        signing_public_key: [seed.wrapping_add(1); 32],
    }
}

fn assert_keys(actual: &InstallationKeys, expected: InstallationKeys) {
    assert_eq!(actual.encryption_public_key, expected.encryption_public_key);
    assert_eq!(actual.signing_public_key, expected.signing_public_key);
}

fn request(
    request_id: [u8; 16],
    login_name: &str,
    password: &str,
    keys: InstallationKeys,
) -> RegistrationRequest {
    RegistrationRequest {
        request_id: RequestId::from_bytes(request_id),
        login: login(login_name),
        password: password.to_owned(),
        keys,
    }
}

#[test]
fn first_registration_creates_one_account_with_one_installation() {
    let mut registry = AccountRegistry::new(params());
    let keys = keys(0x11);

    let outcome = registry
        .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys))
        .expect("first registration must succeed");

    let installation_id = match outcome {
        RegistrationOutcome::AccountCreated { installation_id } => installation_id,
        other => panic!("expected a created account, got {other:?}"),
    };
    let account = registry.account(&login(LOGIN)).expect("account must exist");
    let installation = account
        .installations
        .get(&installation_id)
        .expect("installation must exist");

    assert_eq!(account.login, login(LOGIN));
    assert_eq!(account.installations.len(), 1);
    assert_eq!(installation.id, installation_id);
    assert_keys(&installation.keys, keys);
}

#[test]
fn second_registration_adds_an_installation_to_the_existing_account() {
    let mut registry = AccountRegistry::new(params());
    let first_id = match registry
        .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys(0x21)))
        .expect("first registration must succeed")
    {
        RegistrationOutcome::AccountCreated { installation_id } => installation_id,
        other => panic!("expected a created account, got {other:?}"),
    };

    let second_id = match registry
        .register(request(SECOND_REQUEST, LOGIN, PASSWORD, keys(0x22)))
        .expect("second registration must succeed")
    {
        RegistrationOutcome::InstallationAdded { installation_id } => installation_id,
        other => panic!("expected an added installation, got {other:?}"),
    };

    let account = registry.account(&login(LOGIN)).expect("account must exist");

    assert_ne!(first_id, second_id);
    assert_eq!(account.installations.len(), 2);
    assert!(account.installations.contains_key(&first_id));
    assert!(account.installations.contains_key(&second_id));
}

#[test]
fn wrong_password_is_rejected_without_changing_the_account() {
    let mut registry = AccountRegistry::new(params());
    let first_keys = keys(0x31);
    let first_id = match registry
        .register(request(FIRST_REQUEST, LOGIN, PASSWORD, first_keys))
        .expect("first registration must succeed")
    {
        RegistrationOutcome::AccountCreated { installation_id } => installation_id,
        other => panic!("expected a created account, got {other:?}"),
    };
    let verifier_before = registry
        .account(&login(LOGIN))
        .expect("account must exist")
        .verifier
        .as_str()
        .to_owned();

    let error = registry
        .register(request(SECOND_REQUEST, LOGIN, WRONG_PASSWORD, keys(0x32)))
        .expect_err("wrong password must be rejected");

    let account = registry
        .account(&login(LOGIN))
        .expect("account must remain");
    assert!(matches!(error, RegistrationError::PasswordRejected));
    assert_eq!(account.installations.len(), 1);
    assert_keys(
        &account
            .installations
            .get(&first_id)
            .expect("first installation must remain")
            .keys,
        first_keys,
    );
    assert_eq!(account.verifier.as_str(), verifier_before);
}

#[test]
fn repeated_request_returns_the_outcome_and_different_content_conflicts() {
    let mut registry = AccountRegistry::new(params());
    let first_keys = keys(0x41);
    let first = registry
        .register(request(FIRST_REQUEST, LOGIN, PASSWORD, first_keys))
        .expect("first registration must succeed");

    let repeated = registry
        .register(request(FIRST_REQUEST, LOGIN, PASSWORD, first_keys))
        .expect("repeated registration must succeed");
    let conflict = registry
        .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys(0x42)))
        .expect_err("different content must conflict");

    assert_eq!(repeated, first);
    assert!(matches!(conflict, RegistrationError::RequestIdConflict));
    assert_eq!(
        registry
            .account(&login(LOGIN))
            .expect("account must exist")
            .installations
            .len(),
        1
    );
}
