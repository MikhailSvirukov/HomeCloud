//! Success answers the dedicated server sends to clients.
//!
//! The registration answer carries only the assigned installation identifier
//! (`INT-NET-059 / 1`); the topology answer carries the manifest projection of
//! the cluster in manifest order (`INT-NET-070 / 5`). Both are result envelopes
//! (`INT-NET-061 / 1`) whose codes are `0`, because they describe a successful
//! operation.

use std::net::{IpAddr, SocketAddr};

use ciborium::Value;

use crate::topology::Topology;
use crate::wire::{ResultEnvelope, WireError, IDENTIFIER_BYTES, KEY_BYTES};

/// Number of elements in one topology entry.
pub const TOPOLOGY_ENTRY_ELEMENTS: usize = 6;

/// One node of a decoded topology answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyEntry {
    /// Identifier of the node in the static manifest.
    pub sender_id: [u8; IDENTIFIER_BYTES],
    /// Human-readable label of the node.
    pub label: String,
    /// Canonical address of the node.
    pub address: SocketAddr,
    /// Public key used to encrypt messages for the node.
    pub encryption_public_key: [u8; KEY_BYTES],
    /// Public key used to verify messages signed by the node.
    pub signing_public_key: [u8; KEY_BYTES],
}

/// Build the successful registration answer (`INT-NET-059 / 1`).
pub fn registration_answer(installation_id: &[u8; IDENTIFIER_BYTES]) -> ResultEnvelope {
    ResultEnvelope::success(vec![Value::Bytes(installation_id.to_vec())])
}

/// Build the successful topology answer (`INT-NET-070 / 5`).
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
/// and types, the canonical address form and the configured label limit, so a
/// malformed answer is rejected rather than accepted partially.
pub fn decode_topology_answer(
    bytes: &[u8],
    max_label_bytes: usize,
) -> Result<Vec<TopologyEntry>, WireError> {
    let envelope = ResultEnvelope::decode(bytes)?;
    if envelope.code != 0 {
        return Err(WireError::UnexpectedResultCode(envelope.code));
    }
    let [Value::Array(nodes)] = envelope.arguments.as_slice() else {
        return Err(WireError::InvalidAnswer {
            reason: "the topology answer carries one nodes array",
        });
    };

    let mut entries = Vec::with_capacity(nodes.len());
    for node in nodes {
        let Value::Array(elements) = node else {
            return Err(WireError::InvalidAnswer {
                reason: "each topology entry is an array",
            });
        };
        if elements.len() != TOPOLOGY_ENTRY_ELEMENTS {
            return Err(WireError::InvalidAnswer {
                reason: "each topology entry has six elements",
            });
        }
        let label = match &elements[1] {
            Value::Text(label) => label.clone(),
            _ => {
                return Err(WireError::InvalidAnswer {
                    reason: "the node label is a text string",
                })
            }
        };
        if label.len() > max_label_bytes {
            return Err(WireError::LabelTooLong {
                actual: label.len(),
                max: max_label_bytes,
            });
        }
        let host = match &elements[2] {
            Value::Text(host) => host,
            _ => {
                return Err(WireError::InvalidAnswer {
                    reason: "the node host is a text string",
                })
            }
        };
        let address = parse_address(host, &elements[3])?;

        entries.push(TopologyEntry {
            sender_id: fixed_bytes(&elements[0], IDENTIFIER_BYTES)?,
            label,
            address,
            encryption_public_key: fixed_bytes(&elements[4], KEY_BYTES)?,
            signing_public_key: fixed_bytes(&elements[5], KEY_BYTES)?,
        });
    }

    Ok(entries)
}

fn parse_address(host: &str, port: &Value) -> Result<SocketAddr, WireError> {
    let ip: IpAddr = host.parse().map_err(|_| WireError::InvalidAnswer {
        reason: "the node host is a canonical IPv4 or IPv6 literal",
    })?;
    if ip.to_string() != host {
        return Err(WireError::InvalidAnswer {
            reason: "the node host is not in canonical form",
        });
    }
    let port = match port {
        Value::Integer(port) => u16::try_from(*port).map_err(|_| WireError::InvalidAnswer {
            reason: "the node port is in 1..=65535",
        })?,
        _ => {
            return Err(WireError::InvalidAnswer {
                reason: "the node port is an unsigned integer",
            })
        }
    };
    if port == 0 {
        return Err(WireError::InvalidAnswer {
            reason: "the node port is in 1..=65535",
        });
    }

    Ok(SocketAddr::new(ip, port))
}

fn fixed_bytes<const N: usize>(value: &Value, expected: usize) -> Result<[u8; N], WireError> {
    match value {
        Value::Bytes(bytes) if bytes.len() == expected => {
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| WireError::InvalidAnswer {
                    reason: "a byte string has the wrong length",
                })
        }
        _ => Err(WireError::InvalidAnswer {
            reason: "a topology field has the wrong CBOR type or length",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{NodeId, PublicKey};
    use crate::topology::TopologyNode;

    const MAX_LABEL_BYTES: usize = 64;

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

    #[test]
    fn the_registration_answer_carries_only_the_installation_id() {
        let envelope = registration_answer(&[0xAA; IDENTIFIER_BYTES]);
        let bytes = envelope.encode().expect("encoding must work");

        assert_eq!(envelope.code, 0);
        assert_eq!(bytes, {
            let mut expected = vec![0x82, 0x00, 0x81, 0x50];
            expected.extend([0xAA; IDENTIFIER_BYTES]);
            expected
        });
        assert_eq!(
            ResultEnvelope::decode(&bytes).expect("decoding must work"),
            envelope
        );
    }

    #[test]
    fn the_topology_answer_round_trips_with_manifest_order_and_canonical_hosts() {
        let answer = topology_answer(&topology());
        let bytes = answer.encode().expect("encoding must work");
        let entries = decode_topology_answer(&bytes, MAX_LABEL_BYTES).expect("decoding must work");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "storage-1");
        assert_eq!(entries[0].address, "10.0.0.11:7100".parse().expect("IPv4"));
        assert_eq!(entries[0].sender_id, [0x11; IDENTIFIER_BYTES]);
        assert_eq!(entries[0].encryption_public_key, [0x22; KEY_BYTES]);
        assert_eq!(entries[0].signing_public_key, [0x33; KEY_BYTES]);
        assert_eq!(entries[1].label, "storage-2");
        assert_eq!(entries[1].address, "[fd00::12]:7100".parse().expect("IPv6"));
    }

    #[test]
    fn a_non_hex_ipv4_host_is_rejected() {
        let answer = topology_answer(&topology());
        let mut value = ciborium_value(&answer);
        let Value::Array(envelope) = &mut value else {
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
        node[2] = Value::Text("010.0.0.11".to_owned());

        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { .. })
        ));
    }

    #[test]
    fn a_zero_port_or_wrong_entry_length_is_rejected() {
        let answer = topology_answer(&topology());
        let mut value = ciborium_value(&answer);
        set_port(&mut value, Value::Integer(0.into()));
        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { .. })
        ));

        set_port(&mut value, Value::Integer(7100.into()));
        let Value::Array(envelope) = &mut value else {
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
        node.pop();
        assert!(matches!(
            decode_topology_answer(&encode(&value), MAX_LABEL_BYTES),
            Err(WireError::InvalidAnswer { .. })
        ));
    }

    fn set_port(value: &mut Value, port: Value) {
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
        node[3] = port;
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

    fn ciborium_value(answer: &ResultEnvelope) -> Value {
        let bytes = answer.encode().expect("encoding must work");
        let mut reader = bytes.as_slice();
        ciborium::de::from_reader(&mut reader).expect("decoding must work")
    }

    fn encode(value: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(value, &mut bytes).expect("encoding must work");
        bytes
    }
}
