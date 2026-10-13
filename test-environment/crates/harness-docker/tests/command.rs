//! Docker-free integration tests over the `harness-docker` command renderers.
//!
//! These drive the public command helpers exactly as an embedding scenario
//! would: build a validated topology, derive a [`ClusterPlan`], and assert the
//! ordered `docker` arguments. No Docker daemon or socket is touched.

use std::collections::BTreeMap;

use harness_core::NodeId;
use harness_docker::{
    ClusterPlan, ContainerPlan, PlanSettings, container_name, container_remove, container_run,
    container_stop, network_create,
};
use harness_test_support::topology;

/// The Docker CLI program every command must invoke.
const DOCKER: &str = "docker";
/// `docker network` subcommand.
const NETWORK: &str = "network";
/// `network create` subcommand.
const CREATE: &str = "create";
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
/// Network name configured for the isolated test cluster.
const NETWORK_NAME: &str = "harness-net";
/// Image configured for the dedicated-server container.
const SERVER_IMAGE: &str = "server:1";
/// Image configured for storage containers.
const STORAGE_IMAGE: &str = "storage:1";
/// Node id of the single dedicated server.
const DEDICATED_ID: u16 = 1;
/// First node id handed out to storage nodes.
const FIRST_STORAGE_ID: u16 = 2;
/// Number of storage nodes in the shared fixture.
const STORAGE_NODES: u16 = 3;

/// Derives the plan settings used across these tests.
fn settings() -> PlanSettings {
    PlanSettings::new(NETWORK_NAME, SERVER_IMAGE, STORAGE_IMAGE)
}

/// Derives a plan for the shared test topology.
fn plan() -> ClusterPlan {
    ClusterPlan::from_topology(&topology(STORAGE_NODES), &settings())
}

/// The dedicated-server container of a plan.
fn dedicated(plan: &ClusterPlan) -> &ContainerPlan {
    plan.container(NodeId::new(DEDICATED_ID))
        .expect("the plan always has the dedicated-server container")
}

/// The first storage container of a plan.
fn storage(plan: &ClusterPlan) -> &ContainerPlan {
    plan.container(NodeId::new(FIRST_STORAGE_ID))
        .expect("the plan always has the first storage container")
}

/// Turns `&str` slices into owned argument vectors for comparison.
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// The argument immediately following `flag`.
fn value_after<'a>(args: &'a [String], flag: &str) -> &'a str {
    let position = args
        .iter()
        .position(|arg| arg == flag)
        .expect("the expected flag is present in the argument list");
    args.get(position + 1)
        .map(String::as_str)
        .expect("every rendered flag is followed by a value")
}

/// Renders the `-e KEY=VALUE` arguments for an env map in `BTreeMap` order.
fn env_args(env: &BTreeMap<String, String>) -> Vec<String> {
    env.iter()
        .flat_map(|(key, value)| [ENV.to_string(), format!("{key}={value}")])
        .collect()
}

#[test]
fn network_create_matches_the_configured_network() {
    let plan = plan();
    let command = network_create(&plan);

    assert_eq!(command.program, DOCKER);
    assert_eq!(command.args, strings(&[NETWORK, CREATE, NETWORK_NAME]));
    assert_eq!(plan.network.name, NETWORK_NAME);
}

#[test]
fn container_run_is_consistent_with_each_plan_container() {
    let plan = plan();

    for container in &plan.containers {
        let run = container_run(&plan, container);
        let expected_name = container_name(&plan, container);

        assert_eq!(run.program, DOCKER);
        assert_eq!(value_after(&run.args, NAME), expected_name.as_str());
        assert_eq!(
            value_after(&run.args, HOSTNAME),
            container.hostname.as_str()
        );
        assert_eq!(
            value_after(&run.args, HOSTNAME),
            value_after(&run.args, NETWORK_ALIAS)
        );
        assert_eq!(value_after(&run.args, NETWORK_FLAG), plan.network.name);
        assert_eq!(
            value_after(&run.args, NETWORK_ALIAS),
            container.hostname.as_str()
        );
        assert_eq!(run.args.last(), Some(&container.image));
    }
}

#[test]
fn container_run_passes_every_env_entry() {
    let plan = plan();

    for container in [dedicated(&plan), storage(&plan)] {
        let run = container_run(&plan, container);
        let expected = env_args(&container.env);
        let start = run
            .args
            .iter()
            .position(|arg| arg == ENV)
            .expect("a container run always renders at least one env flag");

        assert_eq!(
            &run.args[start..start + expected.len()],
            expected.as_slice()
        );
        assert_eq!(run.args.get(start + expected.len()), Some(&container.image));
    }
}

#[test]
fn stop_and_remove_target_the_container_name() {
    let plan = plan();
    let container = storage(&plan);
    let name = container_name(&plan, container);

    let stop = container_stop(&plan, container);
    assert_eq!(stop.program, DOCKER);
    assert_eq!(stop.args, strings(&[STOP, &name]));

    let remove = container_remove(&plan, container);
    assert_eq!(remove.program, DOCKER);
    assert_eq!(remove.args, strings(&[REMOVE, FORCE, &name]));
}
