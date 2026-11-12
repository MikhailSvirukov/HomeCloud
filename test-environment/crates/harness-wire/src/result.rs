//! Result envelope and published v1 result codes.
//!
//! The envelope is `[result_code, result_args_array]`; a successful result has
//! code `0`, and every non-zero code carries an empty argument array
//! (`INT-NET-061 / 1`, `INT-NET-069 / 1`).

use crate::cbor::{self, Value};

const RESULT_CODE: &str = "result_code";
const RESULT_ARGS: &str = "result_args_array";

/// Successful execution of a request (`INT-NET-062 / 1`).
pub const CODE_SUCCESS: u32 = 0;
/// The operation code is not defined by the v1 operation table.
pub const UNKNOWN_OPERATION: u32 = 0x0001_0001;
/// The operation arguments are invalid.
pub const INVALID_ARGUMENTS: u32 = 0x0001_0002;
/// The request is not valid in the current request state.
pub const INVALID_REQUEST_STATE: u32 = 0x0001_0003;
/// The `request_id` is repeated with conflicting content.
pub const REPEATED_REQUEST_ID: u32 = 0x0001_0004;
/// Authentication of the request was rejected.
pub const AUTHENTICATION_REJECTED: u32 = 0x0002_0001;
/// Registration conflicts with an existing account or installation.
pub const REGISTRATION_CONFLICT: u32 = 0x0003_0001;
/// Registration was rejected by policy.
pub const REGISTRATION_REJECTED: u32 = 0x0003_0002;
/// The catalog cursor is invalid.
pub const INVALID_CURSOR: u32 = 0x0004_0001;
/// The catalog snapshot has expired.
pub const EXPIRED_SNAPSHOT: u32 = 0x0004_0002;
/// The uploaded file exceeds the size limit.
pub const FILE_SIZE_EXCEEDED: u32 = 0x0005_0001;
/// The upload size, metadata, or hash does not match.
pub const FILE_MISMATCH: u32 = 0x0005_0002;
/// The upload frame sequence is invalid.
pub const INVALID_FRAME_SEQUENCE: u32 = 0x0005_0003;
/// The `upload_id` conflicts with another upload.
pub const UPLOAD_ID_CONFLICT: u32 = 0x0005_0004;
/// The Raft quorum is unavailable.
pub const RAFT_QUORUM_UNAVAILABLE: u32 = 0x0006_0001;
/// The blob replication quorum was not reached.
pub const BLOB_QUORUM_UNAVAILABLE: u32 = 0x0006_0002;
/// The service is temporarily unavailable.
pub const TEMPORARY_UNAVAILABLE: u32 = 0x0007_0001;
/// Storage capacity is exhausted.
pub const STORAGE_CAPACITY_EXHAUSTED: u32 = 0x0007_0002;
/// An internal error occurred.
pub const INTERNAL_ERROR: u32 = 0x0008_0001;

/// The result envelope `[result_code, result_args_array]` (`INT-NET-061`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultEnvelope {
    /// The `uint32` result code; `0` means success.
    pub code: u32,
    /// The result arguments; empty for every non-zero code.
    pub args: Vec<Value>,
}

impl ResultEnvelope {
    /// Encodes the envelope; rejects a non-zero code with non-empty arguments
    /// (`INT-NET-069`).
    pub fn encode(&self) -> Result<Vec<u8>, ResultError> {
        if self.code != CODE_SUCCESS && !self.args.is_empty() {
            return Err(ResultError::NonEmptyErrorArgs);
        }

        let value = Value::Array(vec![
            Value::Uint(u64::from(self.code)),
            Value::Array(self.args.clone()),
        ]);
        Ok(cbor::encode(&value))
    }

    /// Decodes and validates shape, `uint32` code, and empty error arguments.
    pub fn decode(value: &Value) -> Result<Self, ResultError> {
        let Value::Array(items) = value else {
            return Err(ResultError::WrongShape);
        };

        let [code, args] = items.as_slice() else {
            return Err(ResultError::WrongShape);
        };

        let Value::Uint(code) = code else {
            return Err(ResultError::WrongType(RESULT_CODE));
        };
        let code = u32::try_from(*code).map_err(|_| ResultError::CodeOutOfRange(*code))?;

        let Value::Array(args) = args else {
            return Err(ResultError::WrongType(RESULT_ARGS));
        };

        if code != CODE_SUCCESS && !args.is_empty() {
            return Err(ResultError::NonEmptyErrorArgs);
        }

        Ok(Self {
            code,
            args: args.clone(),
        })
    }
}

/// Reserved class of a v1 result code (`INT-NET-064`, Table 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultClass {
    /// Successful execution, code `0`.
    Success,
    /// Request errors, `0x0001_0000–0x0001_FFFF`.
    Request,
    /// Authentication errors, `0x0002_0000–0x0002_FFFF`.
    Authentication,
    /// Registration errors, `0x0003_0000–0x0003_FFFF`.
    Registration,
    /// Catalog errors, `0x0004_0000–0x0004_FFFF`.
    Catalog,
    /// File and upload errors, `0x0005_0000–0x0005_FFFF`.
    FileUpload,
    /// Quorum and cluster errors, `0x0006_0000–0x0006_FFFF`.
    Quorum,
    /// Resource errors, `0x0007_0000–0x0007_FFFF`.
    Resource,
    /// Internal errors, `0x0008_0000–0x0008_FFFF`.
    Internal,
    /// Every other non-zero `uint32` value, including unused in-range values.
    Reserved,
}

/// Class of a code: `0` → [`ResultClass::Success`], the published
/// `0x000N_0000..0x000N_FFFF` ranges, else [`ResultClass::Reserved`].
pub fn class_of(code: u32) -> ResultClass {
    match code {
        0 => ResultClass::Success,
        0x0001_0000..=0x0001_FFFF => ResultClass::Request,
        0x0002_0000..=0x0002_FFFF => ResultClass::Authentication,
        0x0003_0000..=0x0003_FFFF => ResultClass::Registration,
        0x0004_0000..=0x0004_FFFF => ResultClass::Catalog,
        0x0005_0000..=0x0005_FFFF => ResultClass::FileUpload,
        0x0006_0000..=0x0006_FFFF => ResultClass::Quorum,
        0x0007_0000..=0x0007_FFFF => ResultClass::Resource,
        0x0008_0000..=0x0008_FFFF => ResultClass::Internal,
        _ => ResultClass::Reserved,
    }
}

/// Reasons a result envelope is not valid for protocol version 1.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum ResultError {
    /// The value is not a two-element CBOR array.
    #[error("result envelope must be a two-element array")]
    WrongShape,
    /// A position carries the wrong CBOR type.
    #[error("wrong CBOR type for {0}")]
    WrongType(&'static str),
    /// The result code does not fit `uint32`.
    #[error("result code {0} does not fit uint32")]
    CodeOutOfRange(u64),
    /// A non-zero code carries a non-empty argument array.
    #[error("a non-zero result code must have empty arguments")]
    NonEmptyErrorArgs,
}

#[cfg(test)]
mod tests {
    use super::{
        AUTHENTICATION_REJECTED, CODE_SUCCESS, INTERNAL_ERROR, INVALID_ARGUMENTS, INVALID_CURSOR,
        INVALID_FRAME_SEQUENCE, RAFT_QUORUM_UNAVAILABLE, REGISTRATION_CONFLICT, ResultClass,
        ResultEnvelope, ResultError, STORAGE_CAPACITY_EXHAUSTED, TEMPORARY_UNAVAILABLE,
        UNKNOWN_OPERATION, class_of,
    };
    use crate::cbor::{self, Value};

    #[test]
    fn round_trips_success_envelope() {
        let envelope = ResultEnvelope {
            code: CODE_SUCCESS,
            args: vec![Value::Uint(1)],
        };
        let bytes = envelope.encode().expect("envelope is valid");
        let value = cbor::decode(&bytes).expect("envelope bytes are valid CBOR");
        assert_eq!(ResultEnvelope::decode(&value), Ok(envelope));
    }

    #[test]
    fn round_trips_error_envelope_with_empty_arguments() {
        let envelope = ResultEnvelope {
            code: TEMPORARY_UNAVAILABLE,
            args: Vec::new(),
        };
        let bytes = envelope.encode().expect("envelope is valid");
        let value = cbor::decode(&bytes).expect("envelope bytes are valid CBOR");
        assert_eq!(ResultEnvelope::decode(&value), Ok(envelope));
    }

    #[test]
    fn rejects_arguments_with_non_zero_code() {
        let envelope = ResultEnvelope {
            code: INTERNAL_ERROR,
            args: vec![Value::Null],
        };
        assert_eq!(envelope.encode(), Err(ResultError::NonEmptyErrorArgs));
    }

    #[test]
    fn rejects_arguments_with_non_zero_code_when_decoding() {
        let value = Value::Array(vec![
            Value::Uint(u64::from(INTERNAL_ERROR)),
            Value::Array(vec![Value::Null]),
        ]);
        assert_eq!(
            ResultEnvelope::decode(&value),
            Err(ResultError::NonEmptyErrorArgs)
        );
    }

    #[test]
    fn classifies_success_and_every_published_range() {
        assert_eq!(class_of(CODE_SUCCESS), ResultClass::Success);
        assert_eq!(class_of(UNKNOWN_OPERATION), ResultClass::Request);
        assert_eq!(class_of(INVALID_ARGUMENTS), ResultClass::Request);
        assert_eq!(
            class_of(AUTHENTICATION_REJECTED),
            ResultClass::Authentication
        );
        assert_eq!(class_of(REGISTRATION_CONFLICT), ResultClass::Registration);
        assert_eq!(class_of(INVALID_CURSOR), ResultClass::Catalog);
        assert_eq!(class_of(INVALID_FRAME_SEQUENCE), ResultClass::FileUpload);
        assert_eq!(class_of(RAFT_QUORUM_UNAVAILABLE), ResultClass::Quorum);
        assert_eq!(class_of(STORAGE_CAPACITY_EXHAUSTED), ResultClass::Resource);
        assert_eq!(class_of(INTERNAL_ERROR), ResultClass::Internal);
    }

    #[test]
    fn classifies_codes_outside_published_ranges_as_reserved() {
        assert_eq!(class_of(0x0009_0001), ResultClass::Reserved);
        assert_eq!(class_of(u32::MAX), ResultClass::Reserved);
    }
}
