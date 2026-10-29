# Wire contract — protocol version 1

## 1. Scope and precedence

This document is the published wire contract for version `1` of the HomeCloud application protocol. It covers the external frame header, the client logical header, the result envelope, the topology answer, the catalog page answer, and the inter-server envelope with its heartbeat and RPC message types. It restates the wire representation fixed by the SRS requirements in Table 1; it does not add protocol behavior.

The cited SRS requirements are normative. If a statement here differs from a cited requirement, the requirement prevails. `OPEN-003` is an open decision, not a requirement: §9 records what it leaves unfixed and this contract does not fill those gaps. The numeric value of a configured limit is not a wire constant; where a requirement delegates a limit to configuration, this contract names the configuration requirement and does not supply a number.

Requirement identifiers are cited as `ID / revision`. The revision in Table 1 is the revision this contract transcribes; an identifier used without a revision in later sections has the revision shown in Table 1.

**Table 1 — Section-to-requirement map**

| Section | Content | Requirement IDs and revisions |
|---|---|---|
| 1 | Scope and precedence | none (document metadata) |
| 2 | Canonical encoding | `INT-NET-005 / 1`, `INT-NET-006 / 2`, `INT-NET-011 / 1`, `INT-NET-047 / 2`, `INT-NET-049 / 1`; field rules from `INT-NET-050 / 4`, `INT-NET-054 / 1`, `INT-NET-055 / 2`, `INT-NET-063 / 1`, `INT-NET-070 / 5`, `INT-NET-072 / 2`, `INT-NET-073 / 5`, `FR-DED-027 / 1` |
| 3 | Protocol version negotiation | `INT-NET-079 / 1`; `INT-NET-007 / 1` and `INT-NET-043 / 1` as the rejection rules named by `INT-NET-079` |
| 4 | External frame header | `INT-NET-006 / 2`, `INT-NET-008 / 2`, `INT-NET-047 / 2`, `INT-NET-048 / 2`, `INT-NET-049 / 1`, `INT-NET-050 / 4`; `INT-NET-009 / 1` and `INT-NET-011 / 1` for `upload_id`; `CFG-CL-031 / 1` for the label limit |
| 5 | Client logical header | `INT-NET-047 / 2`, `INT-NET-048 / 2`, `INT-NET-049 / 1`, `INT-NET-051 / 3`, `INT-NET-060 / 1`; `INT-NET-054 / 1`, `INT-NET-055 / 2`, `INT-NET-057 / 1` for the worked topology and catalog operations, `FR-CAT-015 / 1` and `INT-NET-080 / 1` for the catalog page request; `FR-DED-027 / 1` and `CFG-CL-029 / 1` for the login and password |
| 6 | Result envelope | `INT-NET-061 / 1`, `INT-NET-062 / 1`, `INT-NET-063 / 1`, `INT-NET-064 / 1`, `INT-NET-069 / 1`, `INT-NET-070 / 5`, `INT-NET-080 / 1` |
| 7 | Topology answer | `INT-NET-070 / 5`; `CFG-CL-031 / 1` for the label limit |
| 7.1 | Catalog page answer | `FR-CAT-007 / 1`, `FR-CAT-009 / 1`, `FR-CAT-012 / 3`, `FR-CAT-013 / 2`, `FR-CAT-014 / 2`, `INT-NET-080 / 1`; `CFG-CL-032 / 1` for the source-name limit |
| 8 | Inter-server envelope and message types | `INT-NET-051 / 3`, `INT-NET-072 / 2`, `INT-NET-073 / 5`, `INT-NET-075 / 1`; `INT-NET-074 / 2` for RPC OpenRaft; `FR-TIME-005 / 3` for the offset source |
| 9 | Elements left unfixed by the SRS | `INT-NET-006 / 2`, `INT-NET-008 / 2`, `INT-NET-011 / 1`, `INT-NET-021 / 3`, `INT-NET-050 / 4`, `INT-NET-051 / 3`, `INT-NET-052 / 4`, `INT-NET-058 / 1`, `INT-NET-060 / 1`, `INT-NET-070 / 5`, `INT-NET-072 / 2`, `INT-NET-073 / 5`, `INT-NET-074 / 2`, `INT-NET-075 / 1`, `INT-NET-079 / 1`, `INT-NET-080 / 1`, `FR-CAT-013 / 2`, `FR-CAT-015 / 1`, `FR-DED-010 / 2`, `FR-DED-035 / 1`, `NFR-MAINT-005 / 1`, `NFR-MAINT-006 / 1`; `OPEN-003` (open decision, not a requirement) |

## 2. Canonical encoding

- **Deterministic CBOR.** The external frame header is serialized as deterministic CBOR according to RFC 8949 (`INT-NET-005 / 1`).
- **Arrays, not maps.** The external and logical headers are CBOR arrays with a fixed positional order defined by the schema of the protocol version; they are not CBOR maps (`INT-NET-047 / 2`). The decrypted inter-server logical header is a positional CBOR array of exactly two elements, `[message_type, args]` (`INT-NET-072 / 2`).
- **Minimal integers.** The minimal deterministic representation is required only for the integers named by the cited requirements: `protocol_version` and `frame_number` (`INT-NET-006 / 2`, `INT-NET-050 / 4`), the operation type (`INT-NET-055 / 2`), the result code (`INT-NET-063 / 1`), `message_type` (`INT-NET-072 / 2`), and every number in the heartbeat arguments (`INT-NET-073 / 5`). This contract does not extend that rule to other integers.
- **Optional positions.** An optional element of a header array is encoded as CBOR `null` in its own position. The array contains every position defined by the schema of the protocol version; positions are not omitted or shifted (`INT-NET-049 / 1`).
- **Text.** Text fields named in this contract are CBOR text strings. No Unicode normalization is applied to `responder_label` (`INT-NET-050 / 4`), the topology `label` (`INT-NET-070 / 5`), or `password` (`INT-NET-054 / 1`). `login` is a CBOR text string restricted to `[A-Za-z0-9._-]` by `FR-DED-027 / 1` and must satisfy that established ASCII set (`INT-NET-054 / 1`). `host` is restricted to the canonical IP literal forms in §7 (`INT-NET-070 / 5`).
- **Byte-string lengths.** The length of a byte string is fixed only where a cited requirement fixes it:

**Table 2 — Fixed byte-string lengths**

| Field | Length | Requirement |
|---|---|---|
| external `sender_id` | 16 bytes | `INT-NET-050 / 4` |
| `request_id` | 16 bytes | `INT-NET-050 / 4` |
| `upload_id` | 16 bytes | `INT-NET-011 / 1` |
| `bootstrap_signing_public_key` | 32 bytes | `INT-NET-050 / 4` |
| topology `sender_id` | 16 bytes | `INT-NET-070 / 5` |
| `x25519_public_key` | 32 bytes | `INT-NET-070 / 5` |
| `ed25519_public_key` | 32 bytes | `INT-NET-070 / 5` |
| catalog `snapshot_token` | 16 bytes | `FR-CAT-013 / 2` |
| catalog `next_cursor` | 16 bytes | `FR-CAT-013 / 2` |
| catalog entry `upload_id` | 16 bytes | `FR-CAT-012 / 3` |
| catalog entry `sha256` | 32 bytes | `FR-CAT-012 / 3` |
| upload `sha256` | 32 bytes | `INT-NET-052 / 4` |
| registration `x25519_public_key` | 32 bytes | `INT-NET-058 / 1` |
| registration `ed25519_public_key` | 32 bytes | `INT-NET-058 / 1` |

These are the only byte-string lengths fixed by the requirements cited in this contract.

## 3. Protocol version negotiation

- **Version 1 only.** Every implementation of the v1 application protocol announces and accepts only version `1`. Connection negotiation uses the one-element list `[1]` (`INT-NET-079 / 1`).
- **Version field.** When the version is carried in the external frame header, it is the first element and is encoded as CBOR `uint32` in the minimal deterministic representation, with the value `1` (`INT-NET-006 / 2`, `INT-NET-079 / 1`).
- **Unsupported version.** A frame carrying any version other than `1` is rejected (`INT-NET-007 / 1`, as required by `INT-NET-079 / 1`).
- **No common version.** If the intersection of the supported-version lists is empty, the receiver sends the version-incompatibility refusal defined by the negotiation schema and closes the connection without processing application requests (`INT-NET-043 / 1`, as required by `INT-NET-079 / 1`).
- **Future versions.** The policy for supporting future protocol versions is outside v1 (`INT-NET-079 / 1`). This contract defines no schema for any version other than `1`.

Because a v1 implementation supports only `1`, the list `[1]` is its complete local preference list; the empty-intersection rule applies when a peer offers only versions other than `1` (`INT-NET-079 / 1`, `INT-NET-043 / 1`).

## 4. External frame header

The external frame header is a positional CBOR array in the order fixed by `INT-NET-050 / 4`. It is not a CBOR map (`INT-NET-047 / 2`). The external-header schema and the client logical-header schema are defined independently; the same array position in the two schemas may denote elements with a different purpose and type (`INT-NET-048 / 2`). Optional elements are `null` at their position, and all schema positions are present (`INT-NET-049 / 1`).

**Table 3 — External frame header positions**

| Position | Element | CBOR type | Size or range | Null rule | Requirement |
|---|---|---|---|---|---|
| 1 | `protocol_version` | `uint32`, minimal deterministic | value `1` | never `null` | `INT-NET-006 / 2`, `INT-NET-050 / 4` |
| 2 | `sender_id` | byte string | 16 bytes | `null` only for primary registration without an assigned `sender_id`; otherwise the assigned sender identifier | `INT-NET-008 / 2`, `INT-NET-050 / 4` |
| 3 | `request_id` | byte string | 16 bytes | never `null` | `INT-NET-050 / 4` |
| 4 | `frame_number` | `uint32`, minimal deterministic | sequence starts at `0` and increases by `1` without gaps; `uint32` overflow is not permitted | never `null` | `INT-NET-006 / 2`, `INT-NET-008 / 2`, `INT-NET-050 / 4` |
| 5 | `upload_id` | byte string | 16 bytes | `null` when `upload_id` does not apply to the message | `INT-NET-009 / 1`, `INT-NET-011 / 1`, `INT-NET-050 / 4` |
| 6 | `responder_label` | UTF-8 text string | at most the configured limit in bytes (`CFG-CL-031 / 1`); no Unicode normalization | `null` when the label of the responding node does not apply to the message | `INT-NET-050 / 4` |
| 7 | `bootstrap_signing_public_key` | byte string | 32 bytes | `null` for every message other than primary registration | `INT-NET-050 / 4` |

`request_id` and `frame_number` are present in every external header. `sender_id` is present in every external header except the primary-registration case in which no `sender_id` has been assigned (`INT-NET-008 / 2`, `INT-NET-050 / 4`). The first frame of a sequence has `frame_number = 0`, and each subsequent frame of the same sequence increments the previous value by one without a gap (`INT-NET-008 / 2`).

**Primary registration.** For primary registration, `sender_id` is CBOR `null` and `bootstrap_signing_public_key` is the installation's open Ed25519 key (`INT-NET-050 / 4`). For every other message, `bootstrap_signing_public_key` is CBOR `null` and `sender_id` contains the assigned sender identifier (`INT-NET-050 / 4`). `upload_id` and `responder_label` are each CBOR `null` when they do not apply to the message (`INT-NET-050 / 4`).

The two header shapes below show where the null positions fall across the seven positions fixed by `INT-NET-050 / 4`; angle-bracketed names stand for the encoded value of that position, and §9 records any position not fixed by the cited requirements:

```text
ordinary message:     [1, <sender_id>, <request_id>, <frame_number>, <upload_id or null>, <responder_label or null>, null]
primary registration: [1, null, <request_id>, <frame_number>, <upload_id or null>, <responder_label or null>, <bootstrap_signing_public_key>]
```

The final position is `null` for an ordinary message because `bootstrap_signing_public_key` applies only to primary registration (`INT-NET-050 / 4`).

## 5. Client logical header

The logical header of a client request authenticated by an account begins with four elements in this order: operation type, login, password, and the operation-arguments array (`INT-NET-051 / 3`). The client logical schema is independent of the external-header schema (`INT-NET-048 / 2`), is a CBOR array rather than a map (`INT-NET-047 / 2`), and contains every schema position, using `null` for an optional element (`INT-NET-049 / 1`). The operation type is encoded as CBOR `uint32` in the minimal deterministic representation (`INT-NET-055 / 2`).

**Table 4 — Leading client logical header positions**

| Position | Element | CBOR type | Rule | Requirement |
|---|---|---|---|---|
| 1 | operation type | `uint32`, minimal deterministic | unknown operation codes are rejected; operation-code values are fixed by the v1 operation table | `INT-NET-051 / 3`, `INT-NET-055 / 2` |
| 2 | login | text string | must satisfy the established ASCII set `[A-Za-z0-9._-]`; minimum and maximum length configured by `CFG-CL-029 / 1` | `INT-NET-051 / 3`, `INT-NET-054 / 1`, `FR-DED-027 / 1`, `CFG-CL-029 / 1` |
| 3 | password | text string | the UTF-8 bytes after decoding exactly match the entered string, without trimming or Unicode normalization; minimum and maximum length configured by `CFG-CL-029 / 1` and measured in UTF-8 bytes | `INT-NET-051 / 3`, `INT-NET-054 / 1`, `CFG-CL-029 / 1` |
| 4 | operation arguments | CBOR array | schema determined uniquely by the operation type and validated strictly | `INT-NET-051 / 3` |

The v1 operation-code table makes `0` invalid and assigns `1` to account/installation registration, `2` to topology retrieval, `3` to a catalog-page request, and `4` to media upload (`INT-NET-057 / 1`).

**Worked operation: topology request.** The topology request is the operation with code `2` (`INT-NET-057 / 1`). Its logical header has the shape

```text
[2, <login>, <password>, []]
```

where `<login>` and `<password>` stand for their CBOR text strings. The fourth element is the operation-arguments array, and for this operation it is the empty CBOR array (`INT-NET-060 / 1`). The installation is identified by the external `sender_id`, and the credentials are in the common prefix of the client logical header (`INT-NET-060 / 1`). A topology request carrying a non-empty argument array is rejected (`INT-NET-060 / 1`).

**Worked operation: catalog page request.** A catalog page is requested directly from a storage node with operation `3` (`INT-NET-057 / 1`), not through the dedicated server and not through the inter-server envelope (`INT-NET-080 / 1`). The logical header begins with the four leading positions

```text
[3, <login>, <password>, [<snapshot_token>, <cursor>]]
```

where both arguments are CBOR `null` for the first page and 16-byte CBOR byte strings taken unchanged from the previous response for a continuation page (`FR-CAT-015 / 1`). The external header of this request carries `upload_id = null` and `responder_label = null`; the `request_id` identifies the client request and is repeated in the response (`INT-NET-080 / 1`, `INT-NET-050 / 4`).

**Worked operation: catalog page response.** The storage node answers with the result envelope of §6: `[result_code, result_args_array]` under the same external `request_id`, with `upload_id = null` and `responder_label` set to the manifest label of the node that served the page (`INT-NET-080 / 1`). On success the code is `0` and the argument array is the catalog page of §7.1; on failure the code is the mapped error and the argument array is empty (`INT-NET-069 / 1`).

Inter-server messages use the `[message_type, args]` shell of §8 and do not carry the client `login` and `password` fields (`INT-NET-051 / 3`).

## 6. Result envelope

The logical header of a server response to a client request contains, in order, the result code and the result-arguments array (`INT-NET-061 / 1`). The external `request_id` of the response equals the `request_id` of the request, and the operation code is not duplicated in the response (`INT-NET-061 / 1`). The envelope is therefore:

```text
[result_code, result_args_array]
```

The result code is a CBOR integer of type `uint32` in the minimal deterministic representation (`INT-NET-063 / 1`). The value `0` means successful execution; every non-zero value is an error (`INT-NET-062 / 1`). For any non-zero result code the server sends an empty CBOR array as `result_args_array`; additional diagnostic data is not carried in v1 error arguments (`INT-NET-069 / 1`). The successful topology answer uses code `0` and the argument array of §7 (`INT-NET-070 / 5`).

The two envelope forms are therefore:

```text
success: [0, <result arguments>]
error:   [<non-zero result_code>, []]
```

**Table 5 — v1 result codes**

| Code | Meaning | Range class |
|---|---|---|
| `0x0001_0001` | unknown operation | request errors |
| `0x0001_0002` | invalid arguments | request errors |
| `0x0001_0003` | invalid request state | request errors |
| `0x0001_0004` | repeated `request_id` conflict | request errors |
| `0x0002_0001` | authentication rejection | authentication errors |
| `0x0003_0001` | registration conflict | registration errors |
| `0x0003_0002` | registration rejected by policy | registration errors |
| `0x0004_0001` | invalid cursor | catalog errors |
| `0x0004_0002` | expired snapshot | catalog errors |
| `0x0005_0001` | file size limit exceeded | file/upload errors |
| `0x0005_0002` | size, metadata or hash mismatch | file/upload errors |
| `0x0005_0003` | invalid frame sequence | file/upload errors |
| `0x0005_0004` | `upload_id` conflict | file/upload errors |
| `0x0006_0001` | Raft quorum unavailable | quorum/cluster errors |
| `0x0006_0002` | blob replication quorum not reached | quorum/cluster errors |
| `0x0007_0001` | temporary unavailability | resource errors |
| `0x0007_0002` | storage capacity exhausted | resource errors |
| `0x0008_0001` | internal error | internal errors |

**Table 6 — Reserved result-code ranges**

| Range | Reserved for |
|---|---|
| `0x0001_0000–0x0001_FFFF` | request errors |
| `0x0002_0000–0x0002_FFFF` | authentication errors |
| `0x0003_0000–0x0003_FFFF` | registration errors |
| `0x0004_0000–0x0004_FFFF` | catalog errors |
| `0x0005_0000–0x0005_FFFF` | file/upload errors |
| `0x0006_0000–0x0006_FFFF` | quorum/cluster errors |
| `0x0007_0000–0x0007_FFFF` | resource errors |
| `0x0008_0000–0x0008_FFFF` | internal errors |

Unused values inside these ranges and every other non-zero `uint32` value remain reserved; published codes must not be reassigned (`INT-NET-063 / 1`).

## 7. Topology answer

A successful topology response has result code `0` and a `result_args_array` of the form `[nodes]` (`INT-NET-070 / 5`). `nodes` is an array of per-node arrays. Each per-node array is exactly

```text
[sender_id, label, host, port, x25519_public_key, ed25519_public_key]
```

and the nodes follow the order of the static manifest (`INT-NET-070 / 5`). The address of each entry consists of one `host` and one `port` (`INT-NET-070 / 5`).

**Table 7 — Per-node array**

| Position | Field | CBOR type | Size or range | Canonical rules | Requirement |
|---|---|---|---|---|---|
| 1 | `sender_id` | byte string | 16 bytes | the node's UUID from the static manifest | `INT-NET-070 / 5` |
| 2 | `label` | UTF-8 text string | at most the configured limit in bytes (`CFG-CL-031 / 1`) | no Unicode normalization | `INT-NET-070 / 5` |
| 3 | `host` | text string | maximum length not fixed by `INT-NET-070 / 5` | canonical IPv4 or IPv6 literal; DNS names and IPv6 zone identifiers are not permitted in v1 | `INT-NET-070 / 5` |
| 4 | `port` | unsigned integer | `1..=65535` | none | `INT-NET-070 / 5` |
| 5 | `x25519_public_key` | byte string | 32 bytes | open X25519 key of the node | `INT-NET-070 / 5` |
| 6 | `ed25519_public_key` | byte string | 32 bytes | open Ed25519 key of the node | `INT-NET-070 / 5` |

**IPv4.** An IPv4 `host` is written as four decimal octets with no leading zeros, except for the value `0` (`INT-NET-070 / 5`).

**IPv6.** An IPv6 `host` is written according to RFC 5952 (`INT-NET-070 / 5`). An IPv6 zone identifier is not permitted in v1, and neither is a DNS name (`INT-NET-070 / 5`).

## 7.1 Catalog page answer

A successful catalog page response has result code `0` and a `result_args_array` of the form `[snapshot_token, next_cursor, entries]` (`FR-CAT-013 / 2`). `snapshot_token` is a 16-byte CBOR byte string and is present on every page; `next_cursor` is a 16-byte CBOR byte string when a further page exists and CBOR `null` on the last page; `entries` is an array of catalog entries (`FR-CAT-013 / 2`). Tokens and cursors are opaque to the client and must be returned unchanged (`FR-CAT-009 / 1`, `FR-CAT-014 / 2`).

**Table 7a — Catalog entry array**

| Position | Field | CBOR type | Size or range | Canonical rules | Requirement |
|---|---|---|---|---|---|
| 1 | `upload_id` | byte string | 16 bytes | UUID of the upload | `FR-CAT-012 / 3` |
| 2 | `source_name` | UTF-8 text string | at most the configured limit (`CFG-CL-032`) | no Unicode normalization | `FR-CAT-012 / 3` |
| 3 | `timestamp_ms` | signed integer | 64-bit Unix time in milliseconds | must match the upload metadata | `FR-CAT-012 / 3` |
| 4 | `original_size_bytes` | unsigned integer | none | must match the upload metadata | `FR-CAT-012 / 3` |
| 5 | `compressed_size_bytes` | unsigned integer | none | must match the upload metadata | `FR-CAT-012 / 3` |
| 6 | `sha256` | byte string | 32 bytes | SHA-256 of the original bytes | `FR-CAT-012 / 3` |

Entries are ordered by ascending `upload_id` (`FR-CAT-007 / 1`).

## 8. Inter-server envelope and message types

The decrypted logical header of an inter-server message is a positional CBOR array of exactly two elements, `[message_type, args]` (`INT-NET-072 / 2`). `message_type` is a CBOR `uint32` in the minimal deterministic representation (`INT-NET-072 / 2`); the code values themselves are fixed by `INT-NET-075 / 1`. The composition, order, types and permitted values of `args` are fixed separately for each message type, and the receiver rejects an unknown code, an incorrect array length, or an incorrect argument type or value (`INT-NET-072 / 2`). The envelope does not carry the client `login` and `password` fields (`INT-NET-051 / 3`).

**Table 8 — Inter-server message types**

| `message_type` | Message type | Status | Requirement |
|---|---|---|---|
| `0` | reserved | reserved and rejected | `INT-NET-075 / 1` |
| `1` | dedicated liveness heartbeat request | assigned | `INT-NET-073 / 5`, `INT-NET-075 / 1` |
| `2` | dedicated liveness heartbeat response | assigned | `INT-NET-073 / 5`, `INT-NET-075 / 1` |
| `3` | RPC OpenRaft | assigned | `INT-NET-074 / 2`, `INT-NET-075 / 1` |

Within one protocol version, each used `message_type` code denotes exactly one inter-server message type, and a published code must not be reassigned (`INT-NET-075 / 1`). The value `0` is reserved and rejected (`INT-NET-075 / 1`).

Catalog page requests and responses are client messages exchanged directly between the client and a storage node (§§5–7.1); no `message_type` code is assigned to them and they must not be sent inside the inter-server envelope (`INT-NET-080 / 1`).

### 8.1 Dedicated liveness heartbeat request

The request is `message_type = 1` (`INT-NET-073 / 5`, `INT-NET-075 / 1`). Its arguments have the order `[sequence_number, dedicated_sent_at_ms, previous_clock_offset_ms]` (`INT-NET-073 / 5`).

**Table 9 — Heartbeat request arguments**

| Position | Field | CBOR type | Unit and meaning | Null rule | Requirement |
|---|---|---|---|---|---|
| 1 | `sequence_number` | `uint64` | heartbeat sequence number | never `null` | `INT-NET-073 / 5` |
| 2 | `dedicated_sent_at_ms` | `int64` | Unix time in milliseconds; the dedicated server sends the request | never `null` | `INT-NET-073 / 5` |
| 3 | `previous_clock_offset_ms` | `int64` when non-zero | milliseconds; the estimate from the previous valid exchange according to `FR-TIME-005 / 3` | CBOR `null` before the first calculated estimate | `INT-NET-073 / 5` |

### 8.2 Dedicated liveness heartbeat response

The response is `message_type = 2` (`INT-NET-073 / 5`, `INT-NET-075 / 1`). Its arguments have the order `[sequence_number, node_received_at_ms, node_sent_at_ms]` (`INT-NET-073 / 5`).

**Table 10 — Heartbeat response arguments**

| Position | Field | CBOR type | Unit and meaning | Null rule | Requirement |
|---|---|---|---|---|---|
| 1 | `sequence_number` | `uint64` | the received `sequence_number`, returned unchanged | never `null` | `INT-NET-073 / 5` |
| 2 | `node_received_at_ms` | `int64` | Unix time in milliseconds; the node receives the request | never `null` | `INT-NET-073 / 5` |
| 3 | `node_sent_at_ms` | `int64` | Unix time in milliseconds; the node sends the response | never `null` | `INT-NET-073 / 5` |

The response returns the received `sequence_number` without modification (`INT-NET-073 / 5`). The three timestamps denote, respectively, the dedicated server sending the request, the node receiving the request, and the node sending the response (`INT-NET-073 / 5`). Every number in the heartbeat arguments is encoded in the minimal deterministic representation (`INT-NET-073 / 5`).

### 8.3 RPC OpenRaft

`message_type = 3` denotes RPC OpenRaft (`INT-NET-075 / 1`). Each RPC between storage nodes is a separate inter-server message with arguments `[rpc_bytes]`, where `rpc_bytes` is a CBOR byte string containing the serialized RPC message produced by the OpenRaft transport adapter (`INT-NET-074 / 2`). The serialization format of `rpc_bytes` is fixed by `NFR-MAINT-005 / 1` (OpenRaft `0.9.25` with the `serde` feature) and `NFR-MAINT-006 / 1` (CBOR per RFC 8949 through `ciborium` over the serde representation of the RPC types); `INT-NET-074 / 2` defines only the byte-string wrapper that this contract restates.

## 9. Elements left unfixed by the SRS

`OPEN-003` remains open for two things: the exact element count and maximum element sizes of the client and response logical headers beyond the positions restated in §§5-6, including a maximum size for the operation-arguments array, and the codes and argument composition of inter-server message types beyond the published `1`, `2` and `3`, in particular the verifier and key distribution messages of `FR-DED-010 / 2` and `FR-DED-035 / 1`. It is closed for the external header, whose every position, type, length and null rule is fixed by `INT-NET-006 / 2`, `INT-NET-008 / 2`, `INT-NET-011 / 1`, `INT-NET-021 / 3` and `INT-NET-050 / 4`; for the four leading client logical positions and the argument schemas of all four v1 operations, fixed by `INT-NET-051 / 3`, `INT-NET-052 / 4`, `INT-NET-058 / 1`, `INT-NET-060 / 1`, `FR-CAT-015 / 1` and `INT-NET-080 / 1`; and for the catalog page exchange, fixed by `INT-NET-080 / 1` as a client exchange that carries no inter-server code. The inter-server shell `[message_type, args]` and the CBOR type of `message_type` are fixed by `INT-NET-072 / 2`; the heartbeat schemas by `INT-NET-073 / 5`; the RPC OpenRaft wrapper by `INT-NET-074 / 2`; the message-type code values by `INT-NET-075 / 1`; the OpenRaft serialization format by `NFR-MAINT-005 / 1` and `NFR-MAINT-006 / 1` (closed `OPEN-007`);

The following elements are left unfixed by the cited requirements, so this contract does not define them:

- **Client logical header length and argument maxima.** `INT-NET-051 / 3` fixes the four leading positions restated in §5, and the cited requirements fix each operation's argument schema, but no cited requirement fixes the exact element count of the client or response logical header, a maximum size for the operation-arguments array, or the maximum size of its individual elements; this contract supplies none of them.
- **Topology `host` maximum length.** `INT-NET-070 / 5` fixes the form of `host` as a canonical IPv4 or IPv6 literal, with DNS names and IPv6 zone identifiers not permitted in v1; that requirement does not fix a maximum length for `host`, and this contract supplies none.
- **Zero `previous_clock_offset_ms`.** `INT-NET-073 / 5` fixes `int64` for a non-zero `previous_clock_offset_ms` and CBOR `null` before the first calculated estimate. It does not state how a computed offset of exactly zero is encoded; this contract does not choose an encoding for that case.
- **Client operations not restated in full.** The argument schema is determined by the operation type (`INT-NET-051 / 3`). This contract restates the topology request (`INT-NET-060 / 1`) and the catalog page request and response (`FR-CAT-015 / 1`, `FR-CAT-013 / 2`, `INT-NET-080 / 1`); the argument arrays of registration and media upload are fixed by `INT-NET-058 / 1` and `INT-NET-052 / 4` and are not restated here.
- **Inter-server message types other than 1, 2 and 3.** Only the heartbeat request, the heartbeat response and RPC OpenRaft have a schema stated in this contract; the codes and argument composition of any further inter-server message type are not fixed (`OPEN-003`). Catalog pages are client messages and do not use this envelope (`INT-NET-080 / 1`).
- **RPC serialization.** `rpc_bytes` is opaque at this layer. Its serialization format is fixed outside this contract by `NFR-MAINT-005 / 1` and `NFR-MAINT-006 / 1` (OpenRaft `0.9.25` with the `serde` feature, CBOR per RFC 8949 through `ciborium`) and is not restated here; `INT-NET-074 / 2` fixes only the byte-string wrapper.
- **Future protocol versions.** Only version `1` and the negotiation list `[1]` are defined for v1; future-version policy and schemas are outside v1 (`INT-NET-079 / 1`).

Where a field's type, size, mandatory rule or limit is not stated in §§4–8, it is not fixed by this contract, and no value should be inferred for it.
