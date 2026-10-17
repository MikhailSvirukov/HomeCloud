//! Docker CLI implementation of the container lifecycle seam.
//!
//! [`CommandRunner`] executes the rendered [`Command`]s and [`CliClusterRuntime`]
//! implements [`ClusterRuntime`](crate::ClusterRuntime) on top of it, so the
//! lifecycle can be driven by the Docker CLI while tests substitute
//! [`RecordingCommandRunner`].

use harness_core::{HarnessError, NodeId};

use crate::command::{
    Command, container_kill, container_remove, container_run, container_start, container_stop,
    network_create, network_remove,
};
use crate::plan::ClusterPlan;
use crate::runtime::ClusterRuntime;

type Result<T> = core::result::Result<T, HarnessError>;

/// Runs a Docker CLI command produced from a [`ClusterPlan`](crate::ClusterPlan).
pub trait CommandRunner {
    /// Runs one command, reporting a failure to start or a non-zero exit.
    fn run(&mut self, command: Command) -> Result<()>;
}

/// A [`CommandRunner`] that records commands instead of executing them.
pub struct RecordingCommandRunner {
    /// Commands recorded in call order.
    pub commands: Vec<Command>,
    fail_on_program: Option<String>,
}

impl RecordingCommandRunner {
    /// Creates a runner that records every command and succeeds.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            fail_on_program: None,
        }
    }

    /// Creates a runner that fails whenever it is asked to run `program`.
    pub fn failing_on(program: impl Into<String>) -> Self {
        Self {
            commands: Vec::new(),
            fail_on_program: Some(program.into()),
        }
    }
}

impl Default for RecordingCommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRunner for RecordingCommandRunner {
    fn run(&mut self, command: Command) -> Result<()> {
        let program = command.program.clone();
        self.commands.push(command);
        match self.fail_on_program.as_deref() == Some(program.as_str()) {
            true => Err(HarnessError::CommandFailed { program, code: 1 }),
            false => Ok(()),
        }
    }
}

/// A [`ClusterRuntime`](crate::ClusterRuntime) that drives the cluster through the Docker CLI.
pub struct CliClusterRuntime<R> {
    /// The command runner used for every lifecycle operation.
    pub runner: R,
    running: Vec<NodeId>,
    created: Vec<NodeId>,
    network_created: bool,
}

impl<R> CliClusterRuntime<R> {
    /// Creates a runtime over the given command runner.
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            running: Vec::new(),
            created: Vec::new(),
            network_created: false,
        }
    }
}

impl<R: CommandRunner> ClusterRuntime for CliClusterRuntime<R> {
    fn start(&mut self, plan: &ClusterPlan) -> Result<()> {
        if !self.network_created {
            self.runner.run(network_create(plan))?;
            self.network_created = true;
        }
        for container in &plan.containers {
            let node = container.node;
            match self.created.contains(&node) {
                false => {
                    self.runner.run(container_run(plan, container))?;
                    self.created.push(node);
                    self.running.push(node);
                }
                true if !self.running.contains(&node) => {
                    self.runner.run(container_start(plan, container))?;
                    self.running.push(node);
                }
                true => {}
            }
        }
        Ok(())
    }

    fn stop(&mut self, plan: &ClusterPlan) -> Result<()> {
        for node in &self.running {
            if let Some(container) = plan.container(*node) {
                self.runner.run(container_stop(plan, container))?;
            }
        }
        self.running.clear();
        Ok(())
    }

    fn kill(&mut self, plan: &ClusterPlan, node: NodeId) -> Result<()> {
        match plan.container(node) {
            Some(container) if self.running.contains(&node) => {
                self.runner.run(container_kill(plan, container))?;
                self.running.retain(|running| *running != node);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn remove(&mut self, plan: &ClusterPlan) -> Result<()> {
        for container in &plan.containers {
            if self.created.contains(&container.node) {
                self.runner.run(container_remove(plan, container))?;
            }
        }
        if self.network_created {
            self.runner.run(network_remove(plan))?;
            self.network_created = false;
        }
        self.created.clear();
        self.running.clear();
        Ok(())
    }

    fn running_containers(&self) -> &[NodeId] {
        &self.running
    }
}
