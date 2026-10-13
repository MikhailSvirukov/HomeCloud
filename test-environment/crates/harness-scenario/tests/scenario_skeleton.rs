//! Fast, Docker-free scenario-engine tests over the harness-core fakes.

use std::{net::SocketAddr, time::Duration};

use harness_core::{
    ClusterTopology, FakeFaultInjector, Fault, FaultInjector, HarnessError, InMemoryNodeControl,
    LossPercent, NetworkConditions, NodeControl, NodeId, NodeSpec, NodeState, PeerLink,
};
use harness_scenario::{
    FaultActive, NoActiveFaults, NodeRunning, Scenario, ScenarioCheck, ScenarioContext,
    ScenarioRunner, StorageQuorumHeld,
};

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

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

fn loss(percent: u8) -> LossPercent {
    LossPercent::new(percent).expect("loss percent is within the accepted range")
}

type Assert = Box<dyn Fn(&ScenarioContext<'_>) -> Result<(), HarnessError>>;

/// A custom [`ScenarioCheck`] that delegates to a closure.
struct Check(&'static str, Assert);

fn check(
    name: &'static str,
    assert: impl Fn(&ScenarioContext<'_>) -> Result<(), HarnessError> + 'static,
) -> Box<dyn ScenarioCheck> {
    Box::new(Check(name, Box::new(assert)))
}

impl ScenarioCheck for Check {
    fn name(&self) -> &str {
        self.0
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        (self.1)(context)
    }
}

fn always_fails(message: &'static str) -> Box<dyn ScenarioCheck> {
    check("always fails", move |_context| {
        Err(HarnessError::CheckFailed {
            message: message.to_string(),
        })
    })
}

#[test]
fn happy_path_scenario_executes_every_step() {
    let (topology, mut nodes, mut faults) = fixture();
    let degradation = Fault::LinkDegradation {
        link: PeerLink::new(NodeId::new(2), NodeId::new(3)),
        conditions: NetworkConditions::new(Duration::from_millis(25), loss(10)),
    };
    let stopped = NodeId::new(4);
    let scenario = Scenario::builder("peer degradation and node recovery")
        .apply_fault(degradation.clone(), "degrade peer link")
        .check(
            Box::new(FaultActive(degradation.clone())),
            "degraded link is active",
        )
        .revert_fault(degradation.clone(), "heal peer link")
        .stop_node(stopped, "stop a storage node")
        .check(
            Box::new(StorageQuorumHeld),
            "remaining storage keeps quorum",
        )
        .start_node(stopped, "restart the storage node")
        .check(Box::new(NodeRunning(stopped)), "restarted node is running")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(report.is_success());
    assert_eq!(report.steps.len(), 7);
    assert!(report.failed_step().is_none());
}

#[test]
fn runner_stops_at_first_failing_check() {
    let (topology, mut nodes, mut faults) = fixture();
    let message = "recovery assertion failed";
    let scenario = Scenario::builder("failing check")
        .check(always_fails(message), "always fails")
        .stop_node(NodeId::new(2), "must not run")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(!report.is_success());
    assert_eq!(report.steps.len(), 1);
    let failed = report.failed_step().expect("a failing step is recorded");
    assert_eq!(failed.description, "always fails");
    assert_eq!(
        failed.error(),
        Some(&HarnessError::CheckFailed {
            message: message.to_string(),
        })
    );
    assert_eq!(nodes.state(NodeId::new(2)), Ok(NodeState::Running));
}

#[test]
fn node_shutdown_fault_apply_and_revert_track_active_set() {
    let (topology, mut nodes, mut faults) = fixture();
    let shutdown = Fault::NodeShutdown {
        node: NodeId::new(3),
    };
    let scenario = Scenario::builder("node shutdown round trip")
        .apply_fault(shutdown.clone(), "apply node shutdown")
        .check(
            Box::new(FaultActive(shutdown.clone())),
            "shutdown is active",
        )
        .revert_fault(shutdown.clone(), "revert node shutdown")
        .check(Box::new(NoActiveFaults), "shutdown is gone")
        .build();
    let report = ScenarioRunner::new(&topology, &mut nodes, &mut faults).run(&scenario);
    assert!(report.is_success());
    assert!(faults.active_faults().is_empty());
}
