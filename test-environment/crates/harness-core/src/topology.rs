//! Validated v1 cluster topology: one dedicated server and an odd set of storage nodes.

use std::collections::HashSet;
use std::fmt;
use std::net::SocketAddr;

use crate::error::HarnessError;

const DEDICATED: &str = "dedicated server";
const STORAGE: &str = "storage";

/// Stable numeric identifier of a node in the test cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u16);

impl NodeId {
    /// Wraps a raw numeric identifier.
    pub fn new(value: u16) -> Self {
        Self(value)
    }
    /// Returns the inner numeric value.
    pub fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The part a node plays in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeRole {
    /// Registration/bootstrap and control-plane node.
    DedicatedServer,
    /// One of the distributed storage nodes.
    Storage,
}

/// A fully specified node, ready to be placed in a topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpec {
    /// Numeric identifier, unique within a topology.
    pub id: NodeId,
    /// The role this node plays.
    pub role: NodeRole,
    /// Human-readable, unique label used in reports and scenarios.
    pub label: String,
    /// Address the node is reachable on inside the test network.
    pub address: SocketAddr,
}

impl NodeSpec {
    /// Builds a dedicated-server spec.
    pub fn dedicated_server(id: NodeId, label: impl Into<String>, address: SocketAddr) -> Self {
        Self {
            id,
            role: NodeRole::DedicatedServer,
            label: label.into(),
            address,
        }
    }
    /// Builds a storage-node spec.
    pub fn storage(id: NodeId, label: impl Into<String>, address: SocketAddr) -> Self {
        Self {
            id,
            role: NodeRole::Storage,
            label: label.into(),
            address,
        }
    }
}

/// A validated v1 cluster: one dedicated server plus storage nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterTopology {
    dedicated: NodeSpec,
    storage: Vec<NodeSpec>,
}

impl ClusterTopology {
    /// Starts building a topology.
    pub fn builder() -> ClusterTopologyBuilder {
        ClusterTopologyBuilder::new()
    }
    /// The single dedicated server (client entry point).
    pub fn dedicated_server(&self) -> &NodeSpec {
        &self.dedicated
    }
    /// The storage nodes in insertion order.
    pub fn storage_nodes(&self) -> &[NodeSpec] {
        &self.storage
    }
    /// Looks up any node, dedicated server or storage.
    pub fn node(&self, id: NodeId) -> Option<&NodeSpec> {
        if self.dedicated.id == id {
            Some(&self.dedicated)
        } else {
            self.storage.iter().find(|spec| spec.id == id)
        }
    }
    /// Iterates every node, dedicated server first, then storage nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &NodeSpec> {
        std::iter::once(&self.dedicated).chain(self.storage.iter())
    }
    /// Total node count, including the dedicated server.
    pub fn node_count(&self) -> usize {
        self.storage.len() + 1
    }
    /// Majority quorum over the storage nodes only.
    pub fn quorum(&self) -> usize {
        self.storage.len() / 2 + 1
    }
    /// Validates specs in a fixed order and assembles the topology.
    ///
    /// # Errors
    ///
    /// Returns a [`HarnessError`] on the first violated count, role, or
    /// uniqueness constraint.
    fn from_parts(dedicated: Vec<NodeSpec>, storage: Vec<NodeSpec>) -> Result<Self, HarnessError> {
        let dedicated: [NodeSpec; 1] = dedicated.try_into().map_err(|excess: Vec<NodeSpec>| {
            HarnessError::DedicatedServerCount {
                count: excess.len(),
            }
        })?;
        let [dedicated] = dedicated;
        if dedicated.role != NodeRole::DedicatedServer {
            return Err(HarnessError::WrongNodeRole {
                id: dedicated.id.get(),
                expected: DEDICATED,
            });
        }
        for spec in &storage {
            if spec.role != NodeRole::Storage {
                return Err(HarnessError::WrongNodeRole {
                    id: spec.id.get(),
                    expected: STORAGE,
                });
            }
        }
        if storage.is_empty() || storage.len().is_multiple_of(2) {
            return Err(HarnessError::StorageNodeCount {
                count: storage.len(),
            });
        }
        let all: Vec<&NodeSpec> = std::iter::once(&dedicated).chain(storage.iter()).collect();
        let mut ids = HashSet::new();
        for spec in &all {
            if !ids.insert(spec.id) {
                return Err(HarnessError::DuplicateNodeId { id: spec.id.get() });
            }
        }
        let mut addresses = HashSet::new();
        for spec in &all {
            if !addresses.insert(spec.address) {
                return Err(HarnessError::DuplicateAddress {
                    address: spec.address,
                });
            }
        }
        let mut labels = HashSet::new();
        for spec in &all {
            if !labels.insert(spec.label.as_str()) {
                return Err(HarnessError::DuplicateLabel {
                    label: spec.label.clone(),
                });
            }
        }
        Ok(Self { dedicated, storage })
    }
}

/// Accumulates node specs before [`ClusterTopology`] validation.
#[derive(Debug, Default)]
pub struct ClusterTopologyBuilder {
    dedicated: Vec<NodeSpec>,
    storage: Vec<NodeSpec>,
}

impl ClusterTopologyBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a dedicated-server spec.
    pub fn dedicated_server(mut self, spec: NodeSpec) -> Self {
        self.dedicated.push(spec);
        self
    }
    /// Adds a storage-node spec.
    pub fn storage_node(mut self, spec: NodeSpec) -> Self {
        self.storage.push(spec);
        self
    }
    /// Validates and assembles the topology.
    ///
    /// # Errors
    ///
    /// Returns a [`HarnessError`] describing the first violated constraint.
    pub fn build(self) -> Result<ClusterTopology, HarnessError> {
        ClusterTopology::from_parts(self.dedicated, self.storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HarnessError as E;

    type Build = Result<ClusterTopology, HarnessError>;
    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }
    fn sto(id: u16, label: &str, port: u16) -> NodeSpec {
        NodeSpec::storage(NodeId::new(id), label, addr(port))
    }
    fn ded(id: u16, label: &str, port: u16) -> NodeSpec {
        NodeSpec::dedicated_server(NodeId::new(id), label, addr(port))
    }
    fn specs(items: &[(u16, u16)]) -> Vec<NodeSpec> {
        items
            .iter()
            .map(|&(id, port)| sto(id, &format!("s-{id}"), port))
            .collect()
    }
    fn assemble(dedicated: Option<NodeSpec>, storage: Vec<NodeSpec>) -> Build {
        let mut b = ClusterTopology::builder();
        if let Some(spec) = dedicated {
            b = b.dedicated_server(spec);
        }
        for spec in storage {
            b = b.storage_node(spec);
        }
        b.build()
    }
    fn build(storage: Vec<NodeSpec>) -> Build {
        assemble(Some(ded(1, "dedicated", 7001)), storage)
    }
    fn topology(n: u16) -> ClusterTopology {
        let items: Vec<(u16, u16)> = (0..n).map(|i| (i + 2, 7002 + i)).collect();
        build(specs(&items)).expect("valid topology")
    }

    #[test]
    fn builds_one_dedicated_and_three_storage() {
        let t = topology(3);
        assert_eq!(t.node_count(), 4);
        assert_eq!(t.storage_nodes().len(), 3);
        assert_eq!(t.dedicated_server().role, NodeRole::DedicatedServer);
    }
    #[test]
    fn missing_dedicated_server_is_rejected() {
        let result = assemble(None, specs(&[(2, 7002), (3, 7003), (4, 7004)]));
        assert!(matches!(result, Err(E::DedicatedServerCount { count: 0 })));
    }
    #[test]
    fn even_storage_count_is_rejected() {
        let result = build(specs(&[(2, 7002), (3, 7003)]));
        assert!(matches!(result, Err(E::StorageNodeCount { count: 2 })));
    }
    #[test]
    fn duplicate_node_id_is_rejected() {
        let result = build(specs(&[(2, 7002), (2, 7003), (4, 7004)]));
        assert!(matches!(result, Err(E::DuplicateNodeId { id: 2 })));
    }
    #[test]
    fn duplicate_address_is_rejected() {
        let result = build(specs(&[(2, 7002), (3, 7002), (4, 7004)]));
        assert!(matches!(result, Err(E::DuplicateAddress { .. })));
    }
    #[test]
    fn duplicate_label_is_rejected() {
        let storage = vec![
            sto(2, "same", 7002),
            sto(3, "same", 7003),
            sto(4, "x", 7004),
        ];
        let result = build(storage);
        assert!(matches!(result, Err(E::DuplicateLabel { .. })));
    }
    #[test]
    fn wrong_role_is_rejected_in_dedicated_slot() {
        let result = assemble(
            Some(sto(1, "dedicated", 7001)),
            specs(&[(2, 7002), (3, 7003), (4, 7004)]),
        );
        assert!(matches!(result, Err(E::WrongNodeRole { id: 1, .. })));
    }
    #[test]
    fn wrong_role_is_rejected_in_storage_slot() {
        let storage = vec![ded(2, "a", 7002), sto(3, "b", 7003), sto(4, "c", 7004)];
        let result = build(storage);
        assert!(matches!(result, Err(E::WrongNodeRole { id: 2, .. })));
    }
    #[test]
    fn quorum_is_strict_majority_of_storage() {
        assert_eq!(topology(3).quorum(), 2);
        assert_eq!(topology(5).quorum(), 3);
    }
    #[test]
    fn node_lookup_hits_known_ids_and_misses_unknown() {
        let t = topology(3);
        assert_eq!(
            t.node(NodeId::new(1)).map(|s| s.label.as_str()),
            Some("dedicated")
        );
        assert_eq!(
            t.node(NodeId::new(3)).map(|s| s.label.as_str()),
            Some("s-3")
        );
        assert_eq!(t.node(NodeId::new(99)), None);
    }
    #[test]
    fn nodes_iterates_dedicated_server_first() {
        let roles: Vec<NodeRole> = topology(3).nodes().map(|s| s.role).collect();
        let expected = [
            NodeRole::DedicatedServer,
            NodeRole::Storage,
            NodeRole::Storage,
            NodeRole::Storage,
        ];
        assert_eq!(roles, expected);
    }
}
