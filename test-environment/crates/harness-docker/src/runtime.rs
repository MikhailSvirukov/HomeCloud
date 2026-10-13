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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use harness_core::{ClusterTopology, NodeSpec};

    use super::*;
    use crate::plan::PlanSettings;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn topology(n: u16) -> ClusterTopology {
        let mut builder = ClusterTopology::builder().dedicated_server(NodeSpec::dedicated_server(
            NodeId::new(1),
            "dedicated",
            addr(7001),
        ));
        for i in 0..n {
            let id = i + 2;
            builder = builder.storage_node(NodeSpec::storage(
                NodeId::new(id),
                format!("s-{id}"),
                addr(7002 + i),
            ));
        }
        builder.build().expect("valid topology")
    }

    fn plan() -> ClusterPlan {
        let settings = PlanSettings::new("harness-net", "server:1", "storage:1");
        ClusterPlan::from_topology(&topology(3), &settings)
    }

    fn plan_order(plan: &ClusterPlan) -> Vec<NodeId> {
        plan.containers.iter().map(|c| c.node).collect()
    }

    #[test]
    fn new_runtime_has_no_running_containers() {
        assert!(FakeClusterRuntime::new().running_containers().is_empty());
    }

    #[test]
    fn start_marks_all_containers_running() {
        let mut runtime = FakeClusterRuntime::new();
        let plan = plan();
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.running_containers(), plan_order(&plan));
    }

    #[test]
    fn start_is_idempotent() {
        let mut runtime = FakeClusterRuntime::new();
        let plan = plan();
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.running_containers(), plan_order(&plan));
    }

    #[test]
    fn stop_clears_running_containers() {
        let mut runtime = FakeClusterRuntime::new();
        let plan = plan();
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.stop(&plan), Ok(()));
        assert!(runtime.running_containers().is_empty());
    }

    #[test]
    fn stop_is_idempotent() {
        let mut runtime = FakeClusterRuntime::new();
        let plan = plan();
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.stop(&plan), Ok(()));
        assert_eq!(runtime.stop(&plan), Ok(()));
        assert!(runtime.running_containers().is_empty());
    }

    #[test]
    fn remove_clears_running_containers() {
        let mut runtime = FakeClusterRuntime::new();
        let plan = plan();
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.remove(&plan), Ok(()));
        assert!(runtime.running_containers().is_empty());
    }

    #[test]
    fn stop_then_start_matches_plan_order() {
        let mut runtime = FakeClusterRuntime::new();
        let plan = plan();
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.stop(&plan), Ok(()));
        assert_eq!(runtime.start(&plan), Ok(()));
        assert_eq!(runtime.running_containers(), plan_order(&plan));
    }
}
