//! Public API integration tests for the local OpenRaft runtime.
//!
//! Three in-process nodes exchange CBOR-encoded RPCs through a test-local
//! [`ChannelTransport`], so leader election, a quorum commit of version
//! metadata, the RPC handling path, and catalog distribution are exercised
//! through the public [`RaftNode`] surface.
//!
//! Restart recovery is covered separately with a single-voter cluster: the
//! persisted catalog is restored from the node's data directory and a repeated
//! bootstrap command is rejected.
//!
//! SRS: `FR-CL-009 / 1`, `FR-CL-010 / 1`, `FR-CL-016 / 1`, `FR-CL-020 / 1`,
//! `FR-CL-028 / 1`, `FR-CL-029 / 1`, `FR-CL-031 / 1`, `NFR-MAINT-006 / 1`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use storage_node::ContentHash;
use storage_node::RaftError;
use storage_node::RaftNode;
use storage_node::RaftRequest;
use storage_node::RaftTiming;
use storage_node::RaftTransport;
use storage_node::RaftTransportError;
use storage_node::UploadId;
use storage_node::UPLOAD_ID_LEN;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

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
    senders: Arc<BTreeMap<u64, mpsc::Sender<Call>>>,
}

impl RaftTransport for ChannelTransport {
    async fn send(&self, target: u64, request: Vec<u8>) -> Result<Vec<u8>, RaftTransportError> {
        let sender = self
            .senders
            .get(&target)
            .ok_or(RaftTransportError::Unreachable { target })?;
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

/// Starts three nodes and one serving task per node.
///
/// The returned temporary directories keep each node's persisted Raft state
/// alive until the test ends.
async fn start_cluster() -> (BTreeMap<u64, Arc<RaftNode>>, Vec<tempfile::TempDir>) {
    let mut senders = BTreeMap::new();
    let mut receivers = BTreeMap::new();
    for node_id in CLUSTER_NODES {
        let (sender, receiver) = mpsc::channel(RPC_QUEUE_CAPACITY);
        senders.insert(node_id, sender);
        receivers.insert(node_id, receiver);
    }
    let transport = ChannelTransport {
        senders: Arc::new(senders),
    };

    let mut nodes = BTreeMap::new();
    let mut dirs = Vec::new();
    for (node_id, mut receiver) in receivers {
        let dir = tempfile::tempdir().expect("a temporary Raft data directory is created");
        let node = Arc::new(
            RaftNode::start(
                node_id,
                transport.clone(),
                RaftTiming::default(),
                dir.path().to_path_buf(),
            )
            .await
            .expect("a local Raft node starts with a file-backed store"),
        );
        dirs.push(dir);
        nodes.insert(node_id, Arc::clone(&node));
        tokio::spawn(async move {
            while let Some(call) = receiver.recv().await {
                let Call { request, reply } = call;
                let response = node.handle_rpc(&request).await.unwrap_or_default();
                // The requesting Raft instance may have stopped waiting.
                let _ = reply.send(response);
            }
        });
    }
    (nodes, dirs)
}

/// Waits until every node's applied catalog contains the committed version.
async fn wait_for_committed_version(
    nodes: &BTreeMap<u64, Arc<RaftNode>>,
    upload_id: UploadId,
    content_hash: ContentHash,
) {
    let deadline = tokio::time::Instant::now() + REPLICATION_BUDGET;
    loop {
        let mut replicated = true;
        for node in nodes.values() {
            let versions = node.committed_versions().await;
            if versions.get(&upload_id) != Some(&content_hash) {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn elects_a_leader_and_replicates_a_committed_version() {
    let (nodes, _dirs) = start_cluster().await;
    let bootstrap = nodes
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
    let follower = nodes
        .get(&follower_id)
        .expect("the follower belongs to the cluster");
    follower
        .wait_for_leader(ELECTION_BUDGET)
        .await
        .expect("the follower learns the elected leader within the election budget");
    let rejected = follower
        .client_write(RaftRequest::CommitVersion {
            upload_id: UploadId::from_bytes([0x22; UPLOAD_ID_LEN]),
            content_hash: ContentHash::digest(b"rejected payload"),
        })
        .await
        .expect_err("a follower rejects a client write");
    let RaftError::ClientWrite(openraft::error::ClientWriteError::ForwardToLeader(forward)) =
        rejected
    else {
        panic!("the follower must signal the current leader");
    };
    assert_eq!(forward.leader_id, Some(leader));

    let upload_id = UploadId::from_bytes([0x11; UPLOAD_ID_LEN]);
    let content_hash = ContentHash::digest(b"payload");
    let leader_node = nodes
        .get(&leader)
        .expect("the elected leader belongs to the cluster");
    let response = leader_node
        .client_write(RaftRequest::CommitVersion {
            upload_id,
            content_hash,
        })
        .await
        .expect("the leader commits the version through a quorum");
    assert_eq!(response.upload_id, Some(upload_id));

    wait_for_committed_version(&nodes, upload_id, content_hash).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_a_second_cluster_initialization() {
    let (nodes, _dirs) = start_cluster().await;
    let bootstrap = nodes
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restores_state_and_rejects_repeat_bootstrap_after_restart() {
    let dir = tempfile::tempdir().expect("a temporary Raft data directory is created");
    let state_dir = dir.path().to_path_buf();
    let upload_id = UploadId::from_bytes([0x33; UPLOAD_ID_LEN]);
    let content_hash = ContentHash::digest(b"persisted");

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
        .client_write(RaftRequest::CommitVersion {
            upload_id,
            content_hash,
        })
        .await
        .expect("the leader commits the version");
    assert_eq!(response.upload_id, Some(upload_id));
    node.shutdown().await.expect("the node shuts down");

    let restarted = RaftNode::start(1, NoopTransport, RaftTiming::default(), state_dir)
        .await
        .expect("the restarted node reopens the persisted state");
    let versions = restarted.committed_versions().await;
    assert_eq!(
        versions.get(&upload_id),
        Some(&content_hash),
        "the restarted node restores the committed catalog"
    );
    assert!(
        restarted.last_applied_index().is_some(),
        "the restarted node restores the last applied log index"
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
