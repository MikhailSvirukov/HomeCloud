//! End-to-end recovery scenarios exercising the reusable library checks.
//!
//! Each test runs a [`Scenario`] through [`ScenarioRunner`] over the
//! in-memory fakes and asserts the resulting [`ScenarioReport`], so the
//! composable checks are validated through the same path scenario authors use.

use std::{net::SocketAddr, time::Duration};

use harness_core::{
    ClusterTopology, FakeFaultInjector, Fault, FaultInjector, HarnessError, InMemoryNodeControl,
    NetworkConditions, NodeId, NodeSpec, PeerLink,
};
use harness_scenario::{
    AllNodesRunning, FaultActive, NoActiveFaults, NodeRunning, NodeStopped, QuorumLost, Scenario,
    ScenarioRunner, StorageQuorumHeld,
};

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// One dedicated server plus three storage nodes, the standard recovery shape.
fn fixture() -> (ClusterTopology, InMemoryNodeControl, FakeFaultInjector) {
    let topology = ClusterTopology::builder()
        .dedicated_server(NodeSpec::dedicated_server(
            NodeId::new(1),
            "dedicated",
            addr(7001),
        ))
        .storage_node(NodeSpec::storage(NodeId::new(2), "storage-1", addr(7002)))
        .storage_node(NodeSpec::storage(NodeId::new(3), "storage-2", addr(7003)))
        .storage_node(NodeSpec::storage(NodeId::new(4), "storage-3", addr(7004)))
        .build()
        .expect("one dedicated plus three storage nodes is valid");
    let nodes = InMemoryNodeControl::new(topology.nodes().map(|spec| spec.id));
    let faults = FakeFaultInjector::new(topology.clone());
    (topology, nodes, faults)
}

fn shutdown(node: u16) -> Fault {
    Fault::NodeShutdown {
        node: NodeId::new(node),
    }
}

fn degradation(a: u16, b: u16) -> Fault {
    Fault::LinkDegradation {
        link: PeerLink::new(NodeId::new(a), NodeId::new(b)),
        conditions: NetworkConditions::new(Duration::from_millis(25), 10)
            .expect("10% loss is within the accepted range"),
    }
}

#[test]
fn node_shutdown_recovery_restores_quorum_and_running_state() {
    let (topology, mut nodes, mut faults) = fixture();
    let stopped = NodeId::new(3);
    let fault = shutdown(3);
    let scenario = Scenario::builder("node shutdown recovery")
        .apply_fault(fault.clone(), "shut down a storage node")
        .stop_node(stopped, "stop the storage node")
        .check(Box::new(NodeStopped(stopped)), "node is stopped")
        .check(
            Box::new(StorageQuorumHeld),
            "two of three storage still hold quorum",
        )
        .start_node(stopped, "restart the storage node")
        .revert_fault(fault.clone(), "clear the shutdown fault")
        .check(Box::new(NodeRunning(stopped)), "node is running again")
        .check(Box::new(AllNodesRunning), "every node is running")
        .check(Box::new(NoActiveFaults), "no faults remain")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(report.is_success());
    assert_eq!(report.steps().len(), 9);
    assert!(report.failed_step().is_none());
}

#[test]
fn link_degradation_round_trip_tracks_active_fault() {
    let (topology, mut nodes, mut faults) = fixture();
    let fault = degradation(2, 3);
    let scenario = Scenario::builder("link degradation round trip")
        .apply_fault(fault.clone(), "degrade peer link")
        .check(
            Box::new(FaultActive(fault.clone())),
            "degraded link is active",
        )
        .revert_fault(fault.clone(), "heal peer link")
        .check(Box::new(NoActiveFaults), "no faults remain")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(report.is_success());
    assert!(faults.active_faults().is_empty());
}

#[test]
fn quorum_lost_check_fails_on_a_healthy_cluster() {
    let (topology, mut nodes, mut faults) = fixture();
    let fault = degradation(2, 3);
    let scenario = Scenario::builder("quorum lost expectation")
        .apply_fault(fault.clone(), "degrade peer link")
        .check(
            Box::new(FaultActive(fault.clone())),
            "degraded link is active",
        )
        .check(Box::new(QuorumLost), "quorum is lost")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(!report.is_success());
    assert_eq!(report.steps().len(), 3);
    let failed = report
        .failed_step()
        .expect("the quorum check fails while all storage nodes run");
    assert_eq!(failed.description(), "quorum is lost");
    let expected = HarnessError::CheckFailed {
        message: "quorum not lost: 3 of 3 storage nodes running, quorum 2".to_string(),
    }
    .to_string();
    assert_eq!(failed.error(), Some(expected.as_str()));
}

#[test]
fn runner_stops_at_first_failing_check_with_check_failed_message() {
    let (topology, mut nodes, mut faults) = fixture();
    let scenario = Scenario::builder("failing check stops the run")
        .check(Box::new(FaultActive(shutdown(3))), "a fault is active")
        .check(Box::new(NodeStopped(NodeId::new(4))), "a node is stopped")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(!report.is_success());
    assert_eq!(report.steps().len(), 1);
    let failed = report
        .failed_step()
        .expect("the first check fails before the second step runs");
    assert_eq!(failed.description(), "a fault is active");
    let expected = HarnessError::CheckFailed {
        message: "expected active fault: node shutdown 3".to_string(),
    }
    .to_string();
    assert_eq!(failed.error(), Some(expected.as_str()));
}
