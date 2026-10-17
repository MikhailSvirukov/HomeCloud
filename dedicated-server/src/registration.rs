//! In-memory account registration for the dedicated server.
//!
//! A registration creates an account with its first installation or adds a
//! further installation to an existing account after checking its password.
//! Replaying a request identifier is idempotent while conflicting content is
//! rejected. Verifiers are derived and checked by the password module; this
//! module performs no I/O.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;
use uuid::Uuid;

use crate::password::{Argon2Params, PasswordError, PasswordVerifier};

type Result<T, E = RegistrationError> = core::result::Result<T, E>;

/// Maximum accepted length of a login, in characters.
pub(crate) const MAX_LOGIN_LENGTH: usize = 64;

/// A validated cluster-unique login.
///
/// A login is a validated namespace component: the relative path components
/// `.` and `..` are rejected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Login(String);

impl Login {
    /// Validate `value` as a login.
    pub fn parse(value: &str) -> Result<Self, LoginError> {
        if value.is_empty() {
            return Err(LoginError::Empty);
        }
        if value.chars().count() > MAX_LOGIN_LENGTH {
            return Err(LoginError::TooLong);
        }
        if let Some(character) = value.chars().find(|c| !is_allowed_login_character(*c)) {
            return Err(LoginError::InvalidCharacter(character));
        }
        if matches!(value, "." | "..") {
            return Err(LoginError::Reserved);
        }
        Ok(Self(value.to_string()))
    }

    /// The login as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Login {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Errors produced while validating a login.
#[derive(Debug, Error)]
pub enum LoginError {
    /// The login was empty.
    #[error("login must not be empty")]
    Empty,
    /// The login had more than [`MAX_LOGIN_LENGTH`] characters.
    #[error("login must be at most {MAX_LOGIN_LENGTH} characters")]
    TooLong,
    /// The login contained a character outside the allowed ASCII subset.
    #[error("login contains a character that is not allowed: {0:?}")]
    InvalidCharacter(char),
    /// The login was a reserved relative path component.
    #[error("login must not be '.' or '..'")]
    Reserved,
}

/// Identifier a client puts on a registration request (16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId([u8; 16]);

impl RequestId {
    /// Wrap the wire bytes of a registration request identifier.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The identifier as wire bytes.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Identifier of one client installation (UUIDv4, 16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstallationId([u8; 16]);

impl InstallationId {
    /// Generate a random UUIDv4 installation identifier.
    pub fn generate() -> Self {
        Self(*Uuid::new_v4().as_bytes())
    }

    /// Wrap the wire bytes of an installation identifier, rejecting anything
    /// that is not a UUIDv4.
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self> {
        let version_ok = bytes[6] >> 4 == 4;
        let variant_ok = bytes[8] >> 6 == 0b10;
        if version_ok && variant_ok {
            Ok(Self(bytes))
        } else {
            Err(RegistrationError::InvalidInstallationId)
        }
    }

    /// The identifier as wire bytes.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Public keys of one client installation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InstallationKeys {
    /// X25519 public key used for encrypted answers.
    pub encryption_public_key: [u8; 32],
    /// Ed25519 public key used to sign the installation's messages.
    pub signing_public_key: [u8; 32],
}

/// One registered client installation.
pub struct Installation {
    pub id: InstallationId,
    pub keys: InstallationKeys,
}

/// A registration request for a login and one installation.
pub struct RegistrationRequest {
    pub request_id: RequestId,
    pub login: Login,
    pub password: String,
    pub keys: InstallationKeys,
}

/// What a successful registration produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationOutcome {
    /// A new account was created with its first installation.
    AccountCreated { installation_id: InstallationId },
    /// A further installation was added to an existing account.
    InstallationAdded { installation_id: InstallationId },
}

/// A registered account: its login, verifier and installations.
pub struct Account {
    pub login: Login,
    pub verifier: PasswordVerifier,
    pub installations: BTreeMap<InstallationId, Installation>,
}

/// The result a request identifier was first used with.
enum RecordedResult {
    Registered(RegistrationOutcome),
    PasswordRejected,
}

/// A previous registration kept for idempotent replays.
struct RecordedRequest {
    login: Login,
    keys: InstallationKeys,
    verifier: PasswordVerifier,
    result: RecordedResult,
}

/// In-memory account table with idempotent registration.
pub struct AccountRegistry {
    params: Argon2Params,
    accounts: BTreeMap<Login, Account>,
    requests: BTreeMap<RequestId, RecordedRequest>,
}

impl AccountRegistry {
    /// Build an empty registry that derives verifiers with `params`.
    pub fn new(params: Argon2Params) -> Self {
        Self {
            params,
            accounts: BTreeMap::new(),
            requests: BTreeMap::new(),
        }
    }

    /// Register a login or an additional installation of an existing login.
    pub fn register(&mut self, request: RegistrationRequest) -> Result<RegistrationOutcome> {
        if let Some(recorded) = self.requests.get(&request.request_id) {
            let matches = recorded.login == request.login
                && recorded.keys == request.keys
                && recorded.verifier.verify(&request.password)?;
            if !matches {
                return Err(RegistrationError::RequestIdConflict);
            }
            return match recorded.result {
                RecordedResult::Registered(outcome) => Ok(outcome),
                RecordedResult::PasswordRejected => Err(RegistrationError::PasswordRejected),
            };
        }

        self.register_fresh(request)
    }

    fn register_fresh(&mut self, request: RegistrationRequest) -> Result<RegistrationOutcome> {
        // A derivation failure happens before a request verifier exists, so it
        // cannot be recorded; the registry parameters are immutable, so a
        // replay of the same request fails identically.
        let request_verifier = PasswordVerifier::derive(&request.password, self.params)?;

        let outcome = match self.accounts.entry(request.login.clone()) {
            Entry::Occupied(mut entry) => {
                let account = entry.get_mut();
                if !account.verifier.verify(&request.password)? {
                    self.requests.insert(
                        request.request_id,
                        RecordedRequest {
                            login: request.login,
                            keys: request.keys,
                            verifier: request_verifier,
                            result: RecordedResult::PasswordRejected,
                        },
                    );
                    return Err(RegistrationError::PasswordRejected);
                }
                let installation_id = InstallationId::generate();
                account.installations.insert(
                    installation_id,
                    Installation {
                        id: installation_id,
                        keys: request.keys,
                    },
                );
                RegistrationOutcome::InstallationAdded { installation_id }
            }
            Entry::Vacant(entry) => {
                let verifier = PasswordVerifier::parse(request_verifier.as_str())?;
                let installation_id = InstallationId::generate();
                let mut installations = BTreeMap::new();
                installations.insert(
                    installation_id,
                    Installation {
                        id: installation_id,
                        keys: request.keys,
                    },
                );
                entry.insert(Account {
                    login: request.login.clone(),
                    verifier,
                    installations,
                });
                RegistrationOutcome::AccountCreated { installation_id }
            }
        };

        self.requests.insert(
            request.request_id,
            RecordedRequest {
                login: request.login,
                keys: request.keys,
                verifier: request_verifier,
                result: RecordedResult::Registered(outcome),
            },
        );
        Ok(outcome)
    }

    /// Look up an account.
    pub fn account(&self, login: &Login) -> Option<&Account> {
        self.accounts.get(login)
    }
}

/// Errors produced by registration.
#[derive(Debug, Error)]
pub enum RegistrationError {
    /// The request identifier was already recorded with different content.
    #[error("registration request was already used with different content")]
    RequestIdConflict,
    /// The password did not match the existing account.
    #[error("the password does not match the registered account")]
    PasswordRejected,
    /// The bytes do not encode a UUIDv4 installation identifier.
    #[error("installation identifier is not a UUIDv4")]
    InvalidInstallationId,
    /// Deriving or checking a password verifier failed.
    #[error(transparent)]
    Password(#[from] PasswordError),
}

/// Whether `character` is allowed in a [`Login`].
fn is_allowed_login_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroUsize};

    use super::*;

    const LOGIN: &str = "user-1.2_x";
    const PASSWORD: &str = "correct horse battery staple";
    const WRONG_PASSWORD: &str = "correct horse battery stapler";
    const REPLACEMENT_PASSWORD: &str = "replacement account password";
    const MEMORY_KIB: u32 = 8;
    const ITERATIONS: u32 = 1;
    const PARALLELISM: u32 = 1;
    const SALT_LENGTH_BYTES: usize = 16;
    const FIRST_REQUEST: [u8; 16] = [0x01; 16];
    const SECOND_REQUEST: [u8; 16] = [0x02; 16];

    fn non_zero(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test values must be non-zero")
    }

    fn params(memory_kib: u32, iterations: u32, parallelism: u32) -> Argon2Params {
        Argon2Params {
            memory_kib: non_zero(memory_kib),
            iterations: non_zero(iterations),
            parallelism: non_zero(parallelism),
            salt_length_bytes: NonZeroUsize::new(SALT_LENGTH_BYTES)
                .expect("test salt length must be non-zero"),
        }
    }

    fn fast_params() -> Argon2Params {
        params(MEMORY_KIB, ITERATIONS, PARALLELISM)
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

    fn created(outcome: RegistrationOutcome) -> InstallationId {
        match outcome {
            RegistrationOutcome::AccountCreated { installation_id } => installation_id,
            other => panic!("expected a created account, got {other:?}"),
        }
    }

    #[test]
    fn parse_accepts_max_length_login_and_as_str_round_trips() {
        let value = "a".repeat(MAX_LOGIN_LENGTH);

        let parsed = Login::parse(&value).expect("max-length login must be valid");

        assert_eq!(parsed.as_str(), value);
        assert_eq!(parsed.as_str().chars().count(), MAX_LOGIN_LENGTH);
    }

    #[test]
    fn parse_rejects_empty_login() {
        assert!(matches!(Login::parse(""), Err(LoginError::Empty)));
    }

    #[test]
    fn parse_rejects_login_one_over_max_length() {
        let value = "a".repeat(MAX_LOGIN_LENGTH + 1);

        assert!(matches!(Login::parse(&value), Err(LoginError::TooLong)));
    }

    #[test]
    fn parse_rejects_disallowed_character_carrying_it() {
        assert!(matches!(
            Login::parse("user/name"),
            Err(LoginError::InvalidCharacter('/'))
        ));
    }

    #[test]
    fn parse_rejects_reserved_namespace_components() {
        assert!(matches!(Login::parse("."), Err(LoginError::Reserved)));
        assert!(matches!(Login::parse(".."), Err(LoginError::Reserved)));
    }

    #[test]
    fn generated_installation_id_is_a_uuid_v4() {
        let installation_id = InstallationId::generate();
        let bytes = installation_id.as_bytes();

        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[6] >> 4, 4);
        assert_eq!(bytes[8] >> 6, 0b10);
    }

    #[test]
    fn new_account_verifier_records_the_registry_parameters() {
        let mut registry = AccountRegistry::new(params(16, 2, 1));
        registry
            .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys(0x81)))
            .expect("registration must succeed");

        let verifier = registry
            .account(&login(LOGIN))
            .expect("account must exist")
            .verifier
            .as_str();

        assert!(
            verifier.starts_with("$argon2id$v=19$m=16,t=2,p=1$"),
            "verifier: {verifier}"
        );
    }

    #[test]
    fn registering_twice_for_one_login_keeps_a_single_account() {
        let mut registry = AccountRegistry::new(fast_params());
        registry
            .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys(0x91)))
            .expect("first registration must succeed");
        registry
            .register(request(SECOND_REQUEST, LOGIN, PASSWORD, keys(0x92)))
            .expect("second registration must succeed");

        let account = registry.account(&login(LOGIN)).expect("account must exist");

        assert_eq!(registry.accounts.len(), 1);
        assert_eq!(account.login, login(LOGIN));
        assert_eq!(account.installations.len(), 2);
    }

    #[test]
    fn failed_password_attempt_reserves_the_request_id() {
        let mut registry = AccountRegistry::new(fast_params());
        let first_keys = keys(0xb1);
        let first_id = created(
            registry
                .register(request(FIRST_REQUEST, LOGIN, PASSWORD, first_keys))
                .expect("first registration must succeed"),
        );

        let error = registry
            .register(request(SECOND_REQUEST, LOGIN, WRONG_PASSWORD, keys(0xb2)))
            .expect_err("wrong password must be rejected");
        let conflict = registry
            .register(request(SECOND_REQUEST, LOGIN, PASSWORD, keys(0xb3)))
            .expect_err("a used request identifier must conflict");

        let account = registry.account(&login(LOGIN)).expect("account must exist");
        assert!(matches!(error, RegistrationError::PasswordRejected));
        assert!(matches!(conflict, RegistrationError::RequestIdConflict));
        assert_eq!(account.installations.len(), 1);
        assert_keys(
            &account
                .installations
                .get(&first_id)
                .expect("first installation must remain")
                .keys,
            first_keys,
        );
    }

    #[test]
    fn replaying_a_failed_attempt_reproduces_the_password_rejection() {
        let mut registry = AccountRegistry::new(fast_params());
        registry
            .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys(0xc1)))
            .expect("first registration must succeed");

        let first = registry
            .register(request(SECOND_REQUEST, LOGIN, WRONG_PASSWORD, keys(0xc2)))
            .expect_err("wrong password must be rejected");
        registry
            .accounts
            .get_mut(&login(LOGIN))
            .expect("account must exist")
            .verifier = PasswordVerifier::derive(WRONG_PASSWORD, fast_params())
            .expect("replacement verifier must derive");
        let repeated = registry
            .register(request(SECOND_REQUEST, LOGIN, WRONG_PASSWORD, keys(0xc2)))
            .expect_err("a recorded rejection must replay without consulting the account");

        assert!(matches!(first, RegistrationError::PasswordRejected));
        assert!(matches!(repeated, RegistrationError::PasswordRejected));
        assert_eq!(
            registry
                .account(&login(LOGIN))
                .expect("account must exist")
                .installations
                .len(),
            1
        );
    }

    #[test]
    fn replay_uses_the_recorded_request_verifier() {
        let mut registry = AccountRegistry::new(fast_params());
        let keys = keys(0xd1);
        let first = registry
            .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys))
            .expect("first registration must succeed");
        registry
            .accounts
            .get_mut(&login(LOGIN))
            .expect("account must exist")
            .verifier = PasswordVerifier::derive(REPLACEMENT_PASSWORD, fast_params())
            .expect("replacement verifier must derive");

        let repeated = registry
            .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys))
            .expect("replay must use the recorded request verifier");

        assert_eq!(repeated, first);
        assert_eq!(
            registry
                .account(&login(LOGIN))
                .expect("account must exist")
                .installations
                .len(),
            1
        );
    }

    #[test]
    fn installation_id_from_bytes_accepts_only_uuid_v4_bits() {
        let mut valid = [0_u8; 16];
        valid[6] = 0x40;
        valid[8] = 0x80;
        assert!(InstallationId::from_bytes(valid).is_ok());

        let mut wrong_version = valid;
        wrong_version[6] = 0x30;
        assert!(matches!(
            InstallationId::from_bytes(wrong_version),
            Err(RegistrationError::InvalidInstallationId)
        ));

        let mut wrong_variant = valid;
        wrong_variant[8] = 0xc0;
        assert!(matches!(
            InstallationId::from_bytes(wrong_variant),
            Err(RegistrationError::InvalidInstallationId)
        ));
    }

    #[test]
    fn derivation_failure_is_reported_without_recording_anything() {
        let mut registry = AccountRegistry::new(params(1, 1, 1));

        let error = registry
            .register(request(FIRST_REQUEST, LOGIN, PASSWORD, keys(0xa1)))
            .expect_err("invalid derivation parameters must fail");

        assert!(matches!(
            error,
            RegistrationError::Password(PasswordError::Parameters(_))
        ));
        assert!(registry.account(&login(LOGIN)).is_none());
        assert!(registry.requests.is_empty());
    }
}
