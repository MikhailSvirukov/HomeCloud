//! Local OpenRaft consensus runtime, catalog state machine, and CBOR RPC adapter.
//!
//! SRS: `FR-CL-009 / 1`, `FR-CL-010 / 1`, `FR-CL-016 / 1`, `FR-CL-031 / 1`,
//! `NFR-MAINT-001 / 1`–`NFR-MAINT-006 / 1`.

mod network;
mod store;

use core::fmt;
use core::num::NonZeroU64;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use openraft::BasicNode;
use openraft::Config;
use openraft::Raft;

use crate::metadata::ContentHash;
use crate::metadata::UploadId;

/// Node identifier used by the storage-cluster Raft instance.
pub type NodeId = u64;

openraft::declare_raft_types!(
    /// Storage-cluster Raft type configuration.
    pub RaftConfig:
        D = RaftRequest,
        R = RaftResponse,
);

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum RaftRequest {
    /// Records a committed upload version in the cluster catalog.
    CommitVersion {
        upload_id: UploadId,
        content_hash: ContentHash,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RaftResponse {
    /// The version recorded by this entry, or `None` for non-application entries.
    pub upload_id: Option<UploadId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaftTiming {
    /// Heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: NonZeroU64,
    /// Lower bound of the randomized election timeout in milliseconds.
    pub election_timeout_min_ms: NonZeroU64,
    /// Upper bound of the randomized election timeout in milliseconds.
    pub election_timeout_max_ms: NonZeroU64,
}

const DEFAULT_HEARTBEAT_INTERVAL_MS: NonZeroU64 = non_zero_millis(500);
const DEFAULT_ELECTION_TIMEOUT_MIN_MS: NonZeroU64 = non_zero_millis(1_500);
const DEFAULT_ELECTION_TIMEOUT_MAX_MS: NonZeroU64 = non_zero_millis(3_000);
const LEADER_POLL_INTERVAL: Duration = Duration::from_millis(50);

const fn non_zero_millis(value: u64) -> NonZeroU64 {
    match NonZeroU64::new(value) {
        Some(value) => value,
        None => NonZeroU64::MIN,
    }
}

impl Default for RaftTiming {
    fn default() -> Self {
        Self {
            heartbeat_interval_ms: DEFAULT_HEARTBEAT_INTERVAL_MS,
            election_timeout_min_ms: DEFAULT_ELECTION_TIMEOUT_MIN_MS,
            election_timeout_max_ms: DEFAULT_ELECTION_TIMEOUT_MAX_MS,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RaftError {
    #[error("the Raft node could not be started")]
    Start(#[from] openraft::error::Fatal<NodeId>),
    #[error("the cluster initialization was rejected")]
    Initialize(#[from] openraft::error::InitializeError<NodeId, BasicNode>),
    #[error("the client write was rejected")]
    ClientWrite(#[from] openraft::error::ClientWriteError<NodeId, BasicNode>),
    #[error("no leader was elected before the timeout")]
    LeaderTimeout,
    #[error("the timing values are invalid")]
    InvalidTiming,
    #[error("the RPC bytes are not valid CBOR for a Raft message")]
    Decode,
    #[error("the RPC reply could not be encoded")]
    Encode,
    #[error("the Raft node could not be shut down")]
    Shutdown,
}

/// Transport that carries encoded Raft RPCs between storage nodes.
pub trait RaftTransport: Clone + Send + Sync + 'static {
    /// Sends one encoded RPC to `target` and returns the encoded reply.
    fn send(
        &self,
        target: NodeId,
        request: Vec<u8>,
    ) -> impl core::future::Future<Output = Result<Vec<u8>, RaftTransportError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum RaftTransportError {
    /// The peer has no reachable connection.
    #[error("peer {target} is unreachable")]
    Unreachable { target: NodeId },
    /// The RPC was sent but the peer did not answer.
    #[error("the RPC to peer {target} failed")]
    Send { target: NodeId },
}

pub struct RaftNode {
    raft: Arc<Raft<RaftConfig>>,
    store: store::MemStore,
}

#[expect(
    clippy::result_large_err,
    reason = "the public error carries openraft's fatal and client-write errors unchanged"
)]
impl RaftNode {
    /// Starts a local OpenRaft instance with a fixed-membership configuration.
    pub async fn start<T: RaftTransport>(
        node_id: NodeId,
        transport: T,
        timing: RaftTiming,
    ) -> Result<Self, RaftError> {
        if timing.election_timeout_min_ms > timing.election_timeout_max_ms {
            return Err(RaftError::InvalidTiming);
        }
        let config = Config {
            heartbeat_interval: timing.heartbeat_interval_ms.get(),
            election_timeout_min: timing.election_timeout_min_ms.get(),
            election_timeout_max: timing.election_timeout_max_ms.get(),
            ..Default::default()
        };
        let config = config.validate().map_err(|_| RaftError::InvalidTiming)?;

        let store = store::MemStore::default();
        let (log_store, state_machine) = openraft::storage::Adaptor::new(store.clone());
        let raft = Raft::new(
            node_id,
            Arc::new(config),
            network::Network::new(transport),
            log_store,
            state_machine,
        )
        .await?;

        Ok(Self {
            raft: Arc::new(raft),
            store,
        })
    }

    /// Bootstraps the cluster with the given node addresses (explicit command, never automatic).
    pub async fn initialize(&self, members: BTreeMap<NodeId, String>) -> Result<(), RaftError> {
        let members = members
            .into_iter()
            .map(|(node_id, address)| (node_id, BasicNode::new(address)))
            .collect::<BTreeMap<_, _>>();
        match self.raft.initialize(members).await {
            Ok(()) => Ok(()),
            Err(openraft::error::RaftError::APIError(error)) => Err(error.into()),
            Err(openraft::error::RaftError::Fatal(error)) => Err(error.into()),
        }
    }

    /// Writes one application request toward the current leader; a follower rejects it with `ClientWriteError::ForwardToLeader` and this layer neither redirects nor forwards.
    pub async fn client_write(&self, request: RaftRequest) -> Result<RaftResponse, RaftError> {
        match self.raft.client_write(request).await {
            Ok(response) => Ok(response.data),
            Err(openraft::error::RaftError::APIError(error)) => Err(error.into()),
            Err(openraft::error::RaftError::Fatal(error)) => Err(error.into()),
        }
    }

    /// Waits until a leader is known and returns its node id.
    pub async fn wait_for_leader(&self, timeout: Duration) -> Result<NodeId, RaftError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(leader) = self.raft.metrics().borrow().current_leader {
                return Ok(leader);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(RaftError::LeaderTimeout);
            }
            tokio::time::sleep(LEADER_POLL_INTERVAL).await;
        }
    }

    /// Returns the last applied log index on this node.
    pub fn last_applied_index(&self) -> Option<u64> {
        self.raft
            .metrics()
            .borrow()
            .last_applied
            .map(|log_id| log_id.index)
    }

    /// Returns the applied catalog on this node.
    pub async fn committed_versions(&self) -> BTreeMap<UploadId, ContentHash> {
        self.store.committed_versions().await
    }

    /// Handles one encoded Raft RPC by passing it to the local OpenRaft instance and returns the encoded reply.
    pub async fn handle_rpc(&self, bytes: &[u8]) -> Result<Vec<u8>, RaftError> {
        match network::decode::<network::RaftRpc>(bytes)? {
            network::RaftRpc::AppendEntries(rpc) => {
                network::encode(&self.raft.append_entries(rpc).await)
            }
            network::RaftRpc::Vote(rpc) => network::encode(&self.raft.vote(rpc).await),
            network::RaftRpc::InstallSnapshot(rpc) => {
                network::encode(&self.raft.install_snapshot(rpc).await)
            }
        }
    }

    /// Stops the local OpenRaft instance.
    pub async fn shutdown(self) -> Result<(), RaftError> {
        self.raft.shutdown().await.map_err(|_| RaftError::Shutdown)
    }
}

impl fmt::Debug for RaftNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node_id = self.raft.metrics().borrow().id;
        formatter
            .debug_struct("RaftNode")
            .field("node_id", &node_id)
            .field("last_applied", &self.last_applied_index())
            .finish()
    }
}
