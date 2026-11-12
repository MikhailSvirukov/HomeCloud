//! Inter-server publish-version and replica-status wire messages.
//!
//! SRS: `INT-NET-093 / 1`, `INT-NET-094 / 1`, `INT-NET-121 / 1`,
//! `INT-NET-122 / 1`.
//!
//! A publish-version command carries the login, upload identity, original
//! metadata, the coordinator node identifier, and the durable replica node
//! identifiers. Replica identifiers are wire UUIDs here; quorum and duplicate
//! validation happen later in [`crate::validate_publish_version`]. A replica
//! status reports the login, upload identity, replica node identifier, original
//! SHA-256, and availability flag. Both result messages use the shared
//! `[result_code, result_args]` envelope, where a zero code carries the upload
//! identity and commit index.

use core::fmt;

use ciborium::value::Value;

use crate::blob_put::{
    canonical_bytes, decode_metadata, decode_value, encode_metadata_value, validate_metadata,
    BlobMetadata, BlobPutError,
};
use crate::client_limits::ClientLimits;
use crate::metadata::{ContentHash, UploadId, CONTENT_HASH_LEN, UPLOAD_ID_LEN};

/// Result alias for replication wire message validation.
type Result<T> = core::result::Result<T, ReplicationExchangeError>;

/// Wire code of the publish-version command.
const PUBLISH_VERSION_MESSAGE: u32 = 8;
/// Wire code of the publish-version result.
const PUBLISH_VERSION_RESULT_MESSAGE: u32 = 9;
/// Wire code of the replica-status command.
const REPLICA_STATUS_MESSAGE: u32 = 16;
/// Wire code of the replica-status result.
const REPLICA_STATUS_RESULT_MESSAGE: u32 = 17;
/// Length of a wire node identifier UUID in bytes.
const NODE_ID_LEN: usize = 16;
/// Success result code.
const RESULT_CODE_SUCCESS: u32 = 0;

/// Publish-version command carrying the upload and its durable replicas.
#[derive(Clone, PartialEq, Eq)]
pub struct PublishVersionMessage {
    /// Login owning the upload.
    pub login: String,
    /// Identifier of the accepted upload version.
    pub upload_id: UploadId,
    /// Original upload metadata.
    pub metadata: BlobMetadata,
    /// Wire identifier of the node that accepted the upload.
    pub coordinator_id: [u8; NODE_ID_LEN],
    /// Wire identifiers of the durable blob replicas.
    pub replica_ids: Vec<[u8; NODE_ID_LEN]>,
}

impl fmt::Debug for PublishVersionMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublishVersionMessage")
            .field("login_len", &self.login.len())
            .field("upload_id", &self.upload_id)
            .field("metadata", &self.metadata)
            .field("coordinator_id", &self.coordinator_id)
            .field("replica_count", &self.replica_ids.len())
            .finish()
    }
}

/// Replica-status command reporting one node's copy of a committed upload.
#[derive(Clone, PartialEq, Eq)]
pub struct ReplicaStatusMessage {
    /// Login owning the upload.
    pub login: String,
    /// Identifier of the committed upload version.
    pub upload_id: UploadId,
    /// Wire identifier of the reporting node.
    pub replica_id: [u8; NODE_ID_LEN],
    /// SHA-256 digest of the original upload bytes.
    pub sha256: ContentHash,
    /// Whether the reporting node holds a verified durable copy.
    pub available: bool,
}

impl fmt::Debug for ReplicaStatusMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplicaStatusMessage")
            .field("login_len", &self.login.len())
            .field("upload_id", &self.upload_id)
            .field("replica_id", &self.replica_id)
            .field("sha256", &self.sha256)
            .field("available", &self.available)
            .finish()
    }
}

/// Publish-version or replica-status outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicationResult {
    /// Protocol result code.
    pub result_code: u32,
    /// Upload identifier on success, absent on error.
    pub upload_id: Option<UploadId>,
    /// Committed Raft index on success, absent on error.
    pub commit_index: Option<u64>,
}

/// Errors from encoding or decoding a replication wire message.
#[derive(Debug, thiserror::Error)]
pub enum ReplicationExchangeError {
    /// The decoded CBOR value is not an array.
    #[error("the replication value is not a CBOR array")]
    NotAnArray,
    /// The message type is not the expected replication type.
    #[error("the replication message has the wrong type")]
    WrongMessageType,
    /// The envelope or argument array has the wrong length.
    #[error("the replication logical message has the wrong length")]
    WrongLength,
    /// The login is outside the configured byte range or uses a non-ASCII byte.
    #[error("the login is invalid")]
    InvalidLogin,
    /// The upload identifier is not exactly 16 bytes.
    #[error("the upload id is invalid")]
    InvalidUploadId,
    /// A coordinator or replica identifier is not exactly 16 bytes.
    #[error("the node id is invalid")]
    InvalidReplicaId,
    /// The SHA-256 value is not exactly 32 bytes.
    #[error("the SHA-256 value is invalid")]
    InvalidHash,
    /// The availability flag is not a CBOR boolean.
    #[error("the availability flag is invalid")]
    InvalidAvailability,
    /// The result code and result arguments do not form a valid combination.
    #[error("the result code/argument combination is invalid")]
    InvalidResult,
    /// The embedded upload metadata failed blob-put validation.
    #[error(transparent)]
    Metadata(#[from] BlobPutError),
    /// The replication value could not be decoded, was non-canonical, or had trailing bytes.
    #[error("the replication value could not be decoded")]
    Decode,
    /// The replication value could not be encoded.
    #[error("the replication value could not be encoded")]
    Encode,
}

/// Encodes a publish-version command as the exact `[8, [login, upload_id, metadata, coordinator_id, replica_ids]]` shape.
pub fn encode_publish_version(
    message: &PublishVersionMessage,
    limits: &ClientLimits,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<Vec<u8>> {
    if !limits.login_allowed(&message.login) {
        return Err(ReplicationExchangeError::InvalidLogin);
    }
    validate_metadata(&message.metadata, max_source_name_len, max_file_bytes)?;

    let replica_ids = message
        .replica_ids
        .iter()
        .map(|replica_id| Value::Bytes(replica_id.to_vec()))
        .collect();
    let arguments = Value::Array(vec![
        Value::Text(message.login.clone()),
        Value::Bytes(message.upload_id.as_bytes().to_vec()),
        encode_metadata_value(&message.metadata),
        Value::Bytes(message.coordinator_id.to_vec()),
        Value::Array(replica_ids),
    ]);
    encode_envelope(PUBLISH_VERSION_MESSAGE, arguments)
}

/// Decodes a publish-version command with the exact `[8, [login, upload_id, metadata, coordinator_id, replica_ids]]` shape.
pub fn decode_publish_version(
    bytes: &[u8],
    limits: &ClientLimits,
    max_source_name_len: usize,
    max_file_bytes: u64,
) -> Result<PublishVersionMessage> {
    let arguments = decode_arguments(bytes, PUBLISH_VERSION_MESSAGE)?;
    let [login, upload_id, metadata, coordinator_id, replica_ids] = arguments
        .try_into()
        .map_err(|_| ReplicationExchangeError::WrongLength)?;

    let login = decode_login(login, limits)?;
    let upload_id = decode_upload_id(upload_id)?;
    let metadata = decode_metadata(metadata, max_source_name_len, max_file_bytes)?;
    let coordinator_id = decode_node_id(coordinator_id)?;
    let Value::Array(replica_ids) = replica_ids else {
        return Err(ReplicationExchangeError::InvalidReplicaId);
    };
    let replica_ids = replica_ids
        .into_iter()
        .map(decode_node_id)
        .collect::<Result<Vec<_>>>()?;

    Ok(PublishVersionMessage {
        login,
        upload_id,
        metadata,
        coordinator_id,
        replica_ids,
    })
}

/// Encodes a publish-version result as the exact `[9, [result_code, result_args]]` shape.
pub fn encode_publish_version_result(result: &ReplicationResult) -> Result<Vec<u8>> {
    encode_result(PUBLISH_VERSION_RESULT_MESSAGE, result)
}

/// Decodes a publish-version result with the exact `[9, [result_code, result_args]]` shape.
pub fn decode_publish_version_result(bytes: &[u8]) -> Result<ReplicationResult> {
    decode_result(PUBLISH_VERSION_RESULT_MESSAGE, bytes)
}

/// Encodes a replica-status command as the exact `[16, [login, upload_id, replica_id, sha256, available]]` shape.
pub fn encode_replica_status(
    status: &ReplicaStatusMessage,
    limits: &ClientLimits,
) -> Result<Vec<u8>> {
    if !limits.login_allowed(&status.login) {
        return Err(ReplicationExchangeError::InvalidLogin);
    }

    let arguments = Value::Array(vec![
        Value::Text(status.login.clone()),
        Value::Bytes(status.upload_id.as_bytes().to_vec()),
        Value::Bytes(status.replica_id.to_vec()),
        Value::Bytes(status.sha256.as_bytes().to_vec()),
        Value::Bool(status.available),
    ]);
    encode_envelope(REPLICA_STATUS_MESSAGE, arguments)
}

/// Decodes a replica-status command with the exact `[16, [login, upload_id, replica_id, sha256, available]]` shape.
pub fn decode_replica_status(bytes: &[u8], limits: &ClientLimits) -> Result<ReplicaStatusMessage> {
    let arguments = decode_arguments(bytes, REPLICA_STATUS_MESSAGE)?;
    let [login, upload_id, replica_id, sha256, available] = arguments
        .try_into()
        .map_err(|_| ReplicationExchangeError::WrongLength)?;

    let login = decode_login(login, limits)?;
    let upload_id = decode_upload_id(upload_id)?;
    let replica_id = decode_node_id(replica_id)?;
    let sha256 = decode_hash(sha256)?;
    let Value::Bool(available) = available else {
        return Err(ReplicationExchangeError::InvalidAvailability);
    };

    Ok(ReplicaStatusMessage {
        login,
        upload_id,
        replica_id,
        sha256,
        available,
    })
}

/// Encodes a replica-status result as the exact `[17, [result_code, result_args]]` shape.
pub fn encode_replica_status_result(result: &ReplicationResult) -> Result<Vec<u8>> {
    encode_result(REPLICA_STATUS_RESULT_MESSAGE, result)
}

/// Decodes a replica-status result with the exact `[17, [result_code, result_args]]` shape.
pub fn decode_replica_status_result(bytes: &[u8]) -> Result<ReplicationResult> {
    decode_result(REPLICA_STATUS_RESULT_MESSAGE, bytes)
}

fn encode_result(message_type: u32, result: &ReplicationResult) -> Result<Vec<u8>> {
    let result_args = match (result.result_code, result.upload_id, result.commit_index) {
        (RESULT_CODE_SUCCESS, Some(upload_id), Some(commit_index)) => vec![
            Value::Bytes(upload_id.as_bytes().to_vec()),
            Value::Integer(commit_index.into()),
        ],
        (RESULT_CODE_SUCCESS, _, _) => return Err(ReplicationExchangeError::InvalidResult),
        (_, Some(_), _) | (_, _, Some(_)) => return Err(ReplicationExchangeError::InvalidResult),
        (_, None, None) => Vec::new(),
    };

    let arguments = Value::Array(vec![
        Value::Integer(result.result_code.into()),
        Value::Array(result_args),
    ]);
    encode_envelope(message_type, arguments)
}

fn decode_result(message_type: u32, bytes: &[u8]) -> Result<ReplicationResult> {
    let arguments = decode_arguments(bytes, message_type)?;
    let [result_code, result_args] = arguments
        .try_into()
        .map_err(|_| ReplicationExchangeError::WrongLength)?;
    let result_code = integer_u32(result_code).ok_or(ReplicationExchangeError::InvalidResult)?;

    let Value::Array(result_args) = result_args else {
        return Err(ReplicationExchangeError::InvalidResult);
    };
    if result_code == RESULT_CODE_SUCCESS {
        let [upload_id, commit_index] = result_args
            .try_into()
            .map_err(|_| ReplicationExchangeError::InvalidResult)?;
        let upload_id =
            decode_upload_id(upload_id).map_err(|_| ReplicationExchangeError::InvalidResult)?;
        let commit_index =
            unsigned_u64(commit_index).ok_or(ReplicationExchangeError::InvalidResult)?;
        Ok(ReplicationResult {
            result_code,
            upload_id: Some(upload_id),
            commit_index: Some(commit_index),
        })
    } else if result_args.is_empty() {
        Ok(ReplicationResult {
            result_code,
            upload_id: None,
            commit_index: None,
        })
    } else {
        Err(ReplicationExchangeError::InvalidResult)
    }
}

fn encode_envelope(message_type: u32, arguments: Value) -> Result<Vec<u8>> {
    let envelope = Value::Array(vec![Value::Integer(message_type.into()), arguments]);
    canonical_bytes(&envelope).map_err(|_| ReplicationExchangeError::Encode)
}

fn decode_arguments(bytes: &[u8], expected: u32) -> Result<Vec<Value>> {
    let Value::Array(elements) =
        decode_value(bytes).map_err(|_| ReplicationExchangeError::Decode)?
    else {
        return Err(ReplicationExchangeError::NotAnArray);
    };
    let [message_type, arguments] = elements
        .try_into()
        .map_err(|_| ReplicationExchangeError::WrongLength)?;
    expect_message_type(message_type, expected)?;

    let Value::Array(arguments) = arguments else {
        return Err(ReplicationExchangeError::WrongLength);
    };
    Ok(arguments)
}

fn expect_message_type(value: Value, expected: u32) -> Result<()> {
    if integer_u32(value) == Some(expected) {
        Ok(())
    } else {
        Err(ReplicationExchangeError::WrongMessageType)
    }
}

fn decode_login(value: Value, limits: &ClientLimits) -> Result<String> {
    let Value::Text(login) = value else {
        return Err(ReplicationExchangeError::InvalidLogin);
    };
    if !limits.login_allowed(&login) {
        return Err(ReplicationExchangeError::InvalidLogin);
    }
    Ok(login)
}

fn decode_upload_id(value: Value) -> Result<UploadId> {
    fixed::<UPLOAD_ID_LEN>(value)
        .map(UploadId::from_bytes)
        .ok_or(ReplicationExchangeError::InvalidUploadId)
}

fn decode_node_id(value: Value) -> Result<[u8; NODE_ID_LEN]> {
    fixed::<NODE_ID_LEN>(value).ok_or(ReplicationExchangeError::InvalidReplicaId)
}

fn decode_hash(value: Value) -> Result<ContentHash> {
    fixed::<CONTENT_HASH_LEN>(value)
        .map(ContentHash::from_bytes)
        .ok_or(ReplicationExchangeError::InvalidHash)
}

fn integer_u32(value: Value) -> Option<u32> {
    let Value::Integer(number) = value else {
        return None;
    };
    u32::try_from(number).ok()
}

fn unsigned_u64(value: Value) -> Option<u64> {
    let Value::Integer(number) = value else {
        return None;
    };
    u64::try_from(number).ok()
}

fn fixed<const N: usize>(value: Value) -> Option<[u8; N]> {
    let Value::Bytes(bytes) = value else {
        return None;
    };
    bytes.as_slice().try_into().ok()
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroUsize;

    use super::*;

    const MAX_SOURCE_NAME_LEN: usize = 64;
    const MAX_FILE_BYTES: u64 = 1 << 20;

    fn non_zero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test bound is non-zero")
    }

    fn limits() -> ClientLimits {
        ClientLimits::new(
            non_zero(1),
            non_zero(64),
            non_zero(8),
            non_zero(1024),
            non_zero(64),
        )
        .expect("test limits are valid")
    }

    fn metadata() -> BlobMetadata {
        BlobMetadata {
            source_name: "report.txt".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            original_size_bytes: 10,
            compressed_size_bytes: 4,
            sha256: ContentHash::from_bytes([0xab; CONTENT_HASH_LEN]),
        }
    }

    fn publish_version() -> PublishVersionMessage {
        PublishVersionMessage {
            login: "alice".to_owned(),
            upload_id: UploadId::from_bytes([1; UPLOAD_ID_LEN]),
            metadata: metadata(),
            coordinator_id: [2; NODE_ID_LEN],
            replica_ids: vec![[2; NODE_ID_LEN], [3; NODE_ID_LEN]],
        }
    }

    fn replica_status() -> ReplicaStatusMessage {
        ReplicaStatusMessage {
            login: "alice".to_owned(),
            upload_id: UploadId::from_bytes([4; UPLOAD_ID_LEN]),
            replica_id: [5; NODE_ID_LEN],
            sha256: ContentHash::from_bytes([6; CONTENT_HASH_LEN]),
            available: true,
        }
    }

    fn encode_publish(message: &PublishVersionMessage) -> Vec<u8> {
        encode_publish_version(message, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
            .expect("test publish version encodes")
    }

    fn encode_raw(message_type: u32, arguments: Vec<Value>) -> Vec<u8> {
        let value = Value::Array(vec![
            Value::Integer(message_type.into()),
            Value::Array(arguments),
        ]);
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&value, &mut bytes).expect("test value encodes");
        bytes
    }

    #[test]
    fn messages_round_trip() {
        let publish = publish_version();
        let bytes = encode_publish(&publish);
        let decoded =
            decode_publish_version(&bytes, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES)
                .expect("test publish version decodes");
        assert_eq!(decoded, publish);

        let status = replica_status();
        let bytes = encode_replica_status(&status, &limits()).expect("test replica status encodes");
        let decoded =
            decode_replica_status(&bytes, &limits()).expect("test replica status decodes");
        assert_eq!(decoded, status);

        let success = ReplicationResult {
            result_code: RESULT_CODE_SUCCESS,
            upload_id: Some(UploadId::from_bytes([9; UPLOAD_ID_LEN])),
            commit_index: Some(42),
        };
        let bytes = encode_publish_version_result(&success).expect("test result encodes");
        let decoded = decode_publish_version_result(&bytes).expect("test result decodes");
        assert_eq!(decoded, success);

        let failure = ReplicationResult {
            result_code: 0x0005_0002,
            upload_id: None,
            commit_index: None,
        };
        let bytes = encode_replica_status_result(&failure).expect("test result encodes");
        let decoded = decode_replica_status_result(&bytes).expect("test result decodes");
        assert_eq!(decoded, failure);
    }

    #[test]
    fn publish_version_rejects_invalid_login() {
        let mut message = publish_version();
        message.login = "not a login".to_owned();

        assert!(matches!(
            encode_publish_version(&message, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
            Err(ReplicationExchangeError::InvalidLogin)
        ));
    }

    #[test]
    fn publish_version_rejects_replica_id_of_wrong_length() {
        let bytes = encode_raw(
            PUBLISH_VERSION_MESSAGE,
            vec![
                Value::Text("alice".to_owned()),
                Value::Bytes(vec![1; UPLOAD_ID_LEN]),
                encode_metadata_value(&metadata()),
                Value::Bytes(vec![2; NODE_ID_LEN]),
                Value::Array(vec![Value::Bytes(vec![3; NODE_ID_LEN - 1])]),
            ],
        );

        assert!(matches!(
            decode_publish_version(&bytes, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
            Err(ReplicationExchangeError::InvalidReplicaId)
        ));
    }

    #[test]
    fn publish_version_rejects_trailing_bytes() {
        let mut bytes = encode_publish(&publish_version());
        bytes.push(0);

        assert!(matches!(
            decode_publish_version(&bytes, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
            Err(ReplicationExchangeError::Decode)
        ));
    }

    #[test]
    fn publish_version_rejects_non_canonical_integer() {
        let bytes = encode_publish(&publish_version());
        let mut non_canonical = vec![0x82, 0x18, 0x08];
        non_canonical.extend_from_slice(&bytes[2..]);

        assert!(matches!(
            decode_publish_version(
                &non_canonical,
                &limits(),
                MAX_SOURCE_NAME_LEN,
                MAX_FILE_BYTES
            ),
            Err(ReplicationExchangeError::Decode)
        ));
    }

    #[test]
    fn publish_version_rejects_wrong_message_type() {
        let result = ReplicationResult {
            result_code: RESULT_CODE_SUCCESS,
            upload_id: Some(UploadId::from_bytes([7; UPLOAD_ID_LEN])),
            commit_index: Some(1),
        };
        let bytes = encode_publish_version_result(&result).expect("test result encodes");

        assert!(matches!(
            decode_publish_version(&bytes, &limits(), MAX_SOURCE_NAME_LEN, MAX_FILE_BYTES),
            Err(ReplicationExchangeError::WrongMessageType)
        ));
    }

    #[test]
    fn publish_version_result_rejects_incomplete_success() {
        let result = ReplicationResult {
            result_code: RESULT_CODE_SUCCESS,
            upload_id: Some(UploadId::from_bytes([8; UPLOAD_ID_LEN])),
            commit_index: None,
        };

        assert!(matches!(
            encode_publish_version_result(&result),
            Err(ReplicationExchangeError::InvalidResult)
        ));
    }

    #[test]
    fn publish_version_result_rejects_incomplete_success_arguments() {
        let bytes = encode_raw(
            PUBLISH_VERSION_RESULT_MESSAGE,
            vec![
                Value::Integer(RESULT_CODE_SUCCESS.into()),
                Value::Array(vec![Value::Bytes(vec![9; UPLOAD_ID_LEN])]),
            ],
        );

        assert!(matches!(
            decode_publish_version_result(&bytes),
            Err(ReplicationExchangeError::InvalidResult)
        ));
    }

    #[test]
    fn replica_status_rejects_non_boolean_availability() {
        let bytes = encode_raw(
            REPLICA_STATUS_MESSAGE,
            vec![
                Value::Text("alice".to_owned()),
                Value::Bytes(vec![1; UPLOAD_ID_LEN]),
                Value::Bytes(vec![2; NODE_ID_LEN]),
                Value::Bytes(vec![3; CONTENT_HASH_LEN]),
                Value::Integer(1u32.into()),
            ],
        );

        assert!(matches!(
            decode_replica_status(&bytes, &limits()),
            Err(ReplicationExchangeError::InvalidAvailability)
        ));
    }

    #[test]
    fn replica_status_rejects_hash_of_wrong_length() {
        let bytes = encode_raw(
            REPLICA_STATUS_MESSAGE,
            vec![
                Value::Text("alice".to_owned()),
                Value::Bytes(vec![1; UPLOAD_ID_LEN]),
                Value::Bytes(vec![2; NODE_ID_LEN]),
                Value::Bytes(vec![3; CONTENT_HASH_LEN - 1]),
                Value::Bool(true),
            ],
        );

        assert!(matches!(
            decode_replica_status(&bytes, &limits()),
            Err(ReplicationExchangeError::InvalidHash)
        ));
    }

    #[test]
    fn replica_status_result_rejects_error_arguments() {
        let bytes = encode_raw(
            REPLICA_STATUS_RESULT_MESSAGE,
            vec![
                Value::Integer(0x0005_0002u32.into()),
                Value::Array(vec![Value::Bytes(vec![1; UPLOAD_ID_LEN])]),
            ],
        );

        assert!(matches!(
            decode_replica_status_result(&bytes),
            Err(ReplicationExchangeError::InvalidResult)
        ));
    }

    #[test]
    fn debug_output_omits_login_and_source_name() {
        let publish = format!("{:?}", publish_version());
        assert!(!publish.contains("alice"));
        assert!(!publish.contains("report.txt"));

        let status = format!("{:?}", replica_status());
        assert!(!status.contains("alice"));
    }
}
