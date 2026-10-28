//! Public API integration tests for the local OpenRaft runtime.
//!
//! Three in-process nodes exchange CBOR-encoded RPCs through a test-local
//! [`ChannelTransport`] that asserts every outbound request is wrapped in the
//! inter-server `[3, [rpc_bytes]]` envelope, so leader election, a quorum commit
//! of full version metadata with its blob replica set, the RPC handling path,
//! and catalog distribution are exercised through the public [`RaftNode`]
//! surface.
//!
//! The cluster also covers a node returning to the unchanged membership: it is
//! stopped, the remaining quorum commits another version, and the restarted node
//! catches up the committed entries from its persisted state directory.
//!
//! A committed version's blob replica set is replaced through the leader, and
//! the update reaches every node's applied catalog; an update for an unknown
//! upload id leaves the catalog unchanged.
//!
//! Restart recovery is covered separately with a single-voter cluster: the
//! persisted catalog is restored from the node's data directory and a repeated
//! bootstrap command is rejected.
//!
//! SRS: `FR-CL-009 / 1`, `FR-CL-010 / 1`, `FR-CL-012 / 2`, `FR-CL-016 / 1`,
//! `FR-CL-020 / 1`, `FR-CL-028 / 1`, `FR-CL-029 / 1`, `FR-CL-031 / 1`,
//! `FR-NODE-005 / 1`–`FR-NODE-008 / 1`, `INT-NET-074 / 2`,
//! `NFR-MAINT-006 / 1`.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use storage_node::CommittedVersion;
use storage_node::ContentHash;
use storage_node::RaftError;
use storage_node::RaftNode;
use storage_node::RaftRequest;
use storage_node::RaftTiming;
use storage_node::RaftTransport;
use storage_node::RaftTransportError;
use storage_node::UploadId;
use storage_node::UploadRecord;
use storage_node::UPLOAD_ID_LEN;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Node ids of the fixed three-node test cluster.
const CLUSTER_NODES: [u64; 3] = [1, 2, 3];
/// The node that receives the explicit bootstrap command.
const BOOTSTRAP_NODE_ID: u64 = 1;
/// Capacity of the queue that carries RPCs to one node's serving task.
const RPC_QUEUE_CAPACITY: usize = 32;
/// Budget for electing a leader after bootstrap.
const ELECTION_BUDGET: Duration = Duration::from_secs(10);
/// Budget for a committed version to reach every node's catalog.
const REPLICATION_BUDGET: Duration = Duration::from_secs(10);
/// Interval between catalog polls.
const CATALOG_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// One encoded RPC delivered to a node's serving task.
struct Call {
    request: Vec<u8>,
    reply: oneshot::Sender<Vec<u8>>,
}

/// In-process transport that routes encoded RPCs to the target node's queue.
#[derive(Clone)]
struct ChannelTransport {
    senders: Arc<RwLock<BTreeMap<u64, mpsc::Sender<Call>>>>,
}

impl ChannelTransport {
    fn new() -> Self {
        Self {
            senders: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Makes `node_id` reachable through its serving task.
    async fn register(&self, node_id: u64, sender: mpsc::Sender<Call>) {
        let _ = self.senders.write().await.insert(node_id, sender);
    }

    /// Stops routing RPCs to `node_id`.
    async fn remove(&self, node_id: u64) {
        let _ = self.senders.write().await.remove(&node_id);
    }
}

impl RaftTransport for ChannelTransport {
    async fn send(&self, target: u64, request: Vec<u8>) -> Result<Vec<u8>, RaftTransportError> {
        let _ = storage_node::decode_raft_rpc(&request)
            .expect("every Raft RPC is carried in the message_type 3 envelope");
        let sender = {
            let senders = self.senders.read().await;
            senders
                .get(&target)
                .cloned()
                .ok_or(RaftTransportError::Unreachable { target })?
        };
        let (reply, response) = oneshot::channel();
        sender
            .send(Call { request, reply })
            .await
            .map_err(|_| RaftTransportError::Send { target })?;
        response
            .await
            .map_err(|_| RaftTransportError::Send { target })
    }
}

/// Transport for a single-voter cluster, which never sends to a peer.
#[derive(Clone)]
struct NoopTransport;

impl RaftTransport for NoopTransport {
    async fn send(&self, target: u64, _request: Vec<u8>) -> Result<Vec<u8>, RaftTransportError> {
        Err(RaftTransportError::Unreachable { target })
    }
}

/// Builds the fixed membership of the test cluster.
fn cluster_membership() -> BTreeMap<u64, String> {
    CLUSTER_NODES
        .iter()
        .map(|node_id| (*node_id, format!("node-{node_id}")))
        .collect()
}

/// Builds a full committed version for `upload_id` over `payload`.
fn committed_version(upload_id: UploadId, payload: &[u8], replicas: &[u64]) -> CommittedVersion {
    CommittedVersion {
        record: UploadRecord::new(
            upload_id,
            "alice",
            "photo.jpg",
            payload.len() as u64,
            payload.len() as u64 / 2,
            ContentHash::digest(payload),
            1_690_000_000_000,
            1_700_000_000_000,
        )
        .expect("the test version metadata is valid"),
        replicas: replicas.iter().copied().collect(),
    }
}

/// A running cluster and the handles needed to stop and restart one node.
struct Cluster {
    nodes: BTreeMap<u64, Arc<RaftNode>>,
    dirs: BTreeMap<u64, tempfile::TempDir>,
    tasks: BTreeMap<u64, JoinHandle<()>>,
    transport: ChannelTransport,
}

/// Starts one node and the serving task that feeds it encoded RPCs.
async fn start_node(
    node_id: u64,
    transport: &ChannelTransport,
    dir: PathBuf,
) -> (Arc<RaftNode>, JoinHandle<()>) {
    let (sender, mut receiver) = mpsc::channel(RPC_QUEUE_CAPACITY);
    transport.register(node_id, sender).await;
    let node = Arc::new(
        RaftNode::start(node_id, transport.clone(), RaftTiming::default(), dir)
            .await
            .expect("a local Raft node starts with a file-backed store"),
    );
    let serving = Arc::clone(&node);
    let task = tokio::spawn(async move {
        while let Some(call) = receiver.recv().await {
            let Call { request, reply } = call;
            let response = serving.handle_rpc(&request).await.unwrap_or_default();
            // The requesting Raft instance may have stopped waiting.
            let _ = reply.send(response);
        }
    });
    (node, task)
}

/// Starts three nodes and one serving task per node.
///
/// The held temporary directories keep each node's persisted Raft state alive
/// until the cluster is dropped.
async fn start_cluster() -> Cluster {
    let transport = ChannelTransport::new();
    let mut nodes = BTreeMap::new();
    let mut dirs = BTreeMap::new();
    let mut tasks = BTreeMap::new();
    for node_id in CLUSTER_NODES {
        let dir = tempfile::tempdir().expect("a temporary Raft data directory is created");
        let (node, task) = start_node(node_id, &transport, dir.path().to_path_buf()).await;
        dirs.insert(node_id, dir);
        nodes.insert(node_id, node);
        tasks.insert(node_id, task);
    }
    Cluster {
        nodes,
        dirs,
        tasks,
        transport,
    }
}

/// Waits until every node's applied catalog contains the committed version.
async fn wait_for_committed_version(
    nodes: &BTreeMap<u64, Arc<RaftNode>>,
    version: &CommittedVersion,
) {
    let upload_id = version.record.upload_id;
    let deadline = tokio::time::Instant::now() + REPLICATION_BUDGET;
    loop {
        let mut replicated = true;
        for node in nodes.values() {
            let versions = node.committed_versions().await;
            if versions.get(&upload_id) != Some(version) {
                replicated = false;
            }
        }
        if replicated {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the committed version {upload_id} did not reach every node's catalog in time"
        );
        tokio::time::sleep(CATALOG_POLL_INTERVAL).await;
    }
}

/// Waits until every node applies `replicas` to the version for `upload_id`
/// without changing the committed record.
async fn wait_for_replica_set(
    nodes: &BTreeMap<u64, Arc<RaftNode>>,
    version: &CommittedVersion,
    replicas: &BTreeSet<u64>,
) {
    let upload_id = version.record.upload_id;
    let deadline = tokio::time::Instant::now() + REPLICATION_BUDGET;
    loop {
        let mut updated = true;
        for node in nodes.values() {
            let versions = node.committed_versions().await;
            let applied = versions.get(&upload_id).is_some_and(|committed| {
                committed.record == version.record && committed.replicas == *replicas
            });
            if !applied {
                updated = false;
            }
        }
        if updated {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the blob replica set of version {upload_id} did not reach every node's catalog in time"
        );
        tokio::time::sleep(CATALOG_POLL_INTERVAL).await;
    }
}

/// Waits until the node reports an applied log index of at least `minimum`.
async fn wait_for_applied(node: &RaftNode, minimum: u64, budget: Duration) {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        if node
            .last_applied_index()
            .is_some_and(|index| index >= minimum)
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the node did not apply log index {minimum} in time"
        );
        tokio::time::sleep(CATALOG_POLL_INTERVAL).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn elects_a_leader_and_replicates_a_committed_version() {
    let cluster = start_cluster().await;
    let bootstrap = cluster
        .nodes
        .get(&BOOTSTRAP_NODE_ID)
        .expect("the bootstrap node is part of the cluster");

    bootstrap
        .initialize(cluster_membership())
        .await
        .expect("the explicit bootstrap command initializes the cluster");
    let leader = bootstrap
        .wait_for_leader(ELECTION_BUDGET)
        .await
        .expect("a leader is elected within the election budget");
    assert!(
        CLUSTER_NODES.contains(&leader),
        "the elected leader is one of the configured storage nodes"
    );

    let follower_id = CLUSTER_NODES
        .iter()
        .copied()
        .find(|node_id| *node_id != leader)
        .expect("the cluster has a follower besides the leader");
    let follower = cluster
        .nodes
        .get(&follower_id)
        .expect("the follower belongs to the cluster");
    follower
        .wait_for_leader(ELECTION_BUDGET)
        .await
        .expect("the follower learns the elected leader within the election budget");
    let rejected_version = committed_version(
        UploadId::from_bytes([0x22; UPLOAD_ID_LEN]),
        b"rejected payload",
        &[2, 3],
    );
    let rejected = follower
        .client_write(RaftRequest::CommitVersion(rejected_version))
        .await
        .expect_err("a follower rejects a client write");
    let RaftError::ClientWrite(openraft::error::ClientWriteError::ForwardToLeader(forward)) =
        rejected
    else {
        panic!("the follower must signal the current leader");
    };
    assert_eq!(forward.leader_id, Some(leader));

    let version = committed_version(
        UploadId::from_bytes([0x11; UPLOAD_ID_LEN]),
        b"payload",
        &[2, 3],
    );
    let leader_node = cluster
        .nodes
        .get(&leader)
        .expect("the elected leader belongs to the cluster");
    let response = leader_node
        .client_write(RaftRequest::CommitVersion(version.clone()))
        .await
        .expect("the leader commits the version through a quorum");
    assert_eq!(response.upload_id, Some(version.record.upload_id));

    wait_for_committed_version(&cluster.nodes, &version).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn updates_the_blob_replica_set_for_a_committed_version() {
    let cluster = start_cluster().await;
    let leader = {
        let bootstrap = cluster
            .nodes
            .get(&BOOTSTRAP_NODE_ID)
            .expect("the bootstrap node is part of the cluster");
        bootstrap
            .initialize(cluster_membership())
            .await
            .expect("the explicit bootstrap command initializes the cluster");
        bootstrap
            .wait_for_leader(ELECTION_BUDGET)
            .await
            .expect("a leader is elected within the election budget")
    };
    let leader_node = cluster
        .nodes
        .get(&leader)
        .expect("the elected leader belongs to the cluster");

    let upload_id = UploadId::from_bytes([0x66; UPLOAD_ID_LEN]);
    let version = committed_version(upload_id, b"replicated payload", &[1, 2]);
    let response = leader_node
        .client_write(RaftRequest::CommitVersion(version.clone()))
        .await
        .expect("the leader commits the version through a quorum");
    assert_eq!(response.upload_id, Some(upload_id));
    wait_for_committed_version(&cluster.nodes, &version).await;

    let expanded_replicas = BTreeSet::from([1, 2, 3]);
    let response = leader_node
        .client_write(RaftRequest::UpdateReplicas {
            upload_id,
            replicas: expanded_replicas.clone(),
        })
        .await
        .expect("the leader updates the blob replica set through a quorum");
    assert_eq!(
        response.upload_id,
        Some(upload_id),
        "the update response names the committed version it changed"
    );
    wait_for_replica_set(&cluster.nodes, &version, &expanded_replicas).await;

    let unknown_id = UploadId::from_bytes([0x77; UPLOAD_ID_LEN]);
    let response = leader_node
        .client_write(RaftRequest::UpdateReplicas {
            upload_id: unknown_id,
            replicas: BTreeSet::from([1]),
        })
        .await
        .expect("the leader applies an update for an unknown upload id");
    assert_eq!(
        response.upload_id, None,
        "an update for an unknown upload id reports no committed version"
    );
    for node in cluster.nodes.values() {
        let versions = node.committed_versions().await;
        assert!(
            !versions.contains_key(&unknown_id),
            "an update for an unknown upload id must not create a catalog entry"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_a_second_cluster_initialization() {
    let cluster = start_cluster().await;
    let bootstrap = cluster
        .nodes
        .get(&BOOTSTRAP_NODE_ID)
        .expect("the bootstrap node is part of the cluster");

    bootstrap
        .initialize(cluster_membership())
        .await
        .expect("the explicit bootstrap command initializes the cluster");
    bootstrap
        .wait_for_leader(ELECTION_BUDGET)
        .await
        .expect("a leader is elected within the election budget");

    let repeated = bootstrap.initialize(cluster_membership()).await;
    assert!(
        matches!(repeated, Err(RaftError::AlreadyBootstrapped)),
        "an initialized node rejects a second bootstrap command"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_returning_node_catches_up_committed_entries() {
    let mut cluster = start_cluster().await;
    let leader = {
        let bootstrap = cluster
            .nodes
            .get(&BOOTSTRAP_NODE_ID)
            .expect("the bootstrap node is part of the cluster");
        bootstrap
            .initialize(cluster_membership())
            .await
            .expect("the explicit bootstrap command initializes the cluster");
        bootstrap
            .wait_for_leader(ELECTION_BUDGET)
            .await
            .expect("a leader is elected within the election budget")
    };
    let leader_node = Arc::clone(
        cluster
            .nodes
            .get(&leader)
            .expect("the elected leader belongs to the cluster"),
    );

    let before_offline = committed_version(
        UploadId::from_bytes([0x44; UPLOAD_ID_LEN]),
        b"before-offline",
        &[1, 2],
    );
    let response = leader_node
        .client_write(RaftRequest::CommitVersion(before_offline.clone()))
        .await
        .expect("the leader commits a version before the node goes offline");
    assert_eq!(response.upload_id, Some(before_offline.record.upload_id));
    wait_for_committed_version(&cluster.nodes, &before_offline).await;

    let offline_id = CLUSTER_NODES
        .iter()
        .copied()
        .find(|node_id| *node_id != leader)
        .expect("the cluster has a node besides the leader");
    cluster.transport.remove(offline_id).await;
    let task = cluster
        .tasks
        .remove(&offline_id)
        .expect("the stopped node has a serving task");
    task.abort();
    let _ = task.await;
    let offline = cluster
        .nodes
        .remove(&offline_id)
        .expect("the stopped node belongs to the cluster");
    let offline = Arc::try_unwrap(offline)
        .expect("the aborted serving task was the only other owner of the node");
    offline
        .shutdown()
        .await
        .expect("the offline node shuts down");

    let while_offline = committed_version(
        UploadId::from_bytes([0x55; UPLOAD_ID_LEN]),
        b"while-offline",
        &[2, 3],
    );
    let response = leader_node
        .client_write(RaftRequest::CommitVersion(while_offline.clone()))
        .await
        .expect("the leader commits a version while the node is offline");
    assert_eq!(response.upload_id, Some(while_offline.record.upload_id));
    wait_for_committed_version(&cluster.nodes, &while_offline).await;

    let dir = cluster
        .dirs
        .get(&offline_id)
        .expect("the stopped node keeps its data directory")
        .path()
        .to_path_buf();
    let (restarted, task) = start_node(offline_id, &cluster.transport, dir).await;
    cluster.nodes.insert(offline_id, Arc::clone(&restarted));
    cluster.tasks.insert(offline_id, task);

    let deadline = tokio::time::Instant::now() + REPLICATION_BUDGET;
    loop {
        let versions = restarted.committed_versions().await;
        let caught_up = versions.get(&before_offline.record.upload_id) == Some(&before_offline)
            && versions.get(&while_offline.record.upload_id) == Some(&while_offline);
        if caught_up {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the returning node did not apply the committed versions {} and {} in time",
            before_offline.record.upload_id,
            while_offline.record.upload_id
        );
        tokio::time::sleep(CATALOG_POLL_INTERVAL).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restores_state_and_rejects_repeat_bootstrap_after_restart() {
    let dir = tempfile::tempdir().expect("a temporary Raft data directory is created");
    let state_dir = dir.path().to_path_buf();
    let version = committed_version(
        UploadId::from_bytes([0x33; UPLOAD_ID_LEN]),
        b"persisted",
        &[1],
    );

    let node = RaftNode::start(1, NoopTransport, RaftTiming::default(), state_dir.clone())
        .await
        .expect("a single-voter Raft node starts with a file-backed store");
    node.initialize(BTreeMap::from([(1, "node-1".to_owned())]))
        .await
        .expect("the explicit bootstrap command initializes the cluster");
    node.wait_for_leader(ELECTION_BUDGET)
        .await
        .expect("the single voter becomes leader within the election budget");
    let response = node
        .client_write(RaftRequest::CommitVersion(version.clone()))
        .await
        .expect("the leader commits the version");
    assert_eq!(response.upload_id, Some(version.record.upload_id));
    wait_for_applied(&node, 1, ELECTION_BUDGET).await;
    let applied_before = node
        .last_applied_index()
        .expect("the committed write is applied before shutdown");
    node.shutdown().await.expect("the node shuts down");

    let restarted = RaftNode::start(1, NoopTransport, RaftTiming::default(), state_dir)
        .await
        .expect("the restarted node reopens the persisted state");
    wait_for_applied(&restarted, applied_before, ELECTION_BUDGET).await;
    let versions = restarted.committed_versions().await;
    assert_eq!(
        versions.get(&version.record.upload_id),
        Some(&version),
        "the restarted node restores the full committed catalog"
    );

    let repeated = restarted
        .initialize(BTreeMap::from([(1, "node-1".to_owned())]))
        .await;
    assert!(
        matches!(repeated, Err(RaftError::AlreadyBootstrapped)),
        "a node with a persisted bootstrap marker rejects a second bootstrap command"
    );
    restarted
        .shutdown()
        .await
        .expect("the restarted node shuts down");
}
