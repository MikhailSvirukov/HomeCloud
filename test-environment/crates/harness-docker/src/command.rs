//! Deterministic Docker CLI rendering for a [`ClusterPlan`].
//!
//! Each helper returns a [`Command`] with exactly the program and ordered
//! arguments needed to stand the plan up. Rendering is side-effect free, so the
//! commands can be asserted in fast, daemon-free tests and later executed by a
//! CLI-based [`ClusterRuntime`](crate::runtime::ClusterRuntime).

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    program: String,
    args: Vec<String>,
}

impl Command {
    /// Creates a command from a program and its arguments.
    pub fn new(program: impl Into<String>, args: impl IntoIterator<Item = String>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().collect(),
        }
    }
    /// The program to execute.
    pub fn program(&self) -> &str {
        &self.program
    }
    /// The ordered arguments.
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

/// Deterministic container name for a node: `<network>-<label>`.
pub fn container_name(plan: &ClusterPlan, container: &ContainerPlan) -> String {
    format!("{}-{}", plan.network().name(), container.label)
}

/// `docker network create <network>`.
pub fn network_create(plan: &ClusterPlan) -> Command {
    Command::new(
        DOCKER,
        [
            NETWORK.to_string(),
            CREATE.to_string(),
            plan.network().name().to_string(),
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
        plan.network().name().to_string(),
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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use harness_core::{ClusterTopology, NodeId, NodeSpec};

    use super::*;
    use crate::plan::PlanSettings;

    const NETWORK_NAME: &str = "harness-net";
    const DEDICATED_IMAGE: &str = "server:1";
    const STORAGE_IMAGE: &str = "storage:1";

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn topology() -> ClusterTopology {
        let mut builder = ClusterTopology::builder().dedicated_server(NodeSpec::dedicated_server(
            NodeId::new(1),
            "dedicated",
            addr(7001),
        ));
        for i in 0..3 {
            let id = i + 2;
            builder = builder.storage_node(NodeSpec::storage(
                NodeId::new(id),
                format!("s-{id}"),
                addr(7002 + i),
            ));
        }
        builder.build().expect("valid topology")
    }

    fn plan() -> ClusterPlan {
        let settings = PlanSettings::new(NETWORK_NAME, DEDICATED_IMAGE, STORAGE_IMAGE);
        ClusterPlan::from_topology(&topology(), &settings)
    }

    fn dedicated(plan: &ClusterPlan) -> &ContainerPlan {
        plan.containers()
            .first()
            .expect("plan has a dedicated container")
    }

    fn storage(plan: &ClusterPlan) -> &ContainerPlan {
        plan.containers()
            .get(1)
            .expect("plan has a storage container")
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn network_create_uses_docker_program() {
        assert_eq!(network_create(&plan()).program(), DOCKER);
    }

    #[test]
    fn network_create_pins_subcommand_and_network_name() {
        assert_eq!(
            network_create(&plan()).args(),
            strings(&[NETWORK, CREATE, NETWORK_NAME]).as_slice()
        );
    }

    #[test]
    fn container_name_for_dedicated_prefixes_network() {
        let plan = plan();
        assert_eq!(
            container_name(&plan, dedicated(&plan)),
            format!("{NETWORK_NAME}-dedicated")
        );
    }

    #[test]
    fn container_name_for_storage_prefixes_network() {
        let plan = plan();
        assert_eq!(
            container_name(&plan, storage(&plan)),
            format!("{NETWORK_NAME}-s-2")
        );
    }

    #[test]
    fn container_run_pins_flags_in_order() {
        let plan = plan();
        let container = dedicated(&plan);
        let run = container_run(&plan, container);
        let expected = strings(&[
            RUN,
            DETACH,
            NAME,
            &container_name(&plan, container),
            HOSTNAME,
            &container.hostname,
            NETWORK_FLAG,
            NETWORK_NAME,
            NETWORK_ALIAS,
            &container.hostname,
        ]);
        assert_eq!(run.program(), DOCKER);
        assert_eq!(&run.args()[..expected.len()], expected.as_slice());
    }

    #[test]
    fn container_run_ends_with_container_image() {
        let plan = plan();
        let container = storage(&plan);
        let run = container_run(&plan, container);
        assert_eq!(run.args().last(), Some(&container.image));
    }

    #[test]
    fn container_run_emits_every_env_entry_in_key_order() {
        let plan = plan();
        let container = dedicated(&plan);
        let run = container_run(&plan, container);
        let expected: Vec<String> = container
            .env
            .iter()
            .flat_map(|(key, value)| [ENV.to_string(), format!("{key}={value}")])
            .collect();
        let start = run
            .args()
            .iter()
            .position(|arg| arg == ENV)
            .expect("run args contain an env flag");
        assert_eq!(
            &run.args()[start..start + expected.len()],
            expected.as_slice()
        );
    }

    #[test]
    fn dedicated_and_storage_use_different_images() {
        let plan = plan();
        let dedicated_run = container_run(&plan, dedicated(&plan));
        let storage_run = container_run(&plan, storage(&plan));
        assert_eq!(
            dedicated_run.args().last(),
            Some(&DEDICATED_IMAGE.to_string())
        );
        assert_eq!(storage_run.args().last(), Some(&STORAGE_IMAGE.to_string()));
    }

    #[test]
    fn container_stop_targets_container_name() {
        let plan = plan();
        let container = storage(&plan);
        assert_eq!(
            container_stop(&plan, container).args(),
            strings(&[STOP, &container_name(&plan, container)]).as_slice()
        );
    }

    #[test]
    fn container_remove_forces_container_name() {
        let plan = plan();
        let container = dedicated(&plan);
        assert_eq!(
            container_remove(&plan, container).args(),
            strings(&[REMOVE, FORCE, &container_name(&plan, container)]).as_slice()
        );
    }
}
