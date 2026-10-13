//! Pure, in-memory account registration for the dedicated server.
//!
//! The dedicated server accepts a client registration for a cluster-unique
//! login and keeps the resulting account record so it can be replicated to the
//! cluster. This module only models login validation and the account table: it
//! does no I/O, is not async, and performs no cryptography.
//!
//! The password verifier and the client encryption public key stay opaque byte
//! strings. The verifier bytes are produced by the password step whose hashing
//! scheme is an open parameter in the v1 specification; this module neither
//! hashes passwords nor chooses key encodings.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

/// Maximum accepted length of a login, in characters.
pub const MAX_LOGIN_LENGTH: usize = 64;

/// Punctuation characters allowed in a [`Login`] in addition to ASCII
/// alphanumerics.
const ALLOWED_LOGIN_SYMBOLS: [char; 3] = ['-', '_', '.'];

/// A validated cluster-unique login.
///
/// A login names the account and, per the v1 specification, the server-side
/// namespace. It is restricted to a conservative ASCII subset for use as a
/// namespace component; callers that build filesystem paths must treat the
/// login as an opaque component and never as a path (`.` and `..` are accepted
/// by the character rule).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Login(String);

impl Login {
    /// Validate `value` as a login.
    ///
    /// # Errors
    /// Returns [`LoginError`] for an empty value, a value longer than
    /// [`MAX_LOGIN_LENGTH`], or a character outside `[A-Za-z0-9._-]`.
    pub fn parse(value: &str) -> Result<Self, LoginError> {
        if value.is_empty() {
            Err(LoginError::Empty)
        } else if value.chars().count() > MAX_LOGIN_LENGTH {
            Err(LoginError::TooLong)
        } else if let Some(character) = value.chars().find(|c| !is_allowed_login_character(*c)) {
            Err(LoginError::InvalidCharacter(character))
        } else {
            Ok(Self(value.to_string()))
        }
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
#[derive(Debug, Error, PartialEq, Eq)]
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
}

/// A validated registration request for a new account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationRequest {
    login: Login,
    password_verifier: Vec<u8>,
    client_encryption_public_key: Vec<u8>,
}

impl RegistrationRequest {
    /// Build a request, rejecting empty verifier or client key material.
    ///
    /// # Errors
    /// [`RegistrationError::EmptyPasswordVerifier`] /
    /// [`RegistrationError::EmptyClientEncryptionPublicKey`].
    pub fn new(
        login: Login,
        password_verifier: Vec<u8>,
        client_encryption_public_key: Vec<u8>,
    ) -> Result<Self, RegistrationError> {
        if password_verifier.is_empty() {
            Err(RegistrationError::EmptyPasswordVerifier)
        } else if client_encryption_public_key.is_empty() {
            Err(RegistrationError::EmptyClientEncryptionPublicKey)
        } else {
            Ok(Self {
                login,
                password_verifier,
                client_encryption_public_key,
            })
        }
    }

    /// The requested login.
    pub fn login(&self) -> &Login {
        &self.login
    }

    /// The opaque password verifier bytes.
    pub fn password_verifier(&self) -> &[u8] {
        &self.password_verifier
    }

    /// The opaque client encryption public key bytes.
    pub fn client_encryption_public_key(&self) -> &[u8] {
        &self.client_encryption_public_key
    }
}

/// The account state the dedicated server keeps and replicates to the cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    login: Login,
    password_verifier: Vec<u8>,
    client_encryption_public_key: Vec<u8>,
}

impl AccountRecord {
    /// The account login.
    pub fn login(&self) -> &Login {
        &self.login
    }

    /// The opaque password verifier bytes.
    pub fn password_verifier(&self) -> &[u8] {
        &self.password_verifier
    }

    /// The opaque client encryption public key bytes.
    pub fn client_encryption_public_key(&self) -> &[u8] {
        &self.client_encryption_public_key
    }
}

impl From<RegistrationRequest> for AccountRecord {
    fn from(request: RegistrationRequest) -> Self {
        Self {
            login: request.login,
            password_verifier: request.password_verifier,
            client_encryption_public_key: request.client_encryption_public_key,
        }
    }
}

/// In-memory account table with cluster-unique logins.
///
/// Records iterate in login order so replication is deterministic.
#[derive(Debug, Default)]
pub struct AccountRegistry {
    accounts: BTreeMap<Login, AccountRecord>,
}

impl AccountRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new account.
    ///
    /// # Errors
    /// [`RegistrationError::LoginAlreadyRegistered`] if the login is taken.
    pub fn register(
        &mut self,
        request: RegistrationRequest,
    ) -> Result<&AccountRecord, RegistrationError> {
        match self.accounts.entry(request.login().clone()) {
            Entry::Occupied(entry) => Err(RegistrationError::LoginAlreadyRegistered(
                entry.key().clone(),
            )),
            Entry::Vacant(entry) => Ok(entry.insert(AccountRecord::from(request))),
        }
    }

    /// Look up an account.
    pub fn get(&self, login: &Login) -> Option<&AccountRecord> {
        self.accounts.get(login)
    }

    /// Number of registered accounts.
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Whether no account is registered.
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    /// All records in login order.
    pub fn records(&self) -> impl Iterator<Item = &AccountRecord> {
        self.accounts.values()
    }
}

/// Errors produced by registration.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistrationError {
    /// The password verifier was empty.
    #[error("password verifier must not be empty")]
    EmptyPasswordVerifier,
    /// The client encryption public key was empty.
    #[error("client encryption public key must not be empty")]
    EmptyClientEncryptionPublicKey,
    /// The login already names a registered account.
    #[error("login '{0}' is already registered")]
    LoginAlreadyRegistered(Login),
}

/// Whether `character` is allowed in a [`Login`].
fn is_allowed_login_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || ALLOWED_LOGIN_SYMBOLS.contains(&character)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN: &str = "user-1.2_x";
    const VERIFIER: &[u8] = &[0x01, 0x02, 0x03];
    const CLIENT_KEY: &[u8] = &[0x04, 0x05, 0x06];

    fn login(value: &str) -> Login {
        Login::parse(value).expect("test login must be valid")
    }

    fn request(name: &str, verifier: &[u8], key: &[u8]) -> RegistrationRequest {
        RegistrationRequest::new(login(name), verifier.to_vec(), key.to_vec())
            .expect("test request must be valid")
    }

    #[test]
    fn parse_accepts_typical_login_and_as_str_returns_input() {
        let parsed = login(LOGIN);

        assert_eq!(parsed.as_str(), LOGIN);
    }

    #[test]
    fn display_prints_the_login_text() {
        assert_eq!(login(LOGIN).to_string(), LOGIN);
    }

    #[test]
    fn parse_accepts_login_at_max_length() {
        let value = "a".repeat(MAX_LOGIN_LENGTH);

        let parsed = Login::parse(&value).expect("max-length login must be valid");

        assert_eq!(parsed.as_str().chars().count(), MAX_LOGIN_LENGTH);
    }

    #[test]
    fn parse_rejects_empty_login() {
        assert_eq!(Login::parse(""), Err(LoginError::Empty));
    }

    #[test]
    fn parse_rejects_login_one_over_max_length() {
        let value = "a".repeat(MAX_LOGIN_LENGTH + 1);

        assert_eq!(Login::parse(&value), Err(LoginError::TooLong));
    }

    #[test]
    fn parse_reports_too_long_before_scanning_characters() {
        let value = format!("{} ", "a".repeat(MAX_LOGIN_LENGTH));

        assert_eq!(Login::parse(&value), Err(LoginError::TooLong));
    }

    #[test]
    fn parse_rejects_disallowed_characters() {
        for character in [' ', '/', '\\', 'é'] {
            let value = format!("user{character}name");

            assert_eq!(
                Login::parse(&value),
                Err(LoginError::InvalidCharacter(character))
            );
        }
    }

    #[test]
    fn request_new_echoes_inputs_verbatim() {
        let request = request(LOGIN, VERIFIER, CLIENT_KEY);

        assert_eq!(request.login().as_str(), LOGIN);
        assert_eq!(request.password_verifier(), VERIFIER);
        assert_eq!(request.client_encryption_public_key(), CLIENT_KEY);
    }

    #[test]
    fn request_new_rejects_empty_verifier() {
        let result = RegistrationRequest::new(login(LOGIN), Vec::new(), CLIENT_KEY.to_vec());

        assert_eq!(result, Err(RegistrationError::EmptyPasswordVerifier));
    }

    #[test]
    fn request_new_rejects_empty_client_key() {
        let result = RegistrationRequest::new(login(LOGIN), VERIFIER.to_vec(), Vec::new());

        assert_eq!(
            result,
            Err(RegistrationError::EmptyClientEncryptionPublicKey)
        );
    }

    #[test]
    fn request_new_reports_verifier_error_before_client_key() {
        let result = RegistrationRequest::new(login(LOGIN), Vec::new(), Vec::new());

        assert_eq!(result, Err(RegistrationError::EmptyPasswordVerifier));
    }

    #[test]
    fn register_inserts_record_with_the_request_payload() {
        let mut registry = AccountRegistry::new();

        let record = registry
            .register(request(LOGIN, VERIFIER, CLIENT_KEY))
            .expect("first registration must succeed");

        assert_eq!(record.login().as_str(), LOGIN);
        assert_eq!(record.password_verifier(), VERIFIER);
        assert_eq!(record.client_encryption_public_key(), CLIENT_KEY);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn register_rejects_duplicate_login_with_identical_payload() {
        let mut registry = AccountRegistry::new();
        registry
            .register(request(LOGIN, VERIFIER, CLIENT_KEY))
            .expect("first registration must succeed");

        let error = registry
            .register(request(LOGIN, VERIFIER, CLIENT_KEY))
            .expect_err("duplicate login must fail");

        assert_eq!(
            error,
            RegistrationError::LoginAlreadyRegistered(login(LOGIN))
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn register_rejects_duplicate_login_with_different_payload_and_keeps_first() {
        let mut registry = AccountRegistry::new();
        registry
            .register(request(LOGIN, VERIFIER, CLIENT_KEY))
            .expect("first registration must succeed");

        let error = registry
            .register(request(LOGIN, &[0x09, 0x09], &[0x08, 0x08]))
            .expect_err("duplicate login must fail");

        assert_eq!(
            error,
            RegistrationError::LoginAlreadyRegistered(login(LOGIN))
        );
        assert_eq!(registry.len(), 1);
        let stored = registry.get(&login(LOGIN)).expect("record must remain");
        assert_eq!(stored.password_verifier(), VERIFIER);
        assert_eq!(stored.client_encryption_public_key(), CLIENT_KEY);
    }

    #[test]
    fn get_returns_record_for_known_login() {
        let mut registry = AccountRegistry::new();
        registry
            .register(request(LOGIN, VERIFIER, CLIENT_KEY))
            .expect("registration must succeed");

        let stored = registry.get(&login(LOGIN)).expect("record must be found");

        assert_eq!(stored.login().as_str(), LOGIN);
    }

    #[test]
    fn get_returns_none_for_unknown_login() {
        let registry = AccountRegistry::new();

        assert!(registry.get(&login("missing")).is_none());
    }

    #[test]
    fn records_returns_all_accounts_in_login_order() {
        let mut registry = AccountRegistry::new();
        for name in ["carol", "alice", "bob"] {
            registry
                .register(request(name, VERIFIER, CLIENT_KEY))
                .expect("registration must succeed");
        }

        let logins: Vec<&str> = registry
            .records()
            .map(|record| record.login().as_str())
            .collect();

        assert_eq!(logins, vec!["alice", "bob", "carol"]);
    }

    #[test]
    fn new_registry_is_empty() {
        let registry = AccountRegistry::new();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}
