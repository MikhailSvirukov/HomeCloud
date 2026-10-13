//! Pure descriptions of the isolated Docker test cluster and its container lifecycle.
//!
//! The plan is derived from a validated [`harness_core::ClusterTopology`] and
//! contains no Docker client code, so it can be built and asserted on in fast,
//! daemon-free tests. The runtime seam abstracts container lifecycle operations;
//! [`FakeClusterRuntime`] is the in-memory implementation used by tests.

pub mod command;
pub mod plan;
pub mod runtime;

pub use command::{
    Command, container_name, container_remove, container_run, container_stop, network_create,
};
pub use plan::{ClusterPlan, ContainerPlan, NetworkPlan, PlanSettings};
pub use runtime::{ClusterRuntime, FakeClusterRuntime};
