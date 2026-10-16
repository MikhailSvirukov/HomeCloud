//! Standard fault/recovery scenarios built from the reusable [`crate::checks`].
//!
//! Each catalogue entry composes the two base fault classes from the v1
//! specification (full node shutdown and selective network degradation) with the
//! reusable recovery checks, so tests share one canonical recovery flow instead
//! of restating the steps.

use harness_core::{Fault, NetworkConditions, NodeId, PeerLink};

use crate::Scenario;
use crate::checks::{FaultActive, NoActiveFaults, NodeRunning, NodeStopped, StorageQuorumHeld};

/// Standard recovery scenario for the full-node fault class: shut a storage node
/// down, assert quorum and node state, bring it back, and assert full recovery.
///
/// The node is stopped and its shutdown fault applied, then `NodeStopped` and
/// `StorageQuorumHeld` are asserted before restarting it and asserting
/// `NodeRunning` plus `NoActiveFaults`.
pub fn node_shutdown_recovery(node: NodeId) -> Scenario {
    let fault = Fault::NodeShutdown { node };
    Scenario::builder(format!("node shutdown recovery for node {node}"))
        .apply_fault(
            fault.clone(),
            format!("apply node shutdown fault for node {node}"),
        )
        .stop_node(node, format!("stop node {node}"))
        .check(
            Box::new(NodeStopped(node)),
            format!("assert node {node} is stopped"),
        )
        .check(Box::new(StorageQuorumHeld), "assert storage quorum is held")
        .start_node(node, format!("start node {node}"))
        .revert_fault(fault, format!("revert node shutdown fault for node {node}"))
        .check(
            Box::new(NodeRunning(node)),
            format!("assert node {node} is running"),
        )
        .check(Box::new(NoActiveFaults), "assert no active faults")
        .build()
}

/// Standard recovery scenario for the selective-network fault class: degrade a
/// peer link, assert the fault and quorum, heal the link, and assert no faults.
///
/// The `Fault::LinkDegradation` is constructed once and reused for apply, the
/// `FaultActive` check, and revert, so all steps refer to the same fault.
pub fn link_degradation_recovery(link: PeerLink, conditions: NetworkConditions) -> Scenario {
    let fault = Fault::LinkDegradation { link, conditions };
    Scenario::builder(format!("link degradation recovery for {link}"))
        .apply_fault(
            fault.clone(),
            format!("apply link degradation fault for {link}"),
        )
        .check(
            Box::new(FaultActive(fault.clone())),
            format!("assert link degradation fault is active on {link}"),
        )
        .check(Box::new(StorageQuorumHeld), "assert storage quorum is held")
        .revert_fault(fault, format!("revert link degradation fault for {link}"))
        .check(Box::new(NoActiveFaults), "assert no active faults")
        .build()
}
