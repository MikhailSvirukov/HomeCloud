# Dedicated server

The v1 dedicated server is the client's bootstrap/control-plane entry point and
source of the static cluster topology. It is **not** a file proxy, **not** a Raft
transport, and does **not** store photos or videos (see
`docs/specification-v1.md`, `docs/agent-dedicated-server.md`); clients get node
addresses, labels, and public keys here, then send files straight to storage.

## Current status

Configuration loading and validation, key-identity loading for the server's own
key pair, internal heartbeat tracking for the fixed cluster, availability
observability for the tracked nodes and quorum, the in-memory account
registration model and a Prometheus metrics endpoint are implemented. The binary
loads the configuration, installs the configured structured log file, loads the
key identity, serves `GET /metrics` on the configured address (plain HTTP on loopback,
HTTPS when TLS is configured), logs
startup, and runs until `SIGINT` or `SIGTERM`, then logs shutdown and exits. Application
network I/O, the wire protocol, registration over
the network, cluster replication, and topology responses are not implemented
yet.

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
serves `GET /metrics` on the address named by `[metrics] bind` (HTTPS when
`[metrics.tls]` is configured), and runs
until the container is stopped (`SIGTERM`) before writing the shutdown record.
It does not listen on `listen` and publishes no application ports until the
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
`failure` and one of the stage error codes below, with a fixed
stage-specific `message`:

- `configuration_error` and `logging_error` are bootstrap failures written to
  stderr before the log file exists; each is the only record of that run.
- `identity_error` and `metrics_error` are written through the configured
  writer and are the last record of the run.
- `runtime_error` means the process could not install its shutdown handling; it
  is written before a successful `startup` record, so a run may contain no
  success record at all. Records never contain file paths or names, logins, cryptographic
keys, verifiers, payloads or other user metadata.

### Runtime events

While the server runs, only significant state changes and failures are
recorded; a successful periodic heartbeat and each internal step of a request
produce no record (`NFR-OBS-002`). `AvailabilityMonitor` in
`src/availability.rs` owns the heartbeat tracker and reports each transition
exactly once, in the configured cluster order:

- a storage node becoming unavailable is one `availability` event with the
  node label in `peer_id`, `outcome` set to `unavailable` and level `WARN`;
- a node that was unavailable and responds again is one `availability` event
  with `outcome` set to `available` and level `INFO`;
- losing or regaining a quorum is one `quorum` event with `outcome` set to
  `lost` or `restored` and a message carrying only the available and total
  counts. A quorum requires a strict majority of the configured nodes
  (`available * 2 > total`), the narrowest reading of the SRS.

A rejected heartbeat response is one `heartbeat` event with `outcome` set to
`failure`, the `heartbeat_error` code, the node label in `peer_id` when the
error names one, and a fixed message. The monitor logs these errors itself and
returns them to the caller. Wiring the monitor into the request loop and the
wire layer is part of the networking work.

### Two-phase start and the non-blocking writer

The configuration names the log file, so the binary starts in two phases:

1. It loads and validates the configuration. If that fails, the log path is
   unknown, so it writes exactly one `startup` failure record (error code
   `configuration_error`) directly to stderr and exits with the failure code.
2. It opens `[logging] path` in append mode, applies the rotation and
   retention limits to the existing files, and installs the JSON Lines
   subscriber. If the file or its rotated set cannot be initialized, or a
   global subscriber is already installed, it writes exactly one `startup`
   failure record (error code `logging_error`) directly to stderr and exits
   with the failure code.
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

- Top level: `listen` (socket address string), `max_label_bytes`,
  `max_frame_bytes`, `max_header_bytes`, `max_verifier_bytes`,
  `min_login_bytes`, `max_login_bytes`, `min_password_bytes`,
  `max_password_bytes`, `max_source_name_bytes`, `max_file_size_bytes`,
  `connect_timeout_ms`, `inter_server_timeout_ms`, `[registration]`,
  `[identity]`, `[heartbeat]`, `[password]`, `[logging]`, `[metrics]`,
  `[[cluster.nodes]]`.
- `max_header_bytes` bounds one encoded logical header (`CFG-CL-046`, range
  `1024..=16777216`); `max_verifier_bytes` bounds a PHC verifier
  (`CFG-CL-047`, range `128..=4096`); the login and password limits are ASCII
  and UTF-8 byte counts (`CFG-CL-029`, login `1..=255`, password `1..=4096`);
  `max_source_name_bytes` bounds the original media name (`CFG-CL-032`,
  `1..=65535`); `max_file_size_bytes` bounds the original and compressed media
  size (`CFG-CL-020`, `1..=i64::MAX`); `connect_timeout_ms` and
  `inter_server_timeout_ms` bound a connection attempt and one control
  inter-server request (`CFG-CL-027`, `CFG-CL-054`, `100..=300000`).
- `registration`: `total_deadline_ms` (`1000..=300000`), `attempt_deadline_ms`
  (`100..=300000`, not above the total), `attempts` (`1..=1000`, the first
  attempt included) and `retry_interval_ms` (`1..=60000`) bound the storage
  commit stage (`CFG-CL-042`-`CFG-CL-045`).
- Every numeric setting is validated against the approved range
  (`CFG-CL-049`), a missing required setting fails before any listener opens
  (`CFG-CL-050`) and the cross-field rules of the profile are enforced
  (`CFG-CL-051`): the heartbeat response timeout is below the interval, each
  credential minimum is not above its maximum, the registration attempt
  deadline is not above the total, the rotation age is within the retention
  period, the metrics port differs from the application port on the same
  address, the worst-case registration logical header and the full topology
  answer fit `max_header_bytes`, the generated PHC verifier fits
  `max_verifier_bytes`, and the logical header plus its worst-case outer
  envelope fits the configured frame body.
- `max_label_bytes`: the maximum length of any node or identity label in UTF-8
  bytes (`NonZeroUsize`). It has no default.
- `max_frame_bytes`: the largest frame payload the server accepts from a peer,
  in bytes (`NonZeroU32`, no default). The receiver checks the announced
  length against it before allocating the payload, so an oversized frame is
  refused without reading its body.
- `identity`: `label`, `encryption_private_key`, `signing_private_key` (paths).
- `heartbeat`: `interval_ms` and `timeout_ms` are non-zero milliseconds
  (`NonZeroU64`), `miss_threshold` is a non-zero count (`NonZeroU32`). A zero
  value is rejected while the configuration is parsed, not by a later check.
- `password`: `memory_kib`, `iterations` and `parallelism` are non-zero
  (`NonZeroU32`), and `salt_length_bytes` is a non-zero byte count
  (`NonZeroUsize`) in the range 16..=48, the range a 64-character PHC salt
  field can carry. The Argon2id parameters and salt length used to derive
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
- `metrics` sets `bind`, the address and TCP port of the Prometheus metrics
  endpoint (for example `127.0.0.1:9101`), and an optional `[metrics.tls]`
  section with `certificate_path`, `private_key_path` and `trust_path`. It is
  required and always needs a non-zero port. A loopback `bind` may use plain
  HTTP; a non-loopback `bind` requires `[metrics.tls]`, otherwise loading the
  configuration fails, so the endpoint is never exposed without TLS. The trust
  material is the certificate or CA the collecting client verifies the server
  against. At startup the server checks that the certificate and trust files
  exist and contain PEM certificates and that the key file contains a PEM
  private key matching the certificate; building a chain from `trust_path` is
  the collecting client's job.
- `cluster.nodes[]`: `sender_id` (unique 16-byte node identifier), `label`
  (non-empty, unique), `address` (unique socket address),
  `encryption_public_key`, `signing_public_key`. Node identifiers and public
  keys are 16- and 32-byte values written in hexadecimal.
- Empty `cluster.nodes` is invalid; unknown keys are rejected.

Example:

```toml
listen = "10.0.0.1:7000"
max_label_bytes = 64
max_frame_bytes = 4194304
max_header_bytes = 1048576
max_verifier_bytes = 512
min_login_bytes = 1
max_login_bytes = 64
min_password_bytes = 8
max_password_bytes = 1024
max_source_name_bytes = 4096
max_file_size_bytes = 10485760
connect_timeout_ms = 5000
inter_server_timeout_ms = 10000

[registration]
total_deadline_ms = 30000
attempt_deadline_ms = 10000
attempts = 5
retry_interval_ms = 1000

[metrics]
bind = "127.0.0.1:9101"

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


## Metrics endpoint

The process serves a Prometheus-compatible endpoint on the configured
`metrics.bind` for its whole lifetime (`NFR-OBS-006`, `CFG-OBS-003`). It is a
separate monitoring interface: it shares no port, framing or credentials with
the application TCP protocol.

- `GET /metrics` returns `200` with `Content-Type: text/plain; version=0.0.4;
  charset=utf-8` and the Prometheus text exposition. Any other path returns
  `404`; a non-`GET` request for `/metrics` returns `405`. A request is read
  with one bounded read under a five-second timeout, connections are served
  one at a time and every response sends `Connection: close`. The read and the
  write each use the five-second socket timeout, so a slow client cannot pin a
  worker for more than the request read plus the response write.
- A loopback `bind` may serve plain HTTP; any non-loopback `bind` must
  configure `[metrics.tls]` and is then served over HTTPS with that certificate
  (`NFR-OBS-007`, `CFG-OBS-004`). A non-loopback bind without TLS is rejected
  while loading the configuration, so the endpoint never runs unencrypted
  outside the host. The collecting client verifies the server against the
  configured trust material.

With TLS configured, the endpoint serves HTTPS on the same path and with the
same exposition. An unreadable certificate, private key or trust file, a file
with no usable PEM material, or a certificate and key that do not match fails
the startup with the `metrics_error` code.

Every label value is owned by the registry, so a scrape can never create a new
one (`NFR-OBS-008`):

| Metric | Type | Labels | Meaning |
|---|---|---|---|
| `homecloud_dedicated_up` | gauge | none | `1` while the process serves metrics |
| `homecloud_dedicated_node_available` | gauge | `node` (manifest label) | `1` while that storage node is available |
| `homecloud_dedicated_quorum_available` | gauge | none | `1` while a strict majority of the configured nodes is available |
| `homecloud_dedicated_log_dropped_records_total` | counter | none | records lost by the bounded log writer (`NFR-OBS-004`) |
| `homecloud_dedicated_log_queue_capacity_records` | gauge | none | capacity of the bounded log writer queue |
| `homecloud_dedicated_request_duration_seconds` | histogram | `operation` (`registration`, `topology`), `le` | duration of handled client requests |

The label set is fixed at startup: `node` values come from the static manifest
and `operation` values from the v1 operation table, so no login, `request_id`,
`upload_id`, file name or other user-unique value can appear. The log-writer queue capacity is exported; its current occupancy is not,
because the underlying appender does not report it, and the dropped-record
counter is the diagnostic for a saturated queue.

The node and quorum gauges are updated by the heartbeat loop and the request
duration histogram by the request loop; both loops belong to the wire layer and
are not part of this version yet. Until they land the gauges keep their initial
`1` value, the histogram renders zero observations, and only `up`, the
dropped-record counter and the queue capacity carry live values. They are
registered now so the endpoint and its labels are fixed and tested, and the
availability and request producers can fill them without changing the
contract.

After a successful start the process serves the endpoint and waits for
`SIGINT` or `SIGTERM`, then writes the `shutdown` record, flushes the log writer
and exits with status `0`. A failure to start the endpoint is a startup failure
with the `metrics_error` code.

## Framing

Every application message on the wire is a 4-byte unsigned big-endian length
prefix followed by exactly that many payload bytes (`INT-NET-002`). The prefix
counts only the bytes after itself (`INT-NET-003`), so a 100-byte payload starts
with `00 00 00 64`. `src/frame.rs` provides the codec every TCP connection
will use: the writer emits the prefix and then the payload, and the reader
checks the announced length against the configured `max_frame_bytes` before
allocating the payload buffer (`INT-NET-004`), refusing an oversized frame
without reading its body. A zero-length frame is valid at this layer; the
message layer decides whether an empty payload is meaningful.

`src/wire.rs` encodes and decodes the CBOR structures inside a frame: the
external header with its seven positions and null rules (`INT-NET-050`), the
client logical header for the four v1 operations (`INT-NET-051`, `INT-NET-057`,
with the registration, topology, catalog page and upload argument schemas), the
result envelope (`INT-NET-061`) and the inter-server envelope `[message_type,
args]` with its heartbeat request, heartbeat response and Raft RPC messages
(`INT-NET-072`-`INT-NET-075`). The inter-server envelope also carries the
registry lookup and lookup result (`INT-NET-089`, `INT-NET-090`) and the
registration propose and result (`INT-NET-091`, `INT-NET-092`), including the
snapshot records and the ASCII verifier bound of `CFG-CL-047`. The client
header decoder takes `HeaderLimits` from the configuration: the encoded header
is bounded by `max_header_bytes` before any CBOR parsing (`INT-NET-085`), and
the login, password and upload source name are bounded before they are copied
(`INT-NET-086`, `CFG-CL-029`, `CFG-CL-032`), with typed errors naming the field,
the observed length and the bound. Encoding is the deterministic minimal
representation, and decoding rejects a wrong element count, type, byte-string
length or null rule, an unknown operation code, a reserved, unassigned or
unsupported message type, a wrong argument count, type or value, and trailing
bytes.

`src/answers.rs` builds the two successful answers the server owes a client: the
registration answer with only the assigned installation identifier
(`INT-NET-059`) and the topology answer with the manifest nodes in order and
canonical hosts (`INT-NET-070`). A strict decoder covers the same topology
schema from the receiving side.

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

- `Login` is a validated, cluster-unique identifier. It must be non-empty, use
  only the conservative ASCII subset `[A-Za-z0-9._-]`, must not be the relative
  path components `.` or `..`, and must fit the configured
  `min_login_bytes`/`max_login_bytes` range (`CFG-CL-029`); `Login::parse`
  takes the configured limits rather than a built-in maximum.
- `Account` holds the login, the password verifier and the client encryption
  public key; it is built through the validating `Account::new` constructor,
  which rejects an empty verifier or client key.
- `AccountRegistry` registers accounts, looks them up and iterates them in login
  order so replication to the cluster is deterministic. Registering an
  already-taken login is rejected and leaves the registry unchanged. A password
  outside the configured `min_password_bytes`/`max_password_bytes` range
  (`CFG-CL-029`) is rejected before any verifier is derived, with the observed
  length and the configured bound in the error.
- `Account` deliberately has no `Debug`, so the verifier and key bytes cannot be
  rendered accidentally.
- The password-hashing scheme and the key encodings are still open parameters
  in `docs/specification-v1.md`; this module only carries those bytes and
  implements no crypto.

## Not in v1

File storage/proxying, Raft relaying, dynamic node registration, key rotation,
admin panel, and health/topology propagation to clients or storage nodes.
