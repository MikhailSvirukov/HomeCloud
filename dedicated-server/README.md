# Dedicated server

The v1 dedicated server is the client's bootstrap/control-plane entry point and
source of the static cluster topology. It is **not** a file proxy, **not** a Raft
transport, and does **not** store photos or videos (see
`docs/specification-v1.md`, `docs/agent-dedicated-server.md`); clients get node
addresses, labels, and public keys here, then send files straight to storage.

## Current status

Configuration loading and validation, key-identity loading for the server's own
key pair, internal heartbeat tracking for the fixed cluster, and the in-memory
account registration model are implemented. The binary loads the configuration,
installs the configured structured log file, loads the key identity, logs
startup and shutdown events, and exits. Network I/O, the wire protocol,
registration over the network, cluster replication, and topology responses are
not implemented yet.

## Build and run

GitHub Actions runs the quality-gate script for pull requests. OpenCode agents
do not run test suites locally; use the PR checks as the test result. CI runs
the area's quality-gate script; agent containers must not invoke it.

Run from `dedicated-server/` (`Cargo.lock` is committed):

```bash
cargo build --locked
```

```bash
HOMECLOUD_DEDICATED_SERVER_CONFIG=/path/to/config.toml cargo run --locked
```

Without that variable the binary reads `dedicated-server.toml` from the current
directory.

The binary installs `mimalloc` as its process allocator instead of the system
default.

The crate's modules are private; its public items are re-exported from the crate
root. Consumers therefore use `dedicated_server::` paths (for example
`dedicated_server::ServerConfig`) rather than module paths such as
`dedicated_server::config::ServerConfig`.

## Running the server image

`Dockerfile` builds a multi-stage image with the repository root as the build
context. The runtime stage contains only the release binary at
`/usr/local/bin/dedicated-server` and runs it as a non-root user (uid/gid
`10001`). The image reads the configuration file named by
`HOMECLOUD_DEDICATED_SERVER_CONFIG`, which is set to
`/etc/homecloud/dedicated-server.toml`; the configuration directory must be
mounted read-only at `/etc/homecloud`.

Private keys must stay outside the checkout. Keep the runtime configuration and
keys in an external directory such as
`${XDG_CONFIG_HOME:-$HOME/.config}/homecloud/dedicated-server`, and make the
directory mode `700` and the key files mode `600`, owned by the container user
`10001:10001` so only that user can read them; world-readable keys are not
acceptable. Copy the example configuration and generate placeholder keys before
the first run:

```bash
export DEDICATED_SERVER_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/homecloud/dedicated-server"
mkdir -p "$DEDICATED_SERVER_CONFIG_DIR/keys"
chmod 700 "$DEDICATED_SERVER_CONFIG_DIR" "$DEDICATED_SERVER_CONFIG_DIR/keys"
cp dedicated-server/config/dedicated-server.example.toml \
  "$DEDICATED_SERVER_CONFIG_DIR/dedicated-server.toml"
chmod 600 "$DEDICATED_SERVER_CONFIG_DIR/dedicated-server.toml"
openssl rand -out "$DEDICATED_SERVER_CONFIG_DIR/keys/encryption.key" 32
openssl rand -out "$DEDICATED_SERVER_CONFIG_DIR/keys/signing.key" 32
chmod 600 "$DEDICATED_SERVER_CONFIG_DIR/keys/encryption.key" \
  "$DEDICATED_SERVER_CONFIG_DIR/keys/signing.key"
sudo chown -R 10001:10001 "$DEDICATED_SERVER_CONFIG_DIR"
docker compose build dedicated-server-runtime
docker compose up dedicated-server-runtime
```

The `dedicated-server-runtime` service requires `DEDICATED_SERVER_CONFIG_DIR`
and mounts it at `/etc/homecloud`. If you cannot change ownership, skip the
`sudo chown`, keep the keys owned by your host user with the same modes, add
`--user "$(id -u):$(id -g)"` to the `docker run` command below, and replace the
log tmpfs `uid=10001,gid=10001` values with `uid=$(id -u),gid=$(id -g)` so
`/var/log/homecloud` stays writable for the overridden user.

The equivalent without Compose:

```bash
docker build -f dedicated-server/Dockerfile -t homecloud-dedicated-server:0.1.0 .
docker run --rm --read-only \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --tmpfs /var/log/homecloud:rw,nosuid,nodev,size=1g,mode=0750,uid=10001,gid=10001 \
  -v "$DEDICATED_SERVER_CONFIG_DIR:/etc/homecloud:ro" \
  homecloud-dedicated-server:0.1.0
```

The two key files named by `[identity]` must exist under the mounted directory,
be non-empty and contain different material. The process validates its
configuration, writes JSON Lines events to the file named by `[logging] path`,
and exits; it does not listen on `listen` and publishes no ports until the
network layer exists. The log directory must be writable by the container user.

## Logging

The binary writes UTF-8 JSON Lines events to the file named by
`[logging] path`: each line is one JSON object and one event. stdout stays
empty. The default level is `info`; set `RUST_LOG` to override it (for example
`RUST_LOG=debug`). Lines are emitted without ANSI colours.

The file rotates so it does not grow without bound. A rotation starts a new
active file when appending the next record would exceed `max_file_size_bytes`,
when the active file is older than `max_file_age_ms`, or when the writer opens
an active file that already exceeds either limit. The previous active file is
renamed in the same directory to `<file name>.<UTC epoch milliseconds of the
segment start>` (for example `dedicated-server.log.1750000000000`); a numeric
suffix such as `.1` is added when that name already exists, so rotation never
overwrites a file. Within one process the timestamp is the maximum of the
segment start and the previous rotation timestamp plus one, so names increase
monotonically and a freed name is not reused. After a restart a timestamp can
be reused; the numeric suffix then separates the generations. Cross-restart
ordering is best effort when a reused timestamp and a previous-run file share a
modification time, because the suffix does not record which run is newer.

Rotated files are pruned oldest first when the writer opens, after every
rotation, before a record that would push the total over `max_total_size_bytes`,
and at least once every `min(retention_ms, max_file_age_ms)` of writing. A
rotated file is deleted when its modification time is older than `retention_ms`,
or when the total size of the active file plus all rotated files must shrink so
the incoming record fits `max_total_size_bytes`; pruning stops once no file is
expired and the incoming record fits. Files are ordered by modification time,
then by the embedded timestamp, then by the numeric suffix, so equal timestamps
still prune deterministically. Only files whose names match the rotated-name
pattern of the configured path are removed, the active file is never removed,
and unrelated files in the directory are left untouched.

A record larger than `max_file_size_bytes` is written whole to an empty active
file so a JSON line is never split. A record larger than `max_total_size_bytes`
is also written whole; either case temporarily leaves the file or the whole set
above its configured limit instead of truncating the record. Reserve free space
beyond `max_total_size_bytes` in the log directory for the active file and for
filesystem overhead; the example values stay well below the 1 GiB tmpfs used by
the runtime setup.

Each event carries:

- `timestamp`: UTC time in RFC 3339 format with milliseconds.
- `component`: `dedicated-server`.
- `level`: importance level such as `INFO` or `ERROR`.
- `event`: event type. Startup uses `startup`; stopping uses `shutdown`.
- `operation`, `peer_id`, `outcome`, `error_code`, `duration_ms`, `message`:
  present when the event carries them.

A successful startup is one `startup` event with `outcome` set to `success`,
followed by one `shutdown` event with `outcome` set to `success` and the message
`server stopped`. A failed startup is one `startup` event with `outcome` set to
`failure`, an `error_code` of `configuration_error`, `logging_error`, or
`identity_error`, and a fixed stage-specific `message`; the failure event is the
last record. Records never contain file paths or names, logins, cryptographic
keys, verifiers, payloads or other user metadata.

### Two-phase start and the non-blocking writer

The configuration names the log file, so the binary starts in two phases:

1. It loads and validates the configuration. If that fails, the log path is
   unknown, so it writes exactly one `startup` failure record (error code
   `configuration_error`) directly to stderr and exits with the failure code.
2. It opens `[logging] path` in append mode and installs the JSON Lines
   subscriber. If the file cannot be opened or a global subscriber is already
   installed, it writes exactly one `startup` failure record (error code
   `logging_error`) directly to stderr and exits with the failure code.
   Otherwise the key identity is loaded; an identity failure is written through
   the installed writer to the configured file.

After a successful start, startup and shutdown events also go to the configured
file. Both stderr fallbacks are one-off bootstrap records, not a second log
stream.

Emitting an event never performs a synchronous file operation on the calling
thread. Records are handed to a bounded queue (4096 buffered lines) drained by a
background writer; a full queue drops the record instead of blocking. The
writer's dropped-or-errored record count is exposed as `LogGuard::dropped_records`
and is the diagnostic counter required by `NFR-OBS-004`. The guard stays alive
for the whole process and flushes buffered records when dropped.

## Configuration schema

- Top level: `listen` (socket address string), `max_label_bytes`, `[identity]`,
  `[heartbeat]`, `[password]`, `[logging]`, `[[cluster.nodes]]`.
- `max_label_bytes`: the maximum length of any node or identity label in UTF-8
  bytes (`NonZeroUsize`). It has no default.
- `identity`: `label`, `encryption_private_key`, `signing_private_key` (paths).
- `heartbeat`: `interval_ms` and `timeout_ms` are non-zero milliseconds
  (`NonZeroU64`), `miss_threshold` is a non-zero count (`NonZeroU32`). A zero
  value is rejected while the configuration is parsed, not by a later check.
- `password`: `memory_kib`, `iterations` and `parallelism` are non-zero
  (`NonZeroU32`), and `salt_length_bytes` is a non-zero byte count
  (`NonZeroUsize`). The Argon2id parameters and salt length used to derive
  password verifiers are configured here.
- `logging`: `path`, the structured JSON Lines log file, and four non-zero
  limits (`NonZeroU64`): `retention_ms` (how long a rotated file is kept),
  `max_file_age_ms` (maximum age of the active file before rotation),
  `max_file_size_bytes` (maximum size of one file), and
  `max_total_size_bytes` (maximum size of the active file plus every rotated
  file). It is required and has no default; the process opens it in append mode.
  `max_total_size_bytes` must be at least `max_file_size_bytes`, otherwise
  loading fails. A path that cannot be initialized is a startup failure. Leave
  free space beyond `max_total_size_bytes` in the log directory for the active
  file and filesystem overhead.
- `cluster.nodes[]`: `sender_id` (unique 16-byte node identifier), `label`
  (non-empty, unique), `address` (unique socket address),
  `encryption_public_key`, `signing_public_key`. Node identifiers and public
  keys are 16- and 32-byte values written in hexadecimal.
- Empty `cluster.nodes` is invalid; unknown keys are rejected.

Example:

```toml
listen = "10.0.0.1:7000"
max_label_bytes = 64

[identity]
label = "dedicated-1"
encryption_private_key = "/etc/homecloud/dedicated/encryption.key"
signing_private_key = "/etc/homecloud/dedicated/signing.key"

[heartbeat]
interval_ms = 1000
timeout_ms = 500
miss_threshold = 3

[password]
memory_kib = 8
iterations = 1
parallelism = 1
salt_length_bytes = 16

[logging]
path = "/var/log/homecloud/dedicated-server.log"
retention_ms = 604800000
max_file_age_ms = 86400000
max_file_size_bytes = 52428800
max_total_size_bytes = 536870912

[[cluster.nodes]]
sender_id = "00000000000000000000000000000001"
label = "storage-1"
address = "10.0.0.11:7100"
encryption_public_key = "1111111111111111111111111111111111111111111111111111111111111111"
signing_public_key = "2222222222222222222222222222222222222222222222222222222222222222"

[[cluster.nodes]]
sender_id = "00000000000000000000000000000002"
label = "storage-2"
address = "10.0.0.12:7100"
encryption_public_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
signing_public_key = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
```

## Key identity

`src/identity.rs` loads the server's own key pair for startup. The identity is
read from the two private-key files named by `identity.encryption_private_key`
and `identity.signing_private_key`; the bytes stay opaque because key encoding
and algorithms are still open parameters in `docs/specification-v1.md`. Loading
fails when either file cannot be read, is empty, is larger than the 64 KiB
limit, or when the same material would serve both the decryption and the
signing role. At most 64 KiB plus one byte of each file is read, so an
oversized file is rejected without being loaded in full, and the returned
identity error names the offending key file; log records omit every path and
file name. The configured
`identity.label` is bound to the loaded identity and is used as the node's
sender label. `ServerIdentity`'s `Debug` output deliberately reports only the
label and key lengths, never the key bytes.

## Heartbeat tracking

`src/heartbeat.rs` holds the scheduling and consecutive-miss bookkeeping for the
dedicated server's own heartbeat exchanges with the fixed cluster. A tracker is
built from the cluster's node labels, a timing value (interval, timeout and
non-zero miss threshold) and a start `Instant`; it is a pure state machine that
callers drive with explicit `Instant` values, and it uses no timers, no I/O, and
no async work.

- Each node's next attempt is scheduled one `heartbeat.interval_ms` after a
  response or a missed attempt; an in-flight attempt that is not answered within
  `heartbeat.timeout_ms` counts as a miss.
- A node is marked internally unavailable after `heartbeat.miss_threshold`
  consecutive misses; a response clears the counter and marks it healthy again.
- Per `docs/specification-v1.md`, this availability state stays internal to the
  dedicated server and is never propagated to clients or storage nodes.

The tracker keeps that availability private and reports only the labels of
unavailable nodes, in the order the labels were supplied. It produces
send/timeout actions, but delivering heartbeats over the network is not
implemented yet.

## Account registration

`src/registration.rs` holds the in-memory account model behind client
registration. It is pure state: no I/O, not async, and no cryptography.

- `Login` is a validated, cluster-unique identifier. It must be non-empty, at
  most 64 characters, use only the conservative ASCII subset
  `[A-Za-z0-9._-]`, and must not be the relative path components `.` or `..`.
- `Account` holds the login, the password verifier and the client encryption
  public key; it is built through the validating `Account::new` constructor,
  which rejects an empty verifier or client key.
- `AccountRegistry` registers accounts, looks them up and iterates them in login
  order so replication to the cluster is deterministic. Registering an
  already-taken login is rejected and leaves the registry unchanged.
- `Account` deliberately has no `Debug`, so the verifier and key bytes cannot be
  rendered accidentally.
- The password-hashing scheme and the key encodings are still open parameters
  in `docs/specification-v1.md`; this module only carries those bytes and
  implements no crypto.

## Not in v1

File storage/proxying, Raft relaying, dynamic node registration, key rotation,
admin panel, and health/topology propagation to clients or storage nodes.
