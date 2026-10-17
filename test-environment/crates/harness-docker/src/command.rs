//! Deterministic Docker CLI rendering for a [`ClusterPlan`].
//!
//! Each helper returns a [`Command`] with exactly the program and ordered
//! arguments needed to stand the plan up. Rendering is side-effect free, so the
//! commands can be asserted in fast, daemon-free tests and later executed by a
//! CLI-based [`ClusterRuntime`](crate::ClusterRuntime).

use crate::plan::{ClusterPlan, ContainerPlan};

/// The Docker CLI program invoked for every command.
const DOCKER: &str = "docker";

/// Subcommands and flag literals reused across the helpers.
const NETWORK: &str = "network";
const CREATE: &str = "create";
const RUN: &str = "run";
const DETACH: &str = "--detach";
const NAME: &str = "--name";
const HOSTNAME: &str = "--hostname";
const NETWORK_FLAG: &str = "--network";
const NETWORK_ALIAS: &str = "--network-alias";
const ENV: &str = "-e";
const STOP: &str = "stop";
const REMOVE: &str = "rm";
const FORCE: &str = "--force";

/// One external command: a program plus its ordered arguments.
pub struct Command {
    /// The program to execute.
    pub program: String,
    /// The ordered arguments.
    pub args: Vec<String>,
}

impl Command {
    /// Creates a command from a program and its arguments.
    pub fn new(program: impl Into<String>, args: impl IntoIterator<Item = String>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().collect(),
        }
    }
}

/// Deterministic container name for a node: `<network>-<label>`.
pub fn container_name(plan: &ClusterPlan, container: &ContainerPlan) -> String {
    format!("{}-{}", plan.network.name, container.label)
}

/// `docker network create <network>`.
pub fn network_create(plan: &ClusterPlan) -> Command {
    Command::new(
        DOCKER,
        [
            NETWORK.to_string(),
            CREATE.to_string(),
            plan.network.name.clone(),
        ],
    )
}

/// `docker network rm <network>`.
pub fn network_remove(plan: &ClusterPlan) -> Command {
    Command::new(
        DOCKER,
        [
            NETWORK.to_string(),
            REMOVE.to_string(),
            plan.network.name.clone(),
        ],
    )
}

/// `docker run --detach ...` for one container on the plan's network.
pub fn container_run(plan: &ClusterPlan, container: &ContainerPlan) -> Command {
    let mut args = vec![
        RUN.to_string(),
        DETACH.to_string(),
        NAME.to_string(),
        container_name(plan, container),
        HOSTNAME.to_string(),
        container.hostname.clone(),
        NETWORK_FLAG.to_string(),
        plan.network.name.clone(),
        NETWORK_ALIAS.to_string(),
        container.hostname.clone(),
    ];
    for (key, value) in &container.env {
        args.push(ENV.to_string());
        args.push(format!("{key}={value}"));
    }
    args.push(container.image.clone());
    Command::new(DOCKER, args)
}

/// `docker stop <container>`.
pub fn container_stop(plan: &ClusterPlan, container: &ContainerPlan) -> Command {
    Command::new(DOCKER, [STOP.to_string(), container_name(plan, container)])
}

/// `docker rm --force <container>`.
pub fn container_remove(plan: &ClusterPlan, container: &ContainerPlan) -> Command {
    Command::new(
        DOCKER,
        [
            REMOVE.to_string(),
            FORCE.to_string(),
            container_name(plan, container),
        ],
    )
}
