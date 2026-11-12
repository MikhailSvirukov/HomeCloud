//! Docker-backed system test for abrupt supervised-process restart (`TST-016/1`).
//!
//! The fixture is a supervisor shaped like the published storage-node runtime
//! contract (`distributed-server/RUNTIME_CONTRACT.md` §2): PID 1 is a shell with
//! exactly one child, and it starts a replacement after that child is killed.
//! The real storage-node image scenario belongs to the cluster scenarios that
//! provide the full manifest and key fixtures.
//!
//! The test is ignored by default so daemon-free runs stay fast; CI executes it
//! in the separate ignored-tests pass against a provided Docker daemon.

mod common;

use std::time::Duration;

use bollard::Docker;
use harness_core::NodeRole;
use harness_docker::{
    BollardClusterRuntime, ClusterPlan, ClusterRuntime, ContainerPlan, DockerProcessFault,
    PlanSettings, ProcessRestart, container_name,
};
use harness_test_support::topology;
use tokio::runtime::{Builder, Runtime};

use common::node_env;

/// Troubleshooting image that provides the supervisor shell for both nodes.
const NETSHOOT_IMAGE: &str = "nicolaka/netshoot";
/// Prefix of the per-run network name, so concurrent test runs do not collide.
const NETWORK_PREFIX: &str = "homecloud-test";
/// Suffix distinguishing this test's network from the other Docker tests'.
const PROCESS_NETWORK_SUFFIX: &str = "-process";
/// Timeout for observing the supervisor's replacement child.
const RESTART_TIMEOUT: Duration = Duration::from_secs(15);
/// Program that becomes PID 1 and supervises the single child.
const SUPERVISOR: &str = "sh";
/// Flag making the supervisor run the restart loop.
const SHELL_FLAG: &str = "-c";
/// Restart loop matching the published process model: one child, replaced after
/// it exits.
const SUPERVISOR_SCRIPT: &str = "while :; do sleep 60 & wait $!; done";

/// Builds a 1 dedicated + 1 storage plan on a unique network.
fn plan() -> ClusterPlan {
    let network = format!(
        "{NETWORK_PREFIX}-{}{PROCESS_NETWORK_SUFFIX}",
        std::process::id()
    );
    let settings = PlanSettings::new(network, NETSHOOT_IMAGE, NETSHOOT_IMAGE, node_env, None)
        .with_command(vec![
            SUPERVISOR.to_string(),
            SHELL_FLAG.to_string(),
            SUPERVISOR_SCRIPT.to_string(),
        ]);
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

/// Returns the single planned storage container.
fn storage_container(plan: &ClusterPlan) -> &ContainerPlan {
    plan.containers
        .iter()
        .find(|container| container.role == NodeRole::Storage)
        .expect("the fixture plan always contains one storage container")
}

/// Removes the started containers and network when the test leaves its scope.
struct ClusterGuard<'a> {
    runtime: &'a mut BollardClusterRuntime,
    plan: &'a ClusterPlan,
}

impl Drop for ClusterGuard<'_> {
    fn drop(&mut self) {
        let _ = self.runtime.remove(self.plan);
    }
}

/// `TST-016/1`, `distributed-server/RUNTIME_CONTRACT.md` §2: killing the
/// supervised child brings up a replacement in the same, non-recreated
/// container.
#[test]
#[ignore = "requires a Docker daemon; CI runs ignored integration tests"]
fn abrupt_child_kill_restarts_the_supervised_process_in_place() {
    let cluster_plan = plan();
    let docker = docker_client();
    let runtime = tokio_runtime();
    let mut cluster =
        BollardClusterRuntime::connect().expect("a Docker daemon is required for this test");

    cluster
        .start(&cluster_plan)
        .expect("the planned containers must start on the Docker daemon");
    let _guard = ClusterGuard {
        runtime: &mut cluster,
        plan: &cluster_plan,
    };

    let storage = storage_container(&cluster_plan);
    let storage_name = container_name(&cluster_plan, storage);
    let faults = DockerProcessFault::connect(cluster_plan.clone())
        .expect("a Docker daemon is required for this test");
    let restart: ProcessRestart = faults
        .restart_node(storage.node, "sleep", RESTART_TIMEOUT)
        .expect("the supervisor must start a replacement child");

    assert_ne!(
        restart.previous_pid, restart.current_pid,
        "the supervisor must start a new child PID"
    );
    assert!(
        !restart.container_id.is_empty(),
        "the restart must report the observed container id"
    );

    let inspected = runtime
        .block_on(docker.inspect_container(&storage_name, None))
        .expect("the restarted container must still be inspectable");

    assert_eq!(
        inspected.id.as_deref(),
        Some(restart.container_id.as_str()),
        "the container must not be recreated"
    );

    let state = inspected
        .state
        .expect("a running container must report its state");
    assert_eq!(
        state.started_at.as_deref(),
        Some(restart.started_at.as_str()),
        "the container StartedAt must be unchanged"
    );
    assert_eq!(
        state.running,
        Some(true),
        "the container must still be running after the child restart"
    );
}
