//! Scenario definition and execution engine.
//!
//! A scenario is an ordered list of actions (apply/revert a fault, stop/start a
//! node, or run a custom check) executed against the cluster model from
//! [`harness_core`]. The custom-check extension point lets new recovery
//! assertions be added without rewriting the runner, as required by the v1
//! test-environment brief.

mod catalogue;
mod checks;
mod identity;
mod scenario;
mod spec;

pub use catalogue::{
    LinkDegradationRecovery, NetworkPartitionRecovery, NodeShutdownRecovery, QuorumLossRecovery,
};
pub use checks::{
    AllNodesRunning, FaultActive, NoActiveFaults, NodeRunning, NodeStopped, QuorumLost,
    StorageQuorumHeld,
};
pub use identity::{
    IdentityError, NodeIdentity, manifest_storage_identities, verify_message_sender,
    verify_topology_identities,
};
pub use scenario::{
    Action, Scenario, ScenarioBuilder, ScenarioCheck, ScenarioContext, ScenarioReport,
    ScenarioRunner, Step, StepOutcome,
};
pub use spec::{ScenarioRegistry, ScenarioSpec};
