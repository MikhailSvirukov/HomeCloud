//! End-to-end recovery scenarios exercising the reusable library checks.
//!
//! Each test runs a [`Scenario`] through [`harness_scenario::ScenarioRunner`]
//! over the in-memory fakes and asserts the resulting
//! [`harness_scenario::ScenarioReport`], so the composable checks are validated
//! through the same path scenario authors use.

mod common;

use std::time::Duration;

use harness_core::{Fault, FaultInjector, HarnessError, NodeId, PeerLink};
use harness_scenario::{
    AllNodesRunning, FaultActive, NoActiveFaults, NodeRunning, NodeStopped, QuorumLost, Scenario,
    StorageQuorumHeld,
};

use common::{Cluster, degraded_conditions};

fn shutdown(node: u16) -> Fault {
    Fault::NodeShutdown {
        node: NodeId::new(node),
    }
}

fn degradation(a: u16, b: u16) -> Fault {
    Fault::LinkDegradation {
        link: PeerLink::new(NodeId::new(a), NodeId::new(b)),
        conditions: degraded_conditions(Duration::from_millis(25), 10),
    }
}

#[test]
fn node_shutdown_recovery_restores_quorum_and_running_state() {
    let mut cluster = Cluster::new();
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
    let report = cluster.run(&scenario);
    assert!(report.is_success());
    assert_eq!(report.steps.len(), 9);
    assert!(report.failed_step().is_none());
}

#[test]
fn link_degradation_round_trip_tracks_active_fault() {
    let mut cluster = Cluster::new();
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
    let report = cluster.run(&scenario);
    assert!(report.is_success());
    assert!(cluster.faults.active_faults().is_empty());
}

#[test]
fn quorum_lost_check_fails_on_a_healthy_cluster() {
    let mut cluster = Cluster::new();
    let fault = degradation(2, 3);
    let scenario = Scenario::builder("quorum lost expectation")
        .apply_fault(fault.clone(), "degrade peer link")
        .check(
            Box::new(FaultActive(fault.clone())),
            "degraded link is active",
        )
        .check(Box::new(QuorumLost), "quorum is lost")
        .build();
    let report = cluster.run(&scenario);
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
