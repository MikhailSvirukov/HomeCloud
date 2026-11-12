//! Public API integration tests for publish-version quorum and replica-status
//! validation.
//!
//! SRS: `INT-NET-113 / 1`, `INT-NET-124 / 1`.

use std::collections::BTreeSet;

use storage_node::{
    validate_publish_version, validate_replica_status, CommittedVersion, ContentHash,
    PublishVersion, ReplicaStatus, ReplicationError, UploadId, UploadRecord, UPLOAD_ID_LEN,
};

const FIXED_LOGIN: &str = "alice";
const FIXED_SOURCE_NAME: &str = "photo.jpg";
const FIXED_PAYLOAD: &[u8] = b"payload";

fn upload_id(seed: u8) -> UploadId {
    UploadId::from_bytes([seed; UPLOAD_ID_LEN])
}

fn record(id: UploadId) -> UploadRecord {
    UploadRecord::new(
        id,
        FIXED_LOGIN,
        FIXED_SOURCE_NAME,
        100,
        50,
        ContentHash::digest(FIXED_PAYLOAD),
        1_690_000_000_000,
        1_700_000_000_000,
    )
    .expect("the fixed test record is valid")
}

fn voters(ids: &[u64]) -> BTreeSet<u64> {
    ids.iter().copied().collect()
}

fn existing_version() -> CommittedVersion {
    CommittedVersion {
        record: record(upload_id(1)),
        replicas: voters(&[1, 2]),
    }
}

#[test]
fn accepts_the_coordinator_and_a_strict_majority() {
    let command = PublishVersion {
        record: record(upload_id(1)),
        coordinator: 1,
        replicas: vec![2, 1],
    };

    let committed = validate_publish_version(&command, &voters(&[1, 2, 3]), None)
        .expect("a coordinator with a strict majority is accepted");

    assert_eq!(committed.record, record(upload_id(1)));
    assert_eq!(committed.replicas, voters(&[1, 2]));
}

#[test]
fn rejects_an_empty_voter_set_and_an_unknown_coordinator() {
    let command = PublishVersion {
        record: record(upload_id(2)),
        coordinator: 1,
        replicas: vec![1],
    };

    assert_eq!(
        validate_publish_version(&command, &BTreeSet::new(), None),
        Err(ReplicationError::EmptyVoterSet)
    );

    let outsider = PublishVersion {
        coordinator: 9,
        replicas: vec![9],
        ..command
    };
    assert_eq!(
        validate_publish_version(&outsider, &voters(&[1, 2, 3]), None),
        Err(ReplicationError::CoordinatorUnknown)
    );
}

#[test]
fn rejects_a_coordinator_missing_from_the_replicas() {
    let command = PublishVersion {
        record: record(upload_id(3)),
        coordinator: 1,
        replicas: vec![2, 3],
    };

    assert_eq!(
        validate_publish_version(&command, &voters(&[1, 2, 3]), None),
        Err(ReplicationError::CoordinatorNotReplica)
    );
}

#[test]
fn rejects_unknown_duplicate_and_non_majority_replicas() {
    let unknown = PublishVersion {
        record: record(upload_id(4)),
        coordinator: 1,
        replicas: vec![1, 9],
    };
    assert_eq!(
        validate_publish_version(&unknown, &voters(&[1, 2, 3]), None),
        Err(ReplicationError::ReplicaUnknown)
    );

    let duplicate = PublishVersion {
        replicas: vec![1, 1, 2],
        ..unknown
    };
    assert_eq!(
        validate_publish_version(&duplicate, &voters(&[1, 2, 3]), None),
        Err(ReplicationError::DuplicateReplica)
    );

    let non_majority = PublishVersion {
        record: record(upload_id(4)),
        coordinator: 1,
        replicas: vec![1, 2],
    };
    assert_eq!(
        validate_publish_version(&non_majority, &voters(&[1, 2, 3, 4]), None),
        Err(ReplicationError::QuorumNotReached)
    );
}

#[test]
fn replays_identical_metadata_and_rejects_a_metadata_conflict() {
    let existing = existing_version();
    let replay = PublishVersion {
        record: record(upload_id(1)),
        coordinator: 1,
        replicas: vec![1, 2],
    };

    assert_eq!(
        validate_publish_version(&replay, &voters(&[1, 2, 3]), Some(&existing)),
        Ok(existing.clone())
    );

    let renamed = PublishVersion {
        record: UploadRecord::new(
            upload_id(1),
            FIXED_LOGIN,
            "renamed.jpg",
            100,
            50,
            ContentHash::digest(FIXED_PAYLOAD),
            1_690_000_000_000,
            1_700_000_000_000,
        )
        .expect("the conflicting record is structurally valid"),
        coordinator: 1,
        replicas: vec![1, 2],
    };
    assert_eq!(
        validate_publish_version(&renamed, &voters(&[1, 2, 3]), Some(&existing)),
        Err(ReplicationError::MetadataConflict)
    );

    let rehashed = PublishVersion {
        record: UploadRecord::new(
            upload_id(1),
            FIXED_LOGIN,
            FIXED_SOURCE_NAME,
            100,
            50,
            ContentHash::digest(b"other-payload"),
            1_690_000_000_000,
            1_700_000_000_000,
        )
        .expect("the conflicting record is structurally valid"),
        coordinator: 1,
        replicas: vec![1, 2],
    };
    assert_eq!(
        validate_publish_version(&rehashed, &voters(&[1, 2, 3]), Some(&existing)),
        Err(ReplicationError::MetadataConflict)
    );
}

#[test]
fn validates_a_replica_status_for_a_fixed_voter_and_committed_upload() {
    let existing = existing_version();
    let available = ReplicaStatus {
        login: FIXED_LOGIN.to_owned(),
        upload_id: upload_id(1),
        replica_id: 2,
        sha256: ContentHash::digest(FIXED_PAYLOAD),
        available: true,
    };

    assert_eq!(
        validate_replica_status(&available, &voters(&[1, 2, 3]), Some(&existing)),
        Ok(())
    );

    let unavailable = ReplicaStatus {
        available: false,
        ..available
    };
    assert_eq!(
        validate_replica_status(&unavailable, &voters(&[1, 2, 3]), Some(&existing)),
        Ok(())
    );
}

#[test]
fn rejects_an_unknown_replica_upload_login_or_hash() {
    let existing = existing_version();
    let unknown_replica = ReplicaStatus {
        login: FIXED_LOGIN.to_owned(),
        upload_id: upload_id(1),
        replica_id: 9,
        sha256: ContentHash::digest(FIXED_PAYLOAD),
        available: true,
    };
    assert_eq!(
        validate_replica_status(&unknown_replica, &voters(&[1, 2, 3]), Some(&existing)),
        Err(ReplicationError::ReplicaUnknown)
    );

    let missing_upload = ReplicaStatus {
        replica_id: 2,
        ..unknown_replica
    };
    assert_eq!(
        validate_replica_status(&missing_upload, &voters(&[1, 2, 3]), None),
        Err(ReplicationError::UnknownUpload)
    );

    let other_version = CommittedVersion {
        record: record(upload_id(2)),
        replicas: voters(&[1, 2]),
    };
    let other_upload = ReplicaStatus {
        login: FIXED_LOGIN.to_owned(),
        upload_id: upload_id(1),
        replica_id: 2,
        sha256: ContentHash::digest(FIXED_PAYLOAD),
        available: true,
    };
    assert_eq!(
        validate_replica_status(&other_upload, &voters(&[1, 2, 3]), Some(&other_version)),
        Err(ReplicationError::UnknownUpload)
    );

    let wrong_login = ReplicaStatus {
        login: "bob".to_owned(),
        ..other_upload
    };
    assert_eq!(
        validate_replica_status(&wrong_login, &voters(&[1, 2, 3]), Some(&existing)),
        Err(ReplicationError::LoginMismatch)
    );

    let wrong_hash = ReplicaStatus {
        login: FIXED_LOGIN.to_owned(),
        upload_id: upload_id(1),
        replica_id: 2,
        sha256: ContentHash::digest(b"other-payload"),
        available: true,
    };
    assert_eq!(
        validate_replica_status(&wrong_hash, &voters(&[1, 2, 3]), Some(&existing)),
        Err(ReplicationError::HashMismatch)
    );
}

#[test]
fn debug_output_redacts_the_login_and_source_name() {
    let command = PublishVersion {
        record: record(upload_id(1)),
        coordinator: 1,
        replicas: vec![1, 2],
    };
    let command_debug = format!("{command:?}");
    assert!(!command_debug.contains(FIXED_LOGIN));
    assert!(!command_debug.contains(FIXED_SOURCE_NAME));

    let status = ReplicaStatus {
        login: FIXED_LOGIN.to_owned(),
        upload_id: upload_id(1),
        replica_id: 2,
        sha256: ContentHash::digest(FIXED_PAYLOAD),
        available: true,
    };
    let status_debug = format!("{status:?}");
    assert!(!status_debug.contains(FIXED_LOGIN));
}
