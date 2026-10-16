//! End-to-end recovery scenarios exercising the reusable library checks.
//!
//! Each test runs a [`Scenario`] through [`ScenarioRunner`] over the
//! in-memory fakes and asserts the resulting [`ScenarioReport`], so the
//! composable checks are validated through the same path scenario authors use.

use std::time::Duration;

use harness_core::{
    ClusterTopology, FakeFaultInjector, Fault, FaultInjector, HarnessError, InMemoryNodeControl,
    LossPercent, NetworkConditions, NodeId, PeerLink,
};
use harness_scenario::{
    AllNodesRunning, FaultActive, NoActiveFaults, NodeRunning, NodeStopped, QuorumLost, Scenario,
    ScenarioRunner, StorageQuorumHeld,
};
use harness_test_support::{faults, nodes, topology};

/// One dedicated server plus three storage nodes, the standard recovery shape.
fn fixture() -> (ClusterTopology, InMemoryNodeControl, FakeFaultInjector) {
    let cluster = topology(3);
    let node_control = nodes(&cluster);
    let fault_injector = faults(&cluster);
    (cluster, node_control, fault_injector)
}

fn shutdown(node: u16) -> Fault {
    Fault::NodeShutdown {
        node: NodeId::new(node),
    }
}

fn degradation(a: u16, b: u16) -> Fault {
    Fault::LinkDegradation {
        link: PeerLink::new(NodeId::new(a), NodeId::new(b)),
        conditions: NetworkConditions::new(
            Duration::from_millis(25),
            LossPercent::new(10).expect("10% loss is within the accepted range"),
        ),
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
    assert_eq!(report.steps.len(), 9);
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
    assert_eq!(report.steps.len(), 3);
    let failed = report
        .failed_step()
        .expect("the quorum check fails while all storage nodes run");
    assert_eq!(failed.description, "quorum is lost");
    assert_eq!(
        failed.error(),
        Some(&HarnessError::StorageQuorumNotLost {
            running: 3,
            total: 3,
            quorum: 2,
        })
    );
}
