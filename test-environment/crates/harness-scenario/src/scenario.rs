//! Scenario definition and execution engine over the [`harness_core`] cluster model.

use harness_core::{ClusterTopology, Fault, FaultInjector, HarnessError, NodeControl, NodeId};

type Result<T> = core::result::Result<T, HarnessError>;

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

/// A single scenario step: a description plus an action.
pub struct Step {
    /// Human-readable description used in reports.
    pub description: String,
    /// The action to execute.
    pub action: Action,
}

/// A named, ordered list of steps describing a fault/recovery scenario.
pub struct Scenario {
    /// Scenario name.
    pub name: String,
    /// Ordered steps.
    pub steps: Vec<Step>,
}

impl Scenario {
    /// Starts building a scenario with the given name.
    pub fn builder(name: impl Into<String>) -> ScenarioBuilder {
        ScenarioBuilder::new(name)
    }
}

/// Builder for [`Scenario`].
#[derive(Default)]
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
        self.steps.push(Step {
            description: description.into(),
            action,
        });
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
        Scenario {
            name: self.name,
            steps: self.steps,
        }
    }
}

/// Read-only view of the cluster handed to custom checks.
pub struct ScenarioContext<'a> {
    /// Fixed topology.
    pub topology: &'a ClusterTopology,
    /// Node lifecycle control.
    pub nodes: &'a dyn NodeControl,
    /// Active fault state.
    pub faults: &'a dyn FaultInjector,
}

impl<'a> ScenarioContext<'a> {
    /// Creates a context over the given cluster model.
    pub fn new(
        topology: &'a ClusterTopology,
        nodes: &'a dyn NodeControl,
        faults: &'a dyn FaultInjector,
    ) -> Self {
        Self {
            topology,
            nodes,
            faults,
        }
    }
}

/// Extension point: a custom assertion evaluated during a scenario.
pub trait ScenarioCheck {
    /// Evaluates the assertion, returning a [`HarnessError`] when it fails.
    fn check(&self, context: &ScenarioContext<'_>) -> Result<()>;
}

/// Outcome of a single executed step.
pub struct StepOutcome {
    /// Step description.
    pub description: String,
    /// Success or the typed error the step failed with.
    pub result: Result<()>,
}

impl StepOutcome {
    /// Whether the step succeeded.
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }
    /// Typed error when the step failed.
    pub fn error(&self) -> Option<&HarnessError> {
        self.result.as_ref().err()
    }
}

/// Report of a scenario run.
pub struct ScenarioReport {
    /// Scenario name.
    pub name: String,
    /// Outcomes in execution order.
    pub steps: Vec<StepOutcome>,
}

impl ScenarioReport {
    /// Whether every executed step succeeded; `true` for an empty run.
    pub fn is_success(&self) -> bool {
        self.steps.iter().all(StepOutcome::is_ok)
    }
    /// The first failing step, when present.
    pub fn failed_step(&self) -> Option<&StepOutcome> {
        self.steps.iter().find(|step| !step.is_ok())
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
        let mut steps = Vec::with_capacity(scenario.steps.len());
        for step in &scenario.steps {
            steps.push(StepOutcome {
                description: step.description.clone(),
                result: self.execute(&step.action),
            });
            if steps.last().is_some_and(|outcome| !outcome.is_ok()) {
                break;
            }
        }
        ScenarioReport {
            name: scenario.name.clone(),
            steps,
        }
    }
    fn execute(&mut self, action: &Action) -> Result<()> {
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
