//! Standard fault/recovery scenarios built from the reusable [`crate::checks`].
//!
//! Each catalogue entry composes the two base fault classes from the v1
//! specification (full node shutdown and selective network degradation) with the
//! reusable recovery checks, so tests share one canonical recovery flow instead
//! of restating the steps.

use harness_core::{ClusterTopology, Fault, NetworkConditions, NodeId, PeerLink};

use crate::Scenario;
use crate::checks::{
    AllNodesRunning, FaultActive, NoActiveFaults, NodeRunning, NodeStopped, QuorumLost,
    StorageQuorumHeld,
};
use crate::spec::ScenarioSpec;

/// Standard recovery scenario for the full-node fault class: shut a storage node
/// down, assert quorum and node state, bring it back, and assert full recovery.
///
/// The node is stopped and its shutdown fault applied, then `NodeStopped` and
/// `StorageQuorumHeld` are asserted before restarting it and asserting
/// `NodeRunning` plus `NoActiveFaults`.
pub struct NodeShutdownRecovery {
    /// Storage node to shut down and restart.
    pub node: NodeId,
}

impl ScenarioSpec for NodeShutdownRecovery {
    fn name(&self) -> String {
        format!("node shutdown recovery for node {}", self.node)
    }

    fn build(&self) -> Scenario {
        let node = self.node;
        let fault = Fault::NodeShutdown { node };
        Scenario::builder(self.name())
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
}

/// Standard recovery scenario for the selective-network fault class: degrade a
/// peer link, assert the fault and quorum, heal the link, and assert no faults.
///
/// The `Fault::LinkDegradation` is constructed once and reused for apply, the
/// `FaultActive` check, and revert, so all steps refer to the same fault.
pub struct LinkDegradationRecovery {
    /// Link to degrade and heal.
    pub link: PeerLink,
    /// Conditions applied to the link.
    pub conditions: NetworkConditions,
}

impl ScenarioSpec for LinkDegradationRecovery {
    fn name(&self) -> String {
        format!("link degradation recovery for {}", self.link)
    }

    fn build(&self) -> Scenario {
        let link = self.link;
        let fault = Fault::LinkDegradation {
            link,
            conditions: self.conditions,
        };
        Scenario::builder(self.name())
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
}

/// Standard recovery scenario for quorum loss: stop the fewest storage nodes
/// whose loss drops the running count below quorum, assert quorum is lost, then
/// restart them and assert full recovery.
///
/// The shutdown faults are built once and reused for apply and revert, so every
/// step refers to the same fault.
pub struct QuorumLossRecovery {
    /// Topology whose storage nodes are stopped.
    pub topology: ClusterTopology,
}

impl QuorumLossRecovery {
    fn stopped_nodes(&self) -> Vec<NodeId> {
        let to_stop = self.topology.storage_nodes().len() - self.topology.quorum() + 1;
        self.topology
            .storage_nodes()
            .iter()
            .take(to_stop)
            .map(|spec| spec.id)
            .collect()
    }
}

impl ScenarioSpec for QuorumLossRecovery {
    fn name(&self) -> String {
        format!(
            "quorum loss recovery stopping {} storage nodes",
            self.stopped_nodes().len()
        )
    }

    fn build(&self) -> Scenario {
        let nodes = self.stopped_nodes();
        let faults: Vec<Fault> = nodes
            .iter()
            .map(|&node| Fault::NodeShutdown { node })
            .collect();

        let mut scenario = Scenario::builder(self.name());
        for (node, fault) in nodes.iter().zip(&faults) {
            scenario = scenario
                .apply_fault(
                    fault.clone(),
                    format!("apply node shutdown fault for node {node}"),
                )
                .stop_node(*node, format!("stop node {node}"));
        }
        scenario = scenario.check(Box::new(QuorumLost), "assert storage quorum is lost");
        for (node, fault) in nodes.iter().zip(&faults) {
            scenario = scenario
                .start_node(*node, format!("start node {node}"))
                .revert_fault(
                    fault.clone(),
                    format!("revert node shutdown fault for node {node}"),
                );
        }
        scenario
            .check(Box::new(StorageQuorumHeld), "assert storage quorum is held")
            .check(Box::new(AllNodesRunning), "assert all nodes are running")
            .check(Box::new(NoActiveFaults), "assert no active faults")
            .build()
    }
}

/// Standard recovery scenario for the selective-network fault class across every
/// storage-to-storage link: degrade them all, assert quorum still holds with the
/// nodes up, heal them, and assert no faults.
///
/// The `Fault::LinkDegradation` values are built once and reused for apply, the
/// `FaultActive` checks, and revert, so all steps refer to the same faults.
pub struct NetworkPartitionRecovery {
    /// Topology whose storage links are degraded.
    pub topology: ClusterTopology,
    /// Conditions applied to every degraded link.
    pub conditions: NetworkConditions,
}

impl NetworkPartitionRecovery {
    fn links(&self) -> Vec<PeerLink> {
        let ids: Vec<NodeId> = self
            .topology
            .storage_nodes()
            .iter()
            .map(|spec| spec.id)
            .collect();
        ids.iter()
            .enumerate()
            .flat_map(|(index, &a)| {
                let rest = &ids[index + 1..];
                rest.iter().map(move |&b| PeerLink::new(a, b))
            })
            .collect()
    }
}

impl ScenarioSpec for NetworkPartitionRecovery {
    fn name(&self) -> String {
        format!(
            "network partition recovery across {} storage links",
            self.links().len()
        )
    }

    fn build(&self) -> Scenario {
        let links = self.links();
        let faults: Vec<Fault> = links
            .iter()
            .map(|&link| Fault::LinkDegradation {
                link,
                conditions: self.conditions,
            })
            .collect();

        let mut scenario = Scenario::builder(self.name());
        for (link, fault) in links.iter().zip(&faults) {
            scenario = scenario.apply_fault(
                fault.clone(),
                format!("apply link degradation fault for {link}"),
            );
        }
        for (link, fault) in links.iter().zip(&faults) {
            scenario = scenario.check(
                Box::new(FaultActive(fault.clone())),
                format!("assert link degradation fault is active on {link}"),
            );
        }
        scenario = scenario
            .check(Box::new(AllNodesRunning), "assert all nodes are running")
            .check(Box::new(StorageQuorumHeld), "assert storage quorum is held");
        for (link, fault) in links.iter().zip(&faults) {
            scenario = scenario.revert_fault(
                fault.clone(),
                format!("revert link degradation fault for {link}"),
            );
        }
        scenario
            .check(Box::new(NoActiveFaults), "assert no active faults")
            .check(Box::new(StorageQuorumHeld), "assert storage quorum is held")
            .build()
    }
}
