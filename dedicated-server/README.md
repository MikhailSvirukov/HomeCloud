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
and the key identity, logs a structured startup event to stderr, and exits.
Network I/O, the wire protocol, registration over the network, cluster
replication, and topology responses are not implemented yet.

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

## Logging

The binary writes UTF-8 JSON Lines events to stderr: each line is one JSON
object and one event. stdout stays empty. The default level is `info`; set
`RUST_LOG` to override it (for example `RUST_LOG=debug`). Lines are emitted
without ANSI colours.

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
`failure`, an `error_code` of `configuration_error` or `identity_error`, and a
fixed stage-specific `message`; the failure event is the last record. Records
never contain file paths or names, logins, cryptographic keys, verifiers,
payloads or other user metadata.

## Configuration schema

- Top level: `listen` (socket address string), `max_label_bytes`, `[identity]`,
  `[heartbeat]`, `[password]`, `[[cluster.nodes]]`.
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
- `cluster.nodes[]`: `sender_id` (unique 16-byte node identifier), `label`
  (non-empty, unique), `address` (unique socket address),
  `encryption_public_key`, `signing_public_key`. Node identifiers and public
  keys are 16- and 32-byte values written in hexadecimal.
- Empty `cluster.nodes` is invalid; unknown keys are rejected.

Example, matching `tests/fixtures/valid_server.toml`:

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
identity error names both the configuration file and the offending key file;
log records omit both. The configured
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
