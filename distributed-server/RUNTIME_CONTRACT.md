# Storage-node runtime contract

This document is the stable public contract between the distributed-server
workstream and the test environment. It covers the container image, the
`storage-node` process, readiness, configuration/data locations, and the public
Rust surface that integration tests may use. It does not describe private
implementation details.

Node configuration is environment-only. The image contains no manifest, key
material, or data; all three are supplied at runtime through environment
variables and mounted paths.

## 1. Image build and layout

The Docker build context is the **repository root**:

```sh
docker build -f distributed-server/Dockerfile .
```

The multi-stage build uses `rust:1.99-bookworm` and runs
`cargo build --release --locked -p storage-node`. The runtime stage is
`debian:bookworm-slim` and contains:

| Property | Value |
|---|---|
| Entrypoint | `/usr/local/bin/storage-node-entrypoint` |
| Binary | `/usr/local/bin/storage-node` |
| User | `storage` |
| UID / GID | `10001` / `10001` |
| Working directory | `/home/storage` |
| Mode of entrypoint and binary | `0555` |

The image has no `CMD`, so the entrypoint runs the supervisor unless the
operator supplies positional arguments (see §2). It does not install a shell
framework, a service manager, or an HTTP health endpoint.

The runtime UID is unprivileged. A mounted manifest and key files only need to
be readable by UID 10001; `DATA_DIR` and the parent directory of
`LOG_FILE_PATH` must be writable by UID 10001. A test harness that bind-mounts
host directories must set that ownership before starting the container.

## 2. Process model

`/usr/local/bin/storage-node-entrypoint` runs as PID 1 and is a supervisor:

1. It starts `/usr/local/bin/storage-node` as a direct child.
2. It waits for that child and applies the restart rule below.
3. It forwards termination signals to the child and reaps it.

The child inherits the container environment and standard streams. Node
configuration is read from the environment by the child, not by the
entrypoint.

### Restart and exit rules

| Event | Supervisor behavior |
|---|---|
| Child exits with status `0` | Container exits with status `0`; the child is not restarted. |
| Child exits with a non-zero status | After `STORAGE_NODE_RESTART_DELAY_SECONDS`, the child is started again. Restarts continue indefinitely with the same fixed delay. |
| `SIGTERM` reaches PID 1 | The supervisor forwards `SIGTERM` to the child, waits for it to exit, then exits with status `143`. |
| `SIGINT` reaches PID 1 | The supervisor forwards `SIGINT` to the child, waits for it to exit, then exits with status `130`. |
| `SIGKILL` reaches PID 1 | Not trappable. The container ends immediately. |

Only the first termination signal determines the supervisor exit status;
additional termination signals keep forwarding to the child. A signal received
during the restart delay ends the supervisor without starting another child.
No Docker restart policy is required for the child-restart behavior; the
supervisor performs it inside the same container.

A node configuration or startup failure makes the child exit non-zero, so a
misconfigured node appears as a restart loop. The node's own diagnostics and
the supervisor's restart line on standard error identify the cause.

### Supervisor settings

`STORAGE_NODE_RESTART_DELAY_SECONDS` controls the delay before a restart:

- Unset or empty: `1`.
- Otherwise: the value must consist only of ASCII decimal digits (`0` through
  `9`), have at most four digits, and be in `1..=3600`. Zero, a signed value, a decimal fraction,
  whitespace, an out-of-range value, or any other character makes the
  entrypoint exit with status `64` before starting the node. There is no
  clamping to another value.

`STORAGE_NODE_BIN` overrides the child executable path:

- Unset or empty: `/usr/local/bin/storage-node`.
- Otherwise: the value is used as one path, without argument splitting. The
  file must be executable; an unusable path produces the non-zero-exit restart
  loop described above.

Both variables are supervisor variables and are not read by `storage-node`
itself.

### Positional arguments

Arguments after the image name replace the supervisor entirely:

```sh
docker run --rm homecloud-storage-node:test storage-node
```

The entrypoint runs `exec "$@"`, so the supplied command becomes PID 1 and the
restart, forwarding, and delay rules do not apply. This form is intended for
diagnostics and does not change the node's environment contract.

### Abrupt child termination (TST-016)

TST-016 verifies that a killed node process is brought back inside the same
container. The supervisor starts exactly one child, so its PID can be read from
the kernel's child list:

```sh
# Abruptly kill the storage-node child, not the supervisor.
docker exec <container> sh -c 'kill -KILL "$(cat /proc/1/task/1/children)"'
```

The container is not recreated: the same container ID and
`.State.StartedAt` remain, the supervisor's restart line appears on standard
error, and after `STORAGE_NODE_RESTART_DELAY_SECONDS` a new child PID appears.
A harness can observe the replacement with:

```sh
docker inspect -f '{{.Id}} {{.State.StartedAt}}' <container>
docker exec <container> cat /proc/1/task/1/children
```

Killing PID 1 itself is not this scenario; `SIGKILL` to PID 1 ends the
container and cannot be forwarded.

## 3. Readiness and bootstrap

### TCP readiness

A node is ready for the test environment when a TCP connection to its
`LISTEN_ADDR` succeeds. The node binds the application listener only after it
has validated its configuration, loaded both private key files, prepared the
data layout, loaded and validated the manifest when one is configured, started
the local Raft runtime, and completed the requested bootstrap command.

Consequences:

- A successful connect proves the listener exists. It does not perform a
  protocol handshake, and it does not prove that Raft has a leader, that a
  quorum is available, or that the catalog is readable.
- Readiness polling must connect to the address reachable from the harness. A
  node listening on `0.0.0.0` inside the container is reached through the
  published port or the container address, not by connecting to `0.0.0.0`.
- A node may still exit after a successful connect if a later operation fails;
  the harness must also observe container state and the supervisor restart
  behavior.

### Metrics

The metrics listener is not part of the readiness contract. It follows the
node's configured metrics policy: `GET /metrics` is served on
`METRICS_BIND_ADDR`, plain HTTP is allowed only on a loopback address, and a
non-loopback address requires both metrics TLS paths, as specified in the
README. There is no HTTP health, readiness, or liveness endpoint other than
`/metrics`, and `/metrics` must not be used as a readiness probe.

### Bootstrap

`RAFT_BOOTSTRAP` is the explicit, one-time initial-cluster command:

- `true` or `1` enables it; `false`, `0`, or an empty value disables it; any
  other value stops startup.
- Enabling it without `MANIFEST_PATH` stops startup before any listener is
  opened.
- It must be enabled on exactly one chosen storage node, once. The command
  initializes the voter membership with every `[[storage]]` entry from the
  manifest.
- After a successful initialize, the node writes the durable marker
  `DATA_DIR/raft/bootstrapped`. A later initialize on the same node is rejected
  and the node exits non-zero, so a container still carrying
  `RAFT_BOOTSTRAP=true` enters a restart loop on every start.
- The marker records that the initial membership command completed; it is not
  recreated or cleared by an ordinary start. A node with persisted state
  (`DATA_DIR/raft/state.cbor`) restores it on restart instead of forming a new
  cluster.
- Readiness of the bootstrap node implies the initialize command returned
  successfully, because the listener is bound afterwards. Readiness of the
  other nodes is only a listener check and does not prove they have caught up.

The harness should start the chosen node with `RAFT_BOOTSTRAP=true`, wait for
the marker or a successful readiness check, and then recreate that container
without the variable. The data directory must persist across that recreation.

## 4. Configuration and data

The complete `storage-node` environment variable set is the one documented in
the README section [Configuration](README.md#configuration). This contract does
not add node variables or defaults beyond that list. In particular, a missing
required value stops startup before a listener is opened, and an explicitly
invalid value is rejected rather than replaced with a default.

The supervisor adds only:

- `STORAGE_NODE_BIN`
- `STORAGE_NODE_RESTART_DELAY_SECONDS`

Key material is separate from the manifest. The manifest contains only open
keys; private encryption and signing keys are read from `ENCRYPTION_KEY_PATH`
and `SIGNING_KEY_PATH`. These files must be readable by UID 10001 and should be
mounted read-only. They are never part of the image or the manifest.

`DATA_DIR` has this layout:

```text
DATA_DIR/
  accounts/
    <hex-login>/<hex-upload-id>
  raft/
    state.cbor
    bootstrapped          # present after a successful bootstrap
```

`accounts/` holds blobs at their final paths; an integrity failure adds a
sibling `<blob>.corrupt` marker. `raft/state.cbor` is the persisted Raft state
and `raft/bootstrapped` is the one-time bootstrap marker. `LOG_FILE_PATH` is a
separate configured path and may live outside `DATA_DIR`; its parent directory
must be writable by UID 10001. Rotated log files use the
`<LOG_FILE_PATH>.<suffix>` naming described in the README.

The image itself contains no `accounts/`, `raft/`, manifest, key, or log
files.

## 5. Stable public Rust surface

Integration tests use the `storage_node` library crate. The authoritative list
is the curated `pub use` facade in
`distributed-server/crates/storage-node/src/lib.rs`; run from
`distributed-server`:

```sh
cargo doc --no-deps -p storage-node
```

Only public items are contract. Private modules, private fields, and internal
storage or transport mechanics are not part of it. The following items are
stable for the test environment.

### Runtime and Raft

| Item | Public behavior |
|---|---|
| `RaftRuntime::start` | Validates the local `[[storage]]` entry, its listen port and non-unspecified IP, both private keys against the manifest open keys, and the responder label; then starts the local OpenRaft instance backed by the encrypted peer transport. |
| `RaftRuntime::initialize` | Sends the one-time initial membership command with all manifest storage voters. A persisted bootstrap is rejected by the Raft layer. |
| `RaftRuntime::node` | Returns the shared `Arc<RaftNode>`. |
| `RaftRuntime::serve` | Serves encrypted peer RPC sessions on a `Listener` until the shutdown future resolves, dispatching verified RPC bytes to the local OpenRaft node. |
| `RaftRuntime::publish_version` | Pre-checks the fixed-voter quorum and local committed metadata, appends `RaftRequest::PublishVersion`, and returns the validated committed version; an invalid apply is a typed rejection. |
| `RaftRuntime::replica_status` | Pre-checks and appends `RaftRequest::ReplicaStatus`; the state machine validates voter/upload/login/hash and persists availability, updating the catalog replica set. |
| `RaftNode::status` | Returns `RaftStatus` with the current leader, term, and last applied index. |
| `RaftNode::client_write` | Writes one `RaftRequest` toward the current leader. A follower returns the OpenRaft client-write error; this layer neither redirects nor forwards. |
| `RaftNode::committed_versions` | Returns the applied catalog as a `BTreeMap<UploadId, CommittedVersion>`. |
| `RaftNode::replica_statuses` | Returns the persisted replica status map keyed by upload id and replica node id. |
| `RaftNode::handle_rpc` | Passes one encoded Raft RPC envelope to the local OpenRaft instance and returns the encoded reply. |

`RaftRuntime::start` and `initialize` are asynchronous and return
`Result<_, RuntimeError>`; `serve` is asynchronous and returns
`Result<(), ListenerError>`. `start` is the manifest-derived startup path used
by the binary; the binary wires `start`, `initialize`, `node`, and `serve`.

`RaftNode::handle_rpc` is wired into incoming peer sessions. In the current
binary `status` feeds the Raft gauges and `client_write`/`committed_versions`
have no client-facing operation caller; tests may call them directly.

### Manifest and listener

| Item | Public behavior |
|---|---|
| `StaticManifest::parse` | Parses and validates manifest TOML text. |
| `StaticManifest::load` | Reads, decodes, parses, and validates a manifest file. |
| `StaticManifest::local` | Resolves exactly one `[[storage]]` entry by label. |
| `StaticManifest::voting_members` | Returns the fixed `BTreeMap<NodeId, String>` voter set. |
| `StaticManifest::peer_directory` | Builds the trusted peer directory from the storage open keys. |
| `Listener::bind` | Binds an `ip:port` listen address. |
| `Listener::local_addr` | Returns the bound `SocketAddr`. |
| `Listener::accept` | Accepts one connection and returns frame reader/writer halves plus the peer address. |

`StaticManifest` exposes the validated `dedicated` and `storage` entries, and
`ManifestEntry` exposes its sender id, label, address, and open keys. The
binary loads the manifest and listener; the test environment may construct
them directly.

### Blob and catalog helpers

| Item | Public behavior |
|---|---|
| `write_blob_direct` | Streams a reader into the final blob path, verifies declared size and SHA-256, fsyncs the file and parent directory, and removes a rejected partial file. |
| `verify_local_blob` | Streams a committed local blob, compares its SHA-256 with the committed record, and maintains the `<blob>.corrupt` marker. |
| `restore_blob_direct` | Replaces a missing or marked-corrupt blob through the same verified durable write; an existing unmarked blob is refused. |
| `BlobPutBegin` | Blob-put begin message: login, upload id, blob metadata, and transfer compressed size. |
| `BlobPutResult` | Blob-put result: result code and optional upload id on success. |
| `BlobGet` | Blob-get request: login and upload id. |
| `BlobGetResult` | Blob-get result: result code, optional metadata, and optional transfer compressed size. |
| `CatalogPageHandler` | Maps a decoded `CatalogRequest` over committed versions to the encoded `[result_code, result_args]` catalog page response. |

The matching encode/decode functions are public for the blob-put, blob-get,
data-frame, and catalog-exchange messages. These helpers are library-only: the
current `storage-node` binary does not yet expose client catalog or blob
operations, start the integrity scan loop, or invoke the blob write/restore
path. Tests may exercise them directly.

### Raft state-machine types

- `CommittedVersion` carries the `UploadRecord` and the `BTreeSet<NodeId>` of
  storage nodes that durably hold the blob.
- `RaftRequest` has `CommitVersion(CommittedVersion)` and
  `UpdateReplicas { upload_id, replicas }`. An unknown upload id in an update is
  a no-op reported as `None`.
- `RaftResponse` carries the affected `Option<UploadId>`.

These are the state-machine request and response types applied by
`RaftNode::client_write`; they are library-only until a client operation uses
them.

## 6. Test-environment usage

The examples use placeholder paths and generated test keys. Do not commit real
private keys, certificates, passwords, or manifests containing production
material.

### Build

From the repository root:

```sh
docker build -f distributed-server/Dockerfile -t homecloud-storage-node:test .
```

### Prepare runtime material

Keep private keys, manifest, and data outside the repository/build context
(`CFG-CL-033`). The operator creates a host-only directory; the key directory
and files must be owned/readable only by container UID/GID 10001 and are
mounted read-only:

```sh
SECRETS_DIR=/srv/homecloud-test
sudo install -d -m 0700 -o 10001 -g 10001 "$SECRETS_DIR/keys"
sudo install -d -m 0750 -o 10001 -g 10001 "$SECRETS_DIR/storage-a"
```

`$SECRETS_DIR/manifest.toml` is the manifest from the README with the test
nodes' open keys and `192.0.2.0/24` addresses. `$SECRETS_DIR/keys/` contains
the corresponding private key files, created by the operator outside the
checkout. `$SECRETS_DIR/storage-a` is writable by UID 10001.

### Run one node

Put the node settings from the README in an environment file, for example
`$SECRETS_DIR/storage-a.env`:

```sh
NODE_ID=storage-a
LISTEN_ADDR=0.0.0.0:7001
DATA_DIR=/var/lib/homecloud
MANIFEST_PATH=/etc/homecloud/manifest.toml
ENCRYPTION_KEY_PATH=/etc/homecloud/keys/encryption.key
SIGNING_KEY_PATH=/etc/homecloud/keys/signing.key
METRICS_BIND_ADDR=127.0.0.1:9001
# Add the remaining required values from README §Configuration.
```

The environment file must contain every required value; this contract supplies
no defaults. The manifest advertises concrete node addresses, so the harness
must attach the containers to the network used by those addresses. Create the
network with the manifest subnet and start the bootstrap node once:

```sh
docker network create --subnet 192.0.2.0/24 homecloud-test

docker run -d --name storage-a \
  --network homecloud-test --ip 192.0.2.11 \
  --env-file "$SECRETS_DIR/storage-a.env" \
  -e RAFT_BOOTSTRAP=true \
  -p 127.0.0.1:7001:7001 \
  -v "$SECRETS_DIR/manifest.toml:/etc/homecloud/manifest.toml:ro" \
  -v "$SECRETS_DIR/keys:/etc/homecloud/keys:ro" \
  -v "$SECRETS_DIR/storage-a:/var/lib/homecloud" \
  homecloud-storage-node:test
```

Start the other storage nodes with the same mounts and their own `NODE_ID`,
`LISTEN_ADDR`, `--ip` matching the manifest, data directory, metrics address,
and published port, but without `RAFT_BOOTSTRAP=true` (or with it set to
`false`).

### Poll readiness

This harness-side poll uses the bash `/dev/tcp` pseudo-device; the entrypoint
itself remains POSIX `sh`:

```sh
until (exec 3<>/dev/tcp/127.0.0.1/7001) 2>/dev/null; do
  sleep 0.2
done
```

The successful connect only proves that the application listener is bound. Use
the Raft and catalog APIs, logs, or metrics to assert cluster behavior.

### Bootstrap once, then restart without it

After `storage-a` is ready and `DATA_DIR/raft/bootstrapped` exists, remove and
recreate the container without the bootstrap variable:

```sh
docker stop storage-a
docker rm storage-a
docker run -d --name storage-a \
  --network homecloud-test --ip 192.0.2.11 \
  --env-file "$SECRETS_DIR/storage-a.env" \
  -p 127.0.0.1:7001:7001 \
  -v "$SECRETS_DIR/manifest.toml:/etc/homecloud/manifest.toml:ro" \
  -v "$SECRETS_DIR/keys:/etc/homecloud/keys:ro" \
  -v "$SECRETS_DIR/storage-a:/var/lib/homecloud" \
  homecloud-storage-node:test
```

The persisted data directory restores the Raft state. Re-running with
`RAFT_BOOTSTRAP=true` after the marker exists is rejected and produces a
restart loop.

### Stop and abrupt-kill scenarios

```sh
docker stop --time 30 storage-a
docker start storage-a
```

`docker stop` sends `SIGTERM` to PID 1, which forwards it to the child and
exits after the child stops. If the child does not exit within the Docker
timeout, the runtime sends `SIGKILL` to PID 1 and the container ends.

To restart only the node process inside the running container, use the
TST-016 command from §2. The container stays up, the child is replaced after
the configured delay, and the data directory is unchanged.
