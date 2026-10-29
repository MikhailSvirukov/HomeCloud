//! Success answers the dedicated server sends to clients.
//!
//! The registration answer carries only the assigned installation identifier
//! (`INT-NET-059 / 1`); the topology answer carries the manifest projection of
//! the cluster in manifest order (`INT-NET-070 / 5`). Both are result envelopes
//! (`INT-NET-061 / 1`) whose codes are `0`, because they describe a successful
//! operation.

use std::fmt;
use std::net::{IpAddr, SocketAddr};

use ciborium::Value;

use crate::config::{NodeId, PublicKey};
use crate::registration::InstallationId;
use crate::topology::Topology;
use crate::wire::{ResultEnvelope, WireError, IDENTIFIER_BYTES, KEY_BYTES};

/// Number of elements in one topology entry.
pub const TOPOLOGY_ENTRY_ELEMENTS: usize = 6;

/// One node of a decoded topology answer.
///
/// The manual [`fmt::Debug`] implementation omits the key material, which must
/// never reach a log record (`NFR-OBS-003`).
#[derive(Clone, PartialEq, Eq)]
pub struct TopologyEntry {
    /// Identifier of the node in the static manifest.
    pub sender_id: NodeId,
    /// Human-readable label of the node.
    pub label: String,
    /// Canonical address of the node.
    pub address: SocketAddr,
    /// Public key used to encrypt messages for the node.
    pub encryption_public_key: PublicKey,
    /// Public key used to verify messages signed by the node.
    pub signing_public_key: PublicKey,
}

/// Build the successful registration answer (`INT-NET-059 / 1`).
///
/// The identifier is a [`InstallationId`], so the UUIDv4 invariant the server
/// assigns is preserved by construction.
pub fn registration_answer(installation_id: InstallationId) -> ResultEnvelope {
    ResultEnvelope::success(vec![Value::Bytes(installation_id.as_bytes().to_vec())])
}

/// Build the successful topology answer (`INT-NET-070 / 5`).
///
/// The input is expected to come from a validated manifest: labels already fit
/// [`crate::config::ServerConfig::max_label_bytes`] and the addresses are
/// canonical [`SocketAddr`] values, so this function only renders them.
pub fn topology_answer(topology: &Topology) -> ResultEnvelope {
    let nodes = topology
        .nodes
        .iter()
        .map(|node| {
            Value::Array(vec![
                Value::Bytes(node.sender_id.as_bytes().to_vec()),
                Value::Text(node.label.clone()),
                Value::Text(node.address.ip().to_string()),
                Value::Integer(u64::from(node.address.port()).into()),
                Value::Bytes(node.encryption_public_key.as_bytes().to_vec()),
                Value::Bytes(node.signing_public_key.as_bytes().to_vec()),
            ])
        })
        .collect();

    ResultEnvelope::success(vec![Value::Array(nodes)])
}

/// Decode a successful topology answer (`INT-NET-070 / 5`).
///
/// The receiver enforces the success code, the argument shape, the entry order
/// and types, the canonical address form (`INT-NET-070 / 5`) and the label
/// limit (`CFG-CL-031 / 1`), so a malformed answer is rejected rather than
/// accepted partially.
pub fn decode_topology_answer(
    bytes: &[u8],
    max_label_bytes: usize,
) -> Result<Vec<TopologyEntry>, WireError> {
    let envelope = ResultEnvelope::decode(bytes)?;
    if envelope.code != 0 {
        return Err(WireError::UnexpectedResultCode(envelope.code));
    }
    let [Value::Array(nodes)] = envelope.arguments.as_slice() else {
        return Err(invalid(
            "arguments",
            "the topology answer carries one nodes array",
        ));
    };

    let mut entries = Vec::with_capacity(nodes.len());
    for node in nodes {
        let Value::Array(elements) = node else {
            return Err(invalid("entry", "each topology entry is an array"));
        };
        if elements.len() != TOPOLOGY_ENTRY_ELEMENTS {
            return Err(invalid("entry", "each topology entry has six elements"));
        }
        let label = match &elements[1] {
            Value::Text(label) => label.clone(),
            _ => return Err(invalid("label", "the node label is a text string")),
        };
        if label.len() > max_label_bytes {
            return Err(WireError::LabelTooLong {
                actual: label.len(),
                max: max_label_bytes,
            });
        }
        let host = match &elements[2] {
            Value::Text(host) => host,
            _ => return Err(invalid("host", "the node host is a text string")),
        };
        let address = parse_address(host, &elements[3])?;

        entries.push(TopologyEntry {
            sender_id: NodeId::from_bytes(bytes16(&elements[0], "sender_id")?),
            label,
            address,
            encryption_public_key: PublicKey::from_bytes(bytes32(
                &elements[4],
                "encryption_public_key",
            )?),
            signing_public_key: PublicKey::from_bytes(bytes32(&elements[5], "signing_public_key")?),
        });
    }

    Ok(entries)
}

impl fmt::Debug for TopologyEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TopologyEntry")
            .field("label", &self.label)
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

fn parse_address(host: &str, port: &Value) -> Result<SocketAddr, WireError> {
    let ip: IpAddr = host
        .parse()
        .map_err(|_| invalid("host", "the node host is a canonical IPv4 or IPv6 literal"))?;
    if ip.to_string() != host {
        return Err(invalid(
            "host",
            "the node host is not in canonical form (Rust renders IPv4-embedded IPv6 in hex)",
        ));
    }
    let port = match port {
        Value::Integer(port) => {
            u16::try_from(*port).map_err(|_| invalid("port", "the node port is in 1..=65535"))?
        }
        _ => return Err(invalid("port", "the node port is an unsigned integer")),
    };
    if port == 0 {
        return Err(invalid("port", "the node port is in 1..=65535"));
    }

    Ok(SocketAddr::new(ip, port))
}

fn bytes16(value: &Value, field: &'static str) -> Result<[u8; IDENTIFIER_BYTES], WireError> {
    fixed_bytes::<IDENTIFIER_BYTES>(value, field)
}

fn bytes32(value: &Value, field: &'static str) -> Result<[u8; KEY_BYTES], WireError> {
    fixed_bytes::<KEY_BYTES>(value, field)
}

fn fixed_bytes<const N: usize>(value: &Value, field: &'static str) -> Result<[u8; N], WireError> {
    match value {
        Value::Bytes(bytes) if bytes.len() == N => bytes
            .as_slice()
            .try_into()
            .map_err(|_| invalid(field, "a byte string has the wrong length")),
        Value::Bytes(_) => Err(invalid(field, "a byte string has the wrong length")),
        _ => Err(invalid(field, "a field has the wrong CBOR type")),
    }
}

fn invalid(field: &'static str, reason: &'static str) -> WireError {
    WireError::InvalidAnswer { field, reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::TopologyNode;

    const MAX_LABEL_BYTES: usize = 64;

    fn installation_id() -> InstallationId {
        let mut bytes = [0xAA; IDENTIFIER_BYTES];
        bytes[6] = 0x47;
        bytes[8] = 0x99;
        InstallationId::from_bytes(bytes).expect("a UUIDv4 identifier")
    }

    fn topology() -> Topology {
        Topology {
            nodes: vec![
                TopologyNode {
                    sender_id: NodeId::from_bytes([0x11; IDENTIFIER_BYTES]),
                    label: "storage-1".to_owned(),
                    address: "10.0.0.11:7100".parse().expect("valid IPv4 address"),
                    encryption_public_key: PublicKey::from_bytes([0x22; KEY_BYTES]),
                    signing_public_key: PublicKey::from_bytes([0x33; KEY_BYTES]),
                },
                TopologyNode {
                    sender_id: NodeId::from_bytes([0x44; IDENTIFIER_BYTES]),
                    label: "storage-2".to_owned(),
                    address: "[fd00::12]:7100".parse().expect("valid IPv6 address"),
                    encryption_public_key: PublicKey::from_bytes([0x55; KEY_BYTES]),
                    signing_public_key: PublicKey::from_bytes([0x66; KEY_BYTES]),
                },
            ],
        }
    }

    fn encoded_answer() -> Vec<u8> {
        topology_answer(&topology())
            .encode()
            .expect("encoding must work")
    }

    fn answer_value() -> Value {
        let bytes = encoded_answer();
        let mut reader = bytes.as_slice();
        ciborium::de::from_reader(&mut reader).expect("decoding must work")
    }

    fn encode(value: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(value, &mut bytes).expect("encoding must work");
        bytes
    }

    fn node_mut(value: &mut Value) -> &mut Vec<Value> {
        let Value::Array(envelope) = value else {
            panic!("the answer is an array");
        };
        let Value::Array(arguments) = &mut envelope[1] else {
            panic!("the answer carries an argument array");
        };
        let Value::Array(nodes) = &mut arguments[0] else {
            panic!("the answer carries a nodes array");
        };
        let Value::Array(node) = &mut nodes[0] else {
            panic!("a node is an array");
        };
        node
    }

    #[test]
    fn the_registration_answer_carries_only_the_installation_id() {
        let id = installation_id();
        let envelope = registration_answer(id);
        let bytes = envelope.encode().expect("encoding must work");

        let mut expected = vec![0x82, 0x00, 0x81, 0x50];
        expected.extend(id.as_bytes());
        assert_eq!(bytes, expected);
        assert_eq!(envelope.code, 0);
    }

    #[test]
    fn the_topology_answer_encodes_the_documented_layout() {
        let bytes = encoded_answer();

        let mut expected = vec![0x82, 0x00, 0x81, 0x82, 0x86, 0x50];
        expected.extend([0x11; IDENTIFIER_BYTES]);
        expected.extend([0x69, b's', b't', b'o', b'r', b'a', b'g', b'e', b'-', b'1']);
        expected.extend([0x69, b'1', b'0', b'.', b'0', b'.', b'0', b'.', b'1', b'1']);
        expected.extend([0x19, 0x1b, 0xbc]);
        expected.extend([0x58, 0x20]);
        expected.extend([0x22; KEY_BYTES]);
        expected.extend([0x58, 0x20]);
        expected.extend([0x33; KEY_BYTES]);

        expected.extend([0x86, 0x50]);
        expected.extend([0x44; IDENTIFIER_BYTES]);
        expected.extend([0x69, b's', b't', b'o', b'r', b'a', b'g', b'e', b'-', b'2']);
        expected.extend([0x68, b'f', b'd', b'0', b'0', b':', b':', b'1', b'2']);
        expected.extend([0x19, 0x1b, 0xbc]);
        expected.extend([0x58, 0x20]);
        expected.extend([0x55; KEY_BYTES]);
        expected.extend([0x58, 0x20]);
        expected.extend([0x66; KEY_BYTES]);

        assert_eq!(bytes, expected);
    }

    #[test]
    fn the_topology_answer_round_trips_in_manifest_order() {
        let entries =
            decode_topology_answer(&encoded_answer(), MAX_LABEL_BYTES).expect("decoding must work");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "storage-1");
        assert_eq!(entries[0].address, "10.0.0.11:7100".parse().expect("IPv4"));
        assert_eq!(
            entries[0].sender_id,
            NodeId::from_bytes([0x11; IDENTIFIER_BYTES])
        );
        assert!(entries[0].encryption_public_key == PublicKey::from_bytes([0x22; KEY_BYTES]));
        assert_eq!(entries[1].label, "storage-2");
        assert_eq!(entries[1].address, "[fd00::12]:7100".parse().expect("IPv6"));
    }

    #[test]
    fn a_label_at_the_limit_is_accepted() {
        let mut value = answer_value();
        node_mut(&mut value)[1] = Value::Text("x".repeat(MAX_LABEL_BYTES));

        assert!(decode_topology_answer(&encode(&value), MAX_LABEL_BYTES).is_ok());
    }

    #[test]
    fn a_label_above_the_limit_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[1] = Value::Text("x".repeat(MAX_LABEL_BYTES + 1));

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::LabelTooLong { .. })
        ));
    }

    #[test]
    fn a_dns_host_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[2] = Value::Text("storage-1.example".to_owned());

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { field: "host", .. })
        ));
    }

    #[test]
    fn a_non_canonical_ipv6_host_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[2] = Value::Text("FD00::12".to_owned());

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { field: "host", .. })
        ));
    }

    #[test]
    fn a_port_above_the_range_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[3] = Value::Integer(65_536.into());

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { field: "port", .. })
        ));
    }

    #[test]
    fn a_negative_port_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[3] = Value::Integer((-1).into());

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { field: "port", .. })
        ));
    }

    #[test]
    fn a_short_sender_id_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[0] = Value::Bytes(vec![0x11; IDENTIFIER_BYTES - 1]);

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer {
                field: "sender_id",
                ..
            })
        ));
    }

    #[test]
    fn a_short_public_key_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value)[4] = Value::Bytes(vec![0x22; KEY_BYTES - 1]);

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer {
                field: "encryption_public_key",
                ..
            })
        ));
    }

    #[test]
    fn a_five_element_entry_is_rejected() {
        let mut value = answer_value();
        node_mut(&mut value).pop();

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { field: "entry", .. })
        ));
    }

    #[test]
    fn a_two_element_argument_array_is_rejected() {
        let mut value = answer_value();
        let Value::Array(envelope) = &mut value else {
            panic!("the answer is an array");
        };
        let Value::Array(arguments) = &mut envelope[1] else {
            panic!("the answer carries an argument array");
        };
        arguments.push(Value::Null);

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer {
                field: "arguments",
                ..
            })
        ));
    }

    #[test]
    fn a_non_zero_result_code_is_rejected() {
        let bytes = ResultEnvelope::failure(std::num::NonZeroU32::new(2).expect("a nonzero code"))
            .encode()
            .expect("encoding must work");

        assert!(matches!(
            decode_topology_answer(&bytes, MAX_LABEL_BYTES),
            Err(WireError::UnexpectedResultCode(2))
        ));
    }
}
