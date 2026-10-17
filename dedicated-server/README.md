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
and the key identity, logs a one-line startup summary to stderr, and exits.
Network I/O, the wire protocol, registration over the network, cluster
replication, and topology responses are not implemented yet.

## Build, test, and run

Run from `dedicated-server/` (`Cargo.lock` is committed):

```bash
cargo build --locked
cargo test --workspace --all-features
bash scripts/ci.sh
```

```bash
HOMECLOUD_DEDICATED_SERVER_CONFIG=/path/to/config.toml cargo run --locked
```

Without that variable the binary reads `dedicated-server.toml` from the current
directory.

The binary logs to stderr, so stdout stays empty for normal operation. The
default level is `info`; set `RUST_LOG` to override it (for example
`RUST_LOG=debug`). Log lines are emitted without ANSI colours. The binary
installs `mimalloc` as its process allocator instead of the system default.

The crate's modules are private; its public items are re-exported from the crate
root. Consumers therefore use `dedicated_server::` paths (for example
`dedicated_server::ServerConfig`) rather than module paths such as
`dedicated_server::config::ServerConfig`.

## Configuration schema

- Top level: `listen` (socket address string), `[identity]`, `[heartbeat]`,
  `[password]`, `[[cluster.nodes]]`.
- `identity`: `label`, `encryption_private_key`, `signing_private_key` (paths).
- `heartbeat`: `interval_ms` and `timeout_ms` are non-zero milliseconds
  (`NonZeroU64`), `miss_threshold` is a non-zero count (`NonZeroU32`). A zero
  value is rejected while the configuration is parsed, not by a later check.
- `password`: `memory_kib`, `iterations` and `parallelism` are non-zero
  (`NonZeroU32`), and `salt_length_bytes` is a non-zero byte count
  (`NonZeroUsize`). The Argon2id parameters and salt length used to derive
  password verifiers are configured here.
- `cluster.nodes[]`: `label` (non-empty, unique), `address` (unique socket
  address), `encryption_public_key`, `signing_public_key` (non-empty strings).
- Empty `cluster.nodes` is invalid; unknown keys are rejected.

Example, matching `tests/fixtures/valid_server.toml`:

```toml
listen = "10.0.0.1:7000"

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
fails when either file cannot be read, is empty, is larger than the 64 KiB
limit, or when the same material would serve both the decryption and the
signing role. At most 64 KiB plus one byte of each file is read, so an
oversized file is rejected without being loaded in full, and an identity failure
names both the configuration file and the offending key file. The configured
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
