//! Reusable [`ScenarioCheck`] blocks for recovery assertions.
//!
//! Each type implements one small, composable assertion over a
//! [`ScenarioContext`] so scenario tests build recovery expectations from
//! library blocks instead of rewriting the same logic. Failures are reported as
//! [`HarnessError::CheckFailed`] with a message that names the observed state.

use harness_core::{Fault, HarnessError, NodeId, NodeState};

use crate::{ScenarioCheck, ScenarioContext};

const NAME_ALL_NODES_RUNNING: &str = "all nodes running";
const NAME_NODE_RUNNING: &str = "node running";
const NAME_NODE_STOPPED: &str = "node stopped";
const NAME_STORAGE_QUORUM_HELD: &str = "storage quorum held";
const NAME_QUORUM_LOST: &str = "quorum lost";
const NAME_FAULT_ACTIVE: &str = "fault active";
const NAME_NO_ACTIVE_FAULTS: &str = "no active faults";

const STATE_RUNNING: &str = "running";
const STATE_STOPPED: &str = "stopped";

const NODE_PREFIX: &str = "node";
const EXPECTED: &str = "expected";

const QUORUM_HELD_FAILURE: &str = "storage quorum not held";
const QUORUM_LOST_FAILURE: &str = "quorum not lost";
const STORAGE: &str = "storage";
const QUORUM: &str = "quorum";

const FAULT_ACTIVE_FAILURE: &str = "expected active fault";
const NO_ACTIVE_FAULTS_FAILURE: &str = "expected no active faults";

/// Lower-case label for a node state, kept stable across failure messages.
fn state_label(state: NodeState) -> &'static str {
    match state {
        NodeState::Running => STATE_RUNNING,
        NodeState::Stopped => STATE_STOPPED,
    }
}

/// Builds the shared "node `<id>` is `<observed>`, expected `<expected>`" failure.
fn node_state_failure(node: NodeId, observed: NodeState, expected: &str) -> HarnessError {
    HarnessError::CheckFailed {
        message: format!(
            "{NODE_PREFIX} {node} is {}, {EXPECTED} {expected}",
            state_label(observed)
        ),
    }
}

/// Counts storage nodes currently [`NodeState::Running`].
fn running_storage_count(context: &ScenarioContext<'_>) -> Result<usize, HarnessError> {
    let mut running = 0;
    for spec in context.topology().storage_nodes() {
        if context.nodes().state(spec.id)? == NodeState::Running {
            running += 1;
        }
    }
    Ok(running)
}

/// Builds the shared quorum failure naming the running count and quorum size.
fn quorum_failure(prefix: &str, running: usize, total: usize, quorum: usize) -> HarnessError {
    HarnessError::CheckFailed {
        message: format!(
            "{prefix}: {running} of {total} {STORAGE} nodes {}, {QUORUM} {quorum}",
            STATE_RUNNING
        ),
    }
}

/// Fails unless every node in the topology is running.
pub struct AllNodesRunning;

impl ScenarioCheck for AllNodesRunning {
    fn name(&self) -> &str {
        NAME_ALL_NODES_RUNNING
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        let failure: Option<(NodeId, NodeState)> =
            context.topology().nodes().try_fold(None, |failure, spec| {
                if failure.is_some() {
                    Ok(failure)
                } else {
                    let state = context.nodes().state(spec.id)?;
                    Ok(if state == NodeState::Running {
                        None
                    } else {
                        Some((spec.id, state))
                    })
                }
            })?;
        match failure {
            Some((node, observed)) => Err(node_state_failure(node, observed, STATE_RUNNING)),
            None => Ok(()),
        }
    }
}

/// Fails unless the given node is running.
pub struct NodeRunning(pub NodeId);

impl ScenarioCheck for NodeRunning {
    fn name(&self) -> &str {
        NAME_NODE_RUNNING
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        let observed = context.nodes().state(self.0)?;
        if observed == NodeState::Running {
            Ok(())
        } else {
            Err(node_state_failure(self.0, observed, STATE_RUNNING))
        }
    }
}

/// Fails unless the given node is stopped.
pub struct NodeStopped(pub NodeId);

impl ScenarioCheck for NodeStopped {
    fn name(&self) -> &str {
        NAME_NODE_STOPPED
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        let observed = context.nodes().state(self.0)?;
        if observed == NodeState::Stopped {
            Ok(())
        } else {
            Err(node_state_failure(self.0, observed, STATE_STOPPED))
        }
    }
}

/// Fails unless enough storage nodes are running to keep quorum.
pub struct StorageQuorumHeld;

impl ScenarioCheck for StorageQuorumHeld {
    fn name(&self) -> &str {
        NAME_STORAGE_QUORUM_HELD
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        let quorum = context.topology().quorum();
        let running = running_storage_count(context)?;
        if running >= quorum {
            Ok(())
        } else {
            Err(quorum_failure(
                QUORUM_HELD_FAILURE,
                running,
                context.topology().storage_nodes().len(),
                quorum,
            ))
        }
    }
}

/// Fails unless fewer storage nodes are running than the quorum size.
pub struct QuorumLost;

impl ScenarioCheck for QuorumLost {
    fn name(&self) -> &str {
        NAME_QUORUM_LOST
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        let quorum = context.topology().quorum();
        let running = running_storage_count(context)?;
        if running < quorum {
            Ok(())
        } else {
            Err(quorum_failure(
                QUORUM_LOST_FAILURE,
                running,
                context.topology().storage_nodes().len(),
                quorum,
            ))
        }
    }
}

/// Fails unless the given fault is active.
pub struct FaultActive(pub Fault);

impl ScenarioCheck for FaultActive {
    fn name(&self) -> &str {
        NAME_FAULT_ACTIVE
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        if context.faults().active_faults().contains(&self.0) {
            Ok(())
        } else {
            Err(HarnessError::CheckFailed {
                message: format!("{FAULT_ACTIVE_FAILURE}: {}", self.0),
            })
        }
    }
}

/// Fails unless no faults are active.
pub struct NoActiveFaults;

impl ScenarioCheck for NoActiveFaults {
    fn name(&self) -> &str {
        NAME_NO_ACTIVE_FAULTS
    }
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError> {
        match context.faults().active_faults().first() {
            None => Ok(()),
            Some(fault) => Err(HarnessError::CheckFailed {
                message: format!("{NO_ACTIVE_FAULTS_FAILURE}, found {fault}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use harness_core::{
        ClusterTopology, FakeFaultInjector, FaultInjector, InMemoryNodeControl, NodeControl,
        NodeSpec,
    };

    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn storage_ids(storage: usize) -> Vec<NodeId> {
        (0..storage).map(|i| NodeId::new((i + 2) as u16)).collect()
    }

    fn topology(storage: usize) -> ClusterTopology {
        let builder = ClusterTopology::builder().dedicated_server(NodeSpec::dedicated_server(
            NodeId::new(1),
            "dedicated",
            addr(7001),
        ));
        storage_ids(storage)
            .into_iter()
            .fold(builder, |builder, id| {
                builder.storage_node(NodeSpec::storage(
                    id,
                    format!("storage-{}", id.get()),
                    addr(7001 + id.get()),
                ))
            })
            .build()
            .expect("valid test topology")
    }

    /// A ready-to-build [`ScenarioContext`] with all nodes running and no faults.
    struct Fixture {
        topology: ClusterTopology,
        nodes: InMemoryNodeControl,
        faults: FakeFaultInjector,
    }

    impl Fixture {
        fn new(storage: usize) -> Self {
            let topology = topology(storage);
            let ids = std::iter::once(NodeId::new(1)).chain(storage_ids(storage));
            Self {
                nodes: InMemoryNodeControl::new(ids),
                faults: FakeFaultInjector::new(topology.clone()),
                topology,
            }
        }

        fn context(&self) -> ScenarioContext<'_> {
            ScenarioContext::new(&self.topology, &self.nodes, &self.faults)
        }
    }

    fn shutdown(node: u16) -> Fault {
        Fault::NodeShutdown {
            node: NodeId::new(node),
        }
    }

    #[test]
    fn checks_expose_stable_names() {
        assert_eq!(AllNodesRunning.name(), "all nodes running");
        assert_eq!(NodeRunning(NodeId::new(2)).name(), "node running");
        assert_eq!(NodeStopped(NodeId::new(2)).name(), "node stopped");
        assert_eq!(StorageQuorumHeld.name(), "storage quorum held");
        assert_eq!(QuorumLost.name(), "quorum lost");
        assert_eq!(FaultActive(shutdown(2)).name(), "fault active");
        assert_eq!(NoActiveFaults.name(), "no active faults");
    }

    #[test]
    fn all_nodes_running_passes_when_every_node_is_running() {
        let fixture = Fixture::new(3);
        assert_eq!(AllNodesRunning.check(&fixture.context()), Ok(()));
    }

    #[test]
    fn all_nodes_running_fails_when_a_node_is_stopped() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(2)), Ok(()));
        let result = AllNodesRunning.check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "node 2 is stopped, expected running"
        ));
    }

    #[test]
    fn all_nodes_running_propagates_node_control_errors() {
        let topology = topology(3);
        let nodes = InMemoryNodeControl::new([NodeId::new(1)]);
        let faults = FakeFaultInjector::new(topology.clone());
        let context = ScenarioContext::new(&topology, &nodes, &faults);
        assert_eq!(
            AllNodesRunning.check(&context),
            Err(HarnessError::UnknownNode { id: 2 })
        );
    }

    #[test]
    fn node_running_passes_for_a_running_node() {
        let fixture = Fixture::new(3);
        assert_eq!(
            NodeRunning(NodeId::new(3)).check(&fixture.context()),
            Ok(())
        );
    }

    #[test]
    fn node_running_fails_for_a_stopped_node() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(3)), Ok(()));
        let result = NodeRunning(NodeId::new(3)).check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "node 3 is stopped, expected running"
        ));
    }

    #[test]
    fn node_running_propagates_unknown_node_error() {
        let fixture = Fixture::new(3);
        assert_eq!(
            NodeRunning(NodeId::new(99)).check(&fixture.context()),
            Err(HarnessError::UnknownNode { id: 99 })
        );
    }

    #[test]
    fn node_stopped_passes_for_a_stopped_node() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(4)), Ok(()));
        assert_eq!(
            NodeStopped(NodeId::new(4)).check(&fixture.context()),
            Ok(())
        );
    }

    #[test]
    fn node_stopped_fails_for_a_running_node() {
        let fixture = Fixture::new(3);
        let result = NodeStopped(NodeId::new(4)).check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "node 4 is running, expected stopped"
        ));
    }

    #[test]
    fn storage_quorum_held_passes_with_two_of_three_storage_running() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(2)), Ok(()));
        assert_eq!(StorageQuorumHeld.check(&fixture.context()), Ok(()));
    }

    #[test]
    fn storage_quorum_held_fails_with_one_of_three_storage_running() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(2)), Ok(()));
        assert_eq!(fixture.nodes.stop(NodeId::new(3)), Ok(()));
        let result = StorageQuorumHeld.check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "storage quorum not held: 1 of 3 storage nodes running, quorum 2"
        ));
    }

    #[test]
    fn quorum_lost_passes_with_one_of_three_storage_running() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(2)), Ok(()));
        assert_eq!(fixture.nodes.stop(NodeId::new(3)), Ok(()));
        assert_eq!(QuorumLost.check(&fixture.context()), Ok(()));
    }

    #[test]
    fn quorum_lost_fails_with_two_of_three_storage_running() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(2)), Ok(()));
        let result = QuorumLost.check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "quorum not lost: 2 of 3 storage nodes running, quorum 2"
        ));
    }

    #[test]
    fn fault_active_passes_when_the_fault_is_active() {
        let mut fixture = Fixture::new(3);
        let fault = shutdown(2);
        assert_eq!(fixture.faults.apply(&fault), Ok(()));
        assert_eq!(FaultActive(fault).check(&fixture.context()), Ok(()));
    }

    #[test]
    fn fault_active_fails_when_the_fault_is_not_active() {
        let fixture = Fixture::new(3);
        let result = FaultActive(shutdown(2)).check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "expected active fault: node shutdown 2"
        ));
    }

    #[test]
    fn no_active_faults_passes_when_none_are_active() {
        let fixture = Fixture::new(3);
        assert_eq!(NoActiveFaults.check(&fixture.context()), Ok(()));
    }

    #[test]
    fn no_active_faults_fails_when_a_fault_is_active() {
        let mut fixture = Fixture::new(3);
        let fault = shutdown(2);
        assert_eq!(fixture.faults.apply(&fault), Ok(()));
        let result = NoActiveFaults.check(&fixture.context());
        assert!(matches!(
            result,
            Err(HarnessError::CheckFailed { message })
                if message == "expected no active faults, found node shutdown 2"
        ));
    }
}
