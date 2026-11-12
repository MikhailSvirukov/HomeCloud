# Static manifest v1

The trusted static manifest is the TOML 1.0 document, encoded as UTF-8, that both
server components load at startup (`CFG-CL-034 / 2`). The operator places the
same manifest content on the dedicated server and on every storage node
(`CFG-CL-004 / 1`), so both sides resolve exactly the same identities, labels,
addresses and open keys (`CFG-CL-021 / 1`). This contract fixes the section and
key names, the value types and the validation rules; the values themselves are
deployment data.

The manifest holds public material only. Private keys are local configuration
of each component and never appear in the manifest.

## Sections

The manifest fixes two sections inside the configuration file of a server
component; they appear next to that component's local sections such as
`[identity]` or `[metrics]`:

| Section | Shape | Cardinality | Meaning |
|---|---|---|---|
| `[dedicated]` | one table | exactly one | the dedicated server's own manifest entry |
| `[[storage]]` | array of tables | one or more | the storage nodes of the fixed cluster, in manifest order |

Both sections use the same entry fields. An entry describes one server of the
cluster:

| Key | TOML type | Value | Requirement |
|---|---|---|---|
| `sender_id` | string | exactly 32 lowercase hexadecimal characters (16 bytes) | `INT-NET-050 / 4` |
| `label` | string | non-empty UTF-8 text, at most the configured `max_label_bytes` in bytes (`CFG-CL-031 / 1`, range `1..=4096`) | `CFG-CL-031 / 1` |
| `address` | string | IP literal and port, for example `10.0.0.11:7100`; no DNS name; port `1..=65535`. The loader parses it into an address and every answer renders the canonical form, so two spellings of the same endpoint compare equal. | `INT-NET-070 / 6` |
| `encryption_public_key` | string | exactly 64 lowercase hexadecimal characters (32 bytes, X25519) | `CFG-CL-021 / 1` |
| `signing_public_key` | string | exactly 64 lowercase hexadecimal characters (32 bytes, Ed25519) | `CFG-CL-021 / 1` |

The manifest order of `[[storage]]` entries is significant: it is the order of
the topology answer (`INT-NET-070 / 6`), of the availability gauges and of the
availability records.

## Validation rules

A loader rejects the manifest, rather than repairing it, when any of the
following holds (`CFG-CL-038 / 1`):

1. the file is not valid TOML 1.0 or not valid UTF-8 (`CFG-CL-034 / 2`);
2. a key inside `[dedicated]` or `[[storage]]` is not one of the names above, or
   a key repeats;
3. `[dedicated]` is missing, or `[[storage]]` names no entry;
4. a required field is missing or empty;
5. `sender_id` is not exactly 32 lowercase hexadecimal characters, or a key is
   not exactly 64 lowercase hexadecimal characters;
6. `label` exceeds the configured maximum in UTF-8 bytes;
7. `address` is not an IP literal with a port in `1..=65535`;
8. two entries, the dedicated entry included, share a `sender_id`, a `label` or
   an `address`.

Identifiers and keys are never normalised: an uppercase hexadecimal character
or a DNS name is rejected, so every component reads the same bytes. A label is
used exactly as written, without trimming or Unicode normalisation. An address
is parsed into an address value and rendered in its canonical form wherever the
protocol carries it.

## Example

```toml
[dedicated]
sender_id = "00000000000000000000000000000000"
label = "dedicated-1"
address = "10.0.0.10:7000"
encryption_public_key = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
signing_public_key = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"

[[storage]]
sender_id = "00000000000000000000000000000001"
label = "storage-1"
address = "10.0.0.11:7100"
encryption_public_key = "1111111111111111111111111111111111111111111111111111111111111111"
signing_public_key = "2222222222222222222222222222222222222222222222222222222222222222"

[[storage]]
sender_id = "00000000000000000000000000000002"
label = "storage-2"
address = "10.0.0.12:7100"
encryption_public_key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
signing_public_key = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
```

## Negative vectors

| Manifest fragment | Rejected because |
|---|---|
| no `[dedicated]` table | the dedicated entry is required |
| `[[storage]]` with no entries | a cluster needs at least one storage node |
| `label = ""` in any entry | labels must not be empty |
| a 300-byte `label` with `max_label_bytes = 256` | labels must fit the configured limit |
| `sender_id = "0000000000000000000000000000000"` | 31 characters |
| `sender_id = "0000000000000000000000000000000G"` | not hexadecimal |
| `sender_id = "0000000000000000000000000000000A"` | uppercase is not canonical |
| `encryption_public_key` with 62 characters | keys are exactly 32 bytes |
| `address = "storage-1:7100"` | the address is an IP literal, not a DNS name |
| `address = "10.0.0.11:0"` | the port must be in `1..=65535` |
| `node_id = "…"` inside `[[storage]]` | unknown keys are rejected |
| two entries with `label = "storage-1"` | labels are unique across all entries |
| `[dedicated]` repeating a storage `sender_id` | identifiers are unique across all entries |
| `[[nodes]]` instead of `[[storage]]` | unknown sections are rejected |
