//! Public API integration tests for the node Raft runtime boundary.
//!
//! A single-node runtime is started in-process from a static manifest and
//! exercises the whole startup boundary: the local storage entry and its
//! advertised address, the owner-only private-key files that must match the
//! manifest open keys, the manifest-derived voter set and transport, the
//! explicit bootstrap command, and a real local OpenRaft commit.
//!
//! SRS: `FR-CL-001 / 1` (storage nodes speak Raft directly, with no dedicated
//! server relay), `FR-CL-015 / 1` and `FR-CL-017 / 1` (the Raft cluster and
//! voter set come from the static manifest), `FR-CL-027 / 1` (the explicit
//! bootstrap initializes exactly the manifest storage voters), `FR-CL-030 / 1`
//! and `FR-CL-031 / 1` (the node process runs a local OpenRaft voting instance
//! and applies a committed client write), `CFG-CL-003 / 1` and
//! `CFG-CL-021 / 1` (the manifest carries the storage identity and open keys),
//! `CFG-CL-033 / 1` and `CFG-CL-039 / 1` (owner-only key files are validated
//! before a listener exists), `CFG-CL-007 / 1` (the peer transport applies the
//! configured storage connect timeout), `CFG-CL-029 / 1` and `CFG-CL-031 / 1`
//! (the local responder label must fit the configured byte limit),
//! `INT-NET-071 / 1` (the local sender id is the manifest entry's stable
//! identifier), `INT-NET-113 / 1` (a publish version validates the fixed-voter
//! quorum and committed metadata before it reaches Raft and again when the
//! entry is applied), `INT-NET-124 / 1` (a replica status is pre-checked and
//! validated again at apply time against fixed membership and committed upload
//! metadata),
//! `NFR-SEC-009 / 1` (local identity keys must match the
//! trusted manifest before peer exchange), and `NFR-OBS-005 / 3` (the node
//! exposes its Raft leader, term, last applied index, and voter count).

use core::num::{NonZeroU32, NonZeroUsize};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use storage_node::{
    ClientLimits, ContentHash, ManifestError, MessageSigningKey, PrivateKey, PublishVersion,
    RaftRequest, RaftResponse, RaftRuntime, RecipientPrivateKey, ReplicaStatus, ReplicationError,
    RuntimeError, StaticManifest, UploadId, UploadRecord, KEY_LEN, UPLOAD_ID_LEN,
};
use tempfile::TempDir;

/// Local storage label from the manifest.
const LOCAL_LABEL: &str = "node-1";
/// Dedicated-server label that is not a storage entry.
const DEDICATED_LABEL: &str = "dedicated";
/// Local storage host from the manifest.
const LOCAL_HOST: &str = "127.0.0.1";
/// Local storage port from the manifest.
const LOCAL_PORT: u16 = 9001;
/// Local storage address from the manifest.
const LOCAL_ADDRESS: &str = "127.0.0.1:9001";
/// A different concrete local IP used to check address validation.
const OTHER_HOST: &str = "127.0.0.2";
/// Dedicated-server sender identifier from the manifest.
const DEDICATED_SENDER_ID: &str = "33333333-3333-4333-8333-333333333333";
/// Storage sender identifier from the manifest.
const LOCAL_SENDER_ID: &str = "11111111-1111-4111-8111-111111111111";
/// Fixed X25519 private key used by the test node.
const ENCRYPTION_KEY: [u8; KEY_LEN] = [9; KEY_LEN];
/// Fixed Ed25519 private key used by the test node.
const SIGNING_KEY: [u8; KEY_LEN] = [7; KEY_LEN];
/// X25519 private key that does not match the manifest.
const WRONG_ENCRYPTION_KEY: [u8; KEY_LEN] = [10; KEY_LEN];
/// Ed25519 private key that does not match the manifest.
const WRONG_SIGNING_KEY: [u8; KEY_LEN] = [8; KEY_LEN];
/// Dedicated-server X25519 seed from the manifest.
const DEDICATED_X25519_KEY: [u8; KEY_LEN] = [0xC1; KEY_LEN];
/// Dedicated-server Ed25519 seed from the manifest.
const DEDICATED_ED25519_KEY: [u8; KEY_LEN] = [0xC2; KEY_LEN];
/// Maximum frame length passed to the runtime.
const MAX_FRAME_LEN: u32 = 1_048_576;
/// Peer storage connect timeout passed to the runtime.
const STORAGE_CONNECT_TIMEOUT: Duration = Duration::from_millis(5_000);
/// Budget for electing the single-node leader.
const LEADER_BUDGET: Duration = Duration::from_secs(10);
/// Minimum client login length in bytes.
const LOGIN_MIN_LEN: usize = 1;
/// Maximum client login length in bytes.
const LOGIN_MAX_LEN: usize = 64;
/// Minimum client password length in bytes.
const PASSWORD_MIN_LEN: usize = 1;
/// Maximum client password length in bytes.
const PASSWORD_MAX_LEN: usize = 256;
/// Maximum responder-label length in bytes.
const RESPONDER_LABEL_MAX_LEN: usize = 64;
/// Fixed upload identifier used by the runtime publish tests.
const PUBLISH_UPLOAD_ID: [u8; UPLOAD_ID_LEN] = [0x5A; UPLOAD_ID_LEN];
/// Upload identifier that has no committed version in the runtime tests.
const UNKNOWN_UPLOAD_ID: [u8; UPLOAD_ID_LEN] = [0x6B; UPLOAD_ID_LEN];
/// Client login stored with the fixed publish record.
const PUBLISH_LOGIN: &str = "alice";
/// Original file name stored with the fixed publish record.
const PUBLISH_ORIGINAL_NAME: &str = "photo.jpg";
/// A different original file name for the conflicting publish record.
const CONFLICTING_ORIGINAL_NAME: &str = "other.jpg";
/// Fixed payload whose digest is stored as the publish content hash.
const PUBLISH_PAYLOAD: &[u8] = b"runtime publish version";
/// Coordinator id that does not belong to the manifest voter set.
const UNKNOWN_COORDINATOR: u64 = 999;

/// Temporary manifest and key directory shared by one test.
struct Fixture {
    directory: TempDir,
    manifest: StaticManifest,
}

impl Fixture {
    /// Builds a manifest with exactly one storage entry at `host:port`.
    fn new(host: &str, port: u16) -> Self {
        let directory = tempfile::tempdir().expect("a temporary test directory is created");
        fs::create_dir(directory.path().join("keys")).expect("the key directory is created");
        let manifest = StaticManifest::parse(&manifest_text(host, port))
            .expect("the single-storage manifest is valid");
        Self {
            directory,
            manifest,
        }
    }

    /// Writes one owner-only key file and loads it through the public key API.
    fn key(&self, name: &str, bytes: &[u8; KEY_LEN]) -> PrivateKey {
        let path = self.directory.path().join("keys").join(name);
        write_owner_only_key(&path, bytes);
        PrivateKey::load(&path).expect("the owner-only key file loads")
    }

    /// Returns a fresh Raft directory below the fixture root.
    fn raft_dir(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }
}

/// Starts the runtime with the fixed test limits.
async fn start_runtime(
    manifest: &StaticManifest,
    label: &str,
    address: &str,
    encryption_key: &PrivateKey,
    signing_key: &PrivateKey,
    raft_dir: PathBuf,
) -> Result<RaftRuntime, RuntimeError> {
    start_runtime_with_limits(
        manifest,
        label,
        address,
        encryption_key,
        signing_key,
        &client_limits(),
        raft_dir,
    )
    .await
}

/// Starts the runtime with explicit client limits.
async fn start_runtime_with_limits(
    manifest: &StaticManifest,
    label: &str,
    address: &str,
    encryption_key: &PrivateKey,
    signing_key: &PrivateKey,
    limits: &ClientLimits,
    raft_dir: PathBuf,
) -> Result<RaftRuntime, RuntimeError> {
    let max_frame = NonZeroU32::new(MAX_FRAME_LEN).expect("the test frame limit is non-zero");
    RaftRuntime::start(
        manifest,
        label,
        address,
        encryption_key,
        signing_key,
        max_frame,
        limits,
        STORAGE_CONNECT_TIMEOUT,
        raft_dir,
    )
    .await
}

/// Builds the fixed structural upload record used by the runtime publish tests.
fn publish_record() -> UploadRecord {
    UploadRecord::new(
        UploadId::from_bytes(PUBLISH_UPLOAD_ID),
        PUBLISH_LOGIN,
        PUBLISH_ORIGINAL_NAME,
        PUBLISH_PAYLOAD.len() as u64,
        PUBLISH_PAYLOAD.len() as u64 / 2,
        ContentHash::digest(PUBLISH_PAYLOAD),
        1_690_000_000_000,
        1_700_000_000_000,
    )
    .expect("the fixed publish record is valid")
}

/// Builds the fixed structural upload record with a different original file name.
fn conflicting_publish_record() -> UploadRecord {
    UploadRecord::new(
        UploadId::from_bytes(PUBLISH_UPLOAD_ID),
        PUBLISH_LOGIN,
        CONFLICTING_ORIGINAL_NAME,
        PUBLISH_PAYLOAD.len() as u64,
        PUBLISH_PAYLOAD.len() as u64 / 2,
        ContentHash::digest(PUBLISH_PAYLOAD),
        1_690_000_000_000,
        1_700_000_000_000,
    )
    .expect("the conflicting publish record is valid")
}

/// Builds a replica status for the fixed publish login.
fn publish_replica_status(
    replica_id: u64,
    upload_id: UploadId,
    sha256: ContentHash,
    available: bool,
) -> ReplicaStatus {
    ReplicaStatus {
        login: PUBLISH_LOGIN.to_owned(),
        upload_id,
        replica_id,
        sha256,
        available,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_initializes_and_commits_on_a_single_node_runtime() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");

    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    let leader = runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let upload_id = UploadId::from_bytes([0x33; UPLOAD_ID_LEN]);
    let response = runtime
        .node()
        .client_write(RaftRequest::UpdateReplicas {
            upload_id,
            replicas: BTreeSet::new(),
        })
        .await
        .expect("the local OpenRaft instance commits the client write");
    assert_eq!(response, RaftResponse { upload_id: None });

    let status = runtime.node().status();
    assert_eq!(
        status.current_leader,
        Some(leader),
        "the node reports the elected single-node leader"
    );
    assert!(
        status.current_term > 0,
        "the node reports a non-zero Raft term after the election"
    );
    assert!(
        status.last_applied.is_some(),
        "the node reports an applied log entry after the committed client write"
    );
    assert_eq!(
        runtime.voter_count(),
        1,
        "the runtime derives one voter from the single-entry manifest"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publishes_a_validated_version_and_replays_idempotently() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-publish"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    let leader = runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let record = publish_record();
    let committed = runtime
        .publish_version(PublishVersion {
            record: record.clone(),
            coordinator: leader,
            replicas: vec![leader],
        })
        .await
        .expect("the runtime validates and commits the publish version");

    assert_eq!(committed.record, record);
    assert_eq!(committed.replicas, BTreeSet::from([leader]));
    assert_eq!(
        runtime
            .node()
            .committed_versions()
            .await
            .get(&record.upload_id),
        Some(&committed),
        "the committed catalog contains the published version"
    );

    let applied_after_publish = runtime
        .node()
        .last_applied_index()
        .expect("the publish version is applied to the catalog");
    let replay = runtime
        .publish_version(PublishVersion {
            record: record.clone(),
            coordinator: leader,
            replicas: vec![leader],
        })
        .await
        .expect("an identical replay returns the committed version");
    assert_eq!(replay, committed);
    assert_eq!(
        runtime.node().last_applied_index(),
        Some(applied_after_publish),
        "an identical replay does not append a second log entry"
    );
    assert_eq!(
        runtime.node().committed_version_count().await,
        1,
        "the catalog still contains exactly one version"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_an_unknown_publish_coordinator_before_raft() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-publish-rejected"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let result = runtime
        .publish_version(PublishVersion {
            record: publish_record(),
            coordinator: UNKNOWN_COORDINATOR,
            replicas: vec![UNKNOWN_COORDINATOR],
        })
        .await;

    assert!(matches!(
        result,
        Err(RuntimeError::Replication(
            ReplicationError::CoordinatorUnknown
        ))
    ));
    assert!(
        runtime.node().committed_versions().await.is_empty(),
        "a publish version rejected before Raft commits nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_an_invalid_publish_entry_at_apply_time() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-apply-rejected"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let response = runtime
        .node()
        .client_write(RaftRequest::PublishVersion(PublishVersion {
            record: publish_record(),
            coordinator: UNKNOWN_COORDINATOR,
            replicas: vec![UNKNOWN_COORDINATOR],
        }))
        .await
        .expect("the invalid publish entry also reaches the state machine as a log entry");

    assert_eq!(
        response,
        RaftResponse { upload_id: None },
        "the state machine rejects the invalid publish entry instead of committing it"
    );
    assert!(
        runtime.node().committed_versions().await.is_empty(),
        "a publish entry rejected at apply time does not mutate the catalog"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_conflicting_metadata_at_apply_time() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-apply-conflict"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    let leader = runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let record = publish_record();
    let committed = runtime
        .publish_version(PublishVersion {
            record: record.clone(),
            coordinator: leader,
            replicas: vec![leader],
        })
        .await
        .expect("the runtime validates and commits the publish version");

    let response = runtime
        .node()
        .client_write(RaftRequest::PublishVersion(PublishVersion {
            record: conflicting_publish_record(),
            coordinator: leader,
            replicas: vec![leader],
        }))
        .await
        .expect("the conflicting publish entry also reaches the state machine as a log entry");

    assert_eq!(
        response,
        RaftResponse { upload_id: None },
        "the state machine rejects metadata that differs from the committed version"
    );
    let versions = runtime.node().committed_versions().await;
    assert_eq!(
        versions.len(),
        1,
        "a conflicting publish entry does not add or replace a catalog version"
    );
    assert_eq!(
        versions.get(&record.upload_id),
        Some(&committed),
        "the catalog still holds the originally committed version"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn records_a_valid_replica_status_and_replays_idempotently() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-replica-status"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    let leader = runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let record = publish_record();
    runtime
        .publish_version(PublishVersion {
            record: record.clone(),
            coordinator: leader,
            replicas: vec![leader],
        })
        .await
        .expect("the runtime validates and commits the publish version");
    let catalog_before = runtime.node().committed_versions().await;

    let status = publish_replica_status(leader, record.upload_id, record.content_hash, true);
    runtime
        .replica_status(status.clone())
        .await
        .expect("a fixed voter reports a matching committed upload");
    runtime
        .replica_status(status)
        .await
        .expect("an identical replica status replay is accepted");

    assert_eq!(
        runtime.node().committed_versions().await,
        catalog_before,
        "replica status entries leave the committed catalog unchanged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_an_unknown_replica_status_before_raft() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-replica-status-rejected"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    let leader = runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let status = publish_replica_status(
        leader,
        UploadId::from_bytes(UNKNOWN_UPLOAD_ID),
        ContentHash::digest(PUBLISH_PAYLOAD),
        true,
    );
    let result = runtime.replica_status(status).await;

    assert!(matches!(
        result,
        Err(RuntimeError::Replication(ReplicationError::UnknownUpload))
    ));
    assert!(
        runtime.node().committed_versions().await.is_empty(),
        "a replica status rejected before Raft commits nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_invalid_replica_status_at_apply_time() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-replica-status-apply-rejected"),
    )
    .await
    .expect("the runtime starts with matching keys, manifest entry, and address");
    runtime
        .initialize()
        .await
        .expect("the explicit bootstrap initializes the single-node cluster");
    let leader = runtime
        .node()
        .wait_for_leader(LEADER_BUDGET)
        .await
        .expect("the single manifest voter becomes leader within the budget");

    let record = publish_record();
    let committed = runtime
        .publish_version(PublishVersion {
            record: record.clone(),
            coordinator: leader,
            replicas: vec![leader],
        })
        .await
        .expect("the runtime validates and commits the publish version");

    let invalid = publish_replica_status(
        leader,
        record.upload_id,
        ContentHash::digest(b"other runtime payload"),
        true,
    );
    let response = runtime
        .node()
        .client_write(RaftRequest::ReplicaStatus(invalid))
        .await
        .expect("the invalid replica status also reaches the state machine as a log entry");

    assert_eq!(
        response,
        RaftResponse { upload_id: None },
        "the state machine rejects a status whose SHA-256 differs from the committed upload"
    );
    let versions = runtime.node().committed_versions().await;
    assert_eq!(
        versions.len(),
        1,
        "an invalid replica status does not add or replace a catalog version"
    );
    assert_eq!(
        versions.get(&record.upload_id),
        Some(&committed),
        "the catalog still holds the published version"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_a_listen_address_mismatch() {
    let port_fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT + 1);
    let port_encryption = port_fixture.key("encryption.key", &ENCRYPTION_KEY);
    let port_signing = port_fixture.key("signing.key", &SIGNING_KEY);
    let port_mismatch = start_runtime(
        &port_fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &port_encryption,
        &port_signing,
        port_fixture.raft_dir("raft-port"),
    )
    .await;
    assert!(
        matches!(port_mismatch, Err(RuntimeError::AddressMismatch)),
        "a manifest port that differs from the listen address is rejected"
    );

    let host_fixture = Fixture::new(OTHER_HOST, LOCAL_PORT);
    let host_encryption = host_fixture.key("encryption.key", &ENCRYPTION_KEY);
    let host_signing = host_fixture.key("signing.key", &SIGNING_KEY);
    let host_mismatch = start_runtime(
        &host_fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &host_encryption,
        &host_signing,
        host_fixture.raft_dir("raft-host"),
    )
    .await;
    assert!(
        matches!(host_mismatch, Err(RuntimeError::AddressMismatch)),
        "a manifest IP that differs from the listen address is rejected"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_private_keys_that_do_not_match_the_manifest() {
    let signing_fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = signing_fixture.key("encryption.key", &ENCRYPTION_KEY);
    let wrong_signing_key = signing_fixture.key("signing.key", &WRONG_SIGNING_KEY);
    let signing_mismatch = start_runtime(
        &signing_fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &wrong_signing_key,
        signing_fixture.raft_dir("raft-signing"),
    )
    .await;
    assert!(
        matches!(signing_mismatch, Err(RuntimeError::Ed25519Mismatch)),
        "an Ed25519 private key that differs from the manifest is rejected"
    );

    let encryption_fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let wrong_encryption_key = encryption_fixture.key("encryption.key", &WRONG_ENCRYPTION_KEY);
    let signing_key = encryption_fixture.key("signing.key", &SIGNING_KEY);
    let encryption_mismatch = start_runtime(
        &encryption_fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &wrong_encryption_key,
        &signing_key,
        encryption_fixture.raft_dir("raft-encryption"),
    )
    .await;
    assert!(
        matches!(encryption_mismatch, Err(RuntimeError::X25519Mismatch)),
        "an X25519 private key that differs from the manifest is rejected"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_entry_comes_from_the_storage_label() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);

    let runtime = start_runtime(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-local"),
    )
    .await
    .expect("the runtime resolves the local storage label");
    assert_eq!(runtime.local().label, LOCAL_LABEL);
    drop(runtime);

    let dedicated_label = start_runtime(
        &fixture.manifest,
        DEDICATED_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        fixture.raft_dir("raft-dedicated"),
    )
    .await;
    assert!(
        matches!(
            dedicated_label,
            Err(RuntimeError::Manifest(ManifestError::LocalNotFound))
        ),
        "a dedicated-server label is not a local storage entry"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_an_invalid_local_responder_label() {
    let fixture = Fixture::new(LOCAL_HOST, LOCAL_PORT);
    let encryption_key = fixture.key("encryption.key", &ENCRYPTION_KEY);
    let signing_key = fixture.key("signing.key", &SIGNING_KEY);
    let limits = client_limits_with_responder_label_max_len(1);

    let result = start_runtime_with_limits(
        &fixture.manifest,
        LOCAL_LABEL,
        LOCAL_ADDRESS,
        &encryption_key,
        &signing_key,
        &limits,
        fixture.raft_dir("raft-label"),
    )
    .await;

    assert!(
        matches!(result, Err(RuntimeError::InvalidLabel)),
        "a local label longer than the responder-label limit is rejected before a listener is bound"
    );
}

/// Renders a manifest with one dedicated entry and one storage entry.
fn manifest_text(host: &str, port: u16) -> String {
    format!(
        "[dedicated]\n\
         sender_id = \"{DEDICATED_SENDER_ID}\"\n\
         label = \"{DEDICATED_LABEL}\"\n\
         host = \"127.0.0.3\"\n\
         port = 7000\n\
         x25519_public_key = \"{}\"\n\
         ed25519_public_key = \"{}\"\n\
         \n\
         [[storage]]\n\
         sender_id = \"{LOCAL_SENDER_ID}\"\n\
         label = \"{LOCAL_LABEL}\"\n\
         host = \"{host}\"\n\
         port = {port}\n\
         x25519_public_key = \"{}\"\n\
         ed25519_public_key = \"{}\"\n",
        recipient_public_key_hex(&DEDICATED_X25519_KEY),
        verifying_key_hex(&DEDICATED_ED25519_KEY),
        recipient_public_key_hex(&ENCRYPTION_KEY),
        verifying_key_hex(&SIGNING_KEY),
    )
}

/// Builds the fixed test client limits.
fn client_limits() -> ClientLimits {
    client_limits_with_responder_label_max_len(RESPONDER_LABEL_MAX_LEN)
}

/// Builds test client limits with an explicit responder-label byte limit.
fn client_limits_with_responder_label_max_len(max_len: usize) -> ClientLimits {
    ClientLimits::new(
        NonZeroUsize::new(LOGIN_MIN_LEN).expect("the test login minimum is non-zero"),
        NonZeroUsize::new(LOGIN_MAX_LEN).expect("the test login maximum is non-zero"),
        NonZeroUsize::new(PASSWORD_MIN_LEN).expect("the test password minimum is non-zero"),
        NonZeroUsize::new(PASSWORD_MAX_LEN).expect("the test password maximum is non-zero"),
        NonZeroUsize::new(max_len).expect("the test responder-label limit is non-zero"),
    )
    .expect("the test client limits are ordered")
}

/// Returns the X25519 public key derived from `private_key` as lower-case hex.
fn recipient_public_key_hex(private_key: &[u8; KEY_LEN]) -> String {
    let key = RecipientPrivateKey::from_bytes(private_key)
        .expect("the fixed X25519 seed builds a valid private key");
    hex(&key.public_key().as_bytes())
}

/// Returns the Ed25519 public key derived from `private_key` as lower-case hex.
fn verifying_key_hex(private_key: &[u8; KEY_LEN]) -> String {
    let key = MessageSigningKey::from_bytes(*private_key);
    hex(&key.verifying_key().as_bytes())
}

/// Encodes bytes as lower-case hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Writes one key file readable only by its owner.
fn write_owner_only_key(path: &Path, bytes: &[u8; KEY_LEN]) {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    let mut file = options.open(path).expect("the key file is created");
    file.write_all(bytes).expect("the key bytes are written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("the key file is owner-only");
    }
}
