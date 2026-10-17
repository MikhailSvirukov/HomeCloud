//! Docker-backed integration tests over the `harness-docker` bollard runtime
//! (TST-004, TST-017).
//!
//! The tests are ignored by default so daemon-free runs stay fast; CI executes
//! them in the separate ignored-tests pass against a provided Docker daemon.

mod common;

use std::error::Error;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters::RemoveVolumeOptionsBuilder;
use futures_util::StreamExt;
use harness_core::NodeRole;
use harness_docker::{
    BollardClusterRuntime, ClusterPlan, ClusterRuntime, ContainerPlan, PlanSettings, container_name,
};
use harness_test_support::topology;
use tokio::runtime::{Builder, Runtime};

use common::{DATA_VOLUME_PATH, node_env, plan_order};

/// Image for both nodes; its default command keeps the container running.
const NGINX_IMAGE: &str = "nginx:alpine";
/// Prefix of the per-run network name, so concurrent test runs do not collide.
const NETWORK_PREFIX: &str = "homecloud-test";
/// Suffix distinguishing the volume test's network from the lifecycle test's.
const VOLUME_NETWORK_SUFFIX: &str = "-volume";
/// Value written to the node data volume and expected back after re-creation.
const PERSISTED: &str = "persisted";
/// File name holding [`PERSISTED`] inside the node data volume.
const MARKER_FILE: &str = "marker";

type TestResult<T> = Result<T, Box<dyn Error>>;

/// Builds a 1 dedicated + 1 storage plan on a network unique to this process.
fn plan() -> ClusterPlan {
    let network = format!("{NETWORK_PREFIX}-{}", std::process::id());
    let settings = PlanSettings::new(network, NGINX_IMAGE, NGINX_IMAGE, node_env, None);
    ClusterPlan::from_topology(&topology(1), &settings)
}

/// Builds a plan with a persistent node data volume on a distinct network.
fn volume_plan() -> ClusterPlan {
    let network = format!(
        "{NETWORK_PREFIX}-{}{VOLUME_NETWORK_SUFFIX}",
        std::process::id()
    );
    let settings = PlanSettings::new(
        network,
        NGINX_IMAGE,
        NGINX_IMAGE,
        node_env,
        Some(DATA_VOLUME_PATH.to_string()),
    );
    ClusterPlan::from_topology(&topology(1), &settings)
}

/// Connects a bollard client to the local Docker daemon.
fn docker_client() -> Docker {
    Docker::connect_with_local_defaults()
        .expect("a local Docker daemon configuration is required for this test")
}

/// Builds the current-thread runtime that drives the asynchronous bollard calls.
fn tokio_runtime() -> Runtime {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread Tokio runtime must be buildable")
}

/// Returns the single storage container of the fixture plan.
fn storage_container(plan: &ClusterPlan) -> &ContainerPlan {
    plan.containers
        .iter()
        .find(|container| container.role == NodeRole::Storage)
        .expect("the fixture plan always contains a storage container")
}

/// Name of the named volume the runtime binds to the container data path.
fn data_volume_name(plan: &ClusterPlan, container: &ContainerPlan) -> String {
    format!("{}-data", container_name(plan, container))
}

/// Path of the marker file inside the node data volume.
fn marker_path() -> String {
    format!("{DATA_VOLUME_PATH}/{MARKER_FILE}")
}

/// Runs a command in a running container and returns its stdout.
fn exec_stdout(
    docker: &Docker,
    runtime: &Runtime,
    container: &str,
    command: &[&str],
) -> TestResult<String> {
    runtime.block_on(async {
        let exec = docker
            .create_exec(
                container,
                CreateExecOptions {
                    cmd: Some(command.to_vec()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        let StartExecResults::Attached { mut output, .. } =
            docker.start_exec(&exec.id, None).await?
        else {
            return Err(std::io::Error::other("exec must attach to its output").into());
        };
        let mut stdout = Vec::new();
        while let Some(chunk) = output.next().await {
            match chunk? {
                LogOutput::StdOut { message } => stdout.extend_from_slice(&message),
                LogOutput::StdErr { .. } | LogOutput::StdIn { .. } | LogOutput::Console { .. } => {}
            }
        }
        Ok(String::from_utf8(stdout)?)
    })
}

#[test]
#[ignore = "requires a Docker daemon; CI runs ignored integration tests"]
fn bollard_runtime_starts_and_removes_a_cluster() {
    let plan = plan();
    let mut runtime =
        BollardClusterRuntime::connect().expect("a Docker daemon is required for this test");

    runtime
        .start(&plan)
        .expect("the planned containers must start on the Docker daemon");

    assert_eq!(
        runtime.running_containers(),
        plan_order(&plan),
        "every planned node is running after start"
    );

    runtime
        .remove(&plan)
        .expect("the started containers and network must be removable");

    assert!(
        runtime.running_containers().is_empty(),
        "no node is running after remove"
    );
}

#[test]
#[ignore = "requires a Docker daemon; CI runs ignored integration tests"]
fn a_recreated_container_keeps_its_data_volume() {
    let plan = volume_plan();
    let docker = docker_client();
    let runtime = tokio_runtime();
    let mut cluster =
        BollardClusterRuntime::connect().expect("a Docker daemon is required for this test");

    cluster
        .start(&plan)
        .expect("the planned containers must start on the Docker daemon");

    let storage = storage_container(&plan);
    let storage_name = container_name(&plan, storage);
    let marker = marker_path();
    let write = format!("printf {PERSISTED} > {marker}");

    exec_stdout(&docker, &runtime, &storage_name, &["sh", "-c", &write])
        .expect("writing the marker must succeed inside the storage container");

    cluster
        .remove(&plan)
        .expect("the started containers and network must be removable");

    cluster
        .start(&plan)
        .expect("the re-created containers must start on the Docker daemon");

    let persisted = exec_stdout(&docker, &runtime, &storage_name, &["cat", &marker]);

    cluster
        .remove(&plan)
        .expect("the re-created containers and network must be removable");
    let volume_options = RemoveVolumeOptionsBuilder::new().force(true).build();
    for container in &plan.containers {
        runtime
            .block_on(docker.remove_volume(
                &data_volume_name(&plan, container),
                Some(volume_options.clone()),
            ))
            .expect("the test-owned node data volumes must be removable");
    }

    assert_eq!(
        persisted.expect("reading the marker must succeed inside the re-created container"),
        PERSISTED,
        "the node data volume survives container removal and re-creation"
    );
}
