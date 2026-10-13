//! Standard fault/recovery scenarios built from the reusable [`crate::checks`].
//!
//! Each catalogue entry composes the two base fault classes from the v1
//! specification (full node shutdown and selective network degradation) with the
//! reusable recovery checks, so tests share one canonical recovery flow instead
//! of restating the steps.

use harness_core::{Fault, NetworkConditions, NodeId, PeerLink};

use crate::Scenario;
use crate::checks::{FaultActive, NoActiveFaults, NodeRunning, NodeStopped, StorageQuorumHeld};

/// Standard recovery scenario for the full-node fault class: shut a storage node
/// down, assert quorum and node state, bring it back, and assert full recovery.
///
/// The node is stopped and its shutdown fault applied, then `NodeStopped` and
/// `StorageQuorumHeld` are asserted before restarting it and asserting
/// `NodeRunning` plus `NoActiveFaults`.
pub fn node_shutdown_recovery(node: NodeId) -> Scenario {
    let fault = Fault::NodeShutdown { node };
    Scenario::builder(format!("node shutdown recovery for node {node}"))
        .apply_fault(
            fault.clone(),
            format!("apply node shutdown fault for node {node}"),
        )
        .stop_node(node, format!("stop node {node}"))
        .check(
            Box::new(NodeStopped(node)),
            format!("assert node {node} is stopped"),
        )
        .check(Box::new(StorageQuorumHeld), "assert storage quorum is held")
        .start_node(node, format!("start node {node}"))
        .revert_fault(fault, format!("revert node shutdown fault for node {node}"))
        .check(
            Box::new(NodeRunning(node)),
            format!("assert node {node} is running"),
        )
        .check(Box::new(NoActiveFaults), "assert no active faults")
        .build()
}

/// Standard recovery scenario for the selective-network fault class: degrade a
/// peer link, assert the fault and quorum, heal the link, and assert no faults.
///
/// The `Fault::LinkDegradation` is constructed once and reused for apply, the
/// `FaultActive` check, and revert, so all steps refer to the same fault.
pub fn link_degradation_recovery(link: PeerLink, conditions: NetworkConditions) -> Scenario {
    let fault = Fault::LinkDegradation { link, conditions };
    Scenario::builder(format!("link degradation recovery for {link}"))
        .apply_fault(
            fault.clone(),
            format!("apply link degradation fault for {link}"),
        )
        .check(
            Box::new(FaultActive(fault.clone())),
            format!("assert link degradation fault is active on {link}"),
        )
        .check(Box::new(StorageQuorumHeld), "assert storage quorum is held")
        .revert_fault(fault, format!("revert link degradation fault for {link}"))
        .check(Box::new(NoActiveFaults), "assert no active faults")
        .build()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use harness_core::{
        ClusterTopology, FakeFaultInjector, FaultInjector, InMemoryNodeControl, NodeControl,
        NodeSpec, NodeState,
    };

    use super::*;
    use crate::scenario::{ScenarioReport, ScenarioRunner};

    const NODE_SHUTDOWN_STEP_COUNT: usize = 8;
    const LINK_DEGRADATION_STEP_COUNT: usize = 5;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn topology() -> ClusterTopology {
        ClusterTopology::builder()
            .dedicated_server(NodeSpec::dedicated_server(
                NodeId::new(1),
                "dedicated",
                addr(7001),
            ))
            .storage_node(NodeSpec::storage(NodeId::new(2), "storage-2", addr(7002)))
            .storage_node(NodeSpec::storage(NodeId::new(3), "storage-3", addr(7003)))
            .storage_node(NodeSpec::storage(NodeId::new(4), "storage-4", addr(7004)))
            .build()
            .expect("test topology is valid")
    }

    /// A 1 dedicated + 3 storage cluster with all nodes running and no faults.
    struct Fixture {
        topology: ClusterTopology,
        nodes: InMemoryNodeControl,
        faults: FakeFaultInjector,
    }

    impl Fixture {
        fn new() -> Self {
            let topology = topology();
            Self {
                nodes: InMemoryNodeControl::new([
                    NodeId::new(1),
                    NodeId::new(2),
                    NodeId::new(3),
                    NodeId::new(4),
                ]),
                faults: FakeFaultInjector::new(topology.clone()),
                topology,
            }
        }

        fn run(&mut self, scenario: &Scenario) -> ScenarioReport {
            ScenarioRunner::new(&self.topology, &mut self.nodes, &mut self.faults).run(scenario)
        }
    }

    #[test]
    fn node_shutdown_recovery_runs_to_success_with_expected_steps() {
        let mut fixture = Fixture::new();
        let scenario = node_shutdown_recovery(NodeId::new(2));
        let report = fixture.run(&scenario);

        assert_eq!(scenario.steps().len(), NODE_SHUTDOWN_STEP_COUNT);
        assert!(
            report.is_success(),
            "failed step: {:?}",
            report.failed_step().and_then(|step| step.error())
        );
    }

    #[test]
    fn node_shutdown_recovery_stops_then_starts_the_node() {
        let mut fixture = Fixture::new();
        let scenario = node_shutdown_recovery(NodeId::new(2));
        let report = fixture.run(&scenario);

        let stop_check = report
            .steps()
            .get(2)
            .expect("scenario has a stopped-node check at index 2");
        assert!(stop_check.is_ok());
        assert_eq!(fixture.nodes.state(NodeId::new(2)), Ok(NodeState::Running));
        assert!(fixture.faults.active_faults().is_empty());
    }

    #[test]
    fn link_degradation_recovery_runs_and_leaves_no_active_faults() {
        let mut fixture = Fixture::new();
        let link = PeerLink::new(NodeId::new(2), NodeId::new(3));
        let scenario = link_degradation_recovery(link, NetworkConditions::healthy());
        let report = fixture.run(&scenario);

        assert_eq!(scenario.steps().len(), LINK_DEGRADATION_STEP_COUNT);
        assert!(
            report.is_success(),
            "failed step: {:?}",
            report.failed_step().and_then(|step| step.error())
        );
        assert!(fixture.faults.active_faults().is_empty());
    }

    #[test]
    fn link_degradation_recovery_name_mentions_the_link() {
        let link = PeerLink::new(NodeId::new(2), NodeId::new(3));
        let scenario = link_degradation_recovery(link, NetworkConditions::healthy());
        assert!(scenario.name().contains(&link.to_string()));
    }

    #[test]
    fn node_shutdown_recovery_for_unknown_node_reports_a_failure() {
        let mut fixture = Fixture::new();
        let scenario = node_shutdown_recovery(NodeId::new(99));
        let report = fixture.run(&scenario);

        assert!(!report.is_success());
        let failure = report
            .failed_step()
            .expect("unknown node must fail the first step");
        assert!(matches!(
            failure.error(),
            Some(error) if error.contains("unknown node id 99")
        ));
    }
}
