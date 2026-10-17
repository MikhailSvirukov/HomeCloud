//! Client-facing topology of the fixed storage cluster.
//!
//! The topology is a projection of the static manifest only: it keeps manifest
//! order and reads no heartbeat or clock state, so it cannot depend on which
//! storage nodes are currently reachable.

use std::net::SocketAddr;

use crate::config::{ClusterConfig, NodeId, PublicKey};

/// One storage node as clients receive it.
pub struct TopologyNode {
    /// Identifier of the node in the static manifest.
    pub sender_id: NodeId,
    /// Human-readable label identifying the node.
    pub label: String,
    /// Address of the node.
    pub address: SocketAddr,
    /// Public key used to encrypt messages for the node.
    pub encryption_public_key: PublicKey,
    /// Public key used to verify messages signed by the node.
    pub signing_public_key: PublicKey,
}

/// The client-facing topology of the fixed cluster.
pub struct Topology {
    /// Storage nodes in static manifest order.
    pub nodes: Vec<TopologyNode>,
}

impl Topology {
    /// Build the topology from the static manifest, preserving manifest order.
    pub(crate) fn from_manifest(cluster: &ClusterConfig) -> Self {
        Self {
            nodes: cluster
                .nodes
                .iter()
                .map(|node| TopologyNode {
                    sender_id: node.sender_id,
                    label: node.label.clone(),
                    address: node.address,
                    encryption_public_key: node.encryption_public_key,
                    signing_public_key: node.signing_public_key,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::de::value::{BorrowedStrDeserializer, Error as ValueError};
    use serde::Deserialize;

    use super::*;
    use crate::config::ClusterNode;

    fn node_id(index: u8) -> NodeId {
        let hex = format!("{index:032x}");
        NodeId::deserialize(BorrowedStrDeserializer::<ValueError>::new(&hex))
            .expect("test node identifiers are 32 hexadecimal characters")
    }

    fn id_bytes(index: u8) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[15] = index;
        bytes
    }

    fn public_key(byte: u8) -> PublicKey {
        let hex = format!("{byte:02x}").repeat(32);
        PublicKey::deserialize(BorrowedStrDeserializer::<ValueError>::new(&hex))
            .expect("test public keys are 64 hexadecimal characters")
    }

    fn manifest() -> ClusterConfig {
        ClusterConfig {
            nodes: vec![
                ClusterNode {
                    sender_id: node_id(3),
                    label: "zeta".to_string(),
                    address: SocketAddr::from(([127, 0, 0, 1], 8001)),
                    encryption_public_key: public_key(0x11),
                    signing_public_key: public_key(0x22),
                },
                ClusterNode {
                    sender_id: node_id(2),
                    label: "alpha".to_string(),
                    address: SocketAddr::from(([127, 0, 0, 1], 8002)),
                    encryption_public_key: public_key(0x33),
                    signing_public_key: public_key(0x44),
                },
                ClusterNode {
                    sender_id: node_id(1),
                    label: "mu".to_string(),
                    address: SocketAddr::from(([127, 0, 0, 1], 8003)),
                    encryption_public_key: public_key(0x55),
                    signing_public_key: public_key(0x66),
                },
            ],
        }
    }

    #[test]
    fn topology_preserves_manifest_order_identifiers_and_labels() {
        let cluster = manifest();

        let topology = Topology::from_manifest(&cluster);

        let entries = topology
            .nodes
            .iter()
            .map(|node| (node.label.as_str(), *node.sender_id.as_bytes()))
            .collect::<Vec<_>>();

        assert_eq!(
            entries,
            vec![
                ("zeta", id_bytes(3)),
                ("alpha", id_bytes(2)),
                ("mu", id_bytes(1)),
            ]
        );
    }

    #[test]
    fn each_entry_carries_one_endpoint_label_and_both_keys() {
        let cluster = manifest();

        let topology = Topology::from_manifest(&cluster);

        assert_eq!(topology.nodes.len(), cluster.nodes.len());
        let first = &topology.nodes[0];
        assert_eq!(first.address, SocketAddr::from(([127, 0, 0, 1], 8001)));
        assert_eq!(first.label, "zeta");
        assert_eq!(first.encryption_public_key.as_bytes(), &[0x11; 32]);
        assert_eq!(first.signing_public_key.as_bytes(), &[0x22; 32]);
    }
}
