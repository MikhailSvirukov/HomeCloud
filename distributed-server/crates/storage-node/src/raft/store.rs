//! File-backed Raft log, state machine, and snapshot store.

use core::fmt;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::io::Write;
use std::ops::RangeBounds;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use openraft::storage::LogState;
use openraft::storage::RaftLogReader;
use openraft::storage::RaftSnapshotBuilder;
use openraft::storage::RaftStorage;
use openraft::storage::Snapshot;
use openraft::BasicNode;
use openraft::Entry;
use openraft::EntryPayload;
use openraft::LogId;
use openraft::OptionalSend;
use openraft::RaftTypeConfig;
use openraft::SnapshotMeta;
use openraft::StorageError;
use openraft::StorageIOError;
use openraft::StoredMembership;
use openraft::Vote;
use tokio::sync::RwLock;

use crate::metadata::UploadId;

use super::{CommittedVersion, NodeId, RaftConfig, RaftRequest, RaftResponse};

/// Version of the persisted `state.cbor` layout, written before the CBOR payload.
const STATE_FORMAT_VERSION: u8 = 2;

/// Cloneable handle to the Raft storage shared by the log and state machine.
///
/// Every mutation is written to the file at `path`. A store built with
/// [`Default`] has no backing file and is only used by unit tests.
#[derive(Clone, Default)]
pub(super) struct MemStore {
    inner: Arc<RwLock<StoreInner>>,
    path: PathBuf,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct StoreInner {
    vote: Option<Vote<NodeId>>,
    committed: Option<LogId<NodeId>>,
    last_purged: Option<LogId<NodeId>>,
    log: BTreeMap<u64, Entry<RaftConfig>>,
    last_applied_log: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    catalog: BTreeMap<UploadId, CommittedVersion>,
    snapshot: Option<StoredSnapshot>,
    snapshot_idx: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredSnapshot {
    meta: SnapshotMeta<NodeId, BasicNode>,
    data: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StateMachineState {
    last_applied_log: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    catalog: BTreeMap<UploadId, CommittedVersion>,
}

/// The persisted state file has an unsupported or missing format version.
#[derive(Debug, thiserror::Error)]
#[error("unsupported Raft state format version {found}")]
struct StateFormatError {
    found: u8,
}

#[expect(
    clippy::result_large_err,
    reason = "the store forwards openraft's storage error unchanged"
)]
impl MemStore {
    /// Opens the store at `path`, restoring a previously persisted state when the file exists.
    pub(super) fn open(path: PathBuf) -> Result<Self, StorageError<NodeId>> {
        let inner: StoreInner = match std::fs::read(&path) {
            Ok(bytes) => {
                let Some((version, payload)) = bytes.split_first() else {
                    return Err(
                        StorageIOError::read_state_machine(&StateFormatError { found: 0 }).into(),
                    );
                };
                if *version != STATE_FORMAT_VERSION {
                    return Err(StorageIOError::read_state_machine(&StateFormatError {
                        found: *version,
                    })
                    .into());
                }
                ciborium::de::from_reader(payload)
                    .map_err(|error| StorageIOError::read_state_machine(&error))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreInner::default(),
            Err(error) => return Err(StorageIOError::read_state_machine(&error).into()),
        };
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
            path,
        })
    }

    /// Writes `inner` to the backing file through a synced temporary file and atomic rename.
    fn persist(&self, inner: &StoreInner) -> Result<(), StorageError<NodeId>> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let mut data = vec![STATE_FORMAT_VERSION];
        ciborium::ser::into_writer(inner, &mut data)
            .map_err(|error| StorageIOError::write_state_machine(&error))?;
        let temporary = self.temporary_path();
        std::fs::File::create(&temporary)
            .and_then(|mut file| {
                file.write_all(&data)?;
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .and_then(|()| sync_parent(&self.path))
            .map_err(|error| StorageIOError::write_state_machine(&error).into())
    }

    fn temporary_path(&self) -> PathBuf {
        let mut path = self.path.clone().into_os_string();
        path.push(".tmp");
        PathBuf::from(path)
    }

    /// Returns the applied version catalog.
    pub(super) async fn committed_versions(&self) -> BTreeMap<UploadId, CommittedVersion> {
        self.inner.read().await.catalog.clone()
    }
}

/// Syncs the directory that contains `path` so a rename survives a crash.
fn sync_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::File::open(parent)?.sync_all(),
        _ => Ok(()),
    }
}

impl fmt::Debug for MemStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Ok(inner) = self.inner.try_read() {
            formatter.debug_tuple("MemStore").field(&*inner).finish()
        } else {
            formatter.write_str("MemStore { locked: true }")
        }
    }
}

impl fmt::Debug for StoreInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoreInner")
            .field("log_entries", &self.log.len())
            .field("catalog_versions", &self.catalog.len())
            .field("has_vote", &self.vote.is_some())
            .field("has_snapshot", &self.snapshot.is_some())
            .finish()
    }
}

impl RaftLogReader<RaftConfig> for MemStore {
    async fn try_get_log_entries<RB>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<RaftConfig>>, StorageError<NodeId>>
    where
        RB: RangeBounds<u64> + Clone + fmt::Debug + OptionalSend,
    {
        let inner = self.inner.read().await;
        Ok(inner
            .log
            .range(range)
            .map(|(_index, entry)| entry.clone())
            .collect())
    }
}

impl RaftSnapshotBuilder<RaftConfig> for MemStore {
    async fn build_snapshot(&mut self) -> Result<Snapshot<RaftConfig>, StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        let state = StateMachineState {
            last_applied_log: inner.last_applied_log,
            last_membership: inner.last_membership.clone(),
            catalog: inner.catalog.clone(),
        };
        let mut data = Vec::new();
        ciborium::ser::into_writer(&state, &mut data)
            .map_err(|error| StorageIOError::read_state_machine(&error))?;

        inner.snapshot_idx += 1;
        let snapshot_id = match inner.last_applied_log {
            Some(last_applied) => format!(
                "{}-{}-{}",
                last_applied.leader_id, last_applied.index, inner.snapshot_idx
            ),
            None => format!("--{}", inner.snapshot_idx),
        };
        let meta = SnapshotMeta {
            last_log_id: inner.last_applied_log,
            last_membership: inner.last_membership.clone(),
            snapshot_id,
        };
        inner.snapshot = Some(StoredSnapshot {
            meta: meta.clone(),
            data: data.clone(),
        });
        self.persist(&inner)?;

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

impl RaftStorage<RaftConfig> for MemStore {
    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        inner.vote = Some(*vote);
        self.persist(&inner)
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.read().await.vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        inner.committed = committed;
        self.persist(&inner)
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.read().await.committed)
    }

    async fn get_log_state(&mut self) -> Result<LogState<RaftConfig>, StorageError<NodeId>> {
        let inner = self.inner.read().await;
        let last_log_id = inner
            .log
            .values()
            .next_back()
            .map(|entry| entry.log_id)
            .or(inner.last_purged);
        Ok(LogState {
            last_purged_log_id: inner.last_purged,
            last_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry<RaftConfig>> + OptionalSend,
    {
        let mut inner = self.inner.write().await;
        for entry in entries {
            inner.log.insert(entry.log_id.index, entry);
        }
        self.persist(&inner)
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> Result<(), StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        let indices: Vec<u64> = inner
            .log
            .range(log_id.index..)
            .map(|(index, _entry)| *index)
            .collect();
        for index in indices {
            inner.log.remove(&index);
        }
        self.persist(&inner)
    }

    async fn purge_logs_upto(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        if inner.last_purged.is_none_or(|last| last < log_id) {
            inner.last_purged = Some(log_id);
        }
        let indices: Vec<u64> = inner
            .log
            .range(..=log_id.index)
            .map(|(index, _entry)| *index)
            .collect();
        for index in indices {
            inner.log.remove(&index);
        }
        self.persist(&inner)
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId<NodeId>>, StoredMembership<NodeId, BasicNode>), StorageError<NodeId>>
    {
        let inner = self.inner.read().await;
        Ok((inner.last_applied_log, inner.last_membership.clone()))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<RaftConfig>],
    ) -> Result<Vec<RaftResponse>, StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        let mut responses = Vec::with_capacity(entries.len());
        for entry in entries {
            inner.last_applied_log = Some(entry.log_id);
            match &entry.payload {
                EntryPayload::Normal(RaftRequest::CommitVersion(version)) => {
                    let upload_id = version.record.upload_id;
                    inner.catalog.insert(upload_id, version.clone());
                    responses.push(RaftResponse {
                        upload_id: Some(upload_id),
                    });
                }
                EntryPayload::Blank => responses.push(RaftResponse { upload_id: None }),
                EntryPayload::Membership(membership) => {
                    inner.last_membership =
                        StoredMembership::new(Some(entry.log_id), membership.clone());
                    responses.push(RaftResponse { upload_id: None });
                }
            }
        }
        self.persist(&inner)?;
        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<<RaftConfig as RaftTypeConfig>::SnapshotData>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        snapshot: Box<<RaftConfig as RaftTypeConfig>::SnapshotData>,
    ) -> Result<(), StorageError<NodeId>> {
        let data = snapshot.into_inner();
        let state: StateMachineState = ciborium::de::from_reader(data.as_slice())
            .map_err(|error| StorageIOError::read_snapshot(Some(meta.signature()), &error))?;
        let mut inner = self.inner.write().await;
        inner.last_applied_log = state.last_applied_log;
        inner.last_membership = state.last_membership;
        inner.catalog = state.catalog;
        inner.snapshot = Some(StoredSnapshot {
            meta: meta.clone(),
            data,
        });
        self.persist(&inner)
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<RaftConfig>>, StorageError<NodeId>> {
        let inner = self.inner.read().await;
        Ok(inner.snapshot.as_ref().map(|snapshot| Snapshot {
            meta: snapshot.meta.clone(),
            snapshot: Box::new(Cursor::new(snapshot.data.clone())),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicU64, Ordering};

    use openraft::CommittedLeaderId;

    use super::*;
    use crate::metadata::{ContentHash, UploadRecord, CONTENT_HASH_LEN, UPLOAD_ID_LEN};

    static NEXT_STATE_FILE: AtomicU64 = AtomicU64::new(0);

    fn temp_state_path(label: &str) -> PathBuf {
        let unique = NEXT_STATE_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "storage-node-raft-{}-{label}-{unique}.cbor",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn blank_entry_advances_applied_log_without_a_catalog_version() {
        let mut store = MemStore::default();
        let log_id = LogId::new(CommittedLeaderId::new(1, 1), 4);

        let responses = store
            .apply_to_state_machine(&[Entry {
                log_id,
                payload: EntryPayload::Blank,
            }])
            .await
            .expect("a blank entry always applies");

        assert_eq!(responses, vec![RaftResponse { upload_id: None }]);
        let (last_applied, _membership) = store
            .last_applied_state()
            .await
            .expect("the state machine is readable");
        assert_eq!(last_applied, Some(log_id));
        assert!(store.committed_versions().await.is_empty());
    }

    #[tokio::test]
    async fn an_installed_snapshot_restores_the_catalog() {
        let source_path = temp_state_path("source");
        let target_path = temp_state_path("target");
        let mut source = MemStore::open(source_path.clone()).expect("the source store opens");
        let upload_id = UploadId::from_bytes([7; UPLOAD_ID_LEN]);
        let version = CommittedVersion {
            record: UploadRecord::new(
                upload_id,
                "alice",
                "original.bin",
                4_096,
                1_024,
                ContentHash::from_bytes([9; CONTENT_HASH_LEN]),
                1_700_000_000_000,
                1_700_000_000_500,
            )
            .expect("the test record is valid"),
            replicas: BTreeSet::from([1, 2, 3]),
        };

        source
            .apply_to_state_machine(&[Entry {
                log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
                payload: EntryPayload::Normal(RaftRequest::CommitVersion(version.clone())),
            }])
            .await
            .expect("the committed version applies");
        let snapshot = source
            .build_snapshot()
            .await
            .expect("the applied state builds a snapshot");

        let mut target = MemStore::open(target_path.clone()).expect("the target store opens");
        target
            .install_snapshot(&snapshot.meta, snapshot.snapshot)
            .await
            .expect("the snapshot installs");

        assert_eq!(
            target.committed_versions().await.get(&upload_id),
            Some(&version)
        );

        drop(target);
        let reopened = MemStore::open(target_path.clone()).expect("the installed state reopens");
        assert_eq!(
            reopened.committed_versions().await,
            BTreeMap::from([(upload_id, version)])
        );

        std::fs::remove_file(source_path).expect("the source state file can be removed");
        std::fs::remove_file(target_path).expect("the target state file can be removed");
    }
}
