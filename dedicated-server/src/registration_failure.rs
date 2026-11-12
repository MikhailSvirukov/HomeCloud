//! Published result code for a failed registration decision.
//!
//! The registration decision reports typed errors, and the contract fixes the
//! codes a client may receive (`INT-NET-062 / 1`-`INT-NET-064 / 1`,
//! `FR-DED-053 / 1`). This module is the single place that translates one into
//! the other, so a request handler can answer a failure without inventing a
//! code.

use crate::frame::FrameError;
use crate::inter_server_client::InterServerClientError;
use crate::registration_flow::RegistrationFlowError;
use crate::result_code::ResultCode;

/// Published result code for a failed registration decision.
pub fn registration_failure(error: &RegistrationFlowError) -> ResultCode {
    match error {
        RegistrationFlowError::Exchange(exchange) => exchange_failure(exchange),
        RegistrationFlowError::Login(_) | RegistrationFlowError::PasswordOutOfBounds { .. } => {
            ResultCode::InvalidArguments
        }
        RegistrationFlowError::PasswordRejected => ResultCode::AuthenticationRejected,
        RegistrationFlowError::RequestIdConflict => ResultCode::RepeatedRequestId,
        RegistrationFlowError::UnexpectedInstallation => ResultCode::Internal,
        RegistrationFlowError::Password(_) => ResultCode::Internal,
        RegistrationFlowError::Tag(_) => ResultCode::Internal,
    }
}

/// Published result code for a failed inter-server exchange.
fn exchange_failure(error: &InterServerClientError) -> ResultCode {
    match error {
        InterServerClientError::Rejected(code) => {
            ResultCode::from_u32(*code).unwrap_or(ResultCode::Internal)
        }
        InterServerClientError::Connect { .. }
        | InterServerClientError::Io(_)
        | InterServerClientError::Frame(FrameError::Io(_))
        | InterServerClientError::Deadline { .. } => ResultCode::TemporaryUnavailable,
        InterServerClientError::Frame(_) | InterServerClientError::Seal(_) => ResultCode::Internal,
        InterServerClientError::Wire(_) => ResultCode::Internal,
        InterServerClientError::Mismatch(_) => ResultCode::Internal,
        InterServerClientError::LogicalHeaderTooLarge { .. } => ResultCode::Internal,
        InterServerClientError::LoginOutOfBounds { .. } => ResultCode::InvalidArguments,
        InterServerClientError::NoStorageNode => ResultCode::TemporaryUnavailable,
        InterServerClientError::KeyLength { .. } => ResultCode::Internal,
        InterServerClientError::HeaderBoundary(_) => ResultCode::Internal,
        InterServerClientError::UnexpectedMessage => ResultCode::Internal,
        InterServerClientError::Tag(_) => ResultCode::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inter_server_client::InterServerClientError;
    use crate::registration::LoginError;

    fn login() -> LoginError {
        LoginError::Reserved
    }

    #[test]
    fn a_rejected_password_answers_the_authentication_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::PasswordRejected),
            ResultCode::AuthenticationRejected
        );
    }

    #[test]
    fn a_reused_request_identifier_answers_the_repeated_request_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::RequestIdConflict),
            ResultCode::RepeatedRequestId
        );
    }

    #[test]
    fn malformed_credentials_answer_the_invalid_arguments_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::Login(login())),
            ResultCode::InvalidArguments
        );
        assert_eq!(
            registration_failure(&RegistrationFlowError::PasswordOutOfBounds {
                actual: 5,
                min: 8,
                max: 1_024
            }),
            ResultCode::InvalidArguments
        );
    }

    #[test]
    fn a_named_storage_rejection_keeps_its_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::Exchange(
                InterServerClientError::Rejected(0x0003_0001)
            )),
            ResultCode::RegistrationConflict
        );
    }

    #[test]
    fn an_unnamed_storage_rejection_answers_the_internal_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::Exchange(
                InterServerClientError::Rejected(0x0001_00FF)
            )),
            ResultCode::Internal
        );
    }

    #[test]
    fn a_transport_failure_answers_the_temporary_unavailability_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::Exchange(
                InterServerClientError::NoStorageNode
            )),
            ResultCode::TemporaryUnavailable
        );
    }

    #[test]
    fn a_read_failure_is_temporary_and_a_malformed_frame_is_internal() {
        let read = InterServerClientError::Frame(FrameError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timeout",
        )));
        let malformed = InterServerClientError::Frame(FrameError::TooLarge {
            announced: 10,
            max: 4,
        });

        assert_eq!(
            registration_failure(&RegistrationFlowError::Exchange(read)),
            ResultCode::TemporaryUnavailable
        );
        assert_eq!(
            registration_failure(&RegistrationFlowError::Exchange(malformed)),
            ResultCode::Internal
        );
    }

    #[test]
    fn an_oversized_response_header_answers_the_internal_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::Exchange(
                InterServerClientError::LogicalHeaderTooLarge {
                    actual: 2_000,
                    max: 1_024
                }
            )),
            ResultCode::Internal
        );
    }

    #[test]
    fn an_unexpected_commit_answers_the_internal_code() {
        assert_eq!(
            registration_failure(&RegistrationFlowError::UnexpectedInstallation),
            ResultCode::Internal
        );
    }
}
