# Dedicated server

The v1 dedicated server is the client's bootstrap/control-plane entry point and
source of the static cluster topology. It is **not** a file proxy, **not** a Raft
transport, and does **not** store photos or videos (see
`docs/specification-v1.md`, `docs/agent-dedicated-server.md`); clients get node
addresses, labels, and public keys here, then send files straight to storage.

## Current status

Only configuration loading and validation are implemented: the binary loads,
validates, and exits. Networking, registration, topology responses, and
heartbeats are not implemented yet.

## Build, test, and run

Run from `dedicated-server/` (`Cargo.lock` is committed):

```bash
cargo build --locked
cargo test --workspace --all-features
bash scripts/ci.sh
```

```bash
PICBACKUP_DEDICATED_SERVER_CONFIG=/path/to/config.toml cargo run --locked
```

Without that variable the binary reads `dedicated-server.toml` from the current
directory.

## Configuration schema

- Top level: `listen` (socket address string), `[identity]`, `[heartbeat]`,
  `[[cluster.nodes]]`.
- `identity`: `label`, `encryption_private_key`, `signing_private_key` (paths).
- `heartbeat`: `interval_ms` (`u64` > 0), `timeout_ms` (`u64` > 0),
  `miss_threshold` (`u32` > 0).
- `cluster.nodes[]`: `label` (non-empty, unique), `address` (unique socket
  address), `encryption_public_key`, `signing_public_key` (non-empty strings).
- Empty `cluster.nodes` is invalid; unknown keys are rejected.

Example, matching `tests/fixtures/valid_server.toml`:

```toml
listen = "10.0.0.1:7000"

[identity]
label = "dedicated-1"
encryption_private_key = "/etc/picbackup/dedicated/encryption.key"
signing_private_key = "/etc/picbackup/dedicated/signing.key"

[heartbeat]
interval_ms = 1000
timeout_ms = 500
miss_threshold = 3

[[cluster.nodes]]
label = "storage-1"
address = "10.0.0.11:7100"
encryption_public_key = "storage-1-encryption-public-key"
signing_public_key = "storage-1-signing-public-key"

[[cluster.nodes]]
label = "storage-2"
address = "10.0.0.12:7100"
encryption_public_key = "storage-2-encryption-public-key"
signing_public_key = "storage-2-signing-public-key"
```

## Not in v1

File storage/proxying, Raft relaying, dynamic node registration, key rotation,
admin panel, and health/topology propagation to clients or storage nodes.
