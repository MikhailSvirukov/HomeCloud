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
- `crates/storage-node/` — the node crate, with `src/framing.rs` and
  `src/buffer_pool.rs`.
- `crates/storage-node/tests/public_api.rs` — public API integration tests.

## Status

Early bootstrap increment: framing and buffer-pool building blocks only. Networking,
actors, and `io_uring` are not implemented yet. Acceptance criteria live in the
[shared v1 specification](../../docs/specification-v1.md).

## Build and test

```sh
./ci/check.sh
```

The toolchain is pinned by `rust-toolchain.toml`.
