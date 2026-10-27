//! In-memory Raft log, state machine, and snapshot store.

use core::fmt;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::ops::RangeBounds;
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

use crate::metadata::ContentHash;
use crate::metadata::UploadId;

use super::{NodeId, RaftConfig, RaftRequest, RaftResponse};

/// Cloneable handle to the in-memory Raft storage shared by the log and state machine.
#[derive(Clone, Default)]
pub(super) struct MemStore {
    inner: Arc<RwLock<StoreInner>>,
}

#[derive(Default)]
struct StoreInner {
    vote: Option<Vote<NodeId>>,
    committed: Option<LogId<NodeId>>,
    last_purged: Option<LogId<NodeId>>,
    log: BTreeMap<u64, Entry<RaftConfig>>,
    last_applied_log: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    catalog: BTreeMap<UploadId, ContentHash>,
    snapshot: Option<StoredSnapshot>,
    snapshot_idx: u64,
}

struct StoredSnapshot {
    meta: SnapshotMeta<NodeId, BasicNode>,
    data: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StateMachineState {
    last_applied_log: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    catalog: BTreeMap<UploadId, ContentHash>,
}

impl MemStore {
    /// Returns the applied version catalog.
    pub(super) async fn committed_versions(&self) -> BTreeMap<UploadId, ContentHash> {
        self.inner.read().await.catalog.clone()
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
        self.inner.write().await.vote = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.read().await.vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        self.inner.write().await.committed = committed;
        Ok(())
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
        Ok(())
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
        Ok(())
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
        Ok(())
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
                EntryPayload::Normal(RaftRequest::CommitVersion {
                    upload_id,
                    content_hash,
                }) => {
                    inner.catalog.insert(*upload_id, *content_hash);
                    responses.push(RaftResponse {
                        upload_id: Some(*upload_id),
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
        Ok(())
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
    use openraft::CommittedLeaderId;

    use super::*;

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
}
