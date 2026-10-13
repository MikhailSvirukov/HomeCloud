//! Fault model: node shutdown and link degradation, plus an in-memory injector.

use std::fmt;
use std::time::Duration;

use crate::error::HarnessError;
use crate::topology::{ClusterTopology, NodeId};

const MAX_LOSS_PERCENT: u8 = 100;

/// An undirected peer link; `new(a, b)` equals `new(b, a)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerLink {
    a: NodeId,
    b: NodeId,
}
impl PeerLink {
    /// Creates a link, normalising endpoint order so both directions compare equal.
    pub fn new(a: NodeId, b: NodeId) -> Self {
        if a.get() <= b.get() {
            Self { a, b }
        } else {
            Self { a: b, b: a }
        }
    }
    /// Lower endpoint by node id.
    pub fn a(self) -> NodeId {
        self.a
    }
    /// Higher endpoint by node id.
    pub fn b(self) -> NodeId {
        self.b
    }
}
impl fmt::Display for PeerLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}<->{}", self.a, self.b)
    }
}

/// Artificial network conditions applied to a peer link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkConditions {
    delay: Duration,
    loss_percent: u8,
}
impl NetworkConditions {
    /// Creates conditions; `loss_percent` must be at most 100.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::InvalidNetworkCondition`] when `loss_percent`
    /// exceeds 100.
    pub fn new(delay: Duration, loss_percent: u8) -> Result<Self, HarnessError> {
        if loss_percent > MAX_LOSS_PERCENT {
            Err(HarnessError::InvalidNetworkCondition {
                reason: format!("loss {loss_percent}% exceeds {MAX_LOSS_PERCENT}%"),
            })
        } else {
            Ok(Self {
                delay,
                loss_percent,
            })
        }
    }
    /// No delay, no loss.
    pub fn healthy() -> Self {
        Self {
            delay: Duration::ZERO,
            loss_percent: 0,
        }
    }
    /// Configured one-way delay.
    pub fn delay(self) -> Duration {
        self.delay
    }
    /// Configured loss percentage in `0..=100`.
    pub fn loss_percent(self) -> u8 {
        self.loss_percent
    }
}

/// The two base fault classes the harness can inject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Full node shutdown: stop/terminate the node's container.
    NodeShutdown {
        /// Node to shut down.
        node: NodeId,
    },
    /// Selective programmatic network fault on a peer link.
    LinkDegradation {
        /// Affected undirected link.
        link: PeerLink,
        /// Applied degradation parameters.
        conditions: NetworkConditions,
    },
}
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::NodeShutdown { node } => write!(f, "node shutdown {node}"),
            Fault::LinkDegradation { link, conditions } => write!(
                f,
                "link degradation {link} delay={:?} loss={}%",
                conditions.delay(),
                conditions.loss_percent()
            ),
        }
    }
}

/// Applies and reverts faults against a cluster.
pub trait FaultInjector {
    /// Applies the fault, rejecting unknown targets and duplicates.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::UnknownNode`] or
    /// [`HarnessError::UnknownPeerLink`] for targets outside the topology, and
    /// [`HarnessError::FaultAlreadyActive`] when the fault is already active.
    fn apply(&mut self, fault: &Fault) -> Result<(), HarnessError>;
    /// Reverts an active fault.
    ///
    /// # Errors
    ///
    /// Returns [`HarnessError::FaultNotActive`] when the fault is not active.
    fn revert(&mut self, fault: &Fault) -> Result<(), HarnessError>;
    /// Currently active faults, in application order.
    fn active_faults(&self) -> &[Fault];
}

/// An in-memory [`FaultInjector`] backed by a known [`ClusterTopology`].
#[derive(Debug)]
pub struct FakeFaultInjector {
    topology: ClusterTopology,
    active: Vec<Fault>,
}
impl FakeFaultInjector {
    /// Creates an injector bound to a topology, with no active faults.
    pub fn new(topology: ClusterTopology) -> Self {
        Self {
            topology,
            active: Vec::new(),
        }
    }
    fn validate_known(&self, fault: &Fault) -> Result<(), HarnessError> {
        match fault {
            Fault::NodeShutdown { node } => self
                .topology
                .node(*node)
                .map(|_| ())
                .ok_or(HarnessError::UnknownNode { id: node.get() }),
            Fault::LinkDegradation { link, .. } => {
                let (a, b) = (link.a(), link.b());
                if self.topology.node(a).is_some() && self.topology.node(b).is_some() {
                    Ok(())
                } else {
                    Err(HarnessError::UnknownPeerLink {
                        a: a.get(),
                        b: b.get(),
                    })
                }
            }
        }
    }
}
impl FaultInjector for FakeFaultInjector {
    fn apply(&mut self, fault: &Fault) -> Result<(), HarnessError> {
        self.validate_known(fault)?;
        if self.active.contains(fault) {
            Err(HarnessError::FaultAlreadyActive {
                fault: fault.to_string(),
            })
        } else {
            self.active.push(fault.clone());
            Ok(())
        }
    }
    fn revert(&mut self, fault: &Fault) -> Result<(), HarnessError> {
        if let Some(index) = self.active.iter().position(|active| active == fault) {
            self.active.remove(index);
            Ok(())
        } else {
            Err(HarnessError::FaultNotActive {
                fault: fault.to_string(),
            })
        }
    }
    fn active_faults(&self) -> &[Fault] {
        &self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HarnessError as E;
    use crate::topology::NodeSpec;

    fn addr(port: u16) -> std::net::SocketAddr {
        std::net::SocketAddr::from(([127, 0, 0, 1], port))
    }
    fn topology() -> ClusterTopology {
        ClusterTopology::builder()
            .dedicated_server(NodeSpec::dedicated_server(
                NodeId::new(1),
                "dedicated",
                addr(7001),
            ))
            .storage_node(NodeSpec::storage(NodeId::new(2), "storage-1", addr(7002)))
            .storage_node(NodeSpec::storage(NodeId::new(3), "storage-2", addr(7003)))
            .storage_node(NodeSpec::storage(NodeId::new(4), "storage-3", addr(7004)))
            .build()
            .expect("valid topology")
    }
    fn injector() -> FakeFaultInjector {
        FakeFaultInjector::new(topology())
    }
    fn shutdown(node: u16) -> Fault {
        Fault::NodeShutdown {
            node: NodeId::new(node),
        }
    }
    fn degradation(a: u16, b: u16) -> Fault {
        Fault::LinkDegradation {
            link: PeerLink::new(NodeId::new(a), NodeId::new(b)),
            conditions: NetworkConditions::healthy(),
        }
    }

    #[test]
    fn peer_link_is_order_independent() {
        let link = PeerLink::new(NodeId::new(3), NodeId::new(1));
        assert_eq!(link, PeerLink::new(NodeId::new(1), NodeId::new(3)));
        assert_eq!((link.a(), link.b()), (NodeId::new(1), NodeId::new(3)));
        assert_eq!(link.to_string(), "1<->3");
    }
    #[test]
    fn network_conditions_reject_loss_above_one_hundred() {
        let result = NetworkConditions::new(Duration::from_millis(5), 101);
        assert!(matches!(result, Err(E::InvalidNetworkCondition { .. })));
    }
    #[test]
    fn healthy_network_conditions_are_zero() {
        let c = NetworkConditions::healthy();
        assert_eq!(c.delay(), Duration::ZERO);
        assert_eq!(c.loss_percent(), 0);
    }
    #[test]
    fn apply_rejects_unknown_node() {
        let result = injector().apply(&shutdown(99));
        assert!(matches!(result, Err(E::UnknownNode { id: 99 })));
    }
    #[test]
    fn apply_rejects_link_with_unknown_endpoint() {
        let result = injector().apply(&degradation(1, 99));
        assert!(matches!(result, Err(E::UnknownPeerLink { a: 1, b: 99 })));
    }
    #[test]
    fn apply_rejects_duplicate_fault() {
        let mut injector = injector();
        let fault = shutdown(2);
        assert_eq!(injector.apply(&fault), Ok(()));
        let result = injector.apply(&fault);
        assert!(matches!(result, Err(E::FaultAlreadyActive { .. })));
    }
    #[test]
    fn revert_of_inactive_fault_errors() {
        let result = injector().revert(&shutdown(2));
        assert!(matches!(result, Err(E::FaultNotActive { .. })));
    }
    #[test]
    fn apply_then_revert_tracks_active_faults_in_order() {
        let mut injector = injector();
        let (first, second) = (shutdown(2), degradation(1, 3));
        assert_eq!(injector.apply(&first), Ok(()));
        assert_eq!(injector.apply(&second), Ok(()));
        assert_eq!(injector.active_faults(), &[first.clone(), second.clone()]);
        assert_eq!(injector.revert(&first), Ok(()));
        assert_eq!(injector.active_faults(), &[second]);
    }
}
