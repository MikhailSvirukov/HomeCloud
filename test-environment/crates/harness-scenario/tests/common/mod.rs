//! Shared fixture for the `harness-scenario` integration tests.
//!
//! Included with `mod common;` from each integration test target through the
//! standard Cargo shared-test-module pattern; it is not a test target itself.
//! It holds only the cluster and network fixtures reused by several tests,
//! never file-specific helpers, and every item is `pub` so the test modules can
//! import what they use.

use std::time::Duration;

use harness_core::{
    ClusterTopology, FakeFaultInjector, InMemoryNodeControl, LossPercent, NetworkConditions,
};
use harness_scenario::{Scenario, ScenarioReport, ScenarioRunner};
use harness_test_support::{faults, nodes, topology};

/// A 1 dedicated + 3 storage cluster with all nodes running and no faults.
pub struct Cluster {
    /// Topology the runner and checks read.
    pub topology: ClusterTopology,
    /// In-memory node control bound to [`Cluster::topology`].
    pub nodes: InMemoryNodeControl,
    /// Fault injector bound to [`Cluster::topology`].
    pub faults: FakeFaultInjector,
}

impl Cluster {
    /// Builds the shared 1 dedicated + 3 storage fixture.
    pub fn new() -> Self {
        let topology = topology(3);
        let nodes = nodes(&topology);
        let faults = faults(&topology);
        Self {
            topology,
            nodes,
            faults,
        }
    }

    /// Runs `scenario` over this fixture's node control and fault injector.
    pub fn run(&mut self, scenario: &Scenario) -> ScenarioReport {
        ScenarioRunner::new(&self.topology, &mut self.nodes, &mut self.faults).run(scenario)
    }
}

impl Default for Cluster {
    fn default() -> Self {
        Self::new()
    }
}

/// Degraded-link conditions with `loss_percent` packet loss over `delay`.
pub fn degraded_conditions(delay: Duration, loss_percent: u8) -> NetworkConditions {
    NetworkConditions::new(
        delay,
        LossPercent::new(loss_percent).expect("loss percent is within the accepted range"),
    )
}
