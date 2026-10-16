//! Typed errors shared by the cluster model.

use std::net::SocketAddr;

use crate::fault::Fault;
use crate::node::NodeState;
use crate::topology::{NodeId, NodeRole};

/// Errors produced while building the topology or operating the in-memory fakes.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HarnessError {
    /// A topology must contain exactly one dedicated server.
    #[error("topology requires exactly one dedicated server, found {count}")]
    DedicatedServerCount { count: usize },
    /// Storage node count must be non-zero and odd so a quorum is well defined.
    #[error("storage node count must be non-zero and odd, found {count}")]
    StorageNodeCount { count: usize },
    /// Node identifiers must be unique within a topology.
    #[error("duplicate node id {id}")]
    DuplicateNodeId { id: NodeId },
    /// Node addresses must be unique within a topology.
    #[error("duplicate node address {address}")]
    DuplicateAddress { address: SocketAddr },
    /// Node labels must be unique within a topology.
    #[error("duplicate node label {label}")]
    DuplicateLabel { label: String },
    /// A node spec was placed in the wrong topology slot.
    #[error("node {id} has the wrong role, expected {expected}")]
    WrongNodeRole { id: NodeId, expected: NodeRole },
    /// The referenced node is not part of the topology.
    #[error("unknown node {id}")]
    UnknownNode { id: NodeId },
    /// The referenced peer link does not connect two known nodes.
    #[error("unknown peer link {a}<->{b}")]
    UnknownPeerLink { a: NodeId, b: NodeId },
    /// The same fault is already active.
    #[error("fault already active: {fault}")]
    FaultAlreadyActive { fault: Fault },
    /// No active fault matches the one being reverted.
    #[error("no active fault matching: {fault}")]
    FaultNotActive { fault: Fault },
    /// A packet-loss percentage is above 100.
    #[error("loss percent {percent} exceeds 100")]
    LossPercentOutOfRange { percent: u8 },
    /// A node's observed state differs from the expected one.
    #[error("node {node} is {observed:?}, expected {expected:?}")]
    NodeStateMismatch {
        node: NodeId,
        observed: NodeState,
        expected: NodeState,
    },
    /// Fewer storage nodes are running than the quorum requires.
    #[error("storage quorum not held: {running} of {total} storage nodes running, quorum {quorum}")]
    StorageQuorumNotHeld {
        running: usize,
        total: usize,
        quorum: usize,
    },
    /// More storage nodes are running than a lost quorum would allow.
    #[error("storage quorum not lost: {running} of {total} storage nodes running, quorum {quorum}")]
    StorageQuorumNotLost {
        running: usize,
        total: usize,
        quorum: usize,
    },
    /// A fault expected to be active is not.
    #[error("fault expected active: {fault}")]
    FaultExpectedActive { fault: Fault },
    /// A fault expected to be gone is still active.
    #[error("fault unexpectedly active: {fault}")]
    FaultUnexpectedlyActive { fault: Fault },
    /// A custom scenario check failed.
    #[error("check failed: {message}")]
    CheckFailed { message: String },
    /// A process command run by the harness failed.
    ///
    /// `code` is `-1` when the process ended without an exit code, for example
    /// when it was terminated by a signal.
    #[error("{program} exited with code {code}")]
    CommandFailed { program: String, code: i32 },
    /// A command could not be started.
    #[error("could not start {program}")]
    CommandNotStarted { program: String },
}
