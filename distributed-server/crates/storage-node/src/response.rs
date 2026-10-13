//! Typed upload outcomes reported by a storage node.
//!
//! [`UploadStatus`] is a storage node's verdict for one upload attempt and
//! carries a stable wire code that distinguishes transient failures from
//! permanent ones, so clients can decide whether to retry. [`ResponseError`]
//! reports a wire code that does not map to any known outcome.

use std::fmt;

// Stable wire codes; see [`UploadStatus::code`].
const CODE_ACCEPTED: u8 = 0;
const CODE_NO_QUORUM: u8 = 1;
const CODE_TIMEOUT: u8 = 2;
const CODE_CREDENTIALS_REJECTED: u8 = 3;
const CODE_CORRUPTED_PAYLOAD: u8 = 4;
const CODE_INSUFFICIENT_SPACE: u8 = 5;

/// Outcome a storage node reports for an upload attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStatus {
    /// The upload was durably stored on the storage quorum and committed.
    Accepted,
    /// The storage quorum could not be reached for this upload.
    NoQuorum,
    /// The upload attempt exceeded its deadline before completing.
    Timeout,
    /// The supplied credentials were rejected.
    CredentialsRejected,
    /// The payload failed decryption or integrity verification.
    CorruptedPayload,
    /// The target storage is out of space.
    InsufficientSpace,
}

impl UploadStatus {
    /// Whether retrying the same upload could succeed.
    ///
    /// Only the transient failures [`UploadStatus::NoQuorum`] and
    /// [`UploadStatus::Timeout`] are retryable; every other outcome, including
    /// [`UploadStatus::Accepted`], is not.
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::NoQuorum | Self::Timeout)
    }

    /// Stable wire code for this outcome.
    ///
    /// The codes are `Accepted = 0`, `NoQuorum = 1`, `Timeout = 2`,
    /// `CredentialsRejected = 3`, `CorruptedPayload = 4`, and
    /// `InsufficientSpace = 5`. They are part of the wire contract and must not
    /// be reordered.
    pub const fn code(self) -> u8 {
        match self {
            Self::Accepted => CODE_ACCEPTED,
            Self::NoQuorum => CODE_NO_QUORUM,
            Self::Timeout => CODE_TIMEOUT,
            Self::CredentialsRejected => CODE_CREDENTIALS_REJECTED,
            Self::CorruptedPayload => CODE_CORRUPTED_PAYLOAD,
            Self::InsufficientSpace => CODE_INSUFFICIENT_SPACE,
        }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseError {
    /// The byte did not match any known [`UploadStatus`] wire code.
    UnknownStatusCode {
        /// The unrecognized wire code.
        code: u8,
    },
}

impl fmt::Display for ResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownStatusCode { code } => {
                write!(formatter, "unknown upload status code {code}")
            }
        }
    }
}

impl std::error::Error for ResponseError {}

impl TryFrom<u8> for UploadStatus {
    type Error = ResponseError;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{ResponseError, UploadStatus};

    const ALL_STATUSES: [UploadStatus; 6] = [
        UploadStatus::Accepted,
        UploadStatus::NoQuorum,
        UploadStatus::Timeout,
        UploadStatus::CredentialsRejected,
        UploadStatus::CorruptedPayload,
        UploadStatus::InsufficientSpace,
    ];

    #[test]
    fn upload_status_codes_are_pairwise_distinct() {
        let mut codes = HashSet::new();
        for status in ALL_STATUSES {
            assert!(
                codes.insert(status.code()),
                "duplicate wire code for {status}"
            );
        }
    }

    #[test]
    fn try_from_round_trips_every_wire_code() {
        for status in ALL_STATUSES {
            assert_eq!(UploadStatus::try_from(status.code()), Ok(status));
        }
    }

    #[test]
    fn try_from_rejects_unknown_status_codes() {
        for code in [6, 255] {
            let expected = ResponseError::UnknownStatusCode { code };
            assert_eq!(UploadStatus::try_from(code), Err(expected));
        }
    }

    #[test]
    fn is_retryable_is_true_for_no_quorum() {
        assert!(UploadStatus::NoQuorum.is_retryable());
    }

    #[test]
    fn is_retryable_is_true_for_timeout() {
        assert!(UploadStatus::Timeout.is_retryable());
    }

    #[test]
    fn is_retryable_is_false_for_non_transient_outcomes() {
        let non_retryable = [
            UploadStatus::Accepted,
            UploadStatus::CredentialsRejected,
            UploadStatus::CorruptedPayload,
            UploadStatus::InsufficientSpace,
        ];
        for status in non_retryable {
            assert!(!status.is_retryable(), "{status} must not be retryable");
        }
    }

    #[test]
    fn display_yields_non_empty_distinct_phrases() {
        let mut phrases = HashSet::new();
        for status in ALL_STATUSES {
            let phrase = status.to_string();
            assert!(!phrase.is_empty(), "empty display phrase for {status:?}");
            assert!(
                phrases.insert(phrase),
                "duplicate display phrase for {status:?}"
            );
        }
    }

    #[test]
    fn unknown_status_error_display_names_the_code() {
        let error = ResponseError::UnknownStatusCode { code: 42 };
        assert_eq!(error.to_string(), "unknown upload status code 42");
    }
}
