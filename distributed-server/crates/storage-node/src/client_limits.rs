//! Client credential and responder-label limits.
//!
//! SRS: `CFG-CL-029 / 1`, `CFG-CL-031 / 1`.

use core::num::NonZeroUsize;

/// Configured byte-length bounds for client credentials and responder labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientLimits {
    /// Minimum accepted login length in UTF-8 bytes.
    pub login_min_len: NonZeroUsize,
    /// Maximum accepted login length in UTF-8 bytes.
    pub login_max_len: NonZeroUsize,
    /// Minimum accepted password length in UTF-8 bytes.
    pub password_min_len: NonZeroUsize,
    /// Maximum accepted password length in UTF-8 bytes.
    pub password_max_len: NonZeroUsize,
    /// Maximum accepted responder-label length in UTF-8 bytes.
    pub responder_label_max_len: NonZeroUsize,
}

impl ClientLimits {
    /// Validates the configured bounds.
    pub fn new(
        login_min_len: NonZeroUsize,
        login_max_len: NonZeroUsize,
        password_min_len: NonZeroUsize,
        password_max_len: NonZeroUsize,
        responder_label_max_len: NonZeroUsize,
    ) -> Result<Self, ClientLimitsError> {
        if login_min_len > login_max_len {
            return Err(ClientLimitsError::LoginRange);
        }
        if password_min_len > password_max_len {
            return Err(ClientLimitsError::PasswordRange);
        }
        Ok(Self {
            login_min_len,
            login_max_len,
            password_min_len,
            password_max_len,
            responder_label_max_len,
        })
    }

    /// Returns whether `login` uses the permitted ASCII set and fits the configured byte range.
    pub fn login_allowed(&self, login: &str) -> bool {
        (self.login_min_len.get()..=self.login_max_len.get()).contains(&login.len())
            && login.bytes().all(is_valid_login_byte)
    }

    /// Returns whether `password` fits the configured UTF-8 byte range without trimming or normalization.
    pub fn password_allowed(&self, password: &str) -> bool {
        (self.password_min_len.get()..=self.password_max_len.get()).contains(&password.len())
    }

    /// Returns whether `label` fits the configured UTF-8 byte limit.
    pub fn responder_label_allowed(&self, label: &str) -> bool {
        label.len() <= self.responder_label_max_len.get()
    }
}

/// Failure modes when building [`ClientLimits`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientLimitsError {
    /// The login minimum length exceeds its maximum.
    #[error("the login minimum length exceeds its maximum")]
    LoginRange,
    /// The password minimum length exceeds its maximum.
    #[error("the password minimum length exceeds its maximum")]
    PasswordRange,
}

fn is_valid_login_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}
