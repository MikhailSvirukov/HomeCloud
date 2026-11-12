//! Publish-version quorum and replica-status validation.
//!
//! SRS: `INT-NET-113 / 1`, `INT-NET-124 / 1`.
//!
//! A publish version is accepted only for a coordinator in the fixed voter set
//! with a unique durable replica list that contains the coordinator and a strict
//! majority of the voters. A repeated committed upload id returns the stored
//! version only when its metadata is identical. Replica status is accepted only
//! for a fixed voter and an existing committed upload whose login and SHA-256
//! match; the `available` flag is carried through without interpretation.

use std::collections::BTreeSet;
use std::fmt;

use crate::metadata::{ContentHash, UploadId, UploadRecord};
use crate::raft::{CommittedVersion, NodeId};

/// Result alias for publish-version and replica-status validation.
type Result<T> = core::result::Result<T, ReplicationError>;

/// A publish-version request with its coordinator and durable blob replicas.
pub struct PublishVersion {
    pub record: UploadRecord,
    pub coordinator: NodeId,
    pub replicas: Vec<NodeId>,
}

impl fmt::Debug for PublishVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublishVersion")
            .field("upload_id", &self.record.upload_id)
            .field("coordinator", &self.coordinator)
            .field("replica_count", &self.replicas.len())
            .finish()
    }
}

/// Replica status reported for one committed upload.
pub struct ReplicaStatus {
    pub login: String,
    pub upload_id: UploadId,
    pub replica_id: NodeId,
    pub sha256: ContentHash,
    pub available: bool,
}

impl fmt::Debug for ReplicaStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplicaStatus")
            .field("upload_id", &self.upload_id)
            .field("replica_id", &self.replica_id)
            .field("sha256", &self.sha256)
            .field("available", &self.available)
            .finish()
    }
}

/// Failure modes when validating a publish version or replica status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReplicationError {
    /// The fixed voter set has no members to reach a majority of.
    #[error("the fixed voter set is empty")]
    EmptyVoterSet,
    /// The coordinator does not belong to the fixed voter set.
    #[error("the coordinator is not a fixed voter")]
    CoordinatorUnknown,
    /// The coordinator is not listed among the durable replicas.
    #[error("the coordinator is not listed as a durable replica")]
    CoordinatorNotReplica,
    /// A durable replica does not belong to the fixed voter set.
    #[error("a replica is not a fixed voter")]
    ReplicaUnknown,
    /// The durable replica list contains the same node more than once.
    #[error("a replica is listed more than once")]
    DuplicateReplica,
    /// The durable replicas do not form a strict majority of the fixed voters.
    #[error("the durable replicas are not a strict majority")]
    QuorumNotReached,
    /// A repeated committed upload id carries different metadata.
    #[error("the committed upload has different metadata")]
    MetadataConflict,
    /// The upload id has no committed version.
    #[error("the upload is not committed")]
    UnknownUpload,
    /// The replica status login differs from the committed upload.
    #[error("the replica status login differs from the committed upload")]
    LoginMismatch,
    /// The replica status SHA-256 differs from the committed upload.
    #[error("the replica status SHA-256 differs from the committed upload")]
    HashMismatch,
}

/// Validates a publish version against the fixed voter set and the existing
/// committed version for the same upload id.
///
/// An empty voter set, an unknown coordinator, a missing coordinator replica,
/// an unknown or repeated replica, and a non-majority replica set are rejected
/// before the repeated upload id is compared. A covered upload id with identical
/// metadata returns the stored version; different metadata is a conflict. A new
/// upload id commits the supplied record with the unique replica set.
pub fn validate_publish_version(
    command: &PublishVersion,
    voters: &BTreeSet<NodeId>,
    existing: Option<&CommittedVersion>,
) -> Result<CommittedVersion> {
    if voters.is_empty() {
        return Err(ReplicationError::EmptyVoterSet);
    }
    if !voters.contains(&command.coordinator) {
        return Err(ReplicationError::CoordinatorUnknown);
    }
    if !command.replicas.contains(&command.coordinator) {
        return Err(ReplicationError::CoordinatorNotReplica);
    }

    let mut unique_replicas = BTreeSet::new();
    for replica in &command.replicas {
        if !voters.contains(replica) {
            return Err(ReplicationError::ReplicaUnknown);
        }
        if !unique_replicas.insert(*replica) {
            return Err(ReplicationError::DuplicateReplica);
        }
    }
    if unique_replicas.len() <= voters.len() / 2 {
        return Err(ReplicationError::QuorumNotReached);
    }

    if let Some(version) = existing {
        if version.record == command.record {
            return Ok(version.clone());
        }
        return Err(ReplicationError::MetadataConflict);
    }
    Ok(CommittedVersion {
        record: command.record.clone(),
        replicas: unique_replicas,
    })
}

/// Validates a replica status against the fixed voter set and the existing
/// committed version for the upload.
///
/// The replica must be a fixed voter and the upload must already be committed.
/// The status login and SHA-256 must match the committed record; the `available`
/// flag is carried through unchanged. Repeating the same status is idempotent
/// and returns `Ok(())`.
pub fn validate_replica_status(
    status: &ReplicaStatus,
    voters: &BTreeSet<NodeId>,
    existing: Option<&CommittedVersion>,
) -> Result<()> {
    if !voters.contains(&status.replica_id) {
        return Err(ReplicationError::ReplicaUnknown);
    }
    let Some(version) = existing else {
        return Err(ReplicationError::UnknownUpload);
    };
    if version.record.upload_id != status.upload_id {
        return Err(ReplicationError::UnknownUpload);
    }
    if version.record.login != status.login {
        return Err(ReplicationError::LoginMismatch);
    }
    if version.record.content_hash != status.sha256 {
        return Err(ReplicationError::HashMismatch);
    }
    Ok(())
}
