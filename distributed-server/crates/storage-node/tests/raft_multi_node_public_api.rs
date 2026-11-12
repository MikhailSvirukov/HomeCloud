//! Multi-runtime integration test over real loopback TCP.
//!
//! Three distinct manifest storage identities start their own local OpenRaft
//! instances, bind their advertised loopback addresses, and serve incoming peer
//! RPCs through the encrypted and signed inter-server transport. One runtime is
//! bootstrapped with the manifest voter set, the cluster elects a leader, a
//! client write commits through a majority, and every node applies the entry.
//! The leader is then stopped, the two surviving voters elect a replacement,
//! and a second client write commits and applies on both of them.
//!
//! SRS: `FR-CL-001 / 1` (storage nodes exchange Raft messages directly, with no
//! dedicated-server relay), `FR-CL-006 / 1` (the metadata commit needs a
//! majority of the fixed cluster), `FR-CL-015 / 1` and `FR-CL-017 / 1` (the
//! cluster and voter set come from the static manifest), `FR-CL-027 / 1` (the
//! bootstrap membership is exactly the manifest storage nodes), `FR-CL-030 / 1`
//! (each node process runs a local OpenRaft voting instance), `FR-CL-031 / 1`
//! (accepted and verified peer RPCs reach the local OpenRaft instance), and
//! `INT-NET-074 / 2` (each Raft RPC is carried as a message type 3 inter-server
//! message).

use core::num::{NonZeroU32, NonZeroUsize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use storage_node::{
    ClientLimits, CommittedVersion, ContentHash, Listener, ListenerError, MessageSigningKey,
    PrivateKey, RaftNode, RaftRequest, RaftRuntime, RecipientPrivateKey, StaticManifest, UploadId,
    UploadRecord, KEY_LEN, UPLOAD_ID_LEN,
};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Number of storage nodes in the fixed test cluster.
const NODE_COUNT: usize = 3;
/// Storage labels in manifest order.
const NODE_LABELS: [&str; NODE_COUNT] = ["node-1", "node-2", "node-3"];
/// Sender identifiers whose first eight bytes differ, so each maps to a distinct
/// Raft node id.
const SENDER_IDS: [&str; NODE_COUNT] = [
    "01010101010101010101010101010101",
    "02020202020202020202020202020202",
    "03030303030303030303030303030303",
];
/// Fixed Ed25519 seeds for the three storage nodes.
const SIGNING_SEEDS: [[u8; KEY_LEN]; NODE_COUNT] = [[11; KEY_LEN], [12; KEY_LEN], [13; KEY_LEN]];
/// Fixed X25519 seeds for the three storage nodes.
const ENCRYPTION_SEEDS: [[u8; KEY_LEN]; NODE_COUNT] = [[21; KEY_LEN], [22; KEY_LEN], [23; KEY_LEN]];
/// Dedicated-server sender identifier.
const DEDICATED_SENDER_ID: &str = "33333333333333333333333333333333";
/// Dedicated-server manifest label.
const DEDICATED_LABEL: &str = "dedicated";
/// Loopback host shared by the dedicated and storage manifest entries.
const LOOPBACK_HOST: &str = "127.0.0.1";
/// Subdirectory that holds the per-node private key files.
const KEYS_DIRECTORY: &str = "keys";
/// Dedicated-server Ed25519 seed.
const DEDICATED_SIGNING_SEED: [u8; KEY_LEN] = [0xC2; KEY_LEN];
/// Dedicated-server X25519 seed.
const DEDICATED_ENCRYPTION_SEED: [u8; KEY_LEN] = [0xC1; KEY_LEN];
/// Dedicated-server advertised port.
const DEDICATED_PORT: u16 = 7000;
/// Maximum frame length passed to every runtime.
const MAX_FRAME_LEN: u32 = 1_048_576;
/// Peer storage connect timeout passed to every runtime.
const STORAGE_CONNECT_TIMEOUT: Duration = Duration::from_millis(5_000);
/// Budget for electing a leader and for every node to observe it.
const LEADER_BUDGET: Duration = Duration::from_secs(15);
/// Budget for the committed entry to be applied on every node.
const APPLY_BUDGET: Duration = Duration::from_secs(15);
/// Interval between status polls.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
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

/// Temporary manifest, key files, and Raft directories shared by the cluster.
struct Fixture {
    directory: TempDir,
    manifest: StaticManifest,
    addresses: [SocketAddr; NODE_COUNT],
}

impl Fixture {
    /// Allocates three distinct loopback ports and builds the shared manifest.
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temporary test directory is created");
        fs::create_dir(directory.path().join(KEYS_DIRECTORY))
            .expect("the key directory is created");
        let ports = allocate_loopback_ports();
        let addresses = ports.map(|port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        let manifest = StaticManifest::parse(&manifest_text(&addresses))
            .expect("the cluster manifest is valid");
        Self {
            directory,
            manifest,
            addresses,
        }
    }

    /// Writes one owner-only key file and loads it through the public key API.
    fn key(&self, name: &str, seed: &[u8; KEY_LEN]) -> PrivateKey {
        let path = self.directory.path().join(KEYS_DIRECTORY).join(name);
        write_owner_only_key(&path, seed);
        PrivateKey::load(&path).expect("the owner-only key file loads")
    }

    /// Returns a fresh per-node Raft directory below the fixture root.
    fn raft_dir(&self, label: &str) -> PathBuf {
        self.directory.path().join(format!("raft-{label}"))
    }

    /// Returns the manifest node id for `label`.
    fn node_id(&self, label: &str) -> u64 {
        self.manifest
            .local(label)
            .expect("the fixture label is a storage entry")
            .node_id()
    }
}

/// One running server task and its graceful shutdown handle.
struct ServerTask {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), ListenerError>>>,
}

impl Drop for ServerTask {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Three started runtimes and the server tasks that accept peer RPCs.
struct Cluster {
    nodes: BTreeMap<u64, Arc<RaftNode>>,
    servers: BTreeMap<u64, ServerTask>,
}

impl Cluster {
    /// Starts every runtime, binds its advertised listener, and serves peers.
    ///
    /// The first runtime is bootstrapped explicitly after the other two server
    /// tasks exist, then its own server task starts.
    async fn start(fixture: &Fixture) -> Self {
        let limits = client_limits();
        let max_frame = NonZeroU32::new(MAX_FRAME_LEN).expect("the test frame limit is non-zero");
        let mut runtimes: Vec<Option<RaftRuntime>> = Vec::with_capacity(NODE_COUNT);
        let mut listeners: Vec<Option<Listener>> = Vec::with_capacity(NODE_COUNT);
        for (index, label) in NODE_LABELS.iter().enumerate() {
            let address = fixture.addresses[index];
            let encryption_key =
                fixture.key(&format!("{label}-encryption.key"), &ENCRYPTION_SEEDS[index]);
            let signing_key = fixture.key(&format!("{label}-signing.key"), &SIGNING_SEEDS[index]);
            let runtime = RaftRuntime::start(
                &fixture.manifest,
                label,
                &address.to_string(),
                &encryption_key,
                &signing_key,
                max_frame,
                &limits,
                STORAGE_CONNECT_TIMEOUT,
                fixture.raft_dir(label),
            )
            .await
            .expect("the node runtime starts with matching keys, manifest entry, and address");
            assert_eq!(
                runtime.voter_count(),
                u64::try_from(NODE_COUNT).expect("the node count fits in u64"),
                "the runtime voter set is exactly the manifest storage cluster"
            );
            assert_eq!(
                runtime.local().label,
                *label,
                "the runtime resolves its own manifest entry"
            );
            let listener = Listener::bind(&address.to_string())
                .await
                .expect("the advertised loopback address is bound");
            assert_eq!(
                listener
                    .local_addr()
                    .expect("the bound listener reports its address"),
                address,
                "the server listens on the manifest address used by its peers"
            );
            runtimes.push(Some(runtime));
            listeners.push(Some(listener));
        }

        let mut nodes = BTreeMap::new();
        let mut servers = BTreeMap::new();
        for (index, runtime) in runtimes.iter_mut().enumerate().skip(1) {
            let runtime = runtime.take().expect("the follower runtime is present");
            let listener = listeners[index]
                .take()
                .expect("the follower listener is present");
            let node_id = fixture.node_id(NODE_LABELS[index]);
            nodes.insert(node_id, runtime.node());
            servers.insert(node_id, spawn_server(runtime, listener));
        }

        let bootstrap = runtimes[0]
            .take()
            .expect("the bootstrap runtime is present");
        tokio::time::timeout(LEADER_BUDGET, bootstrap.initialize())
            .await
            .expect("the explicit bootstrap completes within the leader budget")
            .expect("the explicit bootstrap initializes the manifest storage voters");
        let listener = listeners[0]
            .take()
            .expect("the bootstrap listener is present");
        let node_id = fixture.node_id(NODE_LABELS[0]);
        nodes.insert(node_id, bootstrap.node());
        servers.insert(node_id, spawn_server(bootstrap, listener));

        Self { nodes, servers }
    }

    /// Waits for a leader and for every node to report it.
    async fn wait_for_leader(&self) -> u64 {
        let any_node = self
            .nodes
            .values()
            .next()
            .expect("the cluster has at least one node");
        let leader = any_node
            .wait_for_leader(LEADER_BUDGET)
            .await
            .expect("a leader is elected within the leader budget");
        assert!(
            self.nodes.contains_key(&leader),
            "the elected leader is one of the manifest storage nodes"
        );
        let deadline = tokio::time::Instant::now() + LEADER_BUDGET;
        loop {
            let observed = self
                .nodes
                .values()
                .all(|node| node.status().current_leader == Some(leader));
            if observed {
                return leader;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "every node did not observe leader {leader} within the leader budget"
            );
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Waits for a leader different from `previous` and for every surviving node to report it.
    async fn wait_for_replacement_leader(&self, previous: u64) -> u64 {
        let deadline = tokio::time::Instant::now() + LEADER_BUDGET;
        loop {
            let leaders = self
                .nodes
                .values()
                .map(|node| node.status().current_leader)
                .collect::<BTreeSet<_>>();
            if let Some(Some(leader)) = leaders.iter().next().copied() {
                if leaders.len() == 1 && leader != previous {
                    return leader;
                }
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the surviving nodes did not elect a replacement for leader {previous} within the leader budget"
            );
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Stops one node's peer server and its local OpenRaft instance.
    async fn stop_node(&mut self, node_id: u64) {
        let mut server = self
            .servers
            .remove(&node_id)
            .expect("the stopped node serves peer RPCs");
        if let Some(shutdown) = server.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = server.task.take() {
            let outcome = task.await.expect("the stopped server task does not panic");
            assert!(
                outcome.is_ok(),
                "the stopped server task returns Ok when shutdown resolves"
            );
        }
        let node = self
            .nodes
            .remove(&node_id)
            .expect("the stopped node has a local Raft instance");
        let Ok(node) = Arc::try_unwrap(node) else {
            panic!("the stopped node still holds another Arc reference to its local Raft instance");
        };
        node.shutdown()
            .await
            .expect("the stopped node shuts down its local OpenRaft instance");
    }

    /// Sends shutdown to every server task and awaits its clean exit.
    async fn shutdown(mut self) {
        for server in self.servers.values_mut() {
            if let Some(shutdown) = server.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        for server in self.servers.values_mut() {
            if let Some(task) = server.task.take() {
                let outcome = task.await.expect("the server task does not panic");
                assert!(
                    outcome.is_ok(),
                    "the server task returns Ok when shutdown resolves"
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_loopback_nodes_elect_a_replacement_leader_and_commit_through_the_peer_transport() {
    let fixture = Fixture::new();
    let mut cluster = Cluster::start(&fixture).await;
    let leader = cluster.wait_for_leader().await;
    let leader_node = Arc::clone(
        cluster
            .nodes
            .get(&leader)
            .expect("the elected leader belongs to the cluster"),
    );

    let upload_id = UploadId::from_bytes([0x44; UPLOAD_ID_LEN]);
    let version = committed_version(upload_id, leader);
    let response = tokio::time::timeout(
        APPLY_BUDGET,
        leader_node.client_write(RaftRequest::CommitVersion(version.clone())),
    )
    .await
    .expect("the client write completes within the apply budget")
    .expect("the leader commits the client write through a majority");
    assert_eq!(
        response.upload_id,
        Some(upload_id),
        "the committed version is reported by the leader"
    );

    wait_for_committed_version(&cluster.nodes, &version).await;

    for (node_id, node) in &cluster.nodes {
        let status = node.status();
        assert_eq!(
            status.current_leader,
            Some(leader),
            "node {node_id} reports the elected leader"
        );
        assert!(
            status.last_applied.is_some(),
            "node {node_id} has applied the committed entry"
        );
    }

    drop(leader_node);
    let previous_leader = leader;
    cluster.stop_node(previous_leader).await;

    let replacement = cluster.wait_for_replacement_leader(previous_leader).await;
    assert_ne!(
        replacement, previous_leader,
        "the replacement leader differs from the stopped leader"
    );

    let replacement_node = Arc::clone(
        cluster
            .nodes
            .get(&replacement)
            .expect("the replacement leader belongs to the surviving nodes"),
    );
    let second_upload_id = UploadId::from_bytes([0x45; UPLOAD_ID_LEN]);
    let second_version = committed_version(second_upload_id, replacement);
    let response = tokio::time::timeout(
        APPLY_BUDGET,
        replacement_node.client_write(RaftRequest::CommitVersion(second_version.clone())),
    )
    .await
    .expect("the second client write completes within the apply budget")
    .expect("the replacement leader commits the second client write through a majority");
    assert_eq!(
        response.upload_id,
        Some(second_upload_id),
        "the second committed version is reported by the replacement leader"
    );

    let expected_catalog =
        BTreeMap::from([(upload_id, version), (second_upload_id, second_version)]);
    wait_for_catalog(&cluster.nodes, &expected_catalog).await;

    for (node_id, node) in &cluster.nodes {
        let status = node.status();
        assert_eq!(
            status.current_leader,
            Some(replacement),
            "node {node_id} reports the replacement leader"
        );
        assert!(
            status.last_applied.is_some(),
            "node {node_id} has applied the committed entries"
        );
    }

    cluster.shutdown().await;
}

/// Builds a committed catalog version for the test write.
fn committed_version(upload_id: UploadId, leader: u64) -> CommittedVersion {
    let payload = b"multi-node raft catalog payload";
    CommittedVersion {
        record: UploadRecord::new(
            upload_id,
            "alice",
            "photo.jpg",
            u64::try_from(payload.len()).expect("the payload length fits in u64"),
            u64::try_from(payload.len() / 2).expect("the compressed length fits in u64"),
            ContentHash::digest(payload),
            1_690_000_000_000,
            1_700_000_000_000,
        )
        .expect("the test metadata is valid"),
        replicas: BTreeSet::from([leader]),
    }
}

/// Waits until every node has applied exactly `version` to its catalog.
async fn wait_for_committed_version(
    nodes: &BTreeMap<u64, Arc<RaftNode>>,
    version: &CommittedVersion,
) {
    let upload_id = version.record.upload_id;
    let deadline = tokio::time::Instant::now() + APPLY_BUDGET;
    loop {
        let mut replicated = true;
        for node in nodes.values() {
            if node.committed_versions().await.get(&upload_id) != Some(version) {
                replicated = false;
            }
        }
        if replicated {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the committed version {upload_id} did not reach every node in the apply budget"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Waits until every node's applied catalog equals `expected`.
async fn wait_for_catalog(
    nodes: &BTreeMap<u64, Arc<RaftNode>>,
    expected: &BTreeMap<UploadId, CommittedVersion>,
) {
    let deadline = tokio::time::Instant::now() + APPLY_BUDGET;
    loop {
        let mut converged = true;
        for node in nodes.values() {
            if &node.committed_versions().await != expected {
                converged = false;
            }
        }
        if converged {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the applied catalog did not match on every surviving node in the apply budget"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Spawns one runtime's peer-serving task with its shutdown handle.
fn spawn_server(runtime: RaftRuntime, listener: Listener) -> ServerTask {
    let (shutdown, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(runtime.serve(listener, async move {
        let _ = shutdown_rx.await;
    }));
    ServerTask {
        shutdown: Some(shutdown),
        task: Some(task),
    }
}

/// Allocates distinct loopback ports that do not collide with the dedicated entry.
fn allocate_loopback_ports() -> [u16; NODE_COUNT] {
    loop {
        let listeners: [std::net::TcpListener; NODE_COUNT] = core::array::from_fn(|_| {
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .expect("an ephemeral loopback port is available")
        });
        let ports = listeners.each_ref().map(|listener| {
            listener
                .local_addr()
                .expect("a bound loopback listener reports its address")
                .port()
        });
        drop(listeners);
        if ports.iter().all(|port| *port != DEDICATED_PORT) {
            return ports;
        }
    }
}

/// Renders the shared manifest with one dedicated entry and three storage entries.
fn manifest_text(addresses: &[SocketAddr; NODE_COUNT]) -> String {
    let dedicated = format!(
        "[dedicated]\n\
         sender_id = \"{DEDICATED_SENDER_ID}\"\n\
         label = \"{DEDICATED_LABEL}\"\n\
         address = \"{LOOPBACK_HOST}:{DEDICATED_PORT}\"\n\
         encryption_public_key = \"{}\"\n\
         signing_public_key = \"{}\"\n",
        recipient_public_key_hex(&DEDICATED_ENCRYPTION_SEED),
        verifying_key_hex(&DEDICATED_SIGNING_SEED),
    );
    let storage = NODE_LABELS
        .iter()
        .enumerate()
        .map(|(index, label)| {
            format!(
                "\n[[storage]]\n\
                 sender_id = \"{}\"\n\
                 label = \"{label}\"\n\
                 address = \"{}\"\n\
                 encryption_public_key = \"{}\"\n\
                 signing_public_key = \"{}\"\n",
                SENDER_IDS[index],
                addresses[index],
                recipient_public_key_hex(&ENCRYPTION_SEEDS[index]),
                verifying_key_hex(&SIGNING_SEEDS[index]),
            )
        })
        .collect::<String>();
    format!("{dedicated}{storage}")
}

/// Builds the fixed test client limits.
fn client_limits() -> ClientLimits {
    ClientLimits::new(
        NonZeroUsize::new(LOGIN_MIN_LEN).expect("the test login minimum is non-zero"),
        NonZeroUsize::new(LOGIN_MAX_LEN).expect("the test login maximum is non-zero"),
        NonZeroUsize::new(PASSWORD_MIN_LEN).expect("the test password minimum is non-zero"),
        NonZeroUsize::new(PASSWORD_MAX_LEN).expect("the test password maximum is non-zero"),
        NonZeroUsize::new(RESPONDER_LABEL_MAX_LEN)
            .expect("the test responder-label limit is non-zero"),
    )
    .expect("the test client limits are ordered")
}

/// Returns the X25519 public key derived from `seed` as lower-case hex.
fn recipient_public_key_hex(seed: &[u8; KEY_LEN]) -> String {
    let key = RecipientPrivateKey::from_bytes(seed)
        .expect("the fixed X25519 seed builds a valid private key");
    hex(&key.public_key().as_bytes())
}

/// Returns the Ed25519 public key derived from `seed` as lower-case hex.
fn verifying_key_hex(seed: &[u8; KEY_LEN]) -> String {
    let key = MessageSigningKey::from_bytes(*seed);
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
