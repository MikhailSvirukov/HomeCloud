//! Scenario definition and execution engine over the [`harness_core`] cluster model.

use std::fmt;

use harness_core::{ClusterTopology, Fault, FaultInjector, HarnessError, NodeControl, NodeId};

/// One action in a scenario.
pub enum Action {
    /// Apply a fault to the cluster.
    ApplyFault(Fault),
    /// Revert a previously applied fault.
    RevertFault(Fault),
    /// Stop a node (the full-node fault class).
    StopNode(NodeId),
    /// Start a node again.
    StartNode(NodeId),
    /// Run a custom check — the extension point for new recovery assertions.
    Check(Box<dyn ScenarioCheck>),
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplyFault(fault) => f.debug_tuple("ApplyFault").field(fault).finish(),
            Self::RevertFault(fault) => f.debug_tuple("RevertFault").field(fault).finish(),
            Self::StopNode(node) => f.debug_tuple("StopNode").field(node).finish(),
            Self::StartNode(node) => f.debug_tuple("StartNode").field(node).finish(),
            Self::Check(check) => f.debug_tuple("Check").field(&check.name()).finish(),
        }
    }
}

/// A single scenario step: a description plus an action.
#[derive(Debug)]
pub struct Step(String, Action);

impl Step {
    /// Human-readable description used in reports.
    pub fn description(&self) -> &str {
        &self.0
    }
    /// The action to execute.
    pub fn action(&self) -> &Action {
        &self.1
    }
}

/// A named, ordered list of steps describing a fault/recovery scenario.
pub struct Scenario(String, Vec<Step>);

impl Scenario {
    /// Starts building a scenario with the given name.
    pub fn builder(name: impl Into<String>) -> ScenarioBuilder {
        ScenarioBuilder::new(name)
    }
    /// Scenario name.
    pub fn name(&self) -> &str {
        &self.0
    }
    /// Ordered steps.
    pub fn steps(&self) -> &[Step] {
        &self.1
    }
}

/// Builder for [`Scenario`].
#[derive(Debug, Default)]
pub struct ScenarioBuilder {
    name: String,
    steps: Vec<Step>,
}

impl ScenarioBuilder {
    /// Creates an empty builder with the given scenario name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            steps: Vec::new(),
        }
    }
    /// Appends a step with an explicit description.
    pub fn step(mut self, description: impl Into<String>, action: Action) -> Self {
        self.steps.push(Step(description.into(), action));
        self
    }
    /// Appends an `ApplyFault` step.
    pub fn apply_fault(self, fault: Fault, description: impl Into<String>) -> Self {
        self.step(description, Action::ApplyFault(fault))
    }
    /// Appends a `RevertFault` step.
    pub fn revert_fault(self, fault: Fault, description: impl Into<String>) -> Self {
        self.step(description, Action::RevertFault(fault))
    }
    /// Appends a `StopNode` step.
    pub fn stop_node(self, node: NodeId, description: impl Into<String>) -> Self {
        self.step(description, Action::StopNode(node))
    }
    /// Appends a `StartNode` step.
    pub fn start_node(self, node: NodeId, description: impl Into<String>) -> Self {
        self.step(description, Action::StartNode(node))
    }
    /// Appends a `Check` step wrapping a custom check.
    pub fn check(self, check: Box<dyn ScenarioCheck>, description: impl Into<String>) -> Self {
        self.step(description, Action::Check(check))
    }
    /// Builds the scenario.
    pub fn build(self) -> Scenario {
        Scenario(self.name, self.steps)
    }
}

/// Read-only view of the cluster handed to custom checks.
pub struct ScenarioContext<'a>(
    &'a ClusterTopology,
    &'a dyn NodeControl,
    &'a dyn FaultInjector,
);

impl<'a> ScenarioContext<'a> {
    /// Creates a context over the given cluster model.
    pub fn new(
        topology: &'a ClusterTopology,
        nodes: &'a dyn NodeControl,
        faults: &'a dyn FaultInjector,
    ) -> Self {
        Self(topology, nodes, faults)
    }
    /// Fixed topology.
    pub fn topology(&self) -> &'a ClusterTopology {
        self.0
    }
    /// Node lifecycle control.
    pub fn nodes(&self) -> &'a dyn NodeControl {
        self.1
    }
    /// Active fault state.
    pub fn faults(&self) -> &'a dyn FaultInjector {
        self.2
    }
}

/// Extension point: a custom assertion evaluated during a scenario.
pub trait ScenarioCheck: Send + Sync {
    /// A short, human-readable name for the check.
    fn name(&self) -> &str;
    /// Evaluates the assertion, returning a [`HarnessError`] when it fails.
    fn check(&self, context: &ScenarioContext<'_>) -> Result<(), HarnessError>;
}

/// Outcome of a single executed step.
pub struct StepOutcome(String, Result<(), String>);

impl StepOutcome {
    /// Step description.
    pub fn description(&self) -> &str {
        &self.0
    }
    /// Whether the step succeeded.
    pub fn is_ok(&self) -> bool {
        self.1.is_ok()
    }
    /// Error message when the step failed.
    pub fn error(&self) -> Option<&str> {
        self.1.as_ref().err().map(String::as_str)
    }
}

/// Report of a scenario run.
pub struct ScenarioReport(String, Vec<StepOutcome>);

impl ScenarioReport {
    /// Scenario name.
    pub fn name(&self) -> &str {
        &self.0
    }
    /// Outcomes in execution order.
    pub fn steps(&self) -> &[StepOutcome] {
        &self.1
    }
    /// Whether every executed step succeeded; `true` for an empty run.
    pub fn is_success(&self) -> bool {
        self.1.iter().all(StepOutcome::is_ok)
    }
    /// The first failing step, when present.
    pub fn failed_step(&self) -> Option<&StepOutcome> {
        self.1.iter().find(|step| !step.is_ok())
    }
}

/// Executes scenarios against the cluster model.
pub struct ScenarioRunner<'a> {
    topology: &'a ClusterTopology,
    nodes: &'a mut dyn NodeControl,
    faults: &'a mut dyn FaultInjector,
}

impl<'a> ScenarioRunner<'a> {
    /// Creates a runner over the given cluster model.
    pub fn new(
        topology: &'a ClusterTopology,
        nodes: &'a mut dyn NodeControl,
        faults: &'a mut dyn FaultInjector,
    ) -> Self {
        Self {
            topology,
            nodes,
            faults,
        }
    }
    /// Runs a scenario, stopping at the first failing step.
    pub fn run(&mut self, scenario: &Scenario) -> ScenarioReport {
        let mut steps = Vec::with_capacity(scenario.steps().len());
        for step in scenario.steps() {
            let outcome = self.execute(step.action());
            let failed = outcome.is_err();
            steps.push(StepOutcome(
                step.description().to_string(),
                outcome.map_err(|error| error.to_string()),
            ));
            if failed {
                break;
            }
        }
        ScenarioReport(scenario.name().to_string(), steps)
    }
    fn execute(&mut self, action: &Action) -> Result<(), HarnessError> {
        match action {
            Action::ApplyFault(fault) => self.faults.apply(fault),
            Action::RevertFault(fault) => self.faults.revert(fault),
            Action::StopNode(node) => self.nodes.stop(*node),
            Action::StartNode(node) => self.nodes.start(*node),
            Action::Check(check) => {
                let context = ScenarioContext::new(self.topology, &*self.nodes, &*self.faults);
                check.check(&context)
            }
        }
    }
}
