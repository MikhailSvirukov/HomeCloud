//! Scenario definition and execution engine.
//!
//! A scenario is an ordered list of actions (apply/revert a fault, stop/start a
//! node, or run a custom check) executed against the cluster model from
//! [`harness_core`]. The custom-check extension point lets new recovery
//! assertions be added without rewriting the runner, as required by the v1
//! test-environment brief.

mod catalogue;
mod checks;
mod scenario;

pub use catalogue::{link_degradation_recovery, node_shutdown_recovery};
pub use checks::{
    AllNodesRunning, FaultActive, NoActiveFaults, NodeRunning, NodeStopped, QuorumLost,
    StorageQuorumHeld,
};
pub use scenario::{
    Action, Scenario, ScenarioBuilder, ScenarioCheck, ScenarioContext, ScenarioReport,
    ScenarioRunner, Step, StepOutcome,
};
