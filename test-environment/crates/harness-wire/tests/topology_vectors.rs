//! Golden vectors for the v1 topology answer and its canonical `host` position.
//!
//! Every test pins exact bytes from [`contracts/wire-v1.md`] §7 so component
//! implementations can consume these vectors as external conformance tests.
//!
//! [`contracts/wire-v1.md`]: ../../../../contracts/wire-v1.md

use harness_wire::cbor::{self, Value};
use harness_wire::host::{CanonicalHost, HostError};
use harness_wire::result::ResultEnvelope;
use harness_wire::topology::{NodeEntry, TopologyAnswer, TopologyError};

/// `CFG-CL-031` label limit used by the contract vectors.
const LABEL_LIMIT: usize = 256;

/// Sender identifier of the contract's node row.
const SENDER_ID: [u8; 16] = [0x11; 16];

/// X25519 public key of the contract's node row.
const X25519_PUBLIC_KEY: [u8; 32] = [0x22; 32];

/// Ed25519 public key of the contract's node row.
const ED25519_PUBLIC_KEY: [u8; 32] = [0x33; 32];

/// Canonical host literals accepted by `INT-NET-070 / 6`.
const CANONICAL_HOSTS: [&str; 6] = [
    "192.0.2.10",
    "0.0.0.0",
    "255.255.255.255",
    "::",
    "::1",
    "2001:db8::1",
];

/// Builds the contract node with the given `label`, `host`, and `port`.
fn node(label: &str, host: &str, port: u16) -> NodeEntry {
    NodeEntry {
        sender_id: SENDER_ID,
        label: label.to_owned(),
        host: CanonicalHost::parse(host).expect("host is canonical"),
        port,
        x25519_public_key: X25519_PUBLIC_KEY,
        ed25519_public_key: ED25519_PUBLIC_KEY,
    }
}

/// The contract node row before any golden-field substitution.
fn node_row_value() -> Value {
    Value::Array(vec![
        Value::Bytes(SENDER_ID.to_vec()),
        Value::Text("node-1".to_owned()),
        Value::Text("192.0.2.10".to_owned()),
        Value::Uint(443),
        Value::Bytes(X25519_PUBLIC_KEY.to_vec()),
        Value::Bytes(ED25519_PUBLIC_KEY.to_vec()),
    ])
}

/// Replaces one position of a CBOR array with `replacement`.
fn with_position(value: Value, position: usize, replacement: Value) -> Value {
    let Value::Array(mut items) = value else {
        panic!("expected a CBOR array");
    };
    items[position] = replacement;
    Value::Array(items)
}

/// Literal bytes of the success envelope around the contract node row.
fn expected_answer_bytes(host: &str) -> Vec<u8> {
    let mut bytes = vec![0x82, 0x00, 0x81, 0x86, 0x50];
    bytes.extend_from_slice(&SENDER_ID);
    bytes.push(0x66);
    bytes.extend_from_slice(b"node-1");
    let host_header = 0x60 | u8::try_from(host.len()).expect("contract host is short");
    bytes.push(host_header);
    bytes.extend_from_slice(host.as_bytes());
    bytes.extend_from_slice(&[0x19, 0x01, 0xBB, 0x58, 0x20]);
    bytes.extend_from_slice(&X25519_PUBLIC_KEY);
    bytes.extend_from_slice(&[0x58, 0x20]);
    bytes.extend_from_slice(&ED25519_PUBLIC_KEY);
    bytes
}

/// `INT-NET-070/6`, `TST-023/5`.
#[test]
fn ipv4_topology_answer_matches_contract_vector() {
    let answer = TopologyAnswer {
        nodes: vec![node("node-1", "192.0.2.10", 443)],
    };
    let args = answer.to_args(LABEL_LIMIT).expect("answer is valid");
    let envelope = ResultEnvelope { code: 0, args };
    let expected = expected_answer_bytes("192.0.2.10");

    assert_eq!(expected.len(), 110);
    assert_eq!(envelope.encode(), Ok(expected.clone()));

    let decoded = ResultEnvelope::decode(&cbor::decode(&expected).expect("vector is valid CBOR"))
        .expect("envelope is valid");
    assert_eq!(
        TopologyAnswer::from_args(&decoded.args, LABEL_LIMIT),
        Ok(answer)
    );
}

/// `TST-023/5`: an RFC 5952 compressed IPv6 literal is the canonical row host.
#[test]
fn ipv6_topology_host_matches_contract_vector() {
    let answer = TopologyAnswer {
        nodes: vec![node("node-1", "2001:db8::1", 443)],
    };
    let args = answer.to_args(LABEL_LIMIT).expect("answer is valid");
    let envelope = ResultEnvelope { code: 0, args };
    let expected = expected_answer_bytes("2001:db8::1");

    assert_eq!(expected.len() - 3, 108);
    assert_eq!(envelope.encode(), Ok(expected.clone()));

    let decoded = ResultEnvelope::decode(&cbor::decode(&expected).expect("vector is valid CBOR"))
        .expect("envelope is valid");
    assert_eq!(
        TopologyAnswer::from_args(&decoded.args, LABEL_LIMIT),
        Ok(answer)
    );
}

/// `TST-023/5`: nodes keep static-manifest order through encode and decode.
#[test]
fn topology_preserves_manifest_order() {
    let first = node("node-1", "192.0.2.10", 443);
    let second = NodeEntry {
        sender_id: [0x44; 16],
        label: "node-2".to_owned(),
        ..first.clone()
    };
    let answer = TopologyAnswer {
        nodes: vec![first, second],
    };

    let args = answer.to_args(LABEL_LIMIT).expect("answer is valid");

    assert_eq!(TopologyAnswer::from_args(&args, LABEL_LIMIT), Ok(answer));
}

/// `CFG-CL-031`, `INT-NET-070/6`, `TST-023/5`, `TST-033/1`: the label limit is
/// measured in UTF-8 bytes and is inclusive.
#[test]
fn topology_label_accepts_exactly_the_configured_limit() {
    let accepted = node(&"a".repeat(LABEL_LIMIT), "192.0.2.10", 443);
    let args = TopologyAnswer {
        nodes: vec![accepted.clone()],
    }
    .to_args(LABEL_LIMIT)
    .expect("a 256-byte label is allowed");
    assert_eq!(
        TopologyAnswer::from_args(&args, LABEL_LIMIT),
        Ok(TopologyAnswer {
            nodes: vec![accepted]
        })
    );

    let too_long = TopologyAnswer {
        nodes: vec![node(&"a".repeat(LABEL_LIMIT + 1), "192.0.2.10", 443)],
    };
    assert_eq!(
        too_long.to_args(LABEL_LIMIT),
        Err(TopologyError::LabelTooLong {
            actual: 257,
            limit: 256,
        })
    );
}

/// `TST-033/1`: the topology label is not Unicode-normalized.
#[test]
fn topology_label_round_trips_unicode_without_normalization() {
    let cyrillic = node("узел-1", "192.0.2.10", 443);
    let cyrillic_value = cyrillic.to_value(LABEL_LIMIT).expect("row is valid");
    let cyrillic_bytes = cbor::encode(&cyrillic_value);

    assert_eq!(cyrillic_bytes[18], 0x6A);
    assert_eq!(
        NodeEntry::from_value(&cyrillic_value, LABEL_LIMIT),
        Ok(cyrillic)
    );

    let composed = node("é", "192.0.2.10", 443);
    let decomposed = node("e\u{0301}", "192.0.2.10", 443);
    let composed_bytes = cbor::encode(&composed.to_value(LABEL_LIMIT).expect("row is valid"));
    let decomposed_bytes = cbor::encode(&decomposed.to_value(LABEL_LIMIT).expect("row is valid"));

    assert_ne!(composed_bytes, decomposed_bytes);
    assert_eq!(
        NodeEntry::from_value(
            &cbor::decode(&composed_bytes).expect("row bytes are valid CBOR"),
            LABEL_LIMIT,
        ),
        Ok(composed)
    );
    assert_eq!(
        NodeEntry::from_value(
            &cbor::decode(&decomposed_bytes).expect("row bytes are valid CBOR"),
            LABEL_LIMIT,
        ),
        Ok(decomposed)
    );
}

/// `INT-NET-070/6`, `TST-023/5`: canonical IPv4 and RFC 5952 IPv6 literals are
/// accepted and returned exactly.
#[test]
fn canonical_hosts_are_accepted() {
    for host in CANONICAL_HOSTS {
        let parsed = CanonicalHost::parse(host).expect("canonical host is accepted");
        assert_eq!(parsed.as_str(), host);
    }
}

/// `TST-023/5`: DNS names are not permitted in v1.
#[test]
fn dns_names_are_rejected() {
    assert_eq!(
        CanonicalHost::parse("node-1.example"),
        Err(HostError::DnsName)
    );
    assert_eq!(CanonicalHost::parse("localhost"), Err(HostError::DnsName));
}

/// `INT-NET-070/6`, `TST-023/5`: non-canonical IPv6 spellings are rejected.
#[test]
fn non_canonical_ipv6_is_rejected() {
    assert_eq!(
        CanonicalHost::parse("2001:0DB8::1"),
        Err(HostError::NonCanonicalIpv6)
    );
    assert_eq!(
        CanonicalHost::parse("2001:db8:0:0:0:0:0:1"),
        Err(HostError::NonCanonicalIpv6)
    );
}

/// `INT-NET-070/6`, `TST-023/5`: IPv6 zone identifiers are not permitted in v1.
#[test]
fn zone_identifiers_are_rejected() {
    assert_eq!(
        CanonicalHost::parse("fe80::1%eth0"),
        Err(HostError::ZoneIdentifier)
    );
}

/// `TST-023/5`: hosts that are neither empty nor an IP literal are rejected.
#[test]
fn malformed_and_empty_hosts_are_rejected() {
    assert_eq!(
        CanonicalHost::parse("999.1.1.1"),
        Err(HostError::NotAnIpLiteral)
    );
    assert_eq!(CanonicalHost::parse(""), Err(HostError::Empty));
}

/// `TST-023/5`: the topology row validates its host position.
#[test]
fn topology_rejects_invalid_host_in_the_node_row() {
    let dns_name = with_position(
        node_row_value(),
        2,
        Value::Text("node-1.example".to_owned()),
    );
    assert_eq!(
        NodeEntry::from_value(&dns_name, LABEL_LIMIT),
        Err(TopologyError::Host(HostError::DnsName))
    );

    let zoned = with_position(node_row_value(), 2, Value::Text("fe80::1%eth0".to_owned()));
    assert_eq!(
        NodeEntry::from_value(&zoned, LABEL_LIMIT),
        Err(TopologyError::Host(HostError::ZoneIdentifier))
    );
}

/// `INT-NET-070/6`, `TST-023/5`: the port is an unsigned integer in
/// `1..=65535`.
#[test]
fn topology_rejects_out_of_range_ports() {
    assert_eq!(
        NodeEntry::from_value(
            &with_position(node_row_value(), 3, Value::Uint(0)),
            LABEL_LIMIT
        ),
        Err(TopologyError::PortOutOfRange(0))
    );
    assert_eq!(
        NodeEntry::from_value(
            &with_position(node_row_value(), 3, Value::Uint(65_536)),
            LABEL_LIMIT,
        ),
        Err(TopologyError::PortOutOfRange(65_536))
    );
    assert_eq!(
        NodeEntry::from_value(
            &with_position(node_row_value(), 3, Value::Text("443".to_owned())),
            LABEL_LIMIT,
        ),
        Err(TopologyError::WrongType("port"))
    );
}

/// `INT-NET-070/6`, `TST-023/5`: the answer and every row have fixed shapes,
/// fixed byte-string lengths, and text labels and hosts.
#[test]
fn topology_rejects_wrong_shapes_and_lengths() {
    let Value::Array(items) = node_row_value() else {
        panic!("node row is an array");
    };
    let five_elements = Value::Array(items[..5].to_vec());
    assert_eq!(
        NodeEntry::from_value(&five_elements, LABEL_LIMIT),
        Err(TopologyError::WrongNodeShape)
    );

    let mut seven_items = items.clone();
    seven_items.push(Value::Null);
    assert_eq!(
        NodeEntry::from_value(&Value::Array(seven_items), LABEL_LIMIT),
        Err(TopologyError::WrongNodeShape)
    );

    let row = Value::Array(items.clone());
    assert_eq!(
        TopologyAnswer::from_args(
            &[Value::Array(vec![row.clone()]), Value::Array(vec![row])],
            LABEL_LIMIT
        ),
        Err(TopologyError::WrongAnswerShape)
    );
    assert_eq!(
        TopologyAnswer::from_args(&[], LABEL_LIMIT),
        Err(TopologyError::WrongAnswerShape)
    );

    let short_sender_id = with_position(node_row_value(), 0, Value::Bytes(vec![0x11; 15]));
    assert_eq!(
        NodeEntry::from_value(&short_sender_id, LABEL_LIMIT),
        Err(TopologyError::InvalidLength("sender_id"))
    );

    let short_x25519 = with_position(node_row_value(), 4, Value::Bytes(vec![0x22; 31]));
    assert_eq!(
        NodeEntry::from_value(&short_x25519, LABEL_LIMIT),
        Err(TopologyError::InvalidLength("x25519_public_key"))
    );

    let non_text_label = with_position(node_row_value(), 1, Value::Uint(1));
    assert_eq!(
        NodeEntry::from_value(&non_text_label, LABEL_LIMIT),
        Err(TopologyError::WrongType("label"))
    );

    let non_text_host = with_position(node_row_value(), 2, Value::Uint(1));
    assert_eq!(
        NodeEntry::from_value(&non_text_host, LABEL_LIMIT),
        Err(TopologyError::WrongType("host"))
    );
}
