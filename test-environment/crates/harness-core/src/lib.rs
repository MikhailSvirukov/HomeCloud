//! Docker-independent model of a test cluster.
//!
//! This crate describes the fixed cluster the harness stands up (topology), the
//! two base fault classes from the v1 specification, and node lifecycle control.
//! It contains no Docker or process code: fakes live here so fast isolated tests
//! can exercise scenario logic without containers.
//!
//! See `docs/specification-v1.md` (sections 5, 8, 11) and
//! `docs/agent-test-environment.md` for the behaviour this model represents.

pub mod error;
pub mod fault;
pub mod node;
pub mod topology;

pub use error::HarnessError;
pub use fault::{FakeFaultInjector, Fault, FaultInjector, NetworkConditions, PeerLink};
pub use node::{InMemoryNodeControl, NodeControl, NodeState};
pub use topology::{ClusterTopology, ClusterTopologyBuilder, NodeId, NodeRole, NodeSpec};
