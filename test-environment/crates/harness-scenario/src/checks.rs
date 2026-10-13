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
    use harness_core::{FaultInjector, InMemoryNodeControl, NodeControl};
    use harness_test_support::{faults, nodes, topology};

    use super::*;

    /// A check invocation plus the exact typed error it must surface.
    type Case = (Box<dyn Fn() -> Result<()>>, HarnessError);

    fn node_shutdown(node: u16) -> Fault {
        Fault::NodeShutdown {
            node: NodeId::new(node),
        }
    }

    #[test]
    fn checks_surface_their_typed_errors() {
        let cases: Vec<Case> = vec![
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let mut control = nodes(&cluster);
                    let injector = faults(&cluster);
                    control.stop(NodeId::new(2))?;
                    AllNodesRunning.check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::NodeStateMismatch {
                    node: NodeId::new(2),
                    observed: NodeState::Stopped,
                    expected: NodeState::Running,
                },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let control = InMemoryNodeControl::new([NodeId::new(1)]);
                    let injector = faults(&cluster);
                    AllNodesRunning.check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::UnknownNode { id: NodeId::new(2) },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let mut control = nodes(&cluster);
                    let injector = faults(&cluster);
                    control.stop(NodeId::new(3))?;
                    NodeRunning(NodeId::new(3))
                        .check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::NodeStateMismatch {
                    node: NodeId::new(3),
                    observed: NodeState::Stopped,
                    expected: NodeState::Running,
                },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let control = nodes(&cluster);
                    let injector = faults(&cluster);
                    NodeRunning(NodeId::new(99))
                        .check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::UnknownNode {
                    id: NodeId::new(99),
                },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let control = nodes(&cluster);
                    let injector = faults(&cluster);
                    NodeStopped(NodeId::new(4))
                        .check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::NodeStateMismatch {
                    node: NodeId::new(4),
                    observed: NodeState::Running,
                    expected: NodeState::Stopped,
                },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let mut control = nodes(&cluster);
                    let injector = faults(&cluster);
                    control.stop(NodeId::new(2))?;
                    control.stop(NodeId::new(3))?;
                    StorageQuorumHeld.check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::StorageQuorumNotHeld {
                    running: 1,
                    total: 3,
                    quorum: 2,
                },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let control = nodes(&cluster);
                    let injector = faults(&cluster);
                    FaultActive(node_shutdown(2))
                        .check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::FaultExpectedActive {
                    fault: node_shutdown(2),
                },
            ),
            (
                Box::new(|| {
                    let cluster = topology(3);
                    let control = nodes(&cluster);
                    let mut injector = faults(&cluster);
                    injector.apply(&node_shutdown(2))?;
                    NoActiveFaults.check(&ScenarioContext::new(&cluster, &control, &injector))
                }),
                HarnessError::FaultUnexpectedlyActive {
                    fault: node_shutdown(2),
                },
            ),
        ];

        for (index, (case, expected)) in cases.into_iter().enumerate() {
            assert_eq!(case().unwrap_err(), expected, "case {index}");
        }
    }
}
