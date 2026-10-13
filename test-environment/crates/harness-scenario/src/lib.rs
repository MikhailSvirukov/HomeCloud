//! Scenario definition and execution engine.
//!
//! A scenario is an ordered list of actions (apply/revert a fault, stop/start a
//! node, or run a custom check) executed against the cluster model from
//! [`harness_core`]. The custom-check extension point lets new recovery
//! assertions be added without rewriting the runner, as required by the v1
//! test-environment brief.

pub mod scenario;

pub use harness_core::{ClusterTopology, Fault, FaultInjector, NodeControl, NodeId, NodeState};
pub use scenario::{
    Action, Scenario, ScenarioBuilder, ScenarioCheck, ScenarioContext, ScenarioReport,
    ScenarioRunner, Step, StepOutcome,
};
