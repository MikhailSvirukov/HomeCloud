//! In-memory account registration for the dedicated server.
//!
//! This module validates logins and keeps the account table; it does no I/O or
//! cryptography and treats verifier and client key bytes as opaque.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

type Result<T, E = RegistrationError> = core::result::Result<T, E>;

/// Maximum accepted length of a login, in characters.
pub(crate) const MAX_LOGIN_LENGTH: usize = 64;

/// Punctuation characters allowed in a [`Login`] in addition to ASCII
/// alphanumerics.
const ALLOWED_LOGIN_SYMBOLS: [char; 3] = ['-', '_', '.'];

/// Relative path components that name the current or parent namespace and are
/// therefore never valid account logins.
const RESERVED_LOGIN_NAMES: [&str; 2] = [".", ".."];

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
        if RESERVED_LOGIN_NAMES.contains(&value) {
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

/// A registered account: its login, password verifier and client encryption key.
pub struct Account {
    pub login: Login,
    pub password_verifier: Vec<u8>,
    pub client_encryption_public_key: Vec<u8>,
}

impl Account {
    /// Build an account, rejecting empty verifier or client key material.
    pub fn new(
        login: Login,
        password_verifier: Vec<u8>,
        client_encryption_public_key: Vec<u8>,
    ) -> Result<Self> {
        if password_verifier.is_empty() {
            return Err(RegistrationError::EmptyPasswordVerifier);
        }
        if client_encryption_public_key.is_empty() {
            return Err(RegistrationError::EmptyClientEncryptionPublicKey);
        }
        Ok(Self {
            login,
            password_verifier,
            client_encryption_public_key,
        })
    }
}

/// In-memory account table keyed by cluster-unique login.
#[derive(Default)]
pub struct AccountRegistry {
    accounts: BTreeMap<Login, Account>,
}

impl AccountRegistry {
    /// Register a new account, rejecting an already-registered login.
    pub fn register(&mut self, account: Account) -> Result<&Account> {
        match self.accounts.entry(account.login.clone()) {
            Entry::Occupied(entry) => Err(RegistrationError::LoginAlreadyRegistered(
                entry.key().clone(),
            )),
            Entry::Vacant(entry) => Ok(entry.insert(account)),
        }
    }

    /// Look up an account.
    pub fn get(&self, login: &Login) -> Option<&Account> {
        self.accounts.get(login)
    }

    /// All accounts in login order.
    pub fn accounts(&self) -> impl Iterator<Item = &Account> {
        self.accounts.values()
    }
}

/// Errors produced by registration.
#[derive(Debug, Error)]
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

    fn account(name: &str) -> Account {
        Account::new(login(name), VERIFIER.to_vec(), CLIENT_KEY.to_vec())
            .expect("test account must be valid")
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
    fn account_new_holds_login_and_bytes_and_rejects_empty_material() {
        let account = Account::new(login(LOGIN), VERIFIER.to_vec(), CLIENT_KEY.to_vec())
            .expect("valid material must build an account");

        assert_eq!(account.login.as_str(), LOGIN);
        assert_eq!(account.password_verifier, VERIFIER);
        assert_eq!(account.client_encryption_public_key, CLIENT_KEY);

        let empty_verifier = Account::new(login(LOGIN), Vec::new(), CLIENT_KEY.to_vec());
        let empty_client_key = Account::new(login(LOGIN), VERIFIER.to_vec(), Vec::new());

        assert!(matches!(
            empty_verifier,
            Err(RegistrationError::EmptyPasswordVerifier)
        ));
        assert!(matches!(
            empty_client_key,
            Err(RegistrationError::EmptyClientEncryptionPublicKey)
        ));
    }

    #[test]
    fn register_rejects_duplicate_login_and_keeps_original_account() {
        let mut registry = AccountRegistry::default();
        registry
            .register(account(LOGIN))
            .expect("first registration must succeed");

        let replacement = Account::new(login(LOGIN), vec![0x09], vec![0x08])
            .expect("replacement account must be valid");
        let error = match registry.register(replacement) {
            Ok(_) => panic!("duplicate login must fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            RegistrationError::LoginAlreadyRegistered(ref login) if login.as_str() == LOGIN
        ));

        let stored = registry.get(&login(LOGIN)).expect("account must remain");
        assert_eq!(stored.password_verifier, VERIFIER);
        assert_eq!(stored.client_encryption_public_key, CLIENT_KEY);
        assert_eq!(registry.accounts().count(), 1);
    }

    #[test]
    fn accounts_yields_every_account_in_login_order() {
        let mut registry = AccountRegistry::default();
        for name in ["carol", "alice", "bob"] {
            registry
                .register(account(name))
                .expect("registration must succeed");
        }

        let logins: Vec<&str> = registry
            .accounts()
            .map(|account| account.login.as_str())
            .collect();

        assert_eq!(logins, ["alice", "bob", "carol"]);
    }
}
