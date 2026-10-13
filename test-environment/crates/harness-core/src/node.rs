//! Node lifecycle control backed by an in-memory state map.

use std::collections::BTreeMap;

use crate::error::HarnessError;
use crate::topology::NodeId;

/// Lifecycle state of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// The node process/container is up.
    Running,
    /// The node has been fully shut down (the first base fault class).
    Stopped,
}

/// Controls node lifecycle transitions, abstracted from Docker.
pub trait NodeControl {
    /// Returns the current lifecycle state.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::UnknownNode`] for an unknown node.
    fn state(&self, node: NodeId) -> Result<NodeState, HarnessError>;
    /// Stops the node; idempotent when already stopped.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::UnknownNode`] for an unknown node.
    fn stop(&mut self, node: NodeId) -> Result<(), HarnessError>;
    /// Starts the node; idempotent when already running.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::UnknownNode`] for an unknown node.
    fn start(&mut self, node: NodeId) -> Result<(), HarnessError>;
    /// Restarts the node, leaving it running.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::UnknownNode`] for an unknown node.
    fn restart(&mut self, node: NodeId) -> Result<(), HarnessError>;
}

/// A [`NodeControl`] that keeps every node's state in memory.
#[derive(Debug, Clone)]
pub struct InMemoryNodeControl {
    states: BTreeMap<NodeId, NodeState>,
}
impl InMemoryNodeControl {
    /// Creates a controller where every given node starts [`NodeState::Running`].
    pub fn new(nodes: impl IntoIterator<Item = NodeId>) -> Self {
        Self {
            states: nodes
                .into_iter()
                .map(|id| (id, NodeState::Running))
                .collect(),
        }
    }
    fn set(&mut self, node: NodeId, target: NodeState) -> Result<(), HarnessError> {
        match self.states.get_mut(&node) {
            Some(state) => {
                *state = target;
                Ok(())
            }
            None => Err(HarnessError::UnknownNode { id: node.get() }),
        }
    }
}
impl NodeControl for InMemoryNodeControl {
    fn state(&self, node: NodeId) -> Result<NodeState, HarnessError> {
        self.states
            .get(&node)
            .copied()
            .ok_or(HarnessError::UnknownNode { id: node.get() })
    }
    fn stop(&mut self, node: NodeId) -> Result<(), HarnessError> {
        self.set(node, NodeState::Stopped)
    }
    fn start(&mut self, node: NodeId) -> Result<(), HarnessError> {
        self.set(node, NodeState::Running)
    }
    fn restart(&mut self, node: NodeId) -> Result<(), HarnessError> {
        self.set(node, NodeState::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HarnessError as E;

    fn control() -> InMemoryNodeControl {
        InMemoryNodeControl::new([NodeId::new(1), NodeId::new(2), NodeId::new(3)])
    }

    #[test]
    fn nodes_start_running() {
        assert_eq!(control().state(NodeId::new(1)), Ok(NodeState::Running));
    }
    #[test]
    fn stop_moves_node_to_stopped() {
        let mut control = control();
        assert_eq!(control.stop(NodeId::new(1)), Ok(()));
        assert_eq!(control.state(NodeId::new(1)), Ok(NodeState::Stopped));
    }
    #[test]
    fn start_after_stop_returns_to_running() {
        let mut control = control();
        assert_eq!(control.stop(NodeId::new(1)), Ok(()));
        assert_eq!(control.start(NodeId::new(1)), Ok(()));
        assert_eq!(control.state(NodeId::new(1)), Ok(NodeState::Running));
    }
    #[test]
    fn stop_is_idempotent() {
        let mut control = control();
        assert_eq!(control.stop(NodeId::new(1)), Ok(()));
        assert_eq!(control.stop(NodeId::new(1)), Ok(()));
        assert_eq!(control.state(NodeId::new(1)), Ok(NodeState::Stopped));
    }
    #[test]
    fn restart_leaves_node_running() {
        let mut control = control();
        assert_eq!(control.stop(NodeId::new(1)), Ok(()));
        assert_eq!(control.restart(NodeId::new(1)), Ok(()));
        assert_eq!(control.state(NodeId::new(1)), Ok(NodeState::Running));
    }
    #[test]
    fn unknown_node_errors_on_every_operation() {
        let mut control = control();
        let unknown = NodeId::new(99);
        assert_eq!(control.state(unknown), Err(E::UnknownNode { id: 99 }));
        assert_eq!(control.stop(unknown), Err(E::UnknownNode { id: 99 }));
        assert_eq!(control.start(unknown), Err(E::UnknownNode { id: 99 }));
        assert_eq!(control.restart(unknown), Err(E::UnknownNode { id: 99 }));
    }
}
