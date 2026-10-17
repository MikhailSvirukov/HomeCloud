//! Public scenario interface and registration (TST-009).

use crate::Scenario;

/// A scenario that can be registered without changing the container or network runner.
pub trait ScenarioSpec {
    /// Unique scenario name.
    fn name(&self) -> String;
    /// Builds the scenario's ordered steps.
    fn build(&self) -> Scenario;
}

/// Ordered registry of scenario specifications.
#[derive(Default)]
pub struct ScenarioRegistry {
    specs: Vec<Box<dyn ScenarioSpec>>,
}

impl ScenarioRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a scenario specification.
    pub fn register(&mut self, spec: Box<dyn ScenarioSpec>) {
        self.specs.push(spec);
    }

    /// Registered names in registration order.
    pub fn names(&self) -> impl Iterator<Item = String> + '_ {
        self.specs.iter().map(|spec| spec.name())
    }

    /// Builds the registered scenario with this exact name.
    pub fn build(&self, name: &str) -> Option<Scenario> {
        self.specs
            .iter()
            .find(|spec| spec.name() == name)
            .map(|spec| spec.build())
    }

    /// Whether no specifications are registered.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Number of registered specifications.
    pub fn len(&self) -> usize {
        self.specs.len()
    }
}
