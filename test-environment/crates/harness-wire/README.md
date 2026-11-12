# harness-wire

Test-environment conformance oracle and shared golden-vector source for the
published v1 wire contract in [`contracts/wire-v1.md`](../../../contracts/wire-v1.md).
The crate is dependency-free apart from `thiserror`; it performs no network,
cryptographic, or configuration work.

## Requirements covered

- `TST-019 / 1`, `TST-022 / 1`, `TST-030 / 1`, `TST-032 / 1` — the golden
  vectors and reserved-code classes used to verify every implementation.
- `INT-NET-002/1`, `INT-NET-003/1`, `INT-NET-004/1`, `INT-NET-005/1`,
  `INT-NET-006/2`, `INT-NET-008/2`, `INT-NET-047/2`, `INT-NET-049/1`,
  `INT-NET-050/4`, `INT-NET-061/1`, `INT-NET-062/1`, `INT-NET-063/1`,
  `INT-NET-069/1`, `INT-NET-081/1` — the frame, external header, and result
  envelope representation exercised by those vectors.
- `INT-NET-072/2`, `INT-NET-073/6`, `INT-NET-074/2`, `INT-NET-075/1` and
  `TST-035/2`, `TST-036/5`, `TST-037/2`, `TST-038/1` — the inter-server
  envelope, heartbeat messages, RPC OpenRaft wrapper, and message-type registry.
- `INT-NET-070/6`, `TST-023/5`, `TST-033/1`, `CFG-CL-031` — the topology
  answer, its per-node rows, manifest order, byte-bounded labels, and canonical
  host literals.
- `FR-CAT-012/3`, `FR-CAT-013/2`, `INT-NET-080/1` and `TST-025/2`, `TST-026/2`,
  `TST-028/1`, `TST-034/1`, `CFG-CL-032` — the catalog entry row, page answer,
  first and continuation page requests, and byte-bounded source names.

## API

- `harness_wire::cbor` — `Value`, `CborError`, `encode`, `decode`; strict
  deterministic CBOR (minimal integers, definite lengths, no maps or tags).
- `harness_wire::header` — `ExternalFrameHeader`, `HeaderError`; the
  seven-position array fixed by `INT-NET-050 / 4`.
- `harness_wire::frame` — `Frame`, `FrameError`, `encode`, `decode`,
  `signed_message`; the `[length][header][signature][payload]` layout.
- `harness_wire::result` — `ResultEnvelope`, `ResultClass`, `class_of`, the 18
  published result-code constants, and `ResultError`.
- `harness_wire::inter_server` — `InterServerMessage`, `MessageType`,
  `MessageError`, `V1_REGISTRY`, `validate_registry`, `RegistryError`, and the
  `MESSAGE_TYPE_*` code constants; the `[message_type, args]` envelope.
- `harness_wire::heartbeat` — `HeartbeatRequest`, `HeartbeatResponse`; codes
  `1` and `2` with their three-element argument arrays.
- `harness_wire::raft` — `RaftRpc`; code `3` with the single opaque
  `rpc_bytes` byte string.
- `harness_wire::host` — `CanonicalHost`, `HostError`; canonical IPv4 and IPv6
  literal validation for the topology `host` position.
- `harness_wire::topology` — `TopologyAnswer`, `NodeEntry`, `TopologyError`;
  the `[nodes]` result arguments and the six-position per-node row.
- `harness_wire::catalog` — `CatalogEntry`, `CatalogPage`, `CatalogPageRequest`,
  `CatalogError`; the six-position entry row, the
  `[snapshot_token, next_cursor, entries]` page answer, and the
  `[snapshot_token, cursor]` page request.

## Catalog page requests

The first page request argument array is `[null, null]`. A continuation page
repeats `[snapshot_token, cursor]` exactly as received from the previous answer.
The token and cursor are independent 16-byte byte strings and must both be
present or both be `null`; the last page answer carries a `null` next cursor.
Source names are UTF-8 text limited in bytes by `CFG-CL-032` and are not
normalized.

## Canonical topology hosts

`host` is an unbracketed IP literal, never a DNS name and never an IPv6 zone
identifier. IPv4 is four decimal octets without leading zeros; IPv6 uses the
RFC 5952 canonical form, so only the compressed lowercase spelling is accepted.
Labels are UTF-8 text limited in bytes by `CFG-CL-031` and are not normalized. A
node row carries exactly one `host` and one `port` in `1..=65535`.

## Configuration inputs

Callers pass the limits; the crate holds no configuration of its own.

- `CFG-CL-011` frame body limit — the maximum value of the 4-byte length prefix,
  passed to `frame::encode`, `frame::decode`, and `frame::signed_message`.
- `CFG-CL-031` label limit in UTF-8 bytes — passed to
  `ExternalFrameHeader::encode` and `ExternalFrameHeader::decode`, and to
  `TopologyAnswer`/`NodeEntry` encoding and decoding.
- `CFG-CL-032` source-name limit in UTF-8 bytes — passed to
  `CatalogEntry::to_value`/`from_value` and `CatalogPage::to_args`/`from_args`.

## Scope

The crate validates the decrypted logical structures only. Frame signing and
encryption, and serialization of the RPC payload behind `rpc_bytes`, are
component concerns.

## Consuming the vectors

`tests/golden_vectors.rs` pins the exact contract bytes for the ordinary and
primary-registration headers, the frame body and signed range, and every
published result code; `tests/inter_server_vectors.rs` pins the envelope,
heartbeat, and RPC vectors and the registry table; `tests/topology_vectors.rs`
pins the topology answer, node rows, and canonical host rules;
`tests/catalog_vectors.rs` pins the catalog entry row, page answers, and page
requests. Component implementations (Android, dedicated server, distributed
storage) can consume the same vectors as external conformance tests so that one
encoding is proven across languages.
