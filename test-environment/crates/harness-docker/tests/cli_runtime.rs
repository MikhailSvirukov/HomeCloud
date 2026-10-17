//! Docker-free integration tests over the `harness-docker` CLI runtime.
//!
//! These drive [`CliClusterRuntime`] with a [`RecordingCommandRunner`] exactly as
//! an embedding scenario would: build a validated topology, derive a
//! [`ClusterPlan`], and assert the ordered Docker commands the lifecycle emits.
//! No Docker daemon or socket is touched.

mod common;

use harness_core::HarnessError;
use harness_docker::{
    CliClusterRuntime, ClusterPlan, ClusterRuntime, Command, ContainerPlan, RecordingCommandRunner,
    container_name, container_remove, container_run, container_stop, network_create,
    network_remove,
};

use common::{DATA_VOLUME_PATH, DOCKER, plan, plan_order, plan_with_data_volume};

/// Volume-mount flag for `docker run`.
const VOLUME: &str = "--volume";

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

/// Whether two commands share the same program and ordered arguments.
fn same_command(actual: &Command, expected: &Command) -> bool {
    actual.program == expected.program && actual.args == expected.args
}

/// The `--volume` specification of every recorded `docker run` command.
fn volume_mounts(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| {
            let flag = command.args.iter().position(|arg| arg.as_str() == VOLUME)?;
            command.args.get(flag + 1).cloned()
        })
        .collect()
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
fn start_after_remove_recreates_the_network() {
    let plan = plan();
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::new());
    runtime.start(&plan).expect("a recording start cannot fail");
    runtime
        .remove(&plan)
        .expect("a recording remove cannot fail");
    let after_remove = runtime.runner.commands.len();

    runtime
        .start(&plan)
        .expect("a recording restart cannot fail");

    let network = network_create(&plan);
    let network_creates = runtime
        .runner
        .commands
        .iter()
        .filter(|command| same_command(command, &network))
        .count();
    assert_eq!(
        network_creates, 2,
        "each start recreates the network once removal has torn it down"
    );

    let mut expected = vec![network_create(&plan)];
    expected.extend(commands_for(&plan, container_run));
    assert_commands(&runtime.runner.commands[after_remove..], &expected);
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

    let mut expected = commands_for(&plan, container_remove);
    expected.push(network_remove(&plan));
    assert_commands(&runtime.runner.commands[after_start..], &expected);
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
fn re_created_container_reattaches_the_same_data_volume() {
    let plan = plan_with_data_volume(DATA_VOLUME_PATH);
    let mut runtime = CliClusterRuntime::new(RecordingCommandRunner::new());

    runtime.start(&plan).expect("a recording start cannot fail");
    let after_first_start = runtime.runner.commands.len();

    runtime
        .remove(&plan)
        .expect("a recording remove cannot fail");
    let after_remove = runtime.runner.commands.len();

    runtime
        .start(&plan)
        .expect("a recording restart cannot fail");

    let expected_volumes: Vec<String> = plan
        .containers
        .iter()
        .map(|container| {
            format!(
                "{}-data:{DATA_VOLUME_PATH}",
                container_name(&plan, container)
            )
        })
        .collect();
    assert_eq!(
        volume_mounts(&runtime.runner.commands[1..after_first_start]),
        expected_volumes,
        "the first start mounts each node data volume"
    );
    assert_eq!(
        volume_mounts(&runtime.runner.commands[after_remove + 1..]),
        expected_volumes,
        "re-created containers reattach the same node data volumes"
    );

    let mut expected_removals = commands_for(&plan, container_remove);
    expected_removals.push(network_remove(&plan));
    assert_commands(
        &runtime.runner.commands[after_first_start..after_remove],
        &expected_removals,
    );
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
