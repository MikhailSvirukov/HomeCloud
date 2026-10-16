//! Public-API coverage for the standard scenario catalogue.
//!
//! These integration tests consume the re-exported `node_shutdown_recovery`,
//! `link_degradation_recovery`, `quorum_loss_recovery`, and
//! `network_partition_recovery` entries exactly as a downstream harness user
//! would, running them through [`ScenarioRunner`] over the in-memory fakes.

use std::time::Duration;

use harness_core::{
    ClusterTopology, FakeFaultInjector, Fault, FaultInjector, HarnessError, InMemoryNodeControl,
    LossPercent, NetworkConditions, NodeControl, NodeId, NodeState, PeerLink,
};
use harness_scenario::{
    Action, Scenario, ScenarioReport, ScenarioRunner, link_degradation_recovery,
    network_partition_recovery, node_shutdown_recovery, quorum_loss_recovery,
};
use harness_test_support::{faults, nodes, topology};

const NODE_SHUTDOWN_STEP_COUNT: usize = 8;
const LINK_DEGRADATION_STEP_COUNT: usize = 5;
const QUORUM_LOSS_STEP_COUNT: usize = 12;
const NETWORK_PARTITION_STEP_COUNT: usize = 13;
const STORAGE_LINK_COUNT: usize = 3;

/// A 1 dedicated + 3 storage cluster with all nodes running and no faults.
struct Cluster {
    topology: ClusterTopology,
    nodes: InMemoryNodeControl,
    faults: FakeFaultInjector,
}

impl Cluster {
    fn new() -> Self {
        let cluster = topology(3);
        let node_control = nodes(&cluster);
        let fault_injector = faults(&cluster);
        Self {
            topology: cluster,
            nodes: node_control,
            faults: fault_injector,
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

/// Index of the first step whose action satisfies `predicate`.
fn first_step_index(scenario: &Scenario, predicate: impl Fn(&Action) -> bool) -> Option<usize> {
    scenario
        .steps
        .iter()
        .position(|step| predicate(&step.action))
}

/// Every storage-to-storage link the multi-link partition scenario should degrade.
fn storage_links(topology: &ClusterTopology) -> Vec<PeerLink> {
    let ids: Vec<NodeId> = topology
        .storage_nodes()
        .iter()
        .map(|spec| spec.id)
        .collect();
    ids.iter()
        .enumerate()
        .flat_map(|(index, &a)| ids[index + 1..].iter().map(move |&b| PeerLink::new(a, b)))
        .collect()
}

/// Links targeted by `ApplyFault` link-degradation steps, in scenario order.
fn degraded_links(scenario: &Scenario) -> Vec<PeerLink> {
    scenario
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            Action::ApplyFault(Fault::LinkDegradation { link, .. }) => Some(*link),
            _ => None,
        })
        .collect()
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

#[test]
fn quorum_loss_recovery_loses_then_restores_quorum() {
    let mut cluster = Cluster::new();
    let scenario = quorum_loss_recovery(&cluster.topology);
    let report = cluster.run(&scenario);

    assert!(
        report.is_success(),
        "failed step: {:?}",
        report.failed_step().and_then(|step| step.error())
    );
    assert_eq!(report.steps.len(), QUORUM_LOSS_STEP_COUNT);
    assert!(cluster.faults.active_faults().is_empty());

    let stop = first_step_index(&scenario, |action| matches!(action, Action::StopNode(_)))
        .expect("quorum loss scenario stops storage nodes");
    let start = first_step_index(&scenario, |action| matches!(action, Action::StartNode(_)))
        .expect("quorum loss scenario restarts the stopped nodes");
    assert!(
        stop < start,
        "stop step {stop} must precede start step {start}"
    );
}

#[test]
fn network_partition_recovery_degrades_and_heals_every_storage_link() {
    let mut cluster = Cluster::new();
    let expected_links = storage_links(&cluster.topology);
    let conditions = NetworkConditions::new(
        Duration::from_millis(20),
        LossPercent::new(5).expect("valid conditions"),
    );
    let scenario = network_partition_recovery(&cluster.topology, conditions);
    let report = cluster.run(&scenario);

    assert!(
        report.is_success(),
        "failed step: {:?}",
        report.failed_step().and_then(|step| step.error())
    );
    assert_eq!(report.steps.len(), NETWORK_PARTITION_STEP_COUNT);
    assert!(cluster.faults.active_faults().is_empty());

    let links = degraded_links(&scenario);
    assert_eq!(links.len(), STORAGE_LINK_COUNT);
    assert_eq!(links, expected_links);

    for link in &links {
        let apply = link_transition_index(&scenario, *link, true)
            .expect("network partition scenario applies every storage link");
        let revert = link_transition_index(&scenario, *link, false)
            .expect("network partition scenario heals every storage link");
        assert!(
            apply < revert,
            "apply step {apply} must precede revert step {revert} for {link}"
        );
    }
}
