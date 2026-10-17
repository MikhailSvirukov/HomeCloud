//! Registration and extension coverage for the public scenario interface.
//!
//! TST-009/2 requires a new recovery scenario to be added as a separate
//! implementation of the public scenario interface plus its registration,
//! without changing the shared container/network runner. These tests exercise
//! the catalogue through [`ScenarioRegistry`] and add a test-local scenario the
//! same way a downstream harness user would.

mod common;

use std::time::Duration;

use harness_core::{NodeId, PeerLink};
use harness_scenario::{
    AllNodesRunning, LinkDegradationRecovery, NetworkPartitionRecovery, NodeShutdownRecovery,
    QuorumLossRecovery, Scenario, ScenarioRegistry, ScenarioSpec,
};

use common::{Cluster, degraded_conditions};

/// Name of the test-local scenario added by this module only.
const NEW_SCENARIO_NAME: &str = "all nodes running";

/// A scenario contributed outside the crate: no runner or catalogue change.
struct AllNodesRunningScenario;

impl ScenarioSpec for AllNodesRunningScenario {
    fn name(&self) -> String {
        NEW_SCENARIO_NAME.to_string()
    }

    fn build(&self) -> Scenario {
        Scenario::builder(self.name())
            .check(Box::new(AllNodesRunning), "assert all nodes are running")
            .build()
    }
}

/// Names the catalogue specs report for the shared 1 dedicated + 3 storage fixture.
fn expected_catalogue_names() -> Vec<String> {
    vec![
        "node shutdown recovery for node 4".to_string(),
        "link degradation recovery for 2<->3".to_string(),
        "quorum loss recovery stopping 2 storage nodes".to_string(),
        "network partition recovery across 3 storage links".to_string(),
    ]
}

#[test]
fn registry_registers_and_builds_every_catalogue_scenario() {
    let cluster = Cluster::new();
    let mut registry = ScenarioRegistry::new();
    registry.register(Box::new(NodeShutdownRecovery {
        node: NodeId::new(4),
    }));
    registry.register(Box::new(LinkDegradationRecovery {
        link: PeerLink::new(NodeId::new(2), NodeId::new(3)),
        conditions: degraded_conditions(Duration::from_millis(20), 5),
    }));
    registry.register(Box::new(QuorumLossRecovery {
        topology: cluster.topology.clone(),
    }));
    registry.register(Box::new(NetworkPartitionRecovery {
        topology: cluster.topology.clone(),
        conditions: degraded_conditions(Duration::from_millis(20), 5),
    }));

    assert_eq!(registry.len(), 4);
    assert_eq!(
        registry.names().collect::<Vec<String>>(),
        expected_catalogue_names()
    );

    for name in expected_catalogue_names() {
        let scenario = registry
            .build(&name)
            .expect("each registered catalogue spec builds by its own name");
        assert_eq!(scenario.name, name);
    }
    assert!(registry.build("unregistered scenario").is_none());
}

#[test]
fn a_new_scenario_registers_and_runs_without_changing_the_runner() {
    let mut cluster = Cluster::new();
    let mut registry = ScenarioRegistry::new();
    registry.register(Box::new(AllNodesRunningScenario));

    let scenario = registry
        .build(NEW_SCENARIO_NAME)
        .expect("the registered test-local scenario builds by name");
    let report = cluster.run(&scenario);

    assert!(
        report.is_success(),
        "failed step: {:?}",
        report.failed_step().and_then(|step| step.error())
    );
    assert_eq!(report.name, NEW_SCENARIO_NAME);
    assert_eq!(report.steps.len(), 1);
}
