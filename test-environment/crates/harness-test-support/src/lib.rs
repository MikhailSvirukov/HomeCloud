//! Fixtures shared by test-environment tests.

use std::net::SocketAddr;

use harness_core::{ClusterTopology, FakeFaultInjector, InMemoryNodeControl, NodeId, NodeSpec};

/// Node id of the dedicated server in every fixture.
pub const DEDICATED_ID: u16 = 1;

/// Builds a fixture cluster: one dedicated server and `storage_nodes` storage nodes.
///
/// Storage node ids are `2..=storage_nodes + 1`; labels are `storage-N`; addresses are
/// `127.0.0.1:(7000 + id)`.
pub fn topology(storage_nodes: u16) -> ClusterTopology {
    let dedicated =
        NodeSpec::dedicated_server(NodeId::new(DEDICATED_ID), "dedicated", addr(DEDICATED_ID));
    let mut builder = ClusterTopology::builder().dedicated_server(dedicated);
    for index in 1..=storage_nodes {
        let id = index + 1;
        builder = builder.storage_node(NodeSpec::storage(
            NodeId::new(id),
            format!("storage-{index}"),
            addr(id),
        ));
    }
    builder
        .build()
        .expect("fixture topology is valid by construction")
}

/// Node control bound to `topology`, with every node running.
pub fn nodes(topology: &ClusterTopology) -> InMemoryNodeControl {
    InMemoryNodeControl::new(topology.nodes().map(|spec| spec.id))
}

/// Fault injector bound to `topology`.
pub fn faults(topology: &ClusterTopology) -> FakeFaultInjector {
    FakeFaultInjector::new(topology.clone())
}

/// Listen address used for fixture node `id`.
pub fn addr(id: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7000 + id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_has_one_dedicated_server_and_requested_storage_nodes() {
        let topology = topology(3);
        assert_eq!(topology.node_count(), 4);
        assert_eq!(topology.dedicated_server().id, NodeId::new(DEDICATED_ID));
        assert_eq!(topology.storage_nodes().len(), 3);
    }
}
