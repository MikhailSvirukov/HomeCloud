//! Public-API coverage for the standard scenario catalogue.
//!
//! These integration tests consume the re-exported `node_shutdown_recovery` and
//! `link_degradation_recovery` entries exactly as a downstream harness user
//! would, running them through [`ScenarioRunner`] over the in-memory fakes.

use std::{net::SocketAddr, time::Duration};

use harness_core::{
    ClusterTopology, FakeFaultInjector, Fault, FaultInjector, HarnessError, InMemoryNodeControl,
    LossPercent, NetworkConditions, NodeControl, NodeId, NodeSpec, NodeState, PeerLink,
};
use harness_scenario::{
    Action, Scenario, ScenarioReport, ScenarioRunner, link_degradation_recovery,
    node_shutdown_recovery,
};

const NODE_SHUTDOWN_STEP_COUNT: usize = 8;
const LINK_DEGRADATION_STEP_COUNT: usize = 5;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// A 1 dedicated + 3 storage cluster with all nodes running and no faults.
struct Cluster {
    topology: ClusterTopology,
    nodes: InMemoryNodeControl,
    faults: FakeFaultInjector,
}

impl Cluster {
    fn new() -> Self {
        let topology = ClusterTopology::builder()
            .dedicated_server(NodeSpec::dedicated_server(
                NodeId::new(1),
                "dedicated",
                addr(7001),
            ))
            .storage_node(NodeSpec::storage(NodeId::new(2), "storage-2", addr(7002)))
            .storage_node(NodeSpec::storage(NodeId::new(3), "storage-3", addr(7003)))
            .storage_node(NodeSpec::storage(NodeId::new(4), "storage-4", addr(7004)))
            .build()
            .expect("one dedicated plus three storage nodes is valid");
        let nodes = InMemoryNodeControl::new(topology.nodes().map(|spec| spec.id));
        let faults = FakeFaultInjector::new(topology.clone());
        Self {
            topology,
            nodes,
            faults,
        }
    }

    fn run(&mut self, scenario: &Scenario) -> ScenarioReport {
        ScenarioRunner::new(&self.topology, &mut self.nodes, &mut self.faults).run(scenario)
    }
}

/// Index of the first `StopNode`/`StartNode` step targeting `node`.
fn node_transition_index(scenario: &Scenario, node: NodeId, start: bool) -> Option<usize> {
    scenario.steps.iter().position(|step| match &step.action {
        Action::StopNode(id) => !start && *id == node,
        Action::StartNode(id) => start && *id == node,
        Action::ApplyFault(_) | Action::RevertFault(_) | Action::Check(_) => false,
    })
}

/// Index of the first apply/revert step carrying a link-degradation fault.
fn link_transition_index(scenario: &Scenario, link: PeerLink, apply: bool) -> Option<usize> {
    scenario.steps.iter().position(|step| match &step.action {
        Action::ApplyFault(Fault::LinkDegradation { link: l, .. }) => apply && *l == link,
        Action::RevertFault(Fault::LinkDegradation { link: l, .. }) => !apply && *l == link,
        Action::ApplyFault(_)
        | Action::RevertFault(_)
        | Action::StopNode(_)
        | Action::StartNode(_)
        | Action::Check(_) => false,
    })
}

#[test]
fn node_shutdown_recovery_scenario_stops_then_restores_the_node() {
    let mut cluster = Cluster::new();
    let node = NodeId::new(4);
    let scenario = node_shutdown_recovery(node);
    let report = cluster.run(&scenario);

    assert!(
        report.is_success(),
        "failed step: {:?}",
        report.failed_step().and_then(|step| step.error())
    );
    assert_eq!(report.steps.len(), NODE_SHUTDOWN_STEP_COUNT);
    assert_eq!(cluster.nodes.state(node), Ok(NodeState::Running));
    assert!(cluster.faults.active_faults().is_empty());

    let stop = node_transition_index(&scenario, node, false)
        .expect("catalogue scenario stops the target node");
    let start =
        node_transition_index(&scenario, node, true).expect("catalogue scenario starts the node");
    assert!(
        stop < start,
        "stop step {stop} must precede start step {start}"
    );
}

#[test]
fn link_degradation_recovery_scenario_heals_the_link() {
    let mut cluster = Cluster::new();
    let link = PeerLink::new(NodeId::new(2), NodeId::new(3));
    let conditions = NetworkConditions::new(
        Duration::from_millis(20),
        LossPercent::new(5).expect("valid conditions"),
    );
    let scenario = link_degradation_recovery(link, conditions);
    let report = cluster.run(&scenario);

    assert!(
        report.is_success(),
        "failed step: {:?}",
        report.failed_step().and_then(|step| step.error())
    );
    assert_eq!(report.steps.len(), LINK_DEGRADATION_STEP_COUNT);
    assert!(cluster.faults.active_faults().is_empty());

    let apply = link_transition_index(&scenario, link, true)
        .expect("catalogue scenario applies the degradation");
    let revert =
        link_transition_index(&scenario, link, false).expect("catalogue scenario heals the link");
    assert!(
        apply < revert,
        "apply step {apply} must precede revert step {revert}"
    );
}

#[test]
fn catalogue_scenario_fails_when_the_target_node_is_unknown() {
    let mut cluster = Cluster::new();
    let scenario = node_shutdown_recovery(NodeId::new(99));
    let report = cluster.run(&scenario);

    assert!(!report.is_success());
    let failure = report
        .failed_step()
        .expect("an unknown node must fail the first catalogue step");
    assert!(
        matches!(
            failure.error(),
            Some(HarnessError::UnknownNode { id }) if *id == NodeId::new(99)
        ),
        "unexpected failure: {:?}",
        failure.error()
    );
}
