//! Multi-runtime integration test over real loopback TCP.
//!
//! Three distinct manifest storage identities start their own local OpenRaft
//! instances, bind their advertised loopback addresses, and serve incoming peer
//! RPCs through the encrypted and signed inter-server transport. One runtime is
//! bootstrapped with the manifest voter set, the cluster elects a leader, a
//! client write commits through a majority, and every node applies the entry.
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
    ClientLimits, Listener, ListenerError, MessageSigningKey, PrivateKey, RaftNode, RaftRequest,
    RaftResponse, RaftRuntime, RecipientPrivateKey, StaticManifest, UploadId, KEY_LEN,
    UPLOAD_ID_LEN,
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
    servers: Vec<ServerTask>,
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
        let mut servers = Vec::with_capacity(NODE_COUNT);
        for (index, runtime) in runtimes.iter_mut().enumerate().skip(1) {
            let runtime = runtime.take().expect("the follower runtime is present");
            let listener = listeners[index]
                .take()
                .expect("the follower listener is present");
            nodes.insert(fixture.node_id(NODE_LABELS[index]), runtime.node());
            servers.push(spawn_server(runtime, listener));
        }

        let bootstrap = runtimes[0]
            .take()
            .expect("the bootstrap runtime is present");
        bootstrap
            .initialize()
            .await
            .expect("the explicit bootstrap initializes the manifest storage voters");
        let listener = listeners[0]
            .take()
            .expect("the bootstrap listener is present");
        nodes.insert(fixture.node_id(NODE_LABELS[0]), bootstrap.node());
        servers.push(spawn_server(bootstrap, listener));

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

    /// Sends shutdown to every server task and awaits its clean exit.
    async fn shutdown(mut self) {
        for server in &mut self.servers {
            if let Some(shutdown) = server.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        for server in &mut self.servers {
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
async fn three_loopback_nodes_elect_a_leader_and_commit_through_the_peer_transport() {
    let fixture = Fixture::new();
    let cluster = Cluster::start(&fixture).await;
    let leader = cluster.wait_for_leader().await;
    let leader_node = Arc::clone(
        cluster
            .nodes
            .get(&leader)
            .expect("the elected leader belongs to the cluster"),
    );

    let upload_id = UploadId::from_bytes([0x44; UPLOAD_ID_LEN]);
    let response = leader_node
        .client_write(RaftRequest::UpdateReplicas {
            upload_id,
            replicas: BTreeSet::from([leader]),
        })
        .await
        .expect("the leader commits the client write through a majority");
    assert_eq!(
        response,
        RaftResponse { upload_id: None },
        "an update for an unknown upload id commits without changing a catalog version"
    );

    let applied = wait_for_applied_index(&leader_node).await;
    wait_for_all_nodes_to_apply(&cluster.nodes, applied).await;

    for (node_id, node) in &cluster.nodes {
        let status = node.status();
        assert_eq!(
            status.current_leader,
            Some(leader),
            "node {node_id} reports the elected leader"
        );
        assert_eq!(
            status.last_applied,
            Some(applied),
            "node {node_id} applied the committed entry at index {applied}"
        );
    }

    cluster.shutdown().await;
}

/// Waits until `node` reports at least one applied log entry.
async fn wait_for_applied_index(node: &RaftNode) -> u64 {
    let deadline = tokio::time::Instant::now() + APPLY_BUDGET;
    loop {
        if let Some(index) = node.last_applied_index() {
            return index;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the leader did not apply the committed entry within the apply budget"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Waits until every node applies exactly `index`.
async fn wait_for_all_nodes_to_apply(nodes: &BTreeMap<u64, Arc<RaftNode>>, index: u64) {
    let deadline = tokio::time::Instant::now() + APPLY_BUDGET;
    loop {
        if nodes
            .values()
            .all(|node| node.last_applied_index() == Some(index))
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "every node did not apply log index {index} within the apply budget"
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
         host = \"{LOOPBACK_HOST}\"\n\
         port = {DEDICATED_PORT}\n\
         x25519_public_key = \"{}\"\n\
         ed25519_public_key = \"{}\"\n",
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
                 host = \"{LOOPBACK_HOST}\"\n\
                 port = {}\n\
                 x25519_public_key = \"{}\"\n\
                 ed25519_public_key = \"{}\"\n",
                SENDER_IDS[index],
                addresses[index].port(),
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
