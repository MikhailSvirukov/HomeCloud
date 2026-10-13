//! Docker-independent model of a test cluster.
//!
//! Describes the topology, the two v1 fault classes, and node lifecycle control
//! without any Docker or process code.

mod error;
mod fault;
mod node;
mod topology;

pub use error::HarnessError;
pub use fault::{
    FakeFaultInjector, Fault, FaultInjector, LossPercent, NetworkConditions, PeerLink,
};
pub use node::{InMemoryNodeControl, NodeControl, NodeState};
pub use topology::{ClusterTopology, ClusterTopologyBuilder, NodeId, NodeRole, NodeSpec};
