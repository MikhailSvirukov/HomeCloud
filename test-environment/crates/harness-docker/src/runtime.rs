//! Lifecycle seam over the container plan.
//!
//! [`ClusterRuntime`] is the abstract container lifecycle a Docker-backed
//! runtime must provide. [`FakeClusterRuntime`] implements it in memory so
//! scenario and plan logic can be tested without a Docker daemon.

use harness_core::{HarnessError, NodeId};

use crate::plan::ClusterPlan;

type Result<T> = core::result::Result<T, HarnessError>;

/// Lifecycle operations a Docker-backed runtime must provide.
pub trait ClusterRuntime {
    /// Starts every container in the plan; idempotent for running nodes.
    fn start(&mut self, plan: &ClusterPlan) -> Result<()>;
    /// Stops every container in the plan; idempotent.
    fn stop(&mut self, plan: &ClusterPlan) -> Result<()>;
    /// Removes every container in the plan; idempotent.
    fn remove(&mut self, plan: &ClusterPlan) -> Result<()>;
    /// Node ids whose containers are currently running, in start order.
    fn running_containers(&self) -> &[NodeId];
}

/// In-memory [`ClusterRuntime`] for tests that must not require Docker.
#[derive(Default)]
pub struct FakeClusterRuntime {
    running: Vec<NodeId>,
}

impl FakeClusterRuntime {
    /// Creates a runtime with no running containers.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClusterRuntime for FakeClusterRuntime {
    fn start(&mut self, plan: &ClusterPlan) -> Result<()> {
        for container in &plan.containers {
            if !self.running.contains(&container.node) {
                self.running.push(container.node);
            }
        }
        Ok(())
    }
    fn stop(&mut self, plan: &ClusterPlan) -> Result<()> {
        self.running.retain(|node| plan.container(*node).is_none());
        Ok(())
    }
    fn remove(&mut self, plan: &ClusterPlan) -> Result<()> {
        self.stop(plan)
    }
    fn running_containers(&self) -> &[NodeId] {
        &self.running
    }
}
