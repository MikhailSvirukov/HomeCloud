//! Fault model: node shutdown and link degradation, plus an in-memory injector.

use std::fmt;
use std::time::Duration;

use crate::error::HarnessError;
use crate::topology::{ClusterTopology, NodeId};

type Result<T> = core::result::Result<T, HarnessError>;

/// A packet-loss percentage in `0..=100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossPercent(u8);
impl LossPercent {
    /// Wraps a percentage, rejecting values above 100.
    pub fn new(percent: u8) -> Result<Self> {
        match percent {
            0..=100 => Ok(Self(percent)),
            _ => Err(HarnessError::LossPercentOutOfRange { percent }),
        }
    }
}
impl From<LossPercent> for u8 {
    fn from(percent: LossPercent) -> Self {
        percent.0
    }
}

/// An undirected peer link; `new(a, b)` equals `new(b, a)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerLink {
    a: NodeId,
    b: NodeId,
}
impl PeerLink {
    /// Creates a link, normalising endpoint order so both directions compare equal.
    pub fn new(a: NodeId, b: NodeId) -> Self {
        if a <= b {
            Self { a, b }
        } else {
            Self { a: b, b: a }
        }
    }
    /// Lower endpoint by node id; `a`/`b` expose the normalised pair.
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
    /// Configured one-way delay.
    pub delay: Duration,
    /// Configured loss percentage.
    pub loss_percent: LossPercent,
}
impl NetworkConditions {
    /// Creates conditions.
    pub fn new(delay: Duration, loss_percent: LossPercent) -> Self {
        Self {
            delay,
            loss_percent,
        }
    }
    /// No delay, no loss.
    pub fn healthy() -> Self {
        Self {
            delay: Duration::ZERO,
            loss_percent: LossPercent(0),
        }
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
                conditions.delay, conditions.loss_percent.0
            ),
        }
    }
}

/// Applies and reverts faults against a cluster.
pub trait FaultInjector {
    /// Applies the fault, rejecting unknown targets and duplicates.
    fn apply(&mut self, fault: &Fault) -> Result<()>;
    /// Reverts an active fault.
    fn revert(&mut self, fault: &Fault) -> Result<()>;
    /// Currently active faults, in application order.
    fn active_faults(&self) -> &[Fault];
}

/// An in-memory [`FaultInjector`] backed by a known [`ClusterTopology`].
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
    fn validate_known(&self, fault: &Fault) -> Result<()> {
        match fault {
            Fault::NodeShutdown { node } => self
                .topology
                .node(*node)
                .map(|_| ())
                .ok_or(HarnessError::UnknownNode { id: *node }),
            Fault::LinkDegradation { link, .. } => {
                let (a, b) = (link.a(), link.b());
                match (self.topology.node(a), self.topology.node(b)) {
                    (Some(_), Some(_)) => Ok(()),
                    _ => Err(HarnessError::UnknownPeerLink { a, b }),
                }
            }
        }
    }
}
impl FaultInjector for FakeFaultInjector {
    fn apply(&mut self, fault: &Fault) -> Result<()> {
        self.validate_known(fault)?;
        if self.active.contains(fault) {
            return Err(HarnessError::FaultAlreadyActive {
                fault: fault.clone(),
            });
        }
        self.active.push(fault.clone());
        Ok(())
    }
    fn revert(&mut self, fault: &Fault) -> Result<()> {
        let Some(index) = self.active.iter().position(|active| active == fault) else {
            return Err(HarnessError::FaultNotActive {
                fault: fault.clone(),
            });
        };
        self.active.remove(index);
        Ok(())
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
    fn loss_percent_rejects_above_one_hundred() {
        let result = LossPercent::new(101);
        assert_eq!(result, Err(E::LossPercentOutOfRange { percent: 101 }));
    }
    #[test]
    fn apply_rejects_unknown_node() {
        let result = injector().apply(&shutdown(99));
        assert!(matches!(result, Err(E::UnknownNode { id }) if id == NodeId::new(99)));
    }
    #[test]
    fn apply_rejects_link_with_unknown_endpoint() {
        let result = injector().apply(&degradation(1, 99));
        assert!(matches!(
            result,
            Err(E::UnknownPeerLink { a, b }) if a == NodeId::new(1) && b == NodeId::new(99)
        ));
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
