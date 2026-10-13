//! Reusable [`ScenarioCheck`] blocks for recovery assertions.
//!
//! Each type implements one small, composable assertion over a
//! [`ScenarioContext`] so scenario tests build recovery expectations from
//! library blocks instead of rewriting the same logic. Failures carry the
//! typed [`HarnessError`] variant that matches the assertion.

use harness_core::{Fault, HarnessError, NodeId, NodeState};

use crate::{ScenarioCheck, ScenarioContext};

type Result<T> = core::result::Result<T, HarnessError>;

/// Counts storage nodes currently [`NodeState::Running`].
fn running_storage_count(context: &ScenarioContext<'_>) -> Result<usize> {
    let mut running = 0;
    for spec in context.topology.storage_nodes() {
        if context.nodes.state(spec.id)? == NodeState::Running {
            running += 1;
        }
    }
    Ok(running)
}

/// Fails unless every node in the topology is running.
pub struct AllNodesRunning;

impl ScenarioCheck for AllNodesRunning {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        let failure: Option<(NodeId, NodeState)> =
            context
                .topology
                .nodes()
                .try_fold(None, |failure, spec| match failure {
                    Some(_) => Ok(failure),
                    None => {
                        let observed = context.nodes.state(spec.id)?;
                        Ok((observed != NodeState::Running).then_some((spec.id, observed)))
                    }
                })?;
        failure.map_or(Ok(()), |(node, observed)| {
            Err(HarnessError::NodeStateMismatch {
                node,
                observed,
                expected: NodeState::Running,
            })
        })
    }
}

/// Fails unless the given node is running.
pub struct NodeRunning(pub NodeId);

impl ScenarioCheck for NodeRunning {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        let observed = context.nodes.state(self.0)?;
        (observed == NodeState::Running)
            .then_some(())
            .ok_or(HarnessError::NodeStateMismatch {
                node: self.0,
                observed,
                expected: NodeState::Running,
            })
    }
}

/// Fails unless the given node is stopped.
pub struct NodeStopped(pub NodeId);

impl ScenarioCheck for NodeStopped {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        let observed = context.nodes.state(self.0)?;
        (observed == NodeState::Stopped)
            .then_some(())
            .ok_or(HarnessError::NodeStateMismatch {
                node: self.0,
                observed,
                expected: NodeState::Stopped,
            })
    }
}

/// Fails unless enough storage nodes are running to keep quorum.
pub struct StorageQuorumHeld;

impl ScenarioCheck for StorageQuorumHeld {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        let quorum = context.topology.quorum();
        let running = running_storage_count(context)?;
        let total = context.topology.storage_nodes().len();
        (running >= quorum)
            .then_some(())
            .ok_or(HarnessError::StorageQuorumNotHeld {
                running,
                total,
                quorum,
            })
    }
}

/// Fails unless fewer storage nodes are running than the quorum size.
pub struct QuorumLost;

impl ScenarioCheck for QuorumLost {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        let quorum = context.topology.quorum();
        let running = running_storage_count(context)?;
        let total = context.topology.storage_nodes().len();
        (running < quorum)
            .then_some(())
            .ok_or(HarnessError::StorageQuorumNotLost {
                running,
                total,
                quorum,
            })
    }
}

/// Fails unless the given fault is active.
pub struct FaultActive(pub Fault);

impl ScenarioCheck for FaultActive {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        context
            .faults
            .active_faults()
            .contains(&self.0)
            .then_some(())
            .ok_or_else(|| HarnessError::FaultExpectedActive {
                fault: self.0.clone(),
            })
    }
}

/// Fails unless no faults are active.
pub struct NoActiveFaults;

impl ScenarioCheck for NoActiveFaults {
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()> {
        match context.faults.active_faults().first() {
            None => Ok(()),
            Some(fault) => Err(HarnessError::FaultUnexpectedlyActive {
                fault: fault.clone(),
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
                    format!("storage-{}", id.0),
                    addr(7001 + id.0),
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
    fn all_nodes_running_passes_when_every_node_is_running() {
        let fixture = Fixture::new(3);
        assert_eq!(AllNodesRunning.check(&fixture.context()), Ok(()));
    }

    #[test]
    fn all_nodes_running_fails_when_a_node_is_stopped() {
        let mut fixture = Fixture::new(3);
        assert_eq!(fixture.nodes.stop(NodeId::new(2)), Ok(()));
        let result = AllNodesRunning.check(&fixture.context());
        assert_eq!(
            result,
            Err(HarnessError::NodeStateMismatch {
                node: NodeId::new(2),
                observed: NodeState::Stopped,
                expected: NodeState::Running,
            })
        );
    }

    #[test]
    fn all_nodes_running_propagates_node_control_errors() {
        let topology = topology(3);
        let nodes = InMemoryNodeControl::new([NodeId::new(1)]);
        let faults = FakeFaultInjector::new(topology.clone());
        let context = ScenarioContext::new(&topology, &nodes, &faults);
        assert_eq!(
            AllNodesRunning.check(&context),
            Err(HarnessError::UnknownNode { id: NodeId::new(2) })
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
        assert_eq!(
            result,
            Err(HarnessError::NodeStateMismatch {
                node: NodeId::new(3),
                observed: NodeState::Stopped,
                expected: NodeState::Running,
            })
        );
    }

    #[test]
    fn node_running_propagates_unknown_node_error() {
        let fixture = Fixture::new(3);
        assert_eq!(
            NodeRunning(NodeId::new(99)).check(&fixture.context()),
            Err(HarnessError::UnknownNode {
                id: NodeId::new(99)
            })
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
        assert_eq!(
            result,
            Err(HarnessError::NodeStateMismatch {
                node: NodeId::new(4),
                observed: NodeState::Running,
                expected: NodeState::Stopped,
            })
        );
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
        assert_eq!(
            result,
            Err(HarnessError::StorageQuorumNotHeld {
                running: 1,
                total: 3,
                quorum: 2,
            })
        );
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
        assert_eq!(
            result,
            Err(HarnessError::StorageQuorumNotLost {
                running: 2,
                total: 3,
                quorum: 2,
            })
        );
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
        assert_eq!(
            result,
            Err(HarnessError::FaultExpectedActive { fault: shutdown(2) })
        );
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
        assert_eq!(result, Err(HarnessError::FaultUnexpectedlyActive { fault }));
    }
}
