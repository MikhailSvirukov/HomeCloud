# Dedicated server

The v1 dedicated server is the client's bootstrap/control-plane entry point and
source of the static cluster topology. It is **not** a file proxy, **not** a Raft
transport, and does **not** store photos or videos (see
`docs/specification-v1.md`, `docs/agent-dedicated-server.md`); clients get node
addresses, labels, and public keys here, then send files straight to storage.

## Current status

Configuration loading and validation, key-identity loading for the server's own
key pair, internal heartbeat tracking for the fixed cluster, and the in-memory
account registration model are implemented. The binary loads the configuration
and the key identity, prints a one-line startup summary, and exits. Network I/O,
the wire protocol, registration over the network, cluster replication, and
topology responses are not implemented yet.

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

## Key identity

`src/identity.rs` loads the server's own key pair for startup. The identity is
read from the two private-key files named by `identity.encryption_private_key`
and `identity.signing_private_key`; the bytes stay opaque because key encoding
and algorithms are still open parameters in `docs/specification-v1.md`. Loading
fails when either file cannot be read, is empty, is larger than `MAX_KEY_BYTES`
(64 KiB), or when the same material would serve both the decryption and the
signing role. At most `MAX_KEY_BYTES + 1` bytes of each file are read, so an
oversized file is rejected without being loaded in full, and an identity failure
names both the configuration file and the offending key file. The configured
`identity.label` is bound to the loaded identity and is used as the node's
sender label. `ServerIdentity`'s `Debug` output deliberately reports only the
label and key lengths, never the key bytes.

## Heartbeat tracking

`src/heartbeat.rs` holds the scheduling and consecutive-miss bookkeeping for the
dedicated server's own heartbeat exchanges with the fixed cluster. It is a pure
state machine: callers drive it with explicit `Instant` values, and it uses no
timers, no I/O, and no async work.

- Each node's next attempt is scheduled one `heartbeat.interval_ms` after a
  response or a missed attempt; an in-flight attempt that is not answered within
  `heartbeat.timeout_ms` counts as a miss.
- A node is marked internally unavailable after `heartbeat.miss_threshold`
  consecutive misses; a response clears the counter and marks it healthy again.
- Per `docs/specification-v1.md`, this availability state stays internal to the
  dedicated server and is never propagated to clients or storage nodes.

The tracker only produces send/timeout actions; delivering heartbeats over the
network is not implemented yet.

## Account registration

`src/registration.rs` holds the in-memory account model behind client
registration. It is pure state: no I/O, not async, and no cryptography.

- `Login` is a validated, cluster-unique identifier. It must be non-empty, at
  most `MAX_LOGIN_LENGTH` (64) characters, and use only the conservative ASCII
  subset `[A-Za-z0-9._-]`.
- `RegistrationRequest` carries the login plus two opaque byte strings: the
  password verifier and the client's encryption public key.
- `AccountRegistry` stores `AccountRecord`s. Registering an already-taken login
  is rejected and leaves the registry unchanged. `records()` yields accounts in
  login order so replication to the cluster is deterministic.
- The password-hashing scheme and the key encodings are still open parameters
  in `docs/specification-v1.md`; this module only carries those bytes and
  implements no crypto.

## Not in v1

File storage/proxying, Raft relaying, dynamic node registration, key rotation,
admin panel, and health/topology propagation to clients or storage nodes.
