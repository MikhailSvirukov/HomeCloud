# Distributed storage nodes

Fixed multi-node cluster of Rust storage nodes. Metadata/journal consistency runs over
a direct encrypted node-to-node Raft channel, while file blobs are replicated on a
separate data plane. The dedicated server is only a bootstrap/control-plane entry point
for clients; it does not relay Raft or blob traffic.

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
  and the `pump_frames` forwarding pump),
  `src/buffer_pool.rs`,
  `src/payload.rs` (logical payload chain / chunked streaming
  with backpressure), `src/payload_stream.rs` (file payload streamed as bounded
  frames through the payload buffer pools), `src/config.rs` (node
  identity/configuration), `src/response.rs` (typed upload outcomes / status
  codes), `src/metadata.rs` (idempotent upload ledger: upload id, content
  hash, upload record), `src/listener.rs` (binds the node's listen address
  and yields frame reader/writer halves per accepted TCP connection),
  `src/cpu_pool.rs` (bounded CPU pool for compression/crypto work off the Tokio
  worker threads), and `src/node.rs` (node runtime: serves frame connections on
  a bound listener until shutdown).
- `crates/storage-node/tests/` — public API integration tests
  (`public_api.rs`, `codec_public_api.rs`, `frame_buffer_public_api.rs`,
  `connection_public_api.rs`, `payload_public_api.rs`,
  `payload_stream_public_api.rs`, `config_public_api.rs`,
  `upload_ledger_public_api.rs`, `upload_outcomes_public_api.rs`,
  `listener_public_api.rs`, `cpu_pool_public_api.rs`, `node_public_api.rs`).

## Status

Early bootstrap increment: framing, the frame codec (whole-frame encode/decode
over byte slices), the receive-side frame buffer (chunked bytes → whole frames),
the frame reader/writer halves that move whole frames over an async byte stream
with `split_frames` and the `pump_frames` forwarding pump, buffer-pool building
blocks, the logical payload chain with chunked streaming/backpressure, the
payload streaming path that frames pooled chunks onto the connection writer, the
node identity/configuration model, the idempotent upload ledger (upload id, content
hash, upload record), the typed upload outcomes (status codes), and the TCP
listener that binds the node's listen address and hands back frame reader/writer
halves for each accepted connection, the bounded CPU pool that runs
compression/crypto work off the Tokio worker threads, and the node runtime that
serves those connections until shutdown; the `storage-node` binary now binds the
configured listen address and serves connections. Tokio is now a dependency for
the async connection halves, and `tracing` backs the structured logs the node
emits with a level configured through `RUST_LOG`. The connection actors and
`io_uring` are not implemented yet.
Acceptance criteria live in the
[shared v1 specification](../../docs/specification-v1.md).

## Configuration

The `storage-node` binary reads and validates its configuration from the environment
at startup:

- `NODE_ID` — stable node label (maps to the `node_id` key).
- `LISTEN_ADDR` — `host:port` listen address (maps to the `listen_addr` key).
- `DATA_DIR` — data directory (maps to the `data_dir` key).
- `PEERS` — comma-separated peer addresses (maps to the `peers` key).

`NodeConfig::from_pairs` accepts the same `node_id`, `listen_addr`, `data_dir`, and
`peers` keys; unknown keys are ignored.

## Build and test

```sh
./ci/check.sh
```

The toolchain is pinned by `rust-toolchain.toml`.
