//! Fixtures shared by test-environment tests.

use std::net::SocketAddr;

use harness_core::{ClusterTopology, FakeFaultInjector, InMemoryNodeControl, NodeId, NodeSpec};

/// Node id of the dedicated server in every fixture.
const DEDICATED_ID: u16 = 1;

/// Builds a fixture cluster: one dedicated server and `storage_nodes` storage nodes.
///
/// Storage node ids are `2..=storage_nodes + 1`, labels are `storage-N`, and addresses are
/// `127.0.0.1:(7000 + id)`. `storage_nodes` must be odd and at least 1; any other value is
/// rejected by [`ClusterTopology`] validation and panics here.
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
        .expect("storage node count must be odd and at least 1")
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
