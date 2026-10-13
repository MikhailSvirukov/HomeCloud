//! Node lifecycle control backed by an in-memory state map.

use std::collections::HashMap;

use crate::error::HarnessError;
use crate::topology::NodeId;

type Result<T> = core::result::Result<T, HarnessError>;

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
    fn state(&self, node: NodeId) -> Result<NodeState>;
    /// Stops the node; idempotent when already stopped.
    fn stop(&mut self, node: NodeId) -> Result<()>;
    /// Starts the node; idempotent when already running.
    fn start(&mut self, node: NodeId) -> Result<()>;
    /// Restarts the node, leaving it running.
    fn restart(&mut self, node: NodeId) -> Result<()>;
}

/// A [`NodeControl`] that keeps every node's state in memory.
pub struct InMemoryNodeControl {
    states: HashMap<NodeId, NodeState>,
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
    fn set(&mut self, node: NodeId, target: NodeState) -> Result<()> {
        match self.states.get_mut(&node) {
            Some(state) => {
                *state = target;
                Ok(())
            }
            None => Err(HarnessError::UnknownNode { id: node }),
        }
    }
}
impl NodeControl for InMemoryNodeControl {
    fn state(&self, node: NodeId) -> Result<NodeState> {
        self.states
            .get(&node)
            .copied()
            .ok_or(HarnessError::UnknownNode { id: node })
    }
    fn stop(&mut self, node: NodeId) -> Result<()> {
        self.set(node, NodeState::Stopped)
    }
    fn start(&mut self, node: NodeId) -> Result<()> {
        self.set(node, NodeState::Running)
    }
    fn restart(&mut self, node: NodeId) -> Result<()> {
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
    fn unknown_node_errors_on_every_operation() {
        let mut control = control();
        let unknown = NodeId::new(99);
        assert_eq!(control.state(unknown), Err(E::UnknownNode { id: unknown }));
        assert_eq!(control.stop(unknown), Err(E::UnknownNode { id: unknown }));
        assert_eq!(control.start(unknown), Err(E::UnknownNode { id: unknown }));
        assert_eq!(
            control.restart(unknown),
            Err(E::UnknownNode { id: unknown })
        );
    }
}
