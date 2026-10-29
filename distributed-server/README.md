# Distributed storage nodes

Fixed multi-node cluster of Rust storage nodes. Metadata/journal consistency runs over
a direct encrypted node-to-node Raft channel, where each Raft RPC is carried in the
inter-server `[3, [rpc_bytes]]` envelope, while file blobs are replicated on a separate
data plane. The dedicated server is only a bootstrap/control-plane entry point for
clients; it does not relay Raft or blob traffic.

## Execution model

- One Rust process per node with an internal actor model.
- Tokio drives TCP connections and orchestration (`tokio-uring` is not used).
- A single dedicated OS thread per node owns `io_uring`, receives commands through a
  bounded channel, and never blocks Tokio workers with completion waits.
- Compression and crypto run on a bounded CPU pool outside Tokio worker threads.
- Custom bounded multi-size payload buffer pools with explicit ownership/return and
  backpressure instead of unbounded memory growth.
- The TCP connection actor only frames/deframes and forwards frames.

## Workspace layout

- `Cargo.toml` — workspace manifest.
- `crates/storage-node/` — the node crate, with `src/framing.rs`,
  `src/codec.rs` (whole-frame encode/decode over byte slices),
  `src/frame_buffer.rs` (receive-side frame accumulator: chunked bytes → whole
  frames), `src/connection.rs` (frame reader/writer halves plus `split_frames`
  and the `pump_frames`/`pump_halves` forwarding pumps),
  `src/buffer_pool.rs`,
  `src/payload.rs` (logical payload chain / chunked streaming
  with backpressure), `src/payload_stream.rs` (file payload streamed as bounded
  frames through the payload buffer pools), `src/config.rs` (node
  identity/configuration), `src/keys.rs` (private key material loaded from
  configured owner-only files), `src/response.rs` (typed upload outcomes / status
  codes), `src/metadata.rs` (idempotent upload ledger: upload id, content
  hash, upload record preserving the original source name exactly, with no
  trimming or normalization, plus original and compressed sizes), `src/catalog.rs`
  (catalog entry, page, and request CBOR codecs), `src/catalog_pager.rs`
  (snapshot pager that sorts entries by ascending upload id and pages them with
  CSPRNG snapshot tokens and cursors), `src/catalog_service.rs` (maps the
  committed Raft catalog to catalog entries and paginated pages using the
  configured page size, snapshot lifetime, and source-name limit),
  `src/catalog_error.rs` (maps catalog pager and service failures to the
  `0x0004_0001`, `0x0004_0002`, and `0x0008_0001` protocol error codes),
  `src/listener.rs` (binds
  the node's listen address
  and yields frame reader/writer halves per accepted TCP connection),
  `src/metrics.rs` (Prometheus-compatible metrics set and the loopback
  `GET /metrics` server),
  `src/cpu_pool.rs` (bounded CPU pool for compression/crypto work off the Tokio
  worker threads), `src/node.rs` (node runtime: serves frame connections on
  a bound listener until shutdown), `src/storage_layout.rs` (on-disk
  layout: `accounts/<hex-login>/<hex-upload-id>` plus `raft/`),
  `src/integrity.rs` (local blob integrity check that streams the stored blob,
  compares its SHA-256 with the committed hash, and keeps a `<blob>.corrupt`
  marker), `src/raft/`
  (local OpenRaft runtime, CBOR RPC adapter, and catalog state machine that
  stores full version metadata and blob replica sets with file-backed
  persistence), `src/message.rs` (inter-server `[message_type,
  args]` envelope with `message_type = 3` for Raft RPCs), `src/signing.rs`
  (Ed25519 message signing/verification with a redacted signing key), and
  `src/sealing.rs` (HPKE Base-mode message sealing with X25519, HKDF-SHA-256,
  and AES-256-GCM).
- `crates/storage-node/tests/` — public API integration tests
  (`public_api.rs`, `codec_public_api.rs`, `frame_buffer_public_api.rs`,
  `connection_public_api.rs`, `payload_public_api.rs`,
  `payload_stream_public_api.rs`, `config_public_api.rs`,
  `keys_public_api.rs`, `content_hash_public_api.rs`,
  `upload_ledger_public_api.rs`, `catalog_public_api.rs`,
  `catalog_pager_public_api.rs`, `catalog_service_public_api.rs`,
  `catalog_error_codes_public_api.rs`,
  `upload_outcomes_public_api.rs`,
  `listener_public_api.rs`, `metrics_public_api.rs`, `cpu_pool_public_api.rs`,
  `node_public_api.rs`,
  `raft_cluster_public_api.rs`, `storage_layout_public_api.rs`,
  `integrity_public_api.rs`, `signing_public_api.rs`, `sealing_public_api.rs`,
  `message_public_api.rs`,
  `e2e_pump_over_tcp.rs`, `e2e_upload_over_tcp.rs`).

## Status

Early bootstrap increment: framing, the frame codec (whole-frame encode/decode
over byte slices), the receive-side frame buffer (chunked bytes → whole frames),
the frame reader/writer halves that move whole frames over an async byte stream
with `split_frames` and the `pump_frames`/`pump_halves` forwarding pumps, buffer-pool building
blocks, the logical payload chain with chunked streaming/backpressure, the
payload streaming path that frames pooled chunks onto the connection writer, the
node identity/configuration model, the idempotent upload ledger (upload id, content
hash that computes a SHA-256 digest of the original uncompressed bytes, upload
record storing the original and compressed sizes), the catalog entry, page, and
request CBOR codecs, the catalog pager that returns entries in ascending upload-id
order, keeps one CSPRNG snapshot token per catalog snapshot, and issues CSPRNG
cursors that are only valid together with that token, with the page size and
snapshot lifetime configured through the pager constructor and expired snapshots
evicted when a new snapshot starts, the catalog service that maps the committed
Raft catalog to paginated catalog pages using the configured page size, snapshot
lifetime, and source-name limit, the typed catalog error codes that map pager
and service failures to the `0x0004_0001`, `0x0004_0002`, and `0x0008_0001`
protocol table entries, the typed upload
outcomes (status codes), and the TCP
listener that binds the node's listen address and hands back frame reader/writer
halves for each accepted connection, the bounded CPU pool that runs
compression/crypto work off the Tokio worker threads, and the node runtime that
serves those connections until shutdown; the `storage-node` binary now binds the
configured listen address and serves connections. Tokio is now a dependency for
the async connection halves, and `tracing` backs the structured logs the node
emits with a level configured through `RUST_LOG`. The local OpenRaft state
(`state.cbor`) and the bootstrap marker are persisted under the configured node
data directory, so a restarted node restores its applied catalog and rejects a
repeated bootstrap command. The applied Raft catalog stores the full version
metadata (upload id, login, original file name, original and compressed byte
sizes, SHA-256 of the original bytes, capture/creation time, and acceptance
time) together with the
set of storage nodes that durably hold the blob. A committed version's blob
replica set can be replaced through a Raft update, and the new set is applied on
every node with the catalog. The persisted Raft state file records a format
version. `raft_cluster_public_api.rs` covers restart recovery, blob replica-set
updates, and a returning node catching up committed entries. A local committed
blob is checked by streaming it, comparing its SHA-256 with the committed hash,
and creating a `<blob>.corrupt` marker when the hashes differ; a later check
that matches the committed hash removes the marker. `integrity_public_api.rs`
covers the healthy, mismatching, restored, and missing paths. The connection
actors and `io_uring` are not implemented yet.
Acceptance criteria live in the
[shared v1 specification](../../docs/specification-v1.md).

## Configuration

The `storage-node` binary reads and validates its configuration from the environment
at startup:

- `NODE_ID` — stable node label (maps to the `node_id` key).
- `LISTEN_ADDR` — `host:port` listen address (maps to the `listen_addr` key).
- `DATA_DIR` — data directory (maps to the `data_dir` key).
- `ENCRYPTION_KEY_PATH` — private encryption key file (maps to the
  `encryption_key_path` key).
- `SIGNING_KEY_PATH` — private signing key file (maps to the `signing_key_path` key).
- `MAX_FRAME_LEN` — maximum frame size in bytes; must be non-zero (maps to the
  `max_frame_len` key).
- `QUEUE_CAPACITY` — per-connection inter-actor queue capacity; must be non-zero (maps
  to the `queue_capacity` key).
- `CATALOG_PAGE_SIZE` — maximum number of catalog entries per page; must be non-zero
  (maps to the `catalog_page_size` key).
- `CATALOG_SNAPSHOT_TTL_MS` — catalog snapshot lifetime in milliseconds; must be non-zero
  (maps to the `catalog_snapshot_ttl_ms` key).
- `MAX_SOURCE_NAME_LEN` — maximum `source_name` length in UTF-8 bytes; must be non-zero
  (maps to the `max_source_name_len` key).
- `METRICS_BIND_ADDR` — loopback `SocketAddr` for the Prometheus-compatible metrics
  endpoint; required, with no default (maps to the `metrics_bind_addr` key). The endpoint
  is served over plain HTTP and rejects any non-loopback bind address.
- `LOG_FILE_PATH` — local path of the structured JSON Lines log (maps to the
  `log_file_path` key).
- `LOG_QUEUE_CAPACITY` — bounded structured-log queue capacity in entries; must be
  non-zero (maps to the `log_queue_capacity` key).
- `LOG_MAX_FILE_BYTES` — maximum size of one log file in bytes; must be non-zero (maps
  to the `log_max_file_bytes` key).
- `LOG_MAX_TOTAL_BYTES` — maximum combined size of the current and rotated log files in
  bytes; must be non-zero and at least `LOG_MAX_FILE_BYTES` (maps to the
  `log_max_total_bytes` key).
- `LOG_MAX_FILE_AGE_SECONDS` — maximum age of one log file before rotation, in seconds;
  must be non-zero (maps to the `log_max_file_age_seconds` key).
- `PEERS` — comma-separated peer addresses (maps to the `peers` key).

All five `LOG_*` settings are required and non-zero.

`NodeConfig::from_pairs` accepts the same `node_id`, `listen_addr`, `data_dir`,
`encryption_key_path`, `signing_key_path`, `max_frame_len`, `queue_capacity`,
`catalog_page_size`, `catalog_snapshot_ttl_ms`, `max_source_name_len`,
`metrics_bind_addr`, `log_file_path`, `log_queue_capacity`, `log_max_file_bytes`,
`log_max_total_bytes`, `log_max_file_age_seconds`, and `peers` keys;
an absent or empty value for any required key is reported as missing, and unknown keys are
ignored.
Frames use a 4-byte big-endian length prefix. The prefix itself is always 4 bytes; the
value it encodes is checked against the configured maximum before the frame is buffered,
and a larger value is rejected.

Startup loads both private key files after the configuration is validated and before
the listener binds, so a missing or unreadable key stops the process without opening a
port.

## Metrics

The node exposes a Prometheus-compatible metrics endpoint on `METRICS_BIND_ADDR`.
`GET /metrics` returns the Prometheus text exposition format (`text/plain;
version=0.0.4; charset=utf-8`) with the node liveness, build-info, queue-capacity, and
scrape counters. Labels are limited to bounded static values such as the build version;
they never contain logins, `request_id`, `upload_id`, file names, or other per-request
values. Requests to other paths return `404`, and methods other than `GET` return `405`
with `Allow: GET`. The metrics interface is separate from the application TCP protocol.

## Logging

The node writes UTF-8 JSON Lines to `LOG_FILE_PATH`, one JSON object per line. Every
record carries `timestamp_ms` in UTC, `component`, `level`, and `event_type`, plus the
optional operation, peer, request, upload, outcome, error, duration, and state-change
fields that apply to that event. Request acceptance and completion records share a
`request_id`, and the completion record carries the processing duration. Connections
or frames rejected before a request id exists, such as a frame length above
`MAX_FRAME_LEN`, are recorded as `connection_rejected` events with the peer identifier
and an error category.

Events pass through a bounded queue to a non-blocking writer, so the request path never
performs a synchronous file write. When the queue is full or the writer fails, the event
is dropped and the corresponding dropped-event or write-error counter is incremented.
The current file is `LOG_FILE_PATH`; rotated files are named `<LOG_FILE_PATH>.<suffix>`.
The writer rotates a file when the next record would exceed `LOG_MAX_FILE_BYTES` or the
file reaches `LOG_MAX_FILE_AGE_SECONDS`, then removes aged and oldest rotated files so
the combined size stays within `LOG_MAX_TOTAL_BYTES`.

Log records exclude passwords, verifiers, cryptographic keys, file contents and names,
payloads, logins, and other user metadata. Allowed diagnostic identifiers are
`request_id`, `upload_id`, peer identifiers, operation type, result code, and error
category.

## Build and test

The CI script is run by GitHub Actions for pull requests. OpenCode agents add
tests but do not execute test suites locally; use the PR checks as the result.

```sh
./ci/check.sh
```

The toolchain is pinned by `rust-toolchain.toml`.
