//! Docker-backed system test for selective network fault injection (TST-006/2).
//!
//! The test is ignored by default so daemon-free runs stay fast; CI executes it
//! in the separate ignored-tests pass against a provided Docker daemon.

mod common;

use std::error::Error;
use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::StreamExt;
use harness_core::{
    Fault, FaultInjector, LossPercent, NetworkConditions, NodeId, NodeRole, PeerLink,
};
use harness_docker::{
    BollardClusterRuntime, ClusterPlan, ClusterRuntime, ContainerPlan, DockerFaultInjector,
    PlanSettings, container_name,
};
use harness_test_support::topology;
use tokio::runtime::{Builder, Runtime};

use common::node_env;

/// Troubleshooting image that provides `ping` and `iptables` for both nodes.
const NETSHOOT_IMAGE: &str = "nicolaka/netshoot";
/// Prefix of the per-run network name, so concurrent test runs do not collide.
const NETWORK_PREFIX: &str = "homecloud-test";
/// Suffix distinguishing this test's network from the lifecycle tests'.
const FAULT_NETWORK_SUFFIX: &str = "-faults";
/// Command that keeps both containers running for the duration of the test.
const KEEP_ALIVE: &str = "sleep";
/// Seconds both containers stay alive after their image starts.
const KEEP_ALIVE_SECONDS: &str = "600";
/// Capability `iptables` needs inside the containers for packet filtering.
const NET_ADMIN: &str = "NET_ADMIN";
/// Ping program used to check connectivity between the node pair.
const PING: &str = "ping";
/// Ping flag limiting the probe to a single packet.
const PING_COUNT_FLAG: &str = "-c";
/// Ping flag bounding the wait for a reply.
const PING_TIMEOUT_FLAG: &str = "-W";
/// One packet per probe.
const PING_COUNT: &str = "1";
/// One second per reply wait, keeping the cut-link probe short.
const PING_TIMEOUT_SECONDS: &str = "1";

type TestResult<T> = Result<T, Box<dyn Error>>;

/// Builds a 1 dedicated + 1 storage fault-injection plan on a unique network.
fn plan() -> ClusterPlan {
    let network = format!(
        "{NETWORK_PREFIX}-{}{FAULT_NETWORK_SUFFIX}",
        std::process::id()
    );
    let settings = PlanSettings::new(network, NETSHOOT_IMAGE, NETSHOOT_IMAGE, node_env, None)
        .with_command(vec![KEEP_ALIVE.to_string(), KEEP_ALIVE_SECONDS.to_string()])
        .with_cap_add(vec![NET_ADMIN.to_string()]);
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

/// Returns the single planned container with the given role.
fn container_with_role(plan: &ClusterPlan, role: NodeRole) -> &ContainerPlan {
    plan.containers
        .iter()
        .find(|container| container.role == role)
        .expect("the fixture plan always contains one container per role")
}

/// Resolves a container's address on the plan network through the Docker API.
fn container_ip(
    docker: &Docker,
    runtime: &Runtime,
    plan: &ClusterPlan,
    container: &ContainerPlan,
) -> TestResult<String> {
    let name = container_name(plan, container);
    let inspected = runtime.block_on(docker.inspect_container(&name, None))?;
    inspected
        .network_settings
        .and_then(|settings| settings.networks)
        .and_then(|networks| networks.get(&plan.network.name).cloned())
        .and_then(|endpoint| endpoint.ip_address)
        .filter(|address| !address.is_empty())
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "container {name} has no address on network {}",
                plan.network.name
            ))
            .into()
        })
}

/// Runs an exec inside a container and returns its exit code and merged output.
fn exec(
    docker: &Docker,
    runtime: &Runtime,
    container: &str,
    command: &[&str],
) -> TestResult<(i64, String)> {
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
        let mut collected = Vec::new();
        while let Some(chunk) = output.next().await {
            match chunk? {
                LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                    collected.extend_from_slice(&message);
                }
                LogOutput::StdIn { .. } | LogOutput::Console { .. } => {}
            }
        }
        let inspected = docker.inspect_exec(&exec.id).await?;
        let exit_code = inspected.exit_code.ok_or_else(|| {
            std::io::Error::other("exec exit code must be available once its output ends")
        })?;
        Ok((exit_code, String::from_utf8(collected)?))
    })
}

/// Single-packet probe from the dedicated node towards the storage node.
fn ping_command(storage_ip: &str) -> Vec<&str> {
    vec![
        PING,
        PING_COUNT_FLAG,
        PING_COUNT,
        PING_TIMEOUT_FLAG,
        PING_TIMEOUT_SECONDS,
        storage_ip,
    ]
}

/// Fault that drops every packet on the selected pair link.
fn selected_pair_fault(dedicated: NodeId, storage: NodeId) -> Fault {
    Fault::LinkDegradation {
        link: PeerLink::new(dedicated, storage),
        conditions: NetworkConditions {
            delay: Duration::ZERO,
            loss_percent: LossPercent::new(100).expect("100 is a valid loss percent"),
        },
    }
}

#[test]
#[ignore = "requires a Docker daemon; CI runs ignored integration tests"]
fn a_selected_node_pair_loses_and_regains_connectivity() {
    let cluster_plan = plan();
    let docker = docker_client();
    let runtime = tokio_runtime();
    let mut cluster =
        BollardClusterRuntime::connect().expect("a Docker daemon is required for this test");

    cluster
        .start(&cluster_plan)
        .expect("the planned containers must start on the Docker daemon");

    let dedicated = container_with_role(&cluster_plan, NodeRole::DedicatedServer);
    let storage = container_with_role(&cluster_plan, NodeRole::Storage);
    let dedicated_name = container_name(&cluster_plan, dedicated);
    let storage_ip = container_ip(&docker, &runtime, &cluster_plan, storage)
        .expect("the storage container must have an address on the plan network");
    let ping = ping_command(&storage_ip);

    let (before, output) = exec(&docker, &runtime, &dedicated_name, &ping)
        .expect("ping must run in the dedicated container before the fault");
    assert_eq!(
        before, 0,
        "the dedicated node reaches the storage node before the fault: {output}"
    );

    let mut faults = DockerFaultInjector::connect(cluster_plan.clone())
        .expect("a Docker daemon is required for this test");
    let fault = selected_pair_fault(dedicated.node, storage.node);
    faults
        .apply(&fault)
        .expect("applying the selected pair fault must succeed");

    let (during, output) = exec(&docker, &runtime, &dedicated_name, &ping)
        .expect("ping must run in the dedicated container while the fault is active");
    assert_ne!(
        during, 0,
        "the selected node pair loses connectivity during the fault: {output}"
    );

    faults
        .revert(&fault)
        .expect("reverting the selected pair fault must succeed");

    let (after, output) = exec(&docker, &runtime, &dedicated_name, &ping)
        .expect("ping must run in the dedicated container after the fault");
    assert_eq!(
        after, 0,
        "the selected node pair regains connectivity after the fault is reverted: {output}"
    );

    cluster
        .remove(&cluster_plan)
        .expect("the started containers and network must be removable");
}
