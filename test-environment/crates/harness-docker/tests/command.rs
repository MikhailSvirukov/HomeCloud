//! Docker-free integration tests over the `harness-docker` command renderers.
//!
//! These drive the public command helpers exactly as an embedding scenario
//! would: build a validated topology, derive a [`ClusterPlan`], and assert the
//! ordered `docker` arguments. No Docker daemon or socket is touched.

mod common;

use std::collections::BTreeMap;

use harness_core::NodeId;
use harness_docker::{
    ClusterPlan, Command, ContainerPlan, container_name, container_remove, container_run,
    container_stop, network_create,
};

use common::{FIRST_STORAGE_ID, NETWORK_NAME, plan};

/// The Docker CLI program every command must invoke.
const DOCKER: &str = "docker";
/// `docker network` subcommand.
const NETWORK: &str = "network";
/// `network create` subcommand.
const CREATE: &str = "create";
/// `docker run` subcommand.
const RUN: &str = "run";
/// Detach flag for `docker run`.
const DETACH: &str = "--detach";
/// Container-name flag.
const NAME: &str = "--name";
/// Container-hostname flag.
const HOSTNAME: &str = "--hostname";
/// Network flag.
const NETWORK_FLAG: &str = "--network";
/// Network alias flag.
const NETWORK_ALIAS: &str = "--network-alias";
/// Environment variable flag.
const ENV: &str = "-e";
/// `docker stop` subcommand.
const STOP: &str = "stop";
/// `docker rm` subcommand.
const REMOVE: &str = "rm";
/// Force flag for `docker rm`.
const FORCE: &str = "--force";

/// The first storage container of a plan.
fn storage(plan: &ClusterPlan) -> &ContainerPlan {
    plan.container(NodeId::new(FIRST_STORAGE_ID))
        .expect("the plan always has the first storage container")
}

/// Turns `&str` slices into owned argument vectors for comparison.
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Renders the `-e KEY=VALUE` arguments for an env map in `BTreeMap` order.
fn env_args(env: &BTreeMap<String, String>) -> Vec<String> {
    env.iter()
        .flat_map(|(key, value)| [ENV.to_string(), format!("{key}={value}")])
        .collect()
}

/// The `docker network create` command these tests expect for `plan`.
fn expected_network_create() -> Command {
    Command::new(DOCKER, strings(&[NETWORK, CREATE, NETWORK_NAME]))
}

/// The `docker run --detach` command these tests expect for one container.
fn expected_run_command(plan: &ClusterPlan, container: &ContainerPlan) -> Command {
    let name = container_name(plan, container);
    let mut args = strings(&[
        RUN,
        DETACH,
        NAME,
        name.as_str(),
        HOSTNAME,
        container.hostname.as_str(),
        NETWORK_FLAG,
        plan.network.name.as_str(),
        NETWORK_ALIAS,
        container.hostname.as_str(),
    ]);
    args.extend(env_args(&container.env));
    args.push(container.image.clone());
    Command::new(DOCKER, args)
}

/// The `docker stop` command these tests expect for one container.
fn expected_stop_command(plan: &ClusterPlan, container: &ContainerPlan) -> Command {
    Command::new(
        DOCKER,
        strings(&[STOP, container_name(plan, container).as_str()]),
    )
}

/// The `docker rm --force` command these tests expect for one container.
fn expected_remove_command(plan: &ClusterPlan, container: &ContainerPlan) -> Command {
    Command::new(
        DOCKER,
        strings(&[REMOVE, FORCE, container_name(plan, container).as_str()]),
    )
}

#[test]
fn network_create_matches_the_expected_command() {
    let plan = plan();
    let actual = network_create(&plan);
    let expected = expected_network_create();

    assert_eq!(actual.program, expected.program);
    assert_eq!(actual.args, expected.args);
    assert_eq!(plan.network.name, NETWORK_NAME);
}

#[test]
fn container_run_matches_the_expected_command_for_every_container() {
    let plan = plan();

    for container in &plan.containers {
        let actual = container_run(&plan, container);
        let expected = expected_run_command(&plan, container);

        assert_eq!(actual.program, expected.program);
        assert_eq!(actual.args, expected.args);
    }
}

#[test]
fn container_run_leads_with_run_and_detach() {
    let plan = plan();
    let run = container_run(&plan, storage(&plan));

    assert_eq!(run.args.first().map(String::as_str), Some(RUN));
    assert_eq!(run.args.get(1).map(String::as_str), Some(DETACH));
}

#[test]
fn stop_and_remove_match_the_expected_commands() {
    let plan = plan();
    let container = storage(&plan);

    let stop = container_stop(&plan, container);
    let expected_stop = expected_stop_command(&plan, container);
    assert_eq!(stop.program, expected_stop.program);
    assert_eq!(stop.args, expected_stop.args);

    let remove = container_remove(&plan, container);
    let expected_remove = expected_remove_command(&plan, container);
    assert_eq!(remove.program, expected_remove.program);
    assert_eq!(remove.args, expected_remove.args);
}
