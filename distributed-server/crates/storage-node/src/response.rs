//! Typed upload outcomes reported by a storage node.
//!
//! [`UploadStatus`] is a storage node's verdict for one upload attempt and
//! carries a stable wire code that distinguishes transient failures from
//! permanent ones, so clients can decide whether to retry. [`ResponseError`]
//! reports a wire code that does not map to any known outcome.

use std::fmt;

/// Outcome a storage node reports for an upload attempt.
///
/// The discriminants are the stable wire codes; they are part of the wire
/// contract and must not be reordered. Convert to a wire code with
/// `u8::from(status)` and back with `UploadStatus::try_from(code)`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, num_enum::IntoPrimitive, num_enum::TryFromPrimitive,
)]
#[num_enum(error_type(name = ResponseError, constructor = ResponseError::unknown_status_code))]
#[repr(u8)]
pub enum UploadStatus {
    /// The upload was durably stored on the storage quorum and committed.
    Accepted = 0,
    /// The storage quorum could not be reached for this upload.
    NoQuorum = 1,
    /// The upload attempt exceeded its deadline before completing.
    Timeout = 2,
    /// The supplied credentials were rejected.
    CredentialsRejected = 3,
    /// The payload failed decryption or integrity verification.
    CorruptedPayload = 4,
    /// The target storage is out of space.
    InsufficientSpace = 5,
}

impl UploadStatus {
    /// Whether retrying the same upload could succeed; only transient failures qualify.
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::NoQuorum | Self::Timeout)
    }
}

impl fmt::Display for UploadStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Accepted => "accepted",
            Self::NoQuorum => "no quorum",
            Self::Timeout => "timeout",
            Self::CredentialsRejected => "credentials rejected",
            Self::CorruptedPayload => "corrupted payload",
            Self::InsufficientSpace => "insufficient space",
        })
    }
}

/// A wire code that does not map to any known [`UploadStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResponseError {
    /// The byte did not match any known [`UploadStatus`] wire code.
    #[error("unknown upload status code {code}")]
    UnknownStatusCode {
        /// The unrecognized wire code.
        code: u8,
    },
}

impl ResponseError {
    /// Constructor the wire-code conversion plugs into num-enum's error hook.
    fn unknown_status_code(code: u8) -> Self {
        Self::UnknownStatusCode { code }
    }
}
