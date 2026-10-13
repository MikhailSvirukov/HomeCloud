//! Typed errors shared by the cluster model.
//!
//! Boundary conversions from Docker or network layers translate their failures
//! into these variants instead of unwrapping.

use std::net::SocketAddr;

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
    DuplicateNodeId { id: u16 },
    /// Node addresses must be unique within a topology.
    #[error("duplicate node address {address}")]
    DuplicateAddress { address: SocketAddr },
    /// Node labels must be unique within a topology.
    #[error("duplicate node label {label}")]
    DuplicateLabel { label: String },
    /// A node spec was placed in the wrong topology slot.
    #[error("node {id} has the wrong role, expected {expected}")]
    WrongNodeRole { id: u16, expected: &'static str },
    /// The referenced node is not part of the topology.
    #[error("unknown node id {id}")]
    UnknownNode { id: u16 },
    /// The referenced peer link does not connect two known nodes.
    #[error("unknown peer link {a}<->{b}")]
    UnknownPeerLink { a: u16, b: u16 },
    /// The same fault is already active.
    #[error("fault already active: {fault}")]
    FaultAlreadyActive { fault: String },
    /// No active fault matches the one being reverted.
    #[error("no active fault matching: {fault}")]
    FaultNotActive { fault: String },
    /// A network condition is outside its allowed range.
    #[error("invalid network condition: {reason}")]
    InvalidNetworkCondition { reason: String },
    /// A custom scenario check failed.
    #[error("check failed: {message}")]
    CheckFailed { message: String },
}
