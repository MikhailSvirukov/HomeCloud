//! Docker-free integration tests over the `harness-docker` CLI runtime.
//!
//! These drive [`CliClusterRuntime`] with a [`RecordingCommandRunner`] exactly as
//! an embedding scenario would: build a validated topology, derive a
//! [`ClusterPlan`], and assert the ordered Docker commands the lifecycle emits.
//! No Docker daemon or socket is touched.

use std::collections::BTreeMap;

use harness_core::{HarnessError, NodeId, NodeRole, NodeSpec};
use harness_docker::{
    CliClusterRuntime, ClusterPlan, ClusterRuntime, Command, ContainerPlan, EnvBuilder,
    PlanSettings, RecordingCommandRunner, container_remove, container_run, container_stop,
    network_create,
};
use harness_test_support::topology;

/// The Docker CLI program every command invokes.
const DOCKER: &str = "docker";
/// Network name configured for the isolated test cluster.
const NETWORK: &str = "harness-net";
/// Image configured for the dedicated-server container.
const SERVER_IMAGE: &str = "server:1";
/// Image configured for storage containers.
const STORAGE_IMAGE: &str = "storage:1";
/// Number of storage nodes in the shared fixture: 1 dedicated + 3 storage.
const STORAGE_NODES: u16 = 3;

/// Environment key holding the node id in the shared settings.
const NODE_ID_KEY: &str = "NODE_ID";
/// Environment key holding the node label in the shared settings.
const NODE_LABEL_KEY: &str = "NODE_LABEL";
/// Environment key holding the node role in the shared settings.
const NODE_ROLE_KEY: &str = "NODE_ROLE";
/// Environment key holding the listen address in the shared settings.
const LISTEN_ADDRESS_KEY: &str = "LISTEN_ADDRESS";
/// Role value rendered for the dedicated server.
const DEDICATED_SERVER_ROLE: &str = "dedicated-server";
/// Role value rendered for storage nodes.
const STORAGE_ROLE: &str = "storage";

/// Role value the shared settings builder renders for a role.
fn role_value(role: NodeRole) -> &'static str {
    match role {
        NodeRole::DedicatedServer => DEDICATED_SERVER_ROLE,
        NodeRole::Storage => STORAGE_ROLE,
    }
}

/// Environment builder the shared settings use, matching the caller contract.
fn node_env(spec: &NodeSpec) -> BTreeMap<String, String> {
    BTreeMap::from([
        (NODE_ID_KEY.to_string(), spec.id.to_string()),
        (NODE_LABEL_KEY.to_string(), spec.label.clone()),
        (NODE_ROLE_KEY.to_string(), role_value(spec.role).to_string()),
        (LISTEN_ADDRESS_KEY.to_string(), spec.address.to_string()),
    ])
}

/// Builds the plan settings used across these tests.
fn settings() -> PlanSettings {
    let env: EnvBuilder = node_env;
    PlanSettings::new(NETWORK, SERVER_IMAGE, STORAGE_IMAGE, env)
}

/// Derives a plan for one dedicated server and three storage nodes.
fn plan() -> ClusterPlan {
    ClusterPlan::from_topology(&topology(STORAGE_NODES), &settings())
}

/// The node ids in plan (topology) order.
fn plan_order(plan: &ClusterPlan) -> Vec<NodeId> {
    plan.containers.iter().map(|c| c.node).collect()
}

/// Renders the expected command for every plan container, in plan order.
fn commands_for(
    plan: &ClusterPlan,
    render: fn(&ClusterPlan, &ContainerPlan) -> Command,
) -> Vec<Command> {
    plan.containers.iter().map(|c| render(plan, c)).collect()
}

/// Asserts recorded commands match expected commands, one to one and in order.
fn assert_commands(recorded: &[Command], expected: &[Command]) {
    assert_eq!(recorded.len(), expected.len(), "recorded command count");
    for (index, (actual, expected)) in recorded.iter().zip(expected).enumerate() {
        assert_eq!(actual.program, expected.program, "command {index} program");
        assert_eq!(actual.args, expected.args, "command {index} args");
    }
}

#[test]
fn start_creates_the_network_then_each_container() {
    let plan = plan();
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::new());

    runtime.start(&plan).expect("a recording start cannot fail");

    assert_commands(&runtime.runner.commands[..1], &[network_create(&plan)]);
    assert_commands(
        &runtime.runner.commands[1..],
        &commands_for(&plan, container_run),
    );
    assert_eq!(runtime.running_containers(), plan_order(&plan));
}

#[test]
fn start_is_idempotent_and_does_not_recreate_containers() {
    let plan = plan();
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::new());
    runtime.start(&plan).expect("a recording start cannot fail");
    let after_first_start = runtime.runner.commands.len();

    runtime.start(&plan).expect("repeated start is idempotent");

    assert_eq!(
        runtime.runner.commands.len(),
        after_first_start,
        "an idempotent start records no further commands"
    );
    assert_eq!(runtime.running_containers(), plan_order(&plan));
}

#[test]
fn stop_targets_running_containers_and_is_idempotent() {
    let plan = plan();
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::new());
    runtime.start(&plan).expect("a recording start cannot fail");
    let after_start = runtime.runner.commands.len();

    runtime.stop(&plan).expect("a recording stop cannot fail");

    assert_commands(
        &runtime.runner.commands[after_start..],
        &commands_for(&plan, container_stop),
    );
    assert!(runtime.running_containers().is_empty());

    let after_first_stop = runtime.runner.commands.len();
    runtime.stop(&plan).expect("repeated stop is idempotent");
    assert_eq!(
        runtime.runner.commands.len(),
        after_first_stop,
        "an idempotent stop records no further commands"
    );
}

#[test]
fn remove_removes_every_container() {
    let plan = plan();
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::new());
    runtime.start(&plan).expect("a recording start cannot fail");
    let after_start = runtime.runner.commands.len();

    runtime
        .remove(&plan)
        .expect("a recording remove cannot fail");

    assert_commands(
        &runtime.runner.commands[after_start..],
        &commands_for(&plan, container_remove),
    );
    assert!(runtime.running_containers().is_empty());

    let after_first_remove = runtime.runner.commands.len();
    runtime
        .remove(&plan)
        .expect("repeated remove is idempotent");
    assert_eq!(
        runtime.runner.commands.len(),
        after_first_remove,
        "an idempotent remove records no further commands"
    );
    assert!(runtime.running_containers().is_empty());
}

#[test]
fn a_failing_runner_surfaces_the_command_error() {
    let plan = plan();
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::failing_on(DOCKER));

    let error = runtime
        .start(&plan)
        .expect_err("a failing docker command must surface as an error");

    assert!(
        matches!(&error, HarnessError::CommandFailed { program, .. } if program == DOCKER),
        "expected a docker CommandFailed, got {error:?}"
    );
}
