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
/// contract and must not be reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Wire codes derived once from the [`UploadStatus`] discriminants.
const CODE_ACCEPTED: u8 = UploadStatus::Accepted as u8;
const CODE_NO_QUORUM: u8 = UploadStatus::NoQuorum as u8;
const CODE_TIMEOUT: u8 = UploadStatus::Timeout as u8;
const CODE_CREDENTIALS_REJECTED: u8 = UploadStatus::CredentialsRejected as u8;
const CODE_CORRUPTED_PAYLOAD: u8 = UploadStatus::CorruptedPayload as u8;
const CODE_INSUFFICIENT_SPACE: u8 = UploadStatus::InsufficientSpace as u8;

impl UploadStatus {
    /// Whether retrying the same upload could succeed; only transient failures qualify.
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::NoQuorum | Self::Timeout)
    }

    /// Stable wire code for this outcome, part of the wire contract.
    pub const fn code(self) -> u8 {
        self as u8
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

/// Failure to convert a wire code back into an [`UploadStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResponseError {
    /// The byte did not match any known [`UploadStatus`] wire code.
    #[error("unknown upload status code {code}")]
    UnknownStatusCode {
        /// The unrecognized wire code.
        code: u8,
    },
}

/// Result alias for fallible [`UploadStatus`] conversions.
type Result<T> = core::result::Result<T, ResponseError>;

impl TryFrom<u8> for UploadStatus {
    type Error = ResponseError;

    fn try_from(code: u8) -> Result<Self> {
        match code {
            CODE_ACCEPTED => Ok(Self::Accepted),
            CODE_NO_QUORUM => Ok(Self::NoQuorum),
            CODE_TIMEOUT => Ok(Self::Timeout),
            CODE_CREDENTIALS_REJECTED => Ok(Self::CredentialsRejected),
            CODE_CORRUPTED_PAYLOAD => Ok(Self::CorruptedPayload),
            CODE_INSUFFICIENT_SPACE => Ok(Self::InsufficientSpace),
            other => Err(ResponseError::UnknownStatusCode { code: other }),
        }
    }
}
