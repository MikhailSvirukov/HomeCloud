//! Integration tests exercising only the public `storage_node` upload outcome API.

use std::collections::HashSet;

use storage_node::{ResponseError, UploadStatus};

/// The first wire code that is not assigned to any [`UploadStatus`].
const UNKNOWN_CODE_LOW: u8 = 6;
/// A wire code well above the assigned range.
const UNKNOWN_CODE_HIGH: u8 = 255;

const ALL_STATUSES: [UploadStatus; 6] = [
    UploadStatus::Accepted,
    UploadStatus::NoQuorum,
    UploadStatus::Timeout,
    UploadStatus::CredentialsRejected,
    UploadStatus::CorruptedPayload,
    UploadStatus::InsufficientSpace,
];

#[test]
fn every_status_round_trips_through_its_wire_code() {
    for status in ALL_STATUSES {
        assert_eq!(UploadStatus::try_from(u8::from(status)), Ok(status));
    }
}

#[test]
fn wire_codes_are_pairwise_distinct() {
    let mut codes = HashSet::new();
    for status in ALL_STATUSES {
        assert!(
            codes.insert(u8::from(status)),
            "duplicate wire code for {status}"
        );
    }
}

#[test]
fn unknown_wire_code_is_rejected() {
    for code in [UNKNOWN_CODE_LOW, UNKNOWN_CODE_HIGH] {
        assert_eq!(
            UploadStatus::try_from(code),
            Err(ResponseError::UnknownStatusCode { code })
        );
    }
}

#[test]
fn retryable_outcomes_are_the_transient_failures() {
    assert!(UploadStatus::NoQuorum.is_retryable());
    assert!(UploadStatus::Timeout.is_retryable());

    let permanent = [
        UploadStatus::Accepted,
        UploadStatus::CredentialsRejected,
        UploadStatus::CorruptedPayload,
        UploadStatus::InsufficientSpace,
    ];
    for status in permanent {
        assert!(!status.is_retryable(), "{status} must not be retryable");
    }
}

#[test]
fn display_phrases_are_non_empty() {
    for status in ALL_STATUSES {
        assert!(
            !status.to_string().is_empty(),
            "empty display phrase for {status}"
        );
    }
}
