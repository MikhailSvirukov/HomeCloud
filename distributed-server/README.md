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
  `src/catalog_exchange.rs` (client catalog page exchange codecs for the
  seven-position external header, the operation-3 request, and the result
  envelope), `src/upload_exchange.rs` (operation-4 upload request codec with the
  five upload arguments and the source-name and file-size limits),
  `src/upload_error.rs` (typed upload protocol error codes that map upload
  request, blob-write, integrity, and frame-sequence failures to the
  `INT-NET-064` table entries, including storage-capacity exhaustion on disk-full
  writes), `src/catalog_handler.rs` (maps first-page and continuation
  requests to catalog pages and encodes the result envelope),
  `src/listener.rs` (binds
  the node's listen address
  and yields frame reader/writer halves per accepted TCP connection),
  `src/metrics.rs` (Prometheus-compatible metrics set and the HTTP/TLS
  `GET /metrics` server),
  `src/cpu_pool.rs` (bounded CPU pool for compression/crypto work off the Tokio
  worker threads), `src/node.rs` (node runtime: serves frame connections on
  a bound listener until shutdown), `src/storage_layout.rs` (on-disk
  layout: `accounts/<hex-login>/<hex-upload-id>` plus `raft/`),
  `src/integrity.rs` (local blob integrity check that streams the stored blob,
  compares its SHA-256 with the committed hash, and keeps a `<blob>.corrupt`
  marker), `src/integrity_scan.rs` (periodic scan over committed blobs with a
  bounded number of concurrent checks), `src/blob_writer.rs` (writes a received
  blob directly into its final path, verifying the declared byte size and
  SHA-256 before syncing the file and its parent directory),
  `src/blob_restore.rs` (restores a missing or marked-corrupt blob through the
  same verified durable write, refusing an unmarked local copy), `src/raft/`
  (local OpenRaft runtime, CBOR RPC adapter, and catalog state machine that
  stores full version metadata and blob replica sets with file-backed
  persistence), `src/frame_sequence.rs` (contiguous client frame-number
  validation: the sequence starts at `0`, advances by one, and never wraps
  after `u32::MAX`), `src/version_negotiation.rs` (CBOR codec for supported
  version lists and negotiation that selects protocol version `1`),
  `src/message.rs` (inter-server `[message_type,
  args]` envelope with `message_type = 3` for Raft RPCs), `src/signing.rs`
  (Ed25519 message signing/verification with a redacted signing key),
  `src/sealing.rs` (HPKE Base-mode message sealing with X25519, HKDF-SHA-256,
  and AES-256-GCM), `src/signed_frame.rs` (signed application frame body
  with the length prefix, external header, Ed25519 signature, opaque payload,
  and first-contact bootstrap payload codecs), and `src/server_exchange.rs`
  (encrypted signed inter-server message composition that seals the plaintext
  for the recipient before signing the frame and verifies the signature before
  opening it).
- `crates/storage-node/tests/` — public API integration tests
  (`public_api.rs`, `codec_public_api.rs`, `frame_buffer_public_api.rs`,
  `frame_validation_public_api.rs`,
  `connection_public_api.rs`, `payload_public_api.rs`,
  `payload_stream_public_api.rs`, `config_public_api.rs`,
  `keys_public_api.rs`, `content_hash_public_api.rs`,
  `upload_ledger_public_api.rs`, `catalog_public_api.rs`,
  `catalog_pager_public_api.rs`, `catalog_service_public_api.rs`,
  `catalog_error_codes_public_api.rs`,
  `catalog_exchange_public_api.rs`, `catalog_handler_public_api.rs`,
  `upload_exchange_public_api.rs`, `upload_outcomes_public_api.rs`,
  `upload_error_codes_public_api.rs`,
  `listener_public_api.rs`, `metrics_public_api.rs`,
  `metrics_tls_public_api.rs`, `cpu_pool_public_api.rs`,
  `node_public_api.rs`,
  `raft_cluster_public_api.rs`, `storage_layout_public_api.rs`,
  `integrity_public_api.rs`, `integrity_scan_public_api.rs`,
  `blob_writer_public_api.rs`, `blob_restore_public_api.rs`,
  `signing_public_api.rs`, `sealing_public_api.rs`,
  `signed_frame_public_api.rs`, `server_exchange_public_api.rs`,
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
protocol table entries, the upload request codec for operation `4` with its five
argument positions, exact source-name preservation, and name/size limits, the typed
upload outcomes (status codes), the typed upload protocol error codes that map
upload request, blob-write, integrity, and frame-sequence failures to their
`INT-NET-064` table entries (disk-full writes map to storage-capacity
exhaustion), and the TCP
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
covers the healthy, mismatching, restored, and missing paths. A received blob
is written directly into its final path with declared-size and SHA-256
verification, file and parent-directory fsync, refusal to overwrite, and
partial-file cleanup. A missing or marked-corrupt blob is restored from a
repair source through the same verified durable write, while an unmarked
existing blob is refused; `blob_restore_public_api.rs` covers those paths and
the marker kept by a failed restore. The connection actors and `io_uring` are not
implemented yet. Acceptance criteria live in the
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
- `LOGIN_MIN_LEN` — minimum client login length in UTF-8 bytes; must be non-zero (maps
  to the `login_min_len` key).
- `LOGIN_MAX_LEN` — maximum client login length in UTF-8 bytes; must be non-zero and at
  least `LOGIN_MIN_LEN` (maps to the `login_max_len` key).
- `PASSWORD_MIN_LEN` — minimum client password length in UTF-8 bytes; must be non-zero
  (maps to the `password_min_len` key).
- `PASSWORD_MAX_LEN` — maximum client password length in UTF-8 bytes; must be non-zero
  and at least `PASSWORD_MIN_LEN` (maps to the `password_max_len` key).
- `RESPONDER_LABEL_MAX_LEN` — maximum responder-label length in UTF-8 bytes; must be
  non-zero (maps to the `responder_label_max_len` key).
- `INTEGRITY_SCAN_PERIOD_SECONDS` — delay between full scans of committed blob data, in
  seconds; must be non-zero (maps to the `integrity_scan_period_seconds` key).
- `INTEGRITY_SCAN_MAX_CONCURRENT` — maximum number of blobs checked simultaneously
  during one scan; must be non-zero (maps to the `integrity_scan_max_concurrent` key).
- `METRICS_BIND_ADDR` — `SocketAddr` for the Prometheus-compatible metrics endpoint;
  required, with no default (maps to the `metrics_bind_addr` key). A loopback address may
  be served over plain HTTP or TLS; a non-loopback address requires both metrics TLS
  paths.
- `METRICS_TLS_CERT_PATH` — PEM server certificate chain for the metrics endpoint (maps
  to the `metrics_tls_cert_path` key). Required together with `METRICS_TLS_KEY_PATH`; the
  configured certificate is the trust material a scraper uses to verify the server.
- `METRICS_TLS_KEY_PATH` — PEM private key matching `METRICS_TLS_CERT_PATH` (maps to the
  `metrics_tls_key_path` key). Required together with `METRICS_TLS_CERT_PATH`.
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

The five client-limit settings are also required and non-zero, and each minimum must not
exceed its maximum. The catalog and upload client-header codecs enforce the configured
login and password byte ranges, and the catalog codec enforces the responder-label byte
limit.

`NodeConfig::from_pairs` accepts the same `node_id`, `listen_addr`, `data_dir`,
`encryption_key_path`, `signing_key_path`, `max_frame_len`, `queue_capacity`,
`catalog_page_size`, `catalog_snapshot_ttl_ms`, `max_source_name_len`,
`login_min_len`, `login_max_len`, `password_min_len`, `password_max_len`,
`responder_label_max_len`,
`integrity_scan_period_seconds`, `integrity_scan_max_concurrent`,
`metrics_bind_addr`, `metrics_tls_cert_path`, `metrics_tls_key_path`,
`log_file_path`, `log_queue_capacity`, `log_max_file_bytes`,
`log_max_total_bytes`, `log_max_file_age_seconds`, and `peers` keys;
an absent or empty value for any required key is reported as missing, and unknown keys are
ignored.
Frames use a 4-byte big-endian length prefix. The prefix itself is always 4 bytes; the
value it encodes is checked against the configured maximum before the frame is buffered,
and a larger value is rejected.

Startup loads both private key files after the configuration is validated and before
the listener binds, so a missing or unreadable key stops the process without opening a
port.

## Catalog page exchange

A catalog page request goes directly from the client to a storage node as operation `3`.
Its external header has seven positions in this order: `protocol_version` (`1`),
`sender_id`, `request_id`, `frame_number`, `upload_id`, `responder_label`, and
`bootstrap_signing_public_key`. A catalog request carries `upload_id = null` and
`responder_label = null`; the bootstrap key is `null` for every message except primary
registration, where `sender_id` is `null` and the key is the installation's open
Ed25519 key. The logical header is `[3, login, password, [snapshot_token, cursor]]`,
where both arguments are `null` on the first page and the unchanged 16-byte tokens from
the previous response on a continuation page.

The response repeats the request's `request_id`, carries `upload_id = null` and the
responding storage node's public label in `responder_label`, and uses the logical result
envelope `[result_code, result_args_array]`. A successful page (code `0`) carries
`[snapshot_token, next_cursor, entries]`; every non-zero code carries an empty argument
array. The codec validates the header positions and null rules, the operation code, the
text types of the login and password, the token lengths, and the success and failure
argument shapes. The catalog page exchange codec does not sign or encrypt; the
client-header codecs enforce the configured login, password, and responder-label byte
limits, and signed application frames are described below.

The catalog page handler maps a first-page request (`[null, null]`) to the first
page of a new snapshot and a continuation request (`[snapshot_token, cursor]`) to
the corresponding page of that snapshot. A successful page is encoded with result
code `0` and `[snapshot_token, next_cursor, entries]`; an invalid cursor uses
`0x0004_0001` with an empty argument array, and an unknown or expired snapshot
uses `0x0004_0002` with an empty argument array. Credential checks and transport
handling are separate from the handler.

## Upload request exchange

A media upload goes directly from the client to a storage node as operation `4`. Its
logical header is `[4, login, password, [source_name, timestamp_ms,
original_size_bytes, compressed_size_bytes, sha256]]`. The operation code is encoded as
a minimal `uint32`; an unknown operation code is rejected. The login and password are
CBOR text strings, and the login must be non-empty and use the permitted ASCII set.

The upload argument array has exactly five positions in this order:

- `source_name` — the original media file name as a UTF-8 CBOR text string. It is kept
  exactly as received, without trimming spaces or Unicode normalization, and is
  compared by its exact Unicode content. It must be non-empty and no longer than
  `MAX_SOURCE_NAME_LEN` UTF-8 bytes.
- `timestamp_ms` — the upload time as a signed 64-bit CBOR integer.
- `original_size_bytes` — the original uncompressed size as an unsigned CBOR integer.
- `compressed_size_bytes` — the compressed size as an unsigned CBOR integer.
- `sha256` — the SHA-256 digest of the original bytes as a 32-byte CBOR byte string.

Both sizes must be no larger than the maximum file size passed to the codec along with
the source-name limit. The codec rejects wrong top-level or argument array lengths,
wrong value types, invalid logins, empty or over-long source names, sizes above the
limit, hashes that are not 32 bytes, and trailing bytes. Signing, encryption, and
credential checks are separate from the codec.

## Frame validation and version negotiation

Frame numbers of one client sequence start at `0`; each following frame increases
the number by exactly one. A gap, a repeat, or a number after `u32::MAX` is
reported as a sequence error, so a sequence never wraps around and the `uint32`
space is finite. The check is stateful per sequence and does not accept a
rejected number as if it had advanced the sequence; wiring the validator into the
frame path is a later increment.

Connection negotiation uses the one-element version list `[1]`
(`SUPPORTED_VERSION` is `1`). The list encodes as the CBOR array `[1]`
(`0x81 0x01`). A well-formed remote list that contains `1` selects version `1`,
even when it also offers other versions. A list with no version `1`, including an
empty list, is reported as having no common version; the connection layer that
closes on this result is a later increment. The list decoder rejects values that are
not a CBOR array, elements that are not `uint32` values, and trailing bytes. It
keeps well-formed entries for other version numbers, so support is decided by
negotiation rather than by the decoder.

## Signed application frames

A signed application frame body is the bytes after the 4-byte big-endian length prefix.
The prefix value is the body length and equals the sum of the encoded external header,
the 64-byte Ed25519 signature, and the payload. The body order is fixed: external header,
signature, payload. The external header keeps its seven positions from the catalog page
exchange. The signature covers the 4 prefix bytes, every encoded header byte, and the
payload; it does not cover itself.

A receiver first checks that the declared body length does not exceed the configured
maximum and matches the received body, then decodes the header, then verifies the
signature over the prefix, header, and payload. The payload is not returned or decrypted
until the signature verifies. A declared body length above the maximum is rejected as
`FrameTooLarge`; a frame shorter than the prefix is rejected as `TooShort`; a declared
length that differs from the body is rejected as `LengthMismatch`; and a signature that
does not verify over the prefix, header, and payload, including one checked with another
verifying key, is rejected as `InvalidSignature`.

The only unencrypted payload is the first-contact bootstrap payload: the CBOR array
`[x25519_public_key, ed25519_public_key]` of two 32-byte CBOR byte strings, encoded as
`0x82 0x58 0x20` followed by the 32 X25519 bytes, then `0x58 0x20` followed by the 32
Ed25519 bytes. The frame is signed with the Ed25519 key whose public key it carries, so a
valid signature proves key possession rather than server identity. The codec also rejects
a bootstrap payload with a different arity or key length and any trailing bytes.

## Inter-server message exchange

`encode_server_message` composes an inter-server message as an ordinary signed
application frame whose payload is sealed for the recipient. The plaintext is the
`[message_type, args]` logical header of the message type, such as `[3, [rpc_bytes]]`
for a Raft RPC. The plaintext is sealed with HPKE Base mode
(DHKEM(X25519, HKDF-SHA-256), HKDF-SHA-256, and AES-256-GCM) using empty `info` and
`aad`, so the payload is the 32-byte encapsulated key followed by the AES-256-GCM
ciphertext and tag. A fresh HPKE encapsulation is created for each call.

`decode_server_message` first verifies the Ed25519 frame signature and only then
opens the sealed payload. The caller supplies the sender verifying key that it
resolved from the trusted peer registry for the message's sender; the codec does
not derive or trust a key from the frame itself. A key that does not match the
sender produces a frame `InvalidSignature` error before any payload is decrypted,
and a wrong recipient private key produces `SealingError::Open`. The returned
message carries the verified external header and the decrypted plaintext, and its
Debug form reports the plaintext length instead of the bytes.

## Metrics

The node exposes a Prometheus-compatible metrics endpoint on `METRICS_BIND_ADDR`.
A loopback bind address is served over plain HTTP, or over TLS when both metrics TLS
paths are configured. Any non-loopback bind address requires both paths, and the
configured certificate is the trust material for scrapers, so a scraper without it must
reject the server certificate. TLS and plaintext serving return the same bounded metric
set.
`GET /metrics` returns the Prometheus text exposition format (`text/plain;
version=0.0.4; charset=utf-8`) with the node liveness, build-info, queue-capacity, and
scrape counters, plus the `homecloud_storage_bytes_used` gauge. The gauge is the current
total size of the regular files under the accounts and Raft roots, refreshed periodically.
Labels are limited to bounded static values such as the build version;
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

## Blob writing

`write_blob_direct` writes a received blob straight into its final path, creating the
parent directories first. An existing blob is never replaced: the write fails with
`AlreadyExists` and the original bytes stay in place. The streamed bytes are compared
with the declared size and SHA-256, and either mismatch removes the partially written
file so rejected data is not left on disk. On success the file and its parent directory
are synced, so the content and the metadata needed to discover it survive a crash. The
returned `StoredBlob` carries the final path, byte size, and content hash. The write
does not by itself publish the version; the catalog includes it only after the Raft
metadata commit.

## Blob restore

`restore_blob_direct` replaces a missing or marked-corrupt local blob with bytes
streamed from a repair source. An existing blob without a corruption marker is
never replaced: the call fails with `ExistingBlob` and the local bytes stay
untouched. A restored blob counts as repaired only after the same verified,
durable write used for received blobs accepts it: the streamed SHA-256 and byte
size must match the expected values, and the file and its parent directory are
synced before success is reported. A successful restore removes the
`<blob>.corrupt` marker; a failed restore writes the marker, removes the
rejected partial blob, and leaves the copy marked until a later restore
succeeds.

## Integrity scan

The integrity scanner walks the committed versions supplied by its source on the
configured period. Each scan streams the local blob and compares its SHA-256 with
the hash in the committed metadata, checking at most
`INTEGRITY_SCAN_MAX_CONCURRENT` blobs at once. The source must yield only versions
held by this node. Every scan produces a report with the number of digest-checked
blobs, the upload ids whose local bytes differ from the committed hash, the
upload ids whose file is absent, and the number of layout, permit, task-join,
read, or marker failures. A mismatch creates the `<blob>.corrupt` marker for
that copy; a later matching check removes it. Missing blobs and errors are
reported without stopping the rest of the scan or the next period. The scan loop
keeps running until its shutdown signal resolves. The node runtime does not start
the loop yet; it becomes active when the runtime exposes the committed catalog
source.

## Build and test

The CI script is run by GitHub Actions for pull requests. OpenCode agents add
tests but do not execute test suites locally; use the PR checks as the result.

```sh
./ci/check.sh
```

The toolchain is pinned by `rust-toolchain.toml`.
